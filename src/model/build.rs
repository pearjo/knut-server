//! Building the model from the JSON the prelude records, and validating it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::Deserialize;

use super::{
    AdapterConfig, AdapterKind, BrightnessMapping, Daylight, DeviceRef, Group, GroupMember,
    HomeKitConfig, Light, Line, Location, Model, Room, Scene, ScheduleDay, Sensor, Staging,
    SunEvent, Trigger, TriggerKind,
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

// ---------------------------------------------------------------------------
// The raw JSON recorded by the prelude.

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
pub(crate) struct RawMapping {
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
        let mut errors = Vec::new();

        let mut adapter_names = HashSet::new();
        for adapter in &self.adapters {
            if !adapter_names.insert(adapter.name().to_string()) {
                errors.push(format!(
                    "{}: adapter {:?} is defined twice",
                    adapter.line(),
                    adapter.name()
                ));
            }
        }

        let resolve_adapter = |device: &RawDevice, errors: &mut Vec<String>| -> Option<DeviceRef> {
            let candidates: Vec<&AdapterConfig> = self
                .adapters
                .iter()
                .filter(|adapter| adapter.kind() == device.adapter_kind)
                .filter(|adapter| {
                    device
                        .adapter
                        .as_deref()
                        .is_none_or(|name| adapter.name() == name)
                })
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
                    errors.push(format!(
                        "{}: {:?} needs a {wanted}, but none is defined",
                        device.line, device.name
                    ));
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
                errors.push(format!(
                    "{}: room {:?} is defined twice",
                    raw_room.line, raw_room.name
                ));
                continue;
            }
            let mut room = Room {
                name: raw_room.name.clone(),
                lights: Vec::new(),
                sensors: Vec::new(),
                groups: Vec::new(),
                daylight: raw_room.daylight.clone(),
            };
            let mut names = HashSet::new();
            let mut claim_name = |name: &str, line: &str, errors: &mut Vec<String>| {
                let unique = names.insert(name.to_string());
                if !unique {
                    errors.push(format!(
                        "{line}: {name:?} is already used in room {:?}",
                        raw_room.name
                    ));
                }
                unique
            };

            for raw in &raw_room.lights {
                if !claim_name(&raw.name, &raw.line, &mut errors) {
                    continue;
                }
                let Some(device) = resolve_adapter(raw, &mut errors) else {
                    continue;
                };
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
                let Some(device) = resolve_adapter(raw, &mut errors) else {
                    continue;
                };
                let path = format!("{}/{}", raw_room.name, raw.name);
                room.sensors.push(path.clone());
                sensors.insert(
                    path.clone(),
                    Sensor {
                        name: raw.name.clone(),
                        path,
                        device,
                    },
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
                        errors.push(format!(
                            "{}: group {:?} lists {light:?} twice",
                            raw.line, raw.name
                        ));
                        continue;
                    }
                    grouped_lights.insert(path.clone());
                    members.push(GroupMember {
                        light: path,
                        mapping: None,
                    });
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
                        errors.push(format!(
                            "{}: {:?} is mapped twice",
                            mapping.line, mapping.light
                        ));
                    }
                    match mapping_tables.get(&mapping.proc) {
                        Some(table) => member.mapping = Some(BrightnessMapping::new(table.clone())),
                        None => errors.push(format!(
                            "{}: brightness mapping could not be evaluated",
                            mapping.line
                        )),
                    }
                }
                let stages = raw.stages.as_ref().map(|raw_stages| {
                    let stages: Vec<Vec<String>> = raw_stages
                        .lights
                        .iter()
                        .map(|stage| {
                            stage
                                .iter()
                                .map(|light| format!("{}/{light}", raw_room.name))
                                .collect()
                        })
                        .collect();
                    let staged: Vec<&String> = stages.iter().flatten().collect();
                    for light in &staged {
                        if !members.iter().any(|m| &m.light == *light) {
                            let name = light.rsplit('/').next().unwrap_or(light);
                            errors.push(format!(
                                "{}: stages refer to {name:?}, which is not a light of group {:?}",
                                raw_stages.line, raw.name
                            ));
                        }
                    }
                    for member in &members {
                        if staged
                            .iter()
                            .filter(|light| **light == &member.light)
                            .count()
                            != 1
                        {
                            let name = member.light.rsplit('/').next().unwrap_or(&member.light);
                            errors.push(format!(
                                "{}: stages must list {name:?} exactly once",
                                raw_stages.line
                            ));
                        }
                    }
                    Staging::Stages(stages)
                });
                let custom = raw.staging.as_ref().map(|raw| Staging::Custom {
                    proc_id: raw.proc,
                    line: raw.line.clone(),
                });
                let path = format!("{}/{}", raw_room.name, raw.name);
                room.groups.push(path.clone());
                groups.insert(
                    path.clone(),
                    Group {
                        name: raw.name.clone(),
                        path,
                        exposed: raw.expose,
                        members,
                        staging: stages.or(custom),
                    },
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
                errors.push(format!(
                    "{}: scene {:?} is defined twice",
                    raw.line, raw.name
                ));
                continue;
            }
            scenes.push(Scene {
                name: raw.name,
                auto: raw.auto,
                proc_id: raw.proc,
                line: raw.line,
            });
        }

        let mut triggers = Vec::new();
        for raw in self.triggers {
            match trigger_kind(&raw, &lights, &sensors, self.location.is_some()) {
                Ok(kind) => triggers.push(Trigger {
                    kind,
                    proc_id: raw.proc,
                    line: raw.line,
                }),
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
                errors.push(format!(
                    "{}: HomeKit rejects trivial setup codes like {pin}",
                    homekit.line
                ));
            }
        }

        if !errors.is_empty() {
            return Err(ValidationErrors(errors));
        }
        Ok(Model {
            location: self.location,
            homekit,
            schedule: self.schedule,
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
        match raw
            .options
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
        {
            Some(key) => Err(format!(
                "unknown option {key}: for {:?} trigger",
                raw.source
            )),
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
                [event] if event == "sunrise" => Ok(TriggerKind::SunEvent {
                    event: SunEvent::Sunrise,
                    offset_minutes,
                }),
                [event] if event == "sunset" => Ok(TriggerKind::SunEvent {
                    event: SunEvent::Sunset,
                    offset_minutes,
                }),
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
                    .map(|time| {
                        time.as_str()
                            .map(str::to_owned)
                            .ok_or("at: must contain \"HH:MM\" strings")
                    })
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
            Ok(TriggerKind::Device {
                path: path.to_string(),
                events: raw.events.clone(),
            })
        }
    }
}

fn is_trivial_pin(pin: &str) -> bool {
    let all_same = pin.chars().all(|c| pin.starts_with(c));
    all_same || pin == "12345678" || pin == "87654321"
}
