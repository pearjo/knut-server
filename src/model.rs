//! The configured home: adapters, rooms, lights, groups, scenes, triggers.
//!
//! Built from the JSON the DSL prelude records while the config loads, then
//! validated as a whole so that `knut check` can report every problem.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::Deserialize;

/// Source location in the config, e.g. `config.rb:12`.
pub type Line = String;

#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HomeKitConfig {
    pub name: String,
    pub pin: Option<String>,
    pub port: u16,
    /// Address to serve on; defaults to the first non-loopback interface.
    pub host: Option<std::net::IpAddr>,
    pub line: Line,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AdapterConfig {
    Hue {
        name: String,
        host: String,
        key: Option<String>,
        line: Line,
    },
    Zigbee {
        name: String,
        mqtt: String,
        base_topic: String,
        line: Line,
    },
}

impl AdapterConfig {
    pub fn name(&self) -> &str {
        match self {
            Self::Hue { name, .. } | Self::Zigbee { name, .. } => name,
        }
    }

    pub fn kind(&self) -> AdapterKind {
        match self {
            Self::Hue { .. } => AdapterKind::Hue,
            Self::Zigbee { .. } => AdapterKind::Zigbee,
        }
    }

    pub fn line(&self) -> &str {
        match self {
            Self::Hue { line, .. } | Self::Zigbee { line, .. } => line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdapterKind {
    Hue,
    Zigbee,
}

impl fmt::Display for AdapterKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hue => "hue",
            Self::Zigbee => "zigbee",
        })
    }
}

/// A device on one adapter, e.g. `zigbee:0x00158d0001a2b3c4` on adapter `z2m`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceRef {
    pub adapter: String,
    pub kind: AdapterKind,
    pub device: String,
}

impl fmt::Display for DeviceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{} ({})", self.kind, self.device, self.adapter)
    }
}

#[derive(Debug, Clone)]
pub struct Light {
    pub name: String,
    pub path: String,
    pub device: DeviceRef,
    pub exposed: bool,
}

#[derive(Debug, Clone)]
pub struct Sensor {
    pub name: String,
    pub path: String,
    pub device: DeviceRef,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub path: String,
    pub exposed: bool,
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone)]
pub struct GroupMember {
    /// Path of the member light.
    pub light: String,
    pub mapping: Option<BrightnessMapping>,
}

/// The light's brightness for each group brightness 1..=100, precomputed by
/// evaluating the config's lambda once at load time.
#[derive(Debug, Clone)]
pub struct BrightnessMapping {
    table: Vec<f64>,
}

impl BrightnessMapping {
    pub fn new(table: Vec<f64>) -> Self {
        assert_eq!(table.len(), 100);
        Self { table }
    }

    /// Maps a group brightness (0–100) to the light's brightness, clamped to
    /// 1–100. Fractional input is interpolated between table entries.
    pub fn apply(&self, group_brightness: f64) -> f64 {
        let x = group_brightness.clamp(1.0, 100.0);
        let lower = x.floor() as usize;
        let upper = x.ceil() as usize;
        let at = |i: usize| self.table[i - 1];
        let value = at(lower) + (at(upper) - at(lower)) * (x - lower as f64);
        value.clamp(1.0, 100.0)
    }
}

