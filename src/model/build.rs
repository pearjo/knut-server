//! Building the model from the JSON the prelude records, and validating it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::Deserialize;

use super::{
    entity_name, entity_path, AdapterConfig, AdapterKind, BrightnessMapping, Daylight, DeviceRef,
    Group, GroupMember, HomeKitConfig, Light, Line, Location, Model, Room, Scene, ScheduleDay,
    Sensor, Staging, SunEvent, Trigger, TriggerKind,
};

/// Problems found while validating; each names its config line.
#[derive(Debug)]
pub(crate) struct ValidationErrors(pub Vec<String>);

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, error) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

/// The model as the prelude records it: names instead of resolved
/// references, and nothing validated yet. The `Raw…` types below mirror
/// its JSON.
#[derive(Debug, Deserialize)]
pub(crate) struct RawModel {
    location: Option<Location>,
    homekit: Option<HomeKitConfig>,
    schedule: Option<[ScheduleDay; 7]>,
    adapters: Vec<AdapterConfig>,
    rooms: Vec<RawRoom>,
    scenes: Vec<RawScene>,
    triggers: Vec<RawTrigger>,
}

#[derive(Debug, Deserialize)]
struct RawRoom {
    name: String,
    lights: Vec<RawDevice>,
    sensors: Vec<RawDevice>,
    groups: Vec<RawGroup>,
    daylight: Option<Daylight>,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawDevice {
    name: String,
    adapter_kind: AdapterKind,
    adapter: Option<String>,
    device: String,
    #[serde(default)]
    expose: Option<bool>,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawGroup {
    name: String,
    lights: Vec<String>,
    expose: bool,
    mappings: Vec<RawMapping>,
    stages: Option<RawStages>,
    staging: Option<RawStaging>,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawStages {
    lights: Vec<Vec<String>>,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawStaging {
    proc: usize,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawMapping {
    light: String,
    proc: usize,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawScene {
    name: String,
    auto: bool,
    proc: usize,
    line: Line,
}

#[derive(Debug, Deserialize)]
struct RawTrigger {
    source: String,
    events: Vec<String>,
    options: HashMap<String, serde_json::Value>,
    proc: usize,
    line: Line,
}

impl RawModel {
    /// Ids of all brightness mapping procs, so the caller can evaluate them.
    pub(crate) fn mapping_procs(&self) -> Vec<usize> {
        self.rooms
            .iter()
            .flat_map(|room| &room.groups)
            .flat_map(|group| &group.mappings)
            .map(|mapping| mapping.proc)
            .collect()
    }

    /// Resolves references and validates the whole model. `mapping_tables`
    /// holds the evaluated table for each id from [`Self::mapping_procs`].
    pub(crate) fn build(
        self,
        mapping_tables: &HashMap<usize, Vec<f64>>,
    ) -> Result<Model, ValidationErrors> {
        let mut builder = ModelBuilder::new(&self.adapters, mapping_tables);
        builder.check_adapter_names();
        for room in &self.rooms {
            builder.add_room(room);
        }
        builder.add_scenes(self.scenes);
        builder.add_triggers(self.triggers, self.location.is_some());
        let homekit = builder.homekit(self.homekit);

        if !builder.errors.is_empty() {
            return Err(ValidationErrors(builder.errors));
        }
        Ok(Model {
            location: self.location,
            homekit,
            schedule: self.schedule,
            rooms: builder.rooms,
            lights: builder.lights,
            sensors: builder.sensors,
            groups: builder.groups,
            scenes: builder.scenes,
            triggers: builder.triggers,
            adapters: self.adapters,
        })
    }
}

/// Collects the parts of the model, and every problem found on the way, so
/// that `knut check` can report all of them at once.
struct ModelBuilder<'a> {
    adapters: &'a [AdapterConfig],
    mapping_tables: &'a HashMap<usize, Vec<f64>>,
    rooms: Vec<Room>,
    lights: BTreeMap<String, Light>,
    sensors: BTreeMap<String, Sensor>,
    groups: BTreeMap<String, Group>,
    scenes: Vec<Scene>,
    triggers: Vec<Trigger>,
    /// The light using each device, so that no device is used twice.
    devices_seen: HashMap<DeviceRef, String>,
    errors: Vec<String>,
}

impl<'a> ModelBuilder<'a> {
    fn new(adapters: &'a [AdapterConfig], mapping_tables: &'a HashMap<usize, Vec<f64>>) -> Self {
        Self {
            adapters,
            mapping_tables,
            rooms: Vec::new(),
            lights: BTreeMap::new(),
            sensors: BTreeMap::new(),
            groups: BTreeMap::new(),
            scenes: Vec::new(),
            triggers: Vec::new(),
            devices_seen: HashMap::new(),
            errors: Vec::new(),
        }
    }

    fn check_adapter_names(&mut self) {
        let mut names = HashSet::new();
        for adapter in self.adapters {
            if !names.insert(adapter.name()) {
                self.errors.push(format!(
                    "{}: adapter {:?} is defined twice",
                    adapter.line(),
                    adapter.name()
                ));
            }
        }
    }

    fn add_room(&mut self, raw: &RawRoom) {
        if self.rooms.iter().any(|room| room.name == raw.name) {
            self.errors.push(format!(
                "{}: room {:?} is defined twice",
                raw.line, raw.name
            ));
            return;
        }
        let mut room = Room {
            name: raw.name.clone(),
            lights: Vec::new(),
            sensors: Vec::new(),
            groups: Vec::new(),
            daylight: raw.daylight.clone(),
        };
        let mut names = HashSet::new();
        for light in &raw.lights {
            if !self.claim_name(&mut names, &raw.name, &light.name, &light.line) {
                continue;
            }
            if let Some(path) = self.add_light(&raw.name, light) {
                room.lights.push(path);
            }
        }
        for sensor in &raw.sensors {
            if !self.claim_name(&mut names, &raw.name, &sensor.name, &sensor.line) {
                continue;
            }
            if let Some(path) = self.add_sensor(&raw.name, sensor) {
                room.sensors.push(path);
            }
        }
        for group in &raw.groups {
            if self.claim_name(&mut names, &raw.name, &group.name, &group.line) {
                room.groups.push(self.add_group(&raw.name, group));
            }
        }
        self.set_light_exposure(raw, &room);
        self.rooms.push(room);
    }

    /// Whether `name` is still free in the room; reports it otherwise.
    fn claim_name(
        &mut self,
        names: &mut HashSet<String>,
        room: &str,
        name: &str,
        line: &str,
    ) -> bool {
        let unique = names.insert(name.to_string());
        if !unique {
            self.errors
                .push(format!("{line}: {name:?} is already used in room {room:?}"));
        }
        unique
    }

    /// Adds a light and returns its path. Whether HomeKit shows it is
    /// decided once the room's groups are known, in `set_light_exposure`.
    fn add_light(&mut self, room: &str, raw: &RawDevice) -> Option<String> {
        let device = self.resolve_device(raw)?;
        let path = entity_path(room, &raw.name);
        if let Some(other) = self.devices_seen.insert(device.clone(), path.clone()) {
            self.errors
                .push(format!("{}: {device} is already used by {other}", raw.line));
        }
        let light = Light {
            name: raw.name.clone(),
            path: path.clone(),
            device,
            exposed: false,
        };
        self.lights.insert(path.clone(), light);
        Some(path)
    }

    fn add_sensor(&mut self, room: &str, raw: &RawDevice) -> Option<String> {
        let device = self.resolve_device(raw)?;
        let path = entity_path(room, &raw.name);
        let sensor = Sensor {
            name: raw.name.clone(),
            path: path.clone(),
            device,
        };
        self.sensors.insert(path.clone(), sensor);
        Some(path)
    }

    /// The adapter device behind `zigbee:`, `hue:` and `adapter:`. A device
    /// needs exactly one adapter of its kind that matches.
    fn resolve_device(&mut self, raw: &RawDevice) -> Option<DeviceRef> {
        let wanted_name = raw.adapter.as_deref();
        let candidates: Vec<&AdapterConfig> = self
            .adapters
            .iter()
            .filter(|adapter| adapter.kind() == raw.adapter_kind)
            .filter(|adapter| wanted_name.is_none_or(|name| adapter.name() == name))
            .collect();
        match candidates.as_slice() {
            [adapter] => Some(DeviceRef {
                adapter: adapter.name().to_string(),
                kind: raw.adapter_kind,
                device: raw.device.clone(),
            }),
            [] => {
                let wanted = match &raw.adapter {
                    Some(name) => format!("{} adapter {name:?}", raw.adapter_kind),
                    None => format!("{} adapter", raw.adapter_kind),
                };
                self.errors.push(format!(
                    "{}: {:?} needs a {wanted}, but none is defined",
                    raw.line, raw.name
                ));
                None
            }
            _ => {
                self.errors.push(format!(
                    "{}: {:?}: several {} adapters are defined, pick one with adapter: \"name\"",
                    raw.line, raw.name, raw.adapter_kind
                ));
                None
            }
        }
    }

    /// HomeKit shows a light unless it is in a group of its room; `expose:`
    /// in the config decides either way.
    fn set_light_exposure(&mut self, raw: &RawRoom, room: &Room) {
        let grouped: HashSet<&String> = room
            .groups
            .iter()
            .flat_map(|path| &self.groups[path].members)
            .map(|member| &member.light)
            .collect();
        let mut exposure = Vec::new();
        for light in &raw.lights {
            let path = entity_path(&raw.name, &light.name);
            if room.lights.contains(&path) {
                let exposed = light.expose.unwrap_or(!grouped.contains(&path));
                exposure.push((path, exposed));
            }
        }
        for (path, exposed) in exposure {
            if let Some(light) = self.lights.get_mut(&path) {
                light.exposed = exposed;
            }
        }
    }

    fn add_group(&mut self, room: &str, raw: &RawGroup) -> String {
        let mut members = self.group_members(room, raw);
        self.add_mappings(room, raw, &mut members);
        let staging = self.group_staging(room, raw, &members);
        let path = entity_path(room, &raw.name);
        let group = Group {
            name: raw.name.clone(),
            path: path.clone(),
            exposed: raw.expose,
            members,
            staging,
        };
        self.groups.insert(path.clone(), group);
        path
    }

    /// The group's lights, each a light of the same room, listed once.
    fn group_members(&mut self, room: &str, raw: &RawGroup) -> Vec<GroupMember> {
        let mut members: Vec<GroupMember> = Vec::new();
        for light in &raw.lights {
            let path = entity_path(room, light);
            if !self.lights.contains_key(&path) {
                self.errors.push(format!(
                    "{}: group {:?} refers to unknown light {light:?} in room {room:?}",
                    raw.line, raw.name
                ));
                continue;
            }
            if members.iter().any(|member| member.light == path) {
                self.errors.push(format!(
                    "{}: group {:?} lists {light:?} twice",
                    raw.line, raw.name
                ));
                continue;
            }
            members.push(GroupMember {
                light: path,
                mapping: None,
            });
        }
        members
    }

    /// Attaches each `map` of the group to its member.
    fn add_mappings(&mut self, room: &str, raw: &RawGroup, members: &mut [GroupMember]) {
        for mapping in &raw.mappings {
            let path = entity_path(room, &mapping.light);
            let Some(member) = members.iter_mut().find(|member| member.light == path) else {
                self.errors.push(format!(
                    "{}: map refers to {:?}, which is not a light of group {:?}",
                    mapping.line, mapping.light, raw.name
                ));
                continue;
            };
            if member.mapping.is_some() {
                self.errors.push(format!(
                    "{}: {:?} is mapped twice",
                    mapping.line, mapping.light
                ));
            }
            match self.mapping_tables.get(&mapping.proc) {
                Some(table) => member.mapping = Some(BrightnessMapping::new(table.clone())),
                None => self.errors.push(format!(
                    "{}: brightness mapping could not be evaluated",
                    mapping.line
                )),
            }
        }
    }

    /// The group's `stages` or `staging` block; the prelude allows only one.
    fn group_staging(
        &mut self,
        room: &str,
        raw: &RawGroup,
        members: &[GroupMember],
    ) -> Option<Staging> {
        if let Some(raw_stages) = &raw.stages {
            return Some(self.stages(room, raw, raw_stages, members));
        }
        raw.staging.as_ref().map(|block| Staging::Custom {
            proc_id: block.proc,
            line: block.line.clone(),
        })
    }

    /// The stages as light paths. Every member must be in exactly one stage,
    /// and every staged light must be a member.
    fn stages(
        &mut self,
        room: &str,
        group: &RawGroup,
        raw: &RawStages,
        members: &[GroupMember],
    ) -> Staging {
        let stages: Vec<Vec<String>> = raw
            .lights
            .iter()
            .map(|stage| stage.iter().map(|light| entity_path(room, light)).collect())
            .collect();
        let staged: Vec<&String> = stages.iter().flatten().collect();
        for light in &staged {
            if !members.iter().any(|member| &member.light == *light) {
                let name = entity_name(light);
                self.errors.push(format!(
                    "{}: stages refer to {name:?}, which is not a light of group {:?}",
                    raw.line, group.name
                ));
            }
        }
        for member in members {
            let times_staged = staged
                .iter()
                .filter(|light| **light == &member.light)
                .count();
            if times_staged != 1 {
                let name = entity_name(&member.light);
                self.errors.push(format!(
                    "{}: stages must list {name:?} exactly once",
                    raw.line
                ));
            }
        }
        Staging::Stages(stages)
    }

    fn add_scenes(&mut self, raw: Vec<RawScene>) {
        for scene in raw {
            if self.scenes.iter().any(|known| known.name == scene.name) {
                self.errors.push(format!(
                    "{}: scene {:?} is defined twice",
                    scene.line, scene.name
                ));
                continue;
            }
            self.scenes.push(Scene {
                name: scene.name,
                auto: scene.auto,
                proc_id: scene.proc,
                line: scene.line,
            });
        }
    }

    fn add_triggers(&mut self, raw: Vec<RawTrigger>, has_location: bool) {
        for trigger in raw {
            match trigger_kind(&trigger, &self.lights, &self.sensors, has_location) {
                Ok(kind) => self.triggers.push(Trigger {
                    kind,
                    proc_id: trigger.proc,
                    line: trigger.line,
                }),
                Err(message) => self.errors.push(format!("{}: {message}", trigger.line)),
            }
        }
    }

    /// The `homekit` settings, or the defaults when the config has none.
    fn homekit(&mut self, raw: Option<HomeKitConfig>) -> HomeKitConfig {
        let homekit = raw.unwrap_or(HomeKitConfig {
            name: "Knut".into(),
            pin: None,
            port: 32000,
            host: None,
            line: String::new(),
        });
        if let Some(pin) = &homekit.pin {
            if is_trivial_pin(pin) {
                self.errors.push(format!(
                    "{}: HomeKit rejects trivial setup codes like {pin}",
                    homekit.line
                ));
            }
        }
        homekit
    }
}

/// What `on source, *events, **options` listens for, or why it cannot.
fn trigger_kind(
    raw: &RawTrigger,
    lights: &BTreeMap<String, Light>,
    sensors: &BTreeMap<String, Sensor>,
    has_location: bool,
) -> Result<TriggerKind, String> {
    match raw.source.as_str() {
        "Sun" => sun_trigger(raw, has_location),
        "Clock" => clock_trigger(raw),
        path => {
            let known = sensors.contains_key(path) || lights.contains_key(path);
            if !known {
                return Err(format!(
                    "trigger source {path:?} is neither \"Sun\", \"Clock\" nor a known sensor or light"
                ));
            }
            device_trigger(raw, path)
        }
    }
}

/// `on "Sun", :sun`, or `:sunrise` / `:sunset` with an optional
/// `offset:` in minutes.
fn sun_trigger(raw: &RawTrigger, has_location: bool) -> Result<TriggerKind, String> {
    if !has_location {
        return Err("Sun triggers need `location lat:, lon:`".into());
    }
    allow_options(raw, &["offset"])?;
    let offset_minutes = match raw.options.get("offset") {
        None => 0,
        Some(value) => value.as_i64().ok_or("offset: must be whole minutes")?,
    };

    let sun_event = |event| TriggerKind::SunEvent {
        event,
        offset_minutes,
    };
    match raw.events.as_slice() {
        [event] if event == "sun" => {
            if offset_minutes != 0 {
                return Err("offset: only applies to :sunrise and :sunset".into());
            }
            Ok(TriggerKind::SunTick)
        }
        [event] if event == "sunrise" => Ok(sun_event(SunEvent::Sunrise)),
        [event] if event == "sunset" => Ok(sun_event(SunEvent::Sunset)),
        _ => Err("Sun triggers take exactly one of :sun, :sunrise, :sunset".into()),
    }
}

/// `on "Clock", at: "HH:MM"` or `at: ["HH:MM", …]`.
fn clock_trigger(raw: &RawTrigger) -> Result<TriggerKind, String> {
    allow_options(raw, &["at"])?;
    if !raw.events.is_empty() {
        return Err("Clock triggers take no events, only at: \"HH:MM\"".into());
    }

    let values = clock_time_strings(raw.options.get("at"))?;
    let times = values
        .iter()
        .map(|time| parse_clock_time(time))
        .collect::<Result<_, _>>()?;
    Ok(TriggerKind::Clock { times })
}

/// The `at:` option as a list of strings, whether it holds one or several.
fn clock_time_strings(at: Option<&serde_json::Value>) -> Result<Vec<String>, String> {
    match at {
        Some(serde_json::Value::String(time)) => Ok(vec![time.clone()]),
        Some(serde_json::Value::Array(times)) => times
            .iter()
            .map(|time| {
                let time = time.as_str().ok_or("at: must contain \"HH:MM\" strings")?;
                Ok(time.to_owned())
            })
            .collect(),
        _ => Err("Clock triggers need at: \"HH:MM\"".into()),
    }
}

fn parse_clock_time(time: &str) -> Result<chrono::NaiveTime, String> {
    chrono::NaiveTime::parse_from_str(time, "%H:%M")
        .map_err(|_| format!("at: {time:?} is not a valid \"HH:MM\" time"))
}

/// `on "Room/Sensor", :motion, …`: one or more of the device events.
fn device_trigger(raw: &RawTrigger, path: &str) -> Result<TriggerKind, String> {
    allow_options(raw, &[])?;
    if raw.events.is_empty() {
        return Err(format!("trigger on {path:?} needs an event, e.g. :motion"));
    }

    let device_events = crate::adapter::DEVICE_EVENTS;
    for event in &raw.events {
        if !device_events.contains(&event.as_str()) {
            return Err(format!(
                "unknown device event :{event}, expected one of :{}",
                device_events.join(", :")
            ));
        }
    }
    Ok(TriggerKind::Device {
        path: path.to_string(),
        events: raw.events.clone(),
    })
}

/// Fails for the first option of `raw` that is not in `allowed`.
fn allow_options(raw: &RawTrigger, allowed: &[&str]) -> Result<(), String> {
    let unknown = raw
        .options
        .keys()
        .find(|key| !allowed.contains(&key.as_str()));
    match unknown {
        Some(key) => Err(format!(
            "unknown option {key}: for {:?} trigger",
            raw.source
        )),
        None => Ok(()),
    }
}

fn is_trivial_pin(pin: &str) -> bool {
    let all_same = pin.chars().all(|c| pin.starts_with(c));
    all_same || pin == "12345678" || pin == "87654321"
}
