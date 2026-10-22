//! The daemon's core: entity state, auto mode, scenes and triggers.
//!
//! Every HomeKit-controlled entity is either in auto (the sun handler and
//! `adapt` move it) or overridden (the user picked brightness or colour
//! temperature). See DESIGN.md, "Auto mode".

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, TimeZone, Timelike};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::adapter::{AdapterEvent, Adapters, LightCommand};
use crate::control;
use crate::model::{Model, SunEvent, TriggerKind};
use crate::script::{self, mruby::ruby_string, Action, Script};
use crate::store::Store;
use crate::sun;

/// Transition used when the Auto switch moves entities back to sun values.
const AUTO_RESUME_TRANSITION: f64 = 2.0;
/// Scenes may activate scenes; this stops accidental loops.
const MAX_SCENE_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct EntityState {
    pub on: bool,
    /// 1–100; kept while off so the entity comes back at the same level.
    pub brightness: f64,
    pub kelvin: Option<f64>,
}

impl Default for EntityState {
    fn default() -> Self {
        Self { on: false, brightness: 100.0, kelvin: None }
    }
}

/// Requests from HomeKit.
#[derive(Debug, Clone)]
pub enum Input {
    Entity { path: String, change: Change },
    Scene(String),
    AutoSwitch(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    On(bool),
    Brightness(f64),
    Kelvin(f64),
}

/// State HomeKit should show.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Entity { path: String, state: EntityState },
    /// Whether every entity is in auto.
    Auto(bool),
}

/// Who asked for an action; decides how it affects auto mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Scene { auto: bool },
    Automation,
}

/// The values the last `adapt` asked for, applied when an auto entity
/// comes on.
#[derive(Debug, Clone, Default)]
struct AutoTarget {
    brightness: Option<f64>,
    kelvin: Option<f64>,
}

pub struct Engine {
    model: Arc<Model>,
    script: Script,
    adapters: Adapters,
    store: Store,
    updates: mpsc::UnboundedSender<Update>,
    states: HashMap<String, EntityState>,
    auto_targets: HashMap<String, AutoTarget>,
    /// Groups each light belongs to.
    groups_of_light: HashMap<String, Vec<String>>,
    exposed: HashSet<String>,
    store_dirty: bool,
}

impl Engine {
    pub fn new(
        model: Arc<Model>,
        script: Script,
        adapters: Adapters,
        mut store: Store,
        updates: mpsc::UnboundedSender<Update>,
    ) -> Self {
        let mut states = HashMap::new();
        for path in model.lights.keys().chain(model.groups.keys()) {
            states.insert(path.clone(), EntityState::default());
        }
        let mut groups_of_light: HashMap<String, Vec<String>> = HashMap::new();
        for group in model.groups.values() {
            for member in &group.members {
                groups_of_light.entry(member.light.clone()).or_default().push(group.path.clone());
            }
        }
        let exposed: HashSet<String> = model.exposed_entities().into_iter().collect();
        store.state.auto.retain(|path, _| exposed.contains(path));
        for path in &exposed {
            store.state.auto.entry(path.clone()).or_insert(true);
        }
        Self {
            model,
            script,
            adapters,
            store,
            updates,
            states,
            auto_targets: HashMap::new(),
            groups_of_light,
            exposed,
            store_dirty: true,
        }
    }

    pub fn all_auto(&self) -> bool {
        self.store.state.auto.values().all(|auto| *auto)
    }

    pub async fn run(mut self, mut inputs: mpsc::UnboundedReceiver<Input>, mut events: mpsc::UnboundedReceiver<AdapterEvent>) {
        self.publish_all();
        let mut last_tick = Local::now();
        // Give adapters a moment to report state before the first sun tick.
        let mut next_tick = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut first_tick = true;
        loop {
            tokio::select! {
                Some(input) = inputs.recv() => self.handle_input(input).await,
                Some(event) = events.recv() => self.handle_adapter_event(event).await,
                _ = tokio::time::sleep_until(next_tick) => {
                    let now = Local::now();
                    self.on_tick(last_tick, now, first_tick).await;
                    first_tick = false;
                    last_tick = now;
                    next_tick = tokio::time::Instant::now() + until_next_minute(now);
                }
                else => break,
            }
            self.save_if_dirty();
        }
    }

    // -----------------------------------------------------------------------
    // HomeKit

