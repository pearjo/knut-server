//! The daemon's core: entity state, auto mode, scenes and triggers.
//!
//! Every HomeKit-controlled entity is either in auto (the sun handler and
//! `adapt` move it) or overridden (the user picked brightness or colour
//! temperature). See DESIGN.md, "Auto mode".
//!
//! The engine's methods are split by what they handle: `inputs` (HomeKit
//! and adapters), `triggers` (sun and clock), `actions` (what blocks ask
//! for), `lights` (commands and staging) and `state` (bookkeeping).

mod actions;
mod inputs;
mod lights;
mod state;
mod triggers;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Local, Timelike};
use tokio::sync::mpsc;

use crate::adapter::{AdapterEvent, Adapters};
use crate::model::Model;
use crate::script::Script;
use crate::store::Store;
use crate::sun;

/// Transition used when the Auto switch moves entities back to sun values.
const AUTO_RESUME_TRANSITION: f64 = 2.0;
/// Scenes may activate scenes; this stops accidental loops.
const MAX_SCENE_DEPTH: usize = 4;
/// `adapt` never dims an entity below this value (0–100) while it is on:
/// very low light levels are the one robust discomfort finding.
const COMFORT_MINIMUM: f64 = 5.0;
/// Seconds a stage change takes at least, so stages cross-fade rather than
/// switch even for commands without a transition.
const MIN_STAGE_TRANSITION: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntityState {
    pub on: bool,
    /// 1–100; kept while off so the entity comes back at the same level.
    pub brightness: f64,
    pub kelvin: Option<f64>,
}

impl Default for EntityState {
    fn default() -> Self {
        Self {
            on: false,
            brightness: 100.0,
            kelvin: None,
        }
    }
}

/// Requests from HomeKit.
#[derive(Debug, Clone)]
pub(crate) enum Input {
    Entity { path: String, change: Change },
    Scene(String),
    AutoSwitch(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Change {
    On(bool),
    Brightness(f64),
    Kelvin(f64),
}

/// State HomeKit should show.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Update {
    Entity {
        path: String,
        state: EntityState,
    },
    /// Whether every entity is in auto.
    Auto(bool),
}

/// Who asked for an action; decides how it affects auto mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Scene { auto: bool },
    Automation,
}

/// What staging has switched on in a group that is on.
#[derive(Debug, Clone, PartialEq)]
enum ActiveStaging {
    /// The number of leading stages.
    Stages(usize),
    /// The lights a staging block picked.
    Lights(Vec<String>),
}

/// The values the last `adapt` asked for, applied when an auto entity
/// comes on.
#[derive(Debug, Clone, Default)]
struct AutoTarget {
    brightness: Option<f64>,
    kelvin: Option<f64>,
}

pub(crate) struct Engine {
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
    /// As of the last tick; `None` without a location.
    sun: Option<sun::SunPosition>,
    /// Per staged group that is on.
    active_stages: HashMap<String, ActiveStaging>,
}

impl Engine {
    pub(crate) fn new(
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
                groups_of_light
                    .entry(member.light.clone())
                    .or_default()
                    .push(group.path.clone());
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
            sun: None,
            active_stages: HashMap::new(),
        }
    }

    pub(crate) fn all_auto(&self) -> bool {
        self.store.state.auto.values().all(|auto| *auto)
    }

    pub(crate) async fn run(
        mut self,
        mut inputs: mpsc::UnboundedReceiver<Input>,
        mut events: mpsc::UnboundedReceiver<AdapterEvent>,
    ) {
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
}