impl GroupMember {
    pub fn brightness_for(&self, group_brightness: f64) -> f64 {
        match &self.mapping {
            Some(mapping) => mapping.apply(group_brightness),
            None => group_brightness.clamp(1.0, 100.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Room {
    pub name: String,
    pub lights: Vec<String>,
    pub sensors: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub name: String,
    pub auto: bool,
    pub proc_id: usize,
    pub line: Line,
}

#[derive(Debug, Clone)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub proc_id: usize,
    pub line: Line,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerKind {
    /// Once per minute.
    SunTick,
    /// At sunrise or sunset, shifted by `offset_minutes`.
    SunEvent { event: SunEvent, offset_minutes: i64 },
    /// Every day at the given local times.
    Clock { times: Vec<chrono::NaiveTime> },
    /// An event reported by a sensor or light.
    Device { path: String, events: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunEvent {
    Sunrise,
    Sunset,
}

/// A controllable entity as addressed by `Room/Entity` paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Light,
    Group,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub location: Option<Location>,
    pub homekit: HomeKitConfig,
    pub adapters: Vec<AdapterConfig>,
    pub rooms: Vec<Room>,
    pub lights: BTreeMap<String, Light>,
    pub sensors: BTreeMap<String, Sensor>,
    pub groups: BTreeMap<String, Group>,
    pub scenes: Vec<Scene>,
    pub triggers: Vec<Trigger>,
}

impl Model {
    pub fn entity_kind(&self, path: &str) -> Option<EntityKind> {
        if self.groups.contains_key(path) {
            Some(EntityKind::Group)
        } else if self.lights.contains_key(path) {
            Some(EntityKind::Light)
        } else {
            None
        }
    }

    /// Groups and individually exposed lights: everything HomeKit controls
    /// and that therefore has an auto/override mode.
    pub fn exposed_entities(&self) -> Vec<String> {
        let groups = self.groups.values().filter(|g| g.exposed).map(|g| g.path.clone());
        let lights = self.lights.values().filter(|l| l.exposed).map(|l| l.path.clone());
        groups.chain(lights).collect()
    }

    pub fn scene(&self, name: &str) -> Option<&Scene> {
        self.scenes.iter().find(|scene| scene.name == name)
    }

    pub fn light_by_device(&self, device: &DeviceRef) -> Option<&Light> {
        self.lights.values().find(|light| &light.device == device)
    }

    pub fn sensor_by_device(&self, device: &DeviceRef) -> Option<&Sensor> {
        self.sensors.values().find(|sensor| &sensor.device == device)
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(location) => writeln!(f, "location  {:.4}, {:.4}", location.lat, location.lon)?,
            None => writeln!(f, "location  (none, sun triggers disabled)")?,
        }
        writeln!(f, "homekit   {:?} on port {}", self.homekit.name, self.homekit.port)?;
        writeln!(f)?;
        writeln!(f, "adapters")?;
        for adapter in &self.adapters {
            match adapter {
                AdapterConfig::Hue { name, host, .. } => writeln!(f, "  hue     {name:<12} {host}")?,
                AdapterConfig::Zigbee { name, mqtt, base_topic, .. } => {
                    writeln!(f, "  zigbee  {name:<12} {mqtt} ({base_topic})")?
                }
            }
        }
        for room in &self.rooms {
            writeln!(f)?;
            writeln!(f, "room {}", room.name)?;
            for path in &room.groups {
                let group = &self.groups[path];
                let hidden = if group.exposed { "" } else { " (hidden)" };
                writeln!(f, "  group  {}{hidden}", group.name)?;
                for member in &group.members {
                    let mapping = match &member.mapping {
                        Some(mapping) => format!(
                            "  brightness 25/50/100 → {:.0}/{:.0}/{:.0}",
                            mapping.apply(25.0),
                            mapping.apply(50.0),
                            mapping.apply(100.0)
                        ),
                        None => String::new(),
                    };
                    let name = member.light.rsplit('/').next().unwrap_or(&member.light);
                    writeln!(f, "    - {name}{mapping}")?;
                }
            }
            for path in &room.lights {
                let light = &self.lights[path];
                let exposed = if light.exposed { " (exposed)" } else { "" };
                writeln!(f, "  light  {:<12} {}{exposed}", light.name, light.device)?;
            }
            for path in &room.sensors {
                let sensor = &self.sensors[path];
                writeln!(f, "  sensor {:<12} {}", sensor.name, sensor.device)?;
            }
        }
        if !self.scenes.is_empty() {
            writeln!(f)?;
            writeln!(f, "scenes")?;
            for scene in &self.scenes {
                let mode = if scene.auto { "auto" } else { "override" };
                writeln!(f, "  {:<20} {mode}", scene.name)?;
            }
        }
        if !self.triggers.is_empty() {
            writeln!(f)?;
            writeln!(f, "triggers")?;
            for trigger in &self.triggers {
                let description = match &trigger.kind {
                    TriggerKind::SunTick => "Sun, every minute".to_string(),
                    TriggerKind::SunEvent { event, offset_minutes } => {
                        format!("Sun, {event:?} {offset_minutes:+} min")
                    }
                    TriggerKind::Clock { times } => {
                        let times: Vec<String> = times.iter().map(|t| t.format("%H:%M").to_string()).collect();
                        format!("Clock at {}", times.join(", "))
                    }
                    TriggerKind::Device { path, events } => format!("{path} {}", events.join(", ")),
                };
                writeln!(f, "  {description:<36} {}", trigger.line)?;
            }
        }
        Ok(())
    }
}

/// Problems found while validating; each names its config line.
#[derive(Debug)]
pub struct ValidationErrors(pub Vec<String>);

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

// ---------------------------------------------------------------------------
// The raw JSON recorded by the prelude.

#[derive(Debug, Deserialize)]
pub struct RawModel {
    location: Option<Location>,
    homekit: Option<HomeKitConfig>,
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
    line: Line,
}

#[derive(Debug, Deserialize)]
pub struct RawMapping {
    pub light: String,
    pub proc: usize,
    pub line: Line,
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
    pub fn mapping_procs(&self) -> Vec<usize> {
        self.rooms
            .iter()
            .flat_map(|room| &room.groups)
            .flat_map(|group| &group.mappings)
            .map(|mapping| mapping.proc)
            .collect()
    }

    /// Resolves references and validates the whole model. `mapping_tables`
    /// holds the evaluated table for each id from [`Self::mapping_procs`].
    pub fn build(self, mapping_tables: &HashMap<usize, Vec<f64>>) -> Result<Model, ValidationErrors> {
        let mut errors = Vec::new();

        let mut adapter_names = HashSet::new();
        for adapter in &self.adapters {
            if !adapter_names.insert(adapter.name().to_string()) {
                errors.push(format!("{}: adapter {:?} is defined twice", adapter.line(), adapter.name()));
            }
        }

        let resolve_adapter = |device: &RawDevice, errors: &mut Vec<String>| -> Option<DeviceRef> {
            let candidates: Vec<&AdapterConfig> = self
                .adapters
                .iter()
                .filter(|adapter| adapter.kind() == device.adapter_kind)
                .filter(|adapter| device.adapter.as_deref().is_none_or(|name| adapter.name() == name))
                .collect();
            match candidates.as_slice() {
                [adapter] => Some(DeviceRef {
                    adapter: adapter.name().to_string(),
                    kind: device.adapter_kind,
                    device: device.device.clone(),
                }),
                [] => {
                    let wanted = match &device.adapter {
                        Some(name) => format!("{} adapter {name:?}", device.adapter_kind),
                        None => format!("{} adapter", device.adapter_kind),
                    };
                    errors.push(format!("{}: {:?} needs a {wanted}, but none is defined", device.line, device.name));
                    None
                }
                _ => {
                    errors.push(format!(
                        "{}: {:?}: several {} adapters are defined, pick one with adapter: \"name\"",
                        device.line, device.name, device.adapter_kind
                    ));
                    None
                }
            }
        };

        let mut rooms = Vec::new();
        let mut lights = BTreeMap::new();
        let mut sensors = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let mut room_names = HashSet::new();
        let mut grouped_lights = HashSet::new();
        let mut devices_seen: HashMap<DeviceRef, String> = HashMap::new();

        for raw_room in &self.rooms {
            if !room_names.insert(raw_room.name.clone()) {
                errors.push(format!("{}: room {:?} is defined twice", raw_room.line, raw_room.name));
                continue;
            }
            let mut room = Room {
                name: raw_room.name.clone(),
                lights: Vec::new(),
                sensors: Vec::new(),
                groups: Vec::new(),
            };
            let mut names = HashSet::new();
            let mut claim_name = |name: &str, line: &str, errors: &mut Vec<String>| {
                let unique = names.insert(name.to_string());
                if !unique {
                    errors.push(format!("{line}: {name:?} is already used in room {:?}", raw_room.name));
                }
                unique
            };

            for raw in &raw_room.lights {
                if !claim_name(&raw.name, &raw.line, &mut errors) {
                    continue;
                }
                let Some(device) = resolve_adapter(raw, &mut errors) else { continue };
                let path = format!("{}/{}", raw_room.name, raw.name);
                if let Some(other) = devices_seen.insert(device.clone(), path.clone()) {
                    errors.push(format!("{}: {device} is already used by {other}", raw.line));
                }
                room.lights.push(path.clone());
                lights.insert(
                    path.clone(),
                    Light {
                        name: raw.name.clone(),
                        path,
                        device,
                        exposed: raw.expose.unwrap_or(true),
                    },
                );
            }
            for raw in &raw_room.sensors {
                if !claim_name(&raw.name, &raw.line, &mut errors) {
                    continue;
                }
                let Some(device) = resolve_adapter(raw, &mut errors) else { continue };
                let path = format!("{}/{}", raw_room.name, raw.name);
                room.sensors.push(path.clone());
                sensors.insert(
                    path.clone(),
                    Sensor { name: raw.name.clone(), path, device },
                );
            }
            for raw in &raw_room.groups {
                if !claim_name(&raw.name, &raw.line, &mut errors) {
                    continue;
                }
                let mut members = Vec::new();
                for light in &raw.lights {
                    let path = format!("{}/{light}", raw_room.name);
                    if !lights.contains_key(&path) {
                        errors.push(format!(
                            "{}: group {:?} refers to unknown light {light:?} in room {:?}",
                            raw.line, raw.name, raw_room.name
                        ));
                        continue;
                    }
                    if members.iter().any(|m: &GroupMember| m.light == path) {
                        errors.push(format!("{}: group {:?} lists {light:?} twice", raw.line, raw.name));
                        continue;
                    }
                    grouped_lights.insert(path.clone());
                    members.push(GroupMember { light: path, mapping: None });
                }
                for mapping in &raw.mappings {
                    let path = format!("{}/{}", raw_room.name, mapping.light);
                    let Some(member) = members.iter_mut().find(|m| m.light == path) else {
                        errors.push(format!(
                            "{}: map refers to {:?}, which is not a light of group {:?}",
                            mapping.line, mapping.light, raw.name
                        ));
                        continue;
                    };
                    if member.mapping.is_some() {
                        errors.push(format!("{}: {:?} is mapped twice", mapping.line, mapping.light));
                    }
                    match mapping_tables.get(&mapping.proc) {
                        Some(table) => member.mapping = Some(BrightnessMapping::new(table.clone())),
                        None => errors.push(format!("{}: brightness mapping could not be evaluated", mapping.line)),
                    }
                }
                let path = format!("{}/{}", raw_room.name, raw.name);
                room.groups.push(path.clone());
                groups.insert(
                    path.clone(),
                    Group { name: raw.name.clone(), path, exposed: raw.expose, members },
                );
            }
            rooms.push(room);
        }

        // Grouped lights stay hidden unless the config exposes them explicitly.
        for raw_room in &self.rooms {
            for raw in &raw_room.lights {
                let path = format!("{}/{}", raw_room.name, raw.name);
                if let Some(light) = lights.get_mut(&path) {
                    light.exposed = raw.expose.unwrap_or(!grouped_lights.contains(&path));
                }
            }
        }

        let mut scenes = Vec::new();
        let mut scene_names = HashSet::new();
        for raw in self.scenes {
            if !scene_names.insert(raw.name.clone()) {
                errors.push(format!("{}: scene {:?} is defined twice", raw.line, raw.name));
                continue;
            }
            scenes.push(Scene { name: raw.name, auto: raw.auto, proc_id: raw.proc, line: raw.line });
        }

        let mut triggers = Vec::new();
        for raw in self.triggers {
            match trigger_kind(&raw, &lights, &sensors, self.location.is_some()) {
                Ok(kind) => triggers.push(Trigger { kind, proc_id: raw.proc, line: raw.line }),
                Err(message) => errors.push(format!("{}: {message}", raw.line)),
            }
        }

        let homekit = self.homekit.unwrap_or(HomeKitConfig {
            name: "Knut".into(),
            pin: None,
            port: 32000,
            host: None,
            line: String::new(),
        });
        if let Some(pin) = &homekit.pin {
            if is_trivial_pin(pin) {
                errors.push(format!("{}: HomeKit rejects trivial setup codes like {pin}", homekit.line));
            }
        }

        if !errors.is_empty() {
            return Err(ValidationErrors(errors));
        }
        Ok(Model {
            location: self.location,
            homekit,
            adapters: self.adapters,
            rooms,
            lights,
            sensors,
            groups,
            scenes,
            triggers,
        })
    }
}

fn trigger_kind(
    raw: &RawTrigger,
    lights: &BTreeMap<String, Light>,
    sensors: &BTreeMap<String, Sensor>,
    has_location: bool,
) -> Result<TriggerKind, String> {
    let allow_options = |allowed: &[&str]| -> Result<(), String> {
        match raw.options.keys().find(|key| !allowed.contains(&key.as_str())) {
            Some(key) => Err(format!("unknown option {key}: for {:?} trigger", raw.source)),
            None => Ok(()),
        }
    };
    match raw.source.as_str() {
        "Sun" => {
            if !has_location {
                return Err("Sun triggers need `location lat:, lon:`".into());
            }
            allow_options(&["offset"])?;
            let offset_minutes = match raw.options.get("offset") {
                None => 0,
                Some(value) => value.as_i64().ok_or("offset: must be whole minutes")?,
            };
            match raw.events.as_slice() {
                [event] if event == "sun" => {
                    if offset_minutes != 0 {
                        return Err("offset: only applies to :sunrise and :sunset".into());
                    }
                    Ok(TriggerKind::SunTick)
                }
                [event] if event == "sunrise" => Ok(TriggerKind::SunEvent { event: SunEvent::Sunrise, offset_minutes }),
                [event] if event == "sunset" => Ok(TriggerKind::SunEvent { event: SunEvent::Sunset, offset_minutes }),
                _ => Err("Sun triggers take exactly one of :sun, :sunrise, :sunset".into()),
            }
        }
        "Clock" => {
            allow_options(&["at"])?;
            if !raw.events.is_empty() {
                return Err("Clock triggers take no events, only at: \"HH:MM\"".into());
            }
            let values = match raw.options.get("at") {
                Some(serde_json::Value::String(time)) => vec![time.clone()],
                Some(serde_json::Value::Array(times)) => times
                    .iter()
                    .map(|time| time.as_str().map(str::to_owned).ok_or("at: must contain \"HH:MM\" strings"))
                    .collect::<Result<_, _>>()?,
                _ => return Err("Clock triggers need at: \"HH:MM\"".into()),
            };
            let times = values
                .iter()
                .map(|time| {
                    chrono::NaiveTime::parse_from_str(time, "%H:%M")
                        .map_err(|_| format!("at: {time:?} is not a valid \"HH:MM\" time"))
                })
                .collect::<Result<_, _>>()?;
            Ok(TriggerKind::Clock { times })
        }
        path => {
            allow_options(&[])?;
            if !sensors.contains_key(path) && !lights.contains_key(path) {
                return Err(format!("trigger source {path:?} is neither \"Sun\", \"Clock\" nor a known sensor or light"));
            }
            if raw.events.is_empty() {
                return Err(format!("trigger on {path:?} needs an event, e.g. :motion"));
            }
            for event in &raw.events {
                if !crate::adapter::DEVICE_EVENTS.contains(&event.as_str()) {
                    return Err(format!(
                        "unknown device event :{event}, expected one of :{}",
                        crate::adapter::DEVICE_EVENTS.join(", :")
                    ));
                }
            }
            Ok(TriggerKind::Device { path: path.to_string(), events: raw.events.clone() })
        }
    }
}

fn is_trivial_pin(pin: &str) -> bool {
    let all_same = pin.chars().all(|c| pin.starts_with(c));
    all_same || pin == "12345678" || pin == "87654321"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_clamps_and_interpolates() {
        let mapping = BrightnessMapping::new((1..=100).map(|x| x as f64 * 0.8).collect());
        assert_eq!(mapping.apply(100.0), 80.0);
        assert_eq!(mapping.apply(1.0), 1.0);
        assert!((mapping.apply(50.5) - 40.4).abs() < 1e-9);
    }
}