    async fn handle_input(&mut self, input: Input) {
        match input {
            Input::Entity { path, change } => self.handle_change(&path, change).await,
            Input::Scene(name) => self.activate_scene(&name).await,
            Input::AutoSwitch(true) => {
                log::info!("Auto switch on: every entity back into auto");
                let paths: Vec<String> = self.exposed.iter().cloned().collect();
                for path in paths {
                    self.set_auto(&path, true);
                    if self.state(&path).on {
                        let mut command = self.auto_values(&path);
                        if command.brightness.is_some() || command.kelvin.is_some() {
                            command.transition = Some(AUTO_RESUME_TRANSITION);
                            self.apply(&path, command).await;
                        }
                    }
                }
            }
            Input::AutoSwitch(false) => {
                log::info!("Auto switch off: every entity out of auto");
                let paths: Vec<String> = self.exposed.iter().cloned().collect();
                for path in paths {
                    self.set_auto(&path, false);
                }
            }
        }
    }

    async fn handle_change(&mut self, path: &str, change: Change) {
        if self.model.entity_kind(path).is_none() {
            log::warn!("HomeKit changed unknown entity {path}");
            return;
        }
        let was_on = self.state(path).on;
        match change {
            Change::On(true) if !was_on => {
                // Switching on is not an override; off-and-on again resumes auto.
                self.set_auto(path, true);
                let mut command = self.auto_values(path);
                command.on = Some(true);
                self.apply(path, command).await;
            }
            Change::On(true) => {}
            Change::On(false) => self.apply(path, LightCommand { on: Some(false), ..Default::default() }).await,
            Change::Brightness(brightness) if brightness <= 0.0 => {
                self.apply(path, LightCommand { on: Some(false), ..Default::default() }).await
            }
            Change::Brightness(brightness) => {
                self.set_auto(path, false);
                let command = LightCommand { on: Some(true), brightness: Some(brightness), ..Default::default() };
                self.apply(path, command).await;
            }
            Change::Kelvin(kelvin) => {
                self.set_auto(path, false);
                if was_on {
                    self.apply(path, LightCommand { kelvin: Some(kelvin), ..Default::default() }).await;
                } else {
                    self.update_state(path, |state| state.kelvin = Some(kelvin));
                    self.publish(path);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Adapters

    async fn handle_adapter_event(&mut self, event: AdapterEvent) {
        match event {
            AdapterEvent::Light { device, report } => {
                let Some(light) = self.model.light_by_device(&device) else { return };
                let path = light.path.clone();
                self.update_state(&path, |state| {
                    if let Some(on) = report.on {
                        state.on = on;
                    }
                    if let Some(brightness) = report.brightness.filter(|b| *b > 0.0) {
                        state.brightness = brightness;
                    }
                    if report.kelvin.is_some() {
                        state.kelvin = report.kelvin;
                    }
                });
                self.publish(&path);
                for group in self.groups_of_light.get(&path).cloned().unwrap_or_default() {
                    self.refresh_group(&group);
                }
            }
            AdapterEvent::Device { device, event, data } => {
                let path = match (self.model.sensor_by_device(&device), self.model.light_by_device(&device)) {
                    (Some(sensor), _) => sensor.path.clone(),
                    (None, Some(light)) => light.path.clone(),
                    (None, None) => return,
                };
                let triggers: Vec<(usize, String)> = self
                    .model
                    .triggers
                    .iter()
                    .filter(|trigger| matches!(&trigger.kind, TriggerKind::Device { path: p, events } if *p == path && events.contains(&event)))
                    .map(|trigger| (trigger.proc_id, trigger.line.clone()))
                    .collect();
                if triggers.is_empty() {
                    return;
                }
                let args = format!(
                    "Knut::Event.new({}, :{event}, {}), Time.at({})",
                    ruby_string(&path),
                    script::mruby::ruby_literal(&data),
                    Local::now().timestamp()
                );
                for (proc_id, line) in triggers {
                    self.run_trigger(proc_id, &args, &line).await;
                }
            }
        }
    }

    /// A group shows on while any member is on.
    fn refresh_group(&mut self, group: &str) {
        let Some(members) = self.model.groups.get(group).map(|g| g.members.clone()) else { return };
        let any_on = members.iter().any(|m| self.state(&m.light).on);
        if self.state(group).on != any_on {
            self.update_state(group, |state| state.on = any_on);
            self.publish(group);
        }
    }

    // -----------------------------------------------------------------------
    // Time

    async fn on_tick(&mut self, previous: DateTime<Local>, now: DateTime<Local>, first: bool) {
        let mut due: Vec<(usize, String, String)> = Vec::new();
        let now_ruby = format!("Time.at({})", now.timestamp());
        let dates = [previous.date_naive(), now.date_naive()];

        let sun = self.model.location.as_ref().map(|location| {
            let position = sun::position(location.lat, location.lon, now.to_utc());
            let times = sun::times(location.lat, location.lon, now.date_naive());
            (location.lat, location.lon, position, times)
        });

        for trigger in &self.model.triggers {
            let fire = |args: String| (trigger.proc_id, args, trigger.line.clone());
            match &trigger.kind {
                TriggerKind::SunTick => {
                    if let Some((_, _, position, times)) = &sun {
                        let (sunrise, sunset) = match times {
                            Some(times) => (times.sunrise.timestamp().to_string(), times.sunset.timestamp().to_string()),
                            None => ("nil".into(), "nil".into()),
                        };
                        due.push(fire(format!(
                            "Knut::Sun.new({:.4}, {:.4}, {sunrise}, {sunset}), {now_ruby}",
                            position.elevation, position.azimuth
                        )));
                    }
                }
                TriggerKind::SunEvent { event, offset_minutes } if !first => {
                    let Some((lat, lon, position, times)) = &sun else { continue };
                    let happened = dates.iter().any(|date| {
                        sun::times(*lat, *lon, *date).is_some_and(|day| {
                            let at = match event {
                                SunEvent::Sunrise => day.sunrise,
                                SunEvent::Sunset => day.sunset,
                            } + chrono::Duration::minutes(*offset_minutes);
                            previous.to_utc() < at && at <= now.to_utc()
                        })
                    });
                    if happened {
                        let (sunrise, sunset) = match times {
                            Some(times) => (times.sunrise.timestamp().to_string(), times.sunset.timestamp().to_string()),
                            None => ("nil".into(), "nil".into()),
                        };
                        due.push(fire(format!(
                            "Knut::Sun.new({:.4}, {:.4}, {sunrise}, {sunset}), {now_ruby}",
                            position.elevation, position.azimuth
                        )));
                    }
                }
                TriggerKind::Clock { times } if !first => {
                    let happened = times.iter().any(|time| {
                        dates.iter().any(|date| {
                            local_time(*date, *time).is_some_and(|at| previous < at && at <= now)
                        })
                    });
                    if happened {
                        due.push(fire(now_ruby.clone()));
                    }
                }
                _ => {}
            }
        }
        for (proc_id, args, line) in due {
            self.run_trigger(proc_id, &args, &line).await;
        }
    }

    // -----------------------------------------------------------------------
    // Scripts

    async fn run_trigger(&mut self, proc_id: usize, args: &str, line: &str) {
        let snapshot = self.snapshot();
        match self.script.run(proc_id, args, &snapshot) {
            Ok(actions) => self.execute(actions, Origin::Automation).await,
            Err(error) => log::error!("trigger at {line}: {}", script::describe(&error)),
        }
    }

    async fn activate_scene(&mut self, name: &str) {
        self.execute(vec![Action::Activate { scene: name.to_string() }], Origin::Automation).await;
    }

    async fn execute(&mut self, actions: Vec<Action>, origin: Origin) {
        let mut queue: VecDeque<(Action, Origin, usize)> = actions.into_iter().map(|a| (a, origin, 0)).collect();
        while let Some((action, origin, depth)) = queue.pop_front() {
            match action {
                Action::Set { targets, on, brightness, kelvin, transition } => {
                    for target in &targets {
                        self.set(target, on, brightness, kelvin, transition, origin).await;
                    }
                }
                Action::Adapt { targets, brightness, kelvin, transition } => {
                    for target in &targets {
                        self.adapt(target, brightness, kelvin, transition).await;
                    }
                }
                Action::Activate { scene } => {
                    if depth >= MAX_SCENE_DEPTH {
                        log::error!("scene {scene:?}: scenes nest deeper than {MAX_SCENE_DEPTH}; stopping");
                        continue;
                    }
                    let Some(found) = self.model.scene(&scene).cloned() else {
                        log::error!("unknown scene {scene:?}");
                        continue;
                    };
                    log::info!("scene {:?}", found.name);
                    let snapshot = self.snapshot();
                    match self.script.run(found.proc_id, "", &snapshot) {
                        Ok(actions) => {
                            let origin = Origin::Scene { auto: found.auto };
                            for (i, action) in actions.into_iter().enumerate() {
                                queue.insert(i, (action, origin, depth + 1));
                            }
                        }
                        Err(error) => log::error!("scene {:?} at {}: {}", found.name, found.line, script::describe(&error)),
                    }
                }
                Action::Log { message } => log::info!("script: {message}"),
            }
        }
    }

    async fn set(
        &mut self,
        path: &str,
        on: Option<bool>,
        brightness: Option<f64>,
        kelvin: Option<f64>,
        transition: Option<f64>,
        origin: Origin,
    ) {
        if self.model.entity_kind(path).is_none() {
            log::error!("unknown light or group {path:?}");
            return;
        }
        if on == Some(false) {
            self.apply(path, LightCommand { on: Some(false), transition, ..Default::default() }).await;
            return;
        }
        let has_values = brightness.is_some() || kelvin.is_some();
        let was_on = self.state(path).on;
        match origin {
            Origin::Scene { auto: false } if has_values => self.set_auto(path, false),
            Origin::Scene { auto: true } => self.set_auto(path, true),
            Origin::Scene { auto: false } if !was_on => self.set_auto(path, true),
            _ => {}
        }
        let mut command = LightCommand { on: Some(true), brightness, kelvin, transition };
        if !was_on {
            let auto = self.auto_values(path);
            command.brightness = command.brightness.or(auto.brightness);
            command.kelvin = command.kelvin.or(auto.kelvin);
        }
        self.apply(path, command).await;
    }

    async fn adapt(&mut self, path: &str, brightness: Option<f64>, kelvin: Option<f64>, transition: Option<f64>) {
        if self.model.entity_kind(path).is_none() {
            log::error!("adapt: unknown light or group {path:?}");
            return;
        }
        if !self.is_auto(path) {
            return;
        }
        let target = self.auto_targets.entry(path.to_string()).or_default();
        target.brightness = brightness.or(target.brightness);
        target.kelvin = kelvin.or(target.kelvin);
        let state = self.state(path);
        if !state.on {
            return;
        }
        let unchanged = brightness.is_none_or(|b| (b - state.brightness).abs() < 0.5)
            && kelvin.is_none_or(|k| state.kelvin.is_some_and(|current| (k - current).abs() < 10.0));
        if unchanged {
            return;
        }
        self.apply(path, LightCommand { on: None, brightness, kelvin, transition }).await;
    }

    // -----------------------------------------------------------------------
    // State

    fn state(&self, path: &str) -> EntityState {
        self.states.get(path).cloned().unwrap_or_default()
    }

    fn update_state(&mut self, path: &str, change: impl FnOnce(&mut EntityState)) {
        change(self.states.entry(path.to_string()).or_default());
    }

    /// Entities without an auto flag (hidden lights) always follow `adapt`.
    fn is_auto(&self, path: &str) -> bool {
        self.store.state.auto.get(path).copied().unwrap_or(true)
    }

    fn auto_values(&self, path: &str) -> LightCommand {
        if !self.is_auto(path) {
            return LightCommand::default();
        }
        let target = self.auto_targets.get(path).cloned().unwrap_or_default();
        LightCommand { brightness: target.brightness, kelvin: target.kelvin, ..Default::default() }
    }

    fn set_auto(&mut self, path: &str, auto: bool) {
        if !self.exposed.contains(path) {
            return;
        }
        let was_all_auto = self.all_auto();
        let previous = self.store.state.auto.insert(path.to_string(), auto);
        if previous != Some(auto) {
            log::info!("{path}: {}", if auto { "auto" } else { "overridden" });
            self.store_dirty = true;
            if was_all_auto != self.all_auto() {
                let _ = self.updates.send(Update::Auto(self.all_auto()));
            }
        }
    }

    /// Sends `command` to the lights behind `path` and records the result.
    async fn apply(&mut self, path: &str, command: LightCommand) {
        let commands = control::commands_for(&self.model, path, &command);
        let record = |state: &mut EntityState, command: &LightCommand| {
            if let Some(on) = command.on {
                state.on = on;
            }
            if let Some(brightness) = command.brightness {
                state.brightness = brightness;
            }
            if command.kelvin.is_some() {
                state.kelvin = command.kelvin;
            }
        };
        self.update_state(path, |state| record(state, &command));
        let mut touched_groups = Vec::new();
        for (light, _, light_command) in &commands {
            self.update_state(light, |state| record(state, light_command));
            if light != path {
                self.publish(light);
            }
            touched_groups.extend(self.groups_of_light.get(light).cloned().unwrap_or_default());
        }
        self.publish(path);
        for group in touched_groups {
            if group != path {
                self.refresh_group(&group);
            }
        }
        control::send_all(&self.adapters, &commands).await;
    }

    fn publish(&self, path: &str) {
        if self.exposed.contains(path) {
            let _ = self.updates.send(Update::Entity { path: path.to_string(), state: self.state(path) });
        }
    }

    fn publish_all(&self) {
        for path in &self.exposed {
            self.publish(path);
        }
        let _ = self.updates.send(Update::Auto(self.all_auto()));
    }

    /// Entity state for `state(path)` in scripts.
    fn snapshot(&self) -> Value {
        let entries = self.states.iter().map(|(path, state)| {
            let fields = json!({
                "on": state.on,
                "brightness": state.brightness,
                "kelvin": state.kelvin,
                "auto": self.is_auto(path),
            });
            (path.clone(), fields)
        });
        Value::Object(entries.collect())
    }

    fn save_if_dirty(&mut self) {
        if self.store_dirty {
            match self.store.save() {
                Ok(()) => self.store_dirty = false,
                Err(error) => log::error!("{error:#}"),
            }
        }
    }
}

fn until_next_minute(now: DateTime<Local>) -> std::time::Duration {
    let into_minute = now.second() as u64 * 1000 + now.timestamp_subsec_millis() as u64;
    std::time::Duration::from_millis(60_000 - into_minute.min(59_999))
}

fn local_time(date: NaiveDate, time: chrono::NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LightReport;
    use crate::model::{AdapterKind, DeviceRef};

    const CONFIG: &str = r#"
zigbee "z2m", mqtt: "mqtt://localhost"
room "Küche" do
  light "spot1", zigbee: "0x01"
  light "spot2", zigbee: "0x02"
  group "Spots", lights: [:spot1, :spot2] do
    map :spot2, brightness: ->(x) { x / 2 }
  end
end
room "Flur" do
  light "Licht", zigbee: "0x10"
  sensor "Bewegung", zigbee: "0x11"
end
scene "Hell", auto: false do
  set "Küche/Spots", brightness: 90
end
scene "Morgen", auto: true do
  turn_on "Küche/Spots"
end
scene "Kette", auto: false do
  activate "Hell"
end
on "Flur/Bewegung", :motion do |event, now|
  turn_on "Flur/Licht"
end
on "Clock", at: "06:30" do
  turn_on "Küche/Spots"
end
"#;

    struct Harness {
        engine: Engine,
        updates: mpsc::UnboundedReceiver<Update>,
        dir: std::path::PathBuf,
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn harness(name: &str) -> Harness {
        let (script, model) = Script::load_str(CONFIG, "config.rb").unwrap();
        let dir = std::env::temp_dir().join(format!("knut-engine-{name}-{}", std::process::id()));
        let store = Store::open(&dir).unwrap();
        let (tx, updates) = mpsc::unbounded_channel();
        let engine = Engine::new(Arc::new(model), script, Adapters::none(), store, tx);
        Harness { engine, updates, dir }
    }

    impl Harness {
        fn last_auto_update(&mut self) -> Option<bool> {
            let mut last = None;
            while let Ok(update) = self.updates.try_recv() {
                if let Update::Auto(auto) = update {
                    last = Some(auto);
                }
            }
            last
        }

        async fn homekit(&mut self, path: &str, change: Change) {
            self.engine.handle_input(Input::Entity { path: path.into(), change }).await;
        }
    }

    #[tokio::test]
    async fn brightness_overrides_and_off_on_resumes_auto() {
        let mut h = harness("override");
        assert!(h.engine.all_auto());

        h.homekit("Küche/Spots", Change::On(true)).await;
        assert!(h.engine.is_auto("Küche/Spots"), "switching on alone is not an override");

        h.homekit("Küche/Spots", Change::Brightness(60.0)).await;
        assert!(!h.engine.is_auto("Küche/Spots"));
        assert_eq!(h.last_auto_update(), Some(false));
        assert_eq!(h.engine.state("Küche/spot2").brightness, 30.0, "mapping applies to members");

        h.homekit("Küche/Spots", Change::On(false)).await;
        assert!(!h.engine.is_auto("Küche/Spots"), "switching off alone keeps the override");
        h.homekit("Küche/Spots", Change::On(true)).await;
        assert!(h.engine.is_auto("Küche/Spots"));
        assert_eq!(h.last_auto_update(), Some(true));
    }

    #[tokio::test]
    async fn adapt_moves_only_auto_entities_and_is_used_when_switching_on() {
        let mut h = harness("adapt");
        let adapt = |kelvin| vec![Action::Adapt {
            targets: vec!["Küche/Spots".into(), "Flur/Licht".into()],
            brightness: None,
            kelvin: Some(kelvin),
            transition: Some(60.0),
        }];

        h.homekit("Flur/Licht", Change::On(true)).await;
        h.homekit("Flur/Licht", Change::Kelvin(3000.0)).await;
        h.engine.execute(adapt(2400.0), Origin::Automation).await;
        assert_eq!(h.engine.state("Flur/Licht").kelvin, Some(3000.0), "overridden entity keeps its value");
        assert!(!h.engine.state("Küche/Spots").on, "adapt does not switch anything on");

        h.homekit("Küche/Spots", Change::On(true)).await;
        assert_eq!(h.engine.state("Küche/Spots").kelvin, Some(2400.0), "comes on at the current sun values");
        h.engine.execute(adapt(4000.0), Origin::Automation).await;
        assert_eq!(h.engine.state("Küche/spot1").kelvin, Some(4000.0));
    }

    #[tokio::test]
    async fn auto_switch_sets_every_entity() {
        let mut h = harness("switch");
        h.engine.handle_input(Input::AutoSwitch(false)).await;
        assert!(h.engine.store.state.auto.values().all(|auto| !auto));
        h.engine.handle_input(Input::AutoSwitch(true)).await;
        assert!(h.engine.all_auto());
        assert_eq!(h.last_auto_update(), Some(true));
    }

    #[tokio::test]
    async fn scenes_override_or_keep_auto() {
        let mut h = harness("scenes");
        h.engine.handle_input(Input::Scene("Hell".into())).await;
        assert!(!h.engine.is_auto("Küche/Spots"));
        assert_eq!(h.engine.state("Küche/Spots").brightness, 90.0);

        h.engine.handle_input(Input::Scene("Morgen".into())).await;
        assert!(h.engine.is_auto("Küche/Spots"), "auto scenes put their entities back into auto");

        h.engine.handle_input(Input::Scene("Kette".into())).await;
        assert!(!h.engine.is_auto("Küche/Spots"), "nested scenes run with their own mode");
    }

    #[tokio::test]
    async fn device_events_fire_triggers_and_reports_update_groups() {
        let mut h = harness("events");
        let sensor = DeviceRef { adapter: "z2m".into(), kind: AdapterKind::Zigbee, device: "0x11".into() };
        let event = AdapterEvent::Device { device: sensor, event: "motion".into(), data: json!({"occupancy": true}) };
        h.engine.handle_adapter_event(event).await;
        assert!(h.engine.state("Flur/Licht").on);

        let spot = DeviceRef { adapter: "z2m".into(), kind: AdapterKind::Zigbee, device: "0x01".into() };
        let report = LightReport { on: Some(true), brightness: Some(50.0), kelvin: None };
        h.engine.handle_adapter_event(AdapterEvent::Light { device: spot, report }).await;
        assert!(h.engine.state("Küche/Spots").on, "a group is on while any member is");
        assert!(h.engine.is_auto("Küche/Spots"), "device reports never change auto mode");
    }

    #[tokio::test]
    async fn clock_triggers_fire_once_in_their_minute() {
        let mut h = harness("clock");
        let at = |h, m, s| Local.with_ymd_and_hms(2026, 10, 22, h, m, s).unwrap();
        h.engine.on_tick(at(6, 28, 0), at(6, 29, 0), false).await;
        assert!(!h.engine.state("Küche/Spots").on);
        h.engine.on_tick(at(6, 29, 0), at(6, 30, 0), false).await;
        assert!(h.engine.state("Küche/Spots").on);
        h.homekit("Küche/Spots", Change::On(false)).await;
        h.engine.on_tick(at(6, 30, 0), at(6, 31, 0), false).await;
        assert!(!h.engine.state("Küche/Spots").on, "fired only once");
    }

    #[test]
    fn waits_until_the_next_minute() {
        let now = Local.with_ymd_and_hms(2026, 10, 22, 6, 29, 45).unwrap();
        assert_eq!(until_next_minute(now), std::time::Duration::from_secs(15));
    }
}