fn until_next_minute(now: DateTime<Local>) -> std::time::Duration {
    let into_minute = now.second() as u64 * 1000 + now.timestamp_subsec_millis() as u64;
    std::time::Duration::from_millis(60_000 - into_minute.min(59_999))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{AdapterEvent, LightCommand, LightReport};
    use crate::model::{AdapterKind, DeviceRef};
    use crate::script::Action;
    use chrono::TimeZone;
    use serde_json::json;

    const CONFIG: &str = r#"
zigbee "z2m", mqtt: "mqtt://localhost"
room "Küche" do
  daylight 0.5, facing: :south
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
room "Bad" do
  light "a", zigbee: "0x20"
  light "b", zigbee: "0x21"
  light "c", zigbee: "0x22"
  group "Licht", lights: [:a, :b, :c] do
    stages [:b], [:a, :c]
  end
  group "Spiegel", lights: [:a, :b] do
    staging { |need, now| need > 0.5 ? [:a, :b] : [:a] }
  end
  group "Wanne", lights: [:b, :c] do
    staging { |need, now| [:x] }
  end
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
        Harness {
            engine,
            updates,
            dir,
        }
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
            self.engine
                .handle_input(Input::Entity {
                    path: path.into(),
                    change,
                })
                .await;
        }
    }

    #[tokio::test]
    async fn brightness_overrides_and_off_on_resumes_auto() {
        let mut h = harness("override");
        assert!(h.engine.all_auto());

        h.homekit("Küche/Spots", Change::On(true)).await;
        assert!(
            h.engine.is_auto("Küche/Spots"),
            "switching on alone is not an override"
        );

        h.homekit("Küche/Spots", Change::Brightness(60.0)).await;
        assert!(!h.engine.is_auto("Küche/Spots"));
        assert_eq!(h.last_auto_update(), Some(false));
        assert_eq!(
            h.engine.state("Küche/spot2").brightness,
            30.0,
            "mapping applies to members"
        );

        h.homekit("Küche/Spots", Change::On(false)).await;
        assert!(
            !h.engine.is_auto("Küche/Spots"),
            "switching off alone keeps the override"
        );
        h.homekit("Küche/Spots", Change::On(true)).await;
        assert!(h.engine.is_auto("Küche/Spots"));
        assert_eq!(h.last_auto_update(), Some(true));
    }

    #[tokio::test]
    async fn adapt_moves_only_auto_entities_and_is_used_when_switching_on() {
        let mut h = harness("adapt");
        let adapt = |kelvin| {
            vec![Action::Adapt {
                targets: vec!["Küche/Spots".into(), "Flur/Licht".into()],
                need: None,
                kelvin: Some(kelvin),
                transition: Some(60.0),
            }]
        };

        h.homekit("Flur/Licht", Change::On(true)).await;
        h.homekit("Flur/Licht", Change::Kelvin(3000.0)).await;
        h.engine.execute(adapt(2400.0), Origin::Automation).await;
        assert_eq!(
            h.engine.state("Flur/Licht").kelvin,
            Some(3000.0),
            "overridden entity keeps its value"
        );
        assert!(
            !h.engine.state("Küche/Spots").on,
            "adapt does not switch anything on"
        );

        h.homekit("Küche/Spots", Change::On(true)).await;
        assert_eq!(
            h.engine.state("Küche/Spots").kelvin,
            Some(2400.0),
            "comes on at the current sun values"
        );
        h.engine.execute(adapt(4000.0), Origin::Automation).await;
        assert_eq!(h.engine.state("Küche/spot1").kelvin, Some(4000.0));
    }

    #[tokio::test]
    async fn adapt_subtracts_room_daylight_from_the_need() {
        let mut h = harness("need");
        // Full sky, sun behind the south window: Küche gets 0.5 × 1.0 daylight.
        h.engine.sun = Some(sun::SunPosition {
            elevation: 30.0,
            azimuth: 0.0,
        });
        let adapt = |need| {
            vec![Action::Adapt {
                targets: vec!["Küche/Spots".into(), "Flur/Licht".into()],
                need: Some(need),
                kelvin: None,
                transition: None,
            }]
        };
        let brightness = |h: &Harness, path| h.engine.state(path).brightness;
        h.homekit("Küche/Spots", Change::On(true)).await;
        h.homekit("Flur/Licht", Change::On(true)).await;

        h.engine.execute(adapt(0.8), Origin::Automation).await;
        assert!(
            (brightness(&h, "Küche/Spots") - 30.0).abs() < 1e-9,
            "daylight subtracted"
        );
        assert!(
            (brightness(&h, "Küche/spot2") - 15.0).abs() < 1e-9,
            "then the mapping"
        );
        assert_eq!(
            brightness(&h, "Flur/Licht"),
            80.0,
            "no daylight in the hall"
        );

        h.engine.execute(adapt(0.3), Origin::Automation).await;
        assert_eq!(
            brightness(&h, "Küche/Spots"),
            COMFORT_MINIMUM,
            "daylight exceeds the need"
        );
        h.engine.execute(adapt(0.0), Origin::Automation).await;
        assert_eq!(brightness(&h, "Flur/Licht"), COMFORT_MINIMUM);

        h.homekit("Küche/Spots", Change::Brightness(40.0)).await;
        assert_eq!(
            brightness(&h, "Küche/Spots"),
            40.0,
            "an override is the need itself"
        );
        h.engine.execute(adapt(0.8), Origin::Automation).await;
        assert_eq!(
            brightness(&h, "Küche/Spots"),
            40.0,
            "and adapt leaves it alone"
        );
    }

    #[tokio::test]
    async fn staging_follows_overrides_and_adapt() {
        let mut h = harness("staging");
        let light = |h: &Harness, name: &str| {
            let state = h.engine.state(&format!("Bad/{name}"));
            state.on.then_some(state.brightness)
        };

        h.homekit("Bad/Licht", Change::On(true)).await;
        assert_eq!(
            (light(&h, "a"), light(&h, "b"), light(&h, "c")),
            (Some(100.0), Some(100.0), Some(100.0))
        );

        h.homekit("Bad/Licht", Change::Brightness(20.0)).await;
        assert_eq!(
            h.engine.state("Bad/Licht").brightness,
            20.0,
            "HomeKit shows the need"
        );
        assert_eq!(
            (light(&h, "a"), light(&h, "b"), light(&h, "c")),
            (None, Some(60.0), None),
            "an override is staged too"
        );

        h.homekit("Bad/Licht", Change::On(false)).await;
        h.homekit("Bad/Licht", Change::On(true)).await;
        assert!(h.engine.is_auto("Bad/Licht"));
        let adapt = |need| {
            vec![Action::Adapt {
                targets: vec!["Bad/Licht".into()],
                need: Some(need),
                kelvin: None,
                transition: Some(60.0),
            }]
        };
        h.engine.execute(adapt(0.5), Origin::Automation).await;
        assert_eq!(
            (light(&h, "a"), light(&h, "b"), light(&h, "c")),
            (Some(50.0), Some(50.0), Some(50.0))
        );
        h.engine.execute(adapt(0.26), Origin::Automation).await;
        assert_eq!(
            light(&h, "a"),
            Some(26.0),
            "hysteresis keeps both stages above 75 per light"
        );
        h.engine.execute(adapt(0.2), Origin::Automation).await;
        assert_eq!(
            (light(&h, "a"), light(&h, "b"), light(&h, "c")),
            (None, Some(60.0), None)
        );
    }

    #[tokio::test]
    async fn stage_changes_cross_fade() {
        let mut h = harness("cross-fade");
        h.homekit("Bad/Licht", Change::Brightness(20.0)).await;

        let mut command = LightCommand {
            brightness: Some(50.0),
            ..Default::default()
        };
        let staged = h.engine.stage("Bad/Licht", &mut command).unwrap();
        assert_eq!(staged.on.len(), 3, "the second stage comes on");
        assert_eq!(command.transition, Some(MIN_STAGE_TRANSITION));
        assert_eq!(command.on, Some(true));

        let mut command = LightCommand {
            brightness: Some(45.0),
            transition: Some(60.0),
            ..Default::default()
        };
        let staged = h.engine.stage("Bad/Licht", &mut command).unwrap();
        assert!(staged.off.is_empty());
        assert_eq!(
            command.transition,
            Some(60.0),
            "longer transitions are kept"
        );

        let mut off = LightCommand {
            on: Some(false),
            ..Default::default()
        };
        assert!(h.engine.stage("Bad/Licht", &mut off).is_none());
        assert!(
            !h.engine.active_stages.contains_key("Bad/Licht"),
            "switching off forgets the stages"
        );
    }

    #[tokio::test]
    async fn staging_blocks_pick_the_lights() {
        let mut h = harness("staging-block");
        let lit = |h: &Harness, name: &str| {
            let state = h.engine.state(&format!("Bad/{name}"));
            state.on.then_some(state.brightness)
        };
        let adapt = |need| {
            vec![Action::Adapt {
                targets: vec!["Bad/Spiegel".into()],
                need: Some(need),
                kelvin: None,
                transition: None,
            }]
        };
        h.homekit("Bad/Spiegel", Change::On(true)).await;

        h.engine.execute(adapt(0.3), Origin::Automation).await;
        assert_eq!(
            (lit(&h, "a"), lit(&h, "b")),
            (Some(60.0), None),
            "one of two lights carries 0.3 · 2"
        );
        h.engine.execute(adapt(0.8), Origin::Automation).await;
        assert_eq!((lit(&h, "a"), lit(&h, "b")), (Some(80.0), Some(80.0)));
        assert_eq!(
            h.engine.active_stages["Bad/Spiegel"],
            ActiveStaging::Lights(vec!["Bad/a".into(), "Bad/b".into()])
        );

        h.homekit("Bad/Wanne", Change::Brightness(30.0)).await;
        assert_eq!(
            (lit(&h, "b"), lit(&h, "c")),
            (Some(30.0), Some(30.0)),
            "a failing block lights everything"
        );
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
        assert!(
            h.engine.is_auto("Küche/Spots"),
            "auto scenes put their entities back into auto"
        );

        h.engine.handle_input(Input::Scene("Kette".into())).await;
        assert!(
            !h.engine.is_auto("Küche/Spots"),
            "nested scenes run with their own mode"
        );
    }

    #[tokio::test]
    async fn device_events_fire_triggers_and_reports_update_groups() {
        let mut h = harness("events");
        let sensor = DeviceRef {
            adapter: "z2m".into(),
            kind: AdapterKind::Zigbee,
            device: "0x11".into(),
        };
        let event = AdapterEvent::Device {
            device: sensor,
            event: "motion".into(),
            data: json!({"occupancy": true}),
        };
        h.engine.handle_adapter_event(event).await;
        assert!(h.engine.state("Flur/Licht").on);

        let spot = DeviceRef {
            adapter: "z2m".into(),
            kind: AdapterKind::Zigbee,
            device: "0x01".into(),
        };
        let report = LightReport {
            on: Some(true),
            brightness: Some(50.0),
            kelvin: None,
        };
        h.engine
            .handle_adapter_event(AdapterEvent::Light {
                device: spot,
                report,
            })
            .await;
        assert!(
            h.engine.state("Küche/Spots").on,
            "a group is on while any member is"
        );
        assert!(
            h.engine.is_auto("Küche/Spots"),
            "device reports never change auto mode"
        );
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
