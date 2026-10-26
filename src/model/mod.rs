//! The configured home: adapters, rooms, lights, groups, scenes, triggers.
//!
//! Built from the JSON the DSL prelude records while the config loads, then
//! validated as a whole so that `knut check` can report every problem.

mod build;
mod daylight;
mod display;

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

pub(crate) use build::{RawModel, ValidationErrors};
pub(crate) use daylight::Daylight;

/// Source location in the config, e.g. `config.rb:12`.
pub(crate) type Line = String;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Location {
    pub lat: f64,
    pub lon: f64,
}

/// Wake and bed time of one weekday in minutes after that day's midnight.
/// A bedtime after midnight is 1440 or more and still belongs to this day.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub(crate) struct ScheduleDay {
    pub wake: u32,
    pub bed: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HomeKitConfig {
    pub name: String,
    pub pin: Option<String>,
    pub port: u16,
    /// Address to serve on; defaults to the first non-loopback interface.
    pub host: Option<std::net::IpAddr>,
    pub line: Line,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum AdapterConfig {
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
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Hue { name, .. } | Self::Zigbee { name, .. } => name,
        }
    }

    pub(crate) fn kind(&self) -> AdapterKind {
        match self {
            Self::Hue { .. } => AdapterKind::Hue,
            Self::Zigbee { .. } => AdapterKind::Zigbee,
        }
    }

    pub(crate) fn line(&self) -> &str {
        match self {
            Self::Hue { line, .. } | Self::Zigbee { line, .. } => line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AdapterKind {
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
pub(crate) struct DeviceRef {
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
pub(crate) struct Light {
    pub name: String,
    pub path: String,
    pub device: DeviceRef,
    pub exposed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Sensor {
    pub name: String,
    pub path: String,
    pub device: DeviceRef,
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub name: String,
    pub path: String,
    pub exposed: bool,
    pub members: Vec<GroupMember>,
    pub staging: Option<Staging>,
}

/// How a group decides which of its lights are on. See DESIGN.md, "Staging".
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Staging {
    /// Light paths per stage, in the order staging switches them on. Every
    /// member is in exactly one stage.
    Stages(Vec<Vec<String>>),
    /// A `staging { |need, now| … }` block that picks the lights itself.
    Custom { proc_id: usize, line: Line },
}

#[derive(Debug, Clone)]
pub(crate) struct GroupMember {
    /// Path of the member light.
    pub light: String,
    pub mapping: Option<BrightnessMapping>,
}

/// The light's brightness for each group brightness 1..=100, precomputed by
/// evaluating the config's lambda once at load time.
#[derive(Debug, Clone)]
pub(crate) struct BrightnessMapping {
    table: Vec<f64>,
}

impl BrightnessMapping {
    pub(crate) fn new(table: Vec<f64>) -> Self {
        assert_eq!(table.len(), 100);
        Self { table }
    }

    /// Maps a group brightness (0–100) to the light's brightness, clamped to
    /// 1–100. Fractional input is interpolated between table entries.
    pub(crate) fn apply(&self, group_brightness: f64) -> f64 {
        let x = group_brightness.clamp(1.0, 100.0);
        let lower = x.floor() as usize;
        let upper = x.ceil() as usize;
        let at = |i: usize| self.table[i - 1];
        let value = at(lower) + (at(upper) - at(lower)) * (x - lower as f64);
        value.clamp(1.0, 100.0)
    }
}

impl GroupMember {
    pub(crate) fn brightness_for(&self, group_brightness: f64) -> f64 {
        match &self.mapping {
            Some(mapping) => mapping.apply(group_brightness),
            None => group_brightness.clamp(1.0, 100.0),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Room {
    pub name: String,
    pub lights: Vec<String>,
    pub sensors: Vec<String>,
    pub groups: Vec<String>,
    pub daylight: Option<Daylight>,
}

#[derive(Debug, Clone)]
pub(crate) struct Scene {
    pub name: String,
    pub auto: bool,
    pub proc_id: usize,
    pub line: Line,
}

#[derive(Debug, Clone)]
pub(crate) struct Trigger {
    pub kind: TriggerKind,
    pub proc_id: usize,
    pub line: Line,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TriggerKind {
    /// Once per minute.
    SunTick,
    /// At sunrise or sunset, shifted by `offset_minutes`.
    SunEvent {
        event: SunEvent,
        offset_minutes: i64,
    },
    /// Every day at the given local times.
    Clock { times: Vec<chrono::NaiveTime> },
    /// An event reported by a sensor or light.
    Device { path: String, events: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SunEvent {
    Sunrise,
    Sunset,
}

/// A controllable entity as addressed by `Room/Entity` paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityKind {
    Light,
    Group,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub location: Option<Location>,
    pub homekit: HomeKitConfig,
    /// Indexed by `Weekday::num_days_from_sunday`.
    pub schedule: Option<[ScheduleDay; 7]>,
    pub adapters: Vec<AdapterConfig>,
    pub rooms: Vec<Room>,
    pub lights: BTreeMap<String, Light>,
    pub sensors: BTreeMap<String, Sensor>,
    pub groups: BTreeMap<String, Group>,
    pub scenes: Vec<Scene>,
    pub triggers: Vec<Trigger>,
}

impl Model {
    pub(crate) fn entity_kind(&self, path: &str) -> Option<EntityKind> {
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
    pub(crate) fn exposed_entities(&self) -> Vec<String> {
        let groups = self
            .groups
            .values()
            .filter(|g| g.exposed)
            .map(|g| g.path.clone());
        let lights = self
            .lights
            .values()
            .filter(|l| l.exposed)
            .map(|l| l.path.clone());
        groups.chain(lights).collect()
    }

    pub(crate) fn room(&self, name: &str) -> Option<&Room> {
        self.rooms.iter().find(|room| room.name == name)
    }

    pub(crate) fn scene(&self, name: &str) -> Option<&Scene> {
        self.scenes.iter().find(|scene| scene.name == name)
    }

    pub(crate) fn light_by_device(&self, device: &DeviceRef) -> Option<&Light> {
        self.lights.values().find(|light| &light.device == device)
    }

    pub(crate) fn sensor_by_device(&self, device: &DeviceRef) -> Option<&Sensor> {
        self.sensors
            .values()
            .find(|sensor| &sensor.device == device)
    }

    /// The triggers listening for `event` from the sensor or light at `path`.
    pub(crate) fn device_triggers<'a>(
        &'a self,
        path: &'a str,
        event: &'a str,
    ) -> impl Iterator<Item = &'a Trigger> {
        self.triggers
            .iter()
            .filter(move |trigger| match &trigger.kind {
                TriggerKind::Device {
                    path: source,
                    events,
                } => source == path && events.iter().any(|listened| listened == event),
                _ => false,
            })
    }
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
