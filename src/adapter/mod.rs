//! The boundary between Knut and the devices: each adapter turns light
//! commands into its protocol and reports state and events back.

pub(crate) mod hue;
pub(crate) mod zigbee;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

use crate::model::{AdapterConfig, AdapterKind, DeviceRef, Model};

/// Device events triggers can listen for.
pub(crate) const DEVICE_EVENTS: &[&str] = &["motion", "clear", "action", "change"];

/// What to change on a light. Unset fields are left alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LightCommand {
    pub on: Option<bool>,
    /// 1–100.
    pub brightness: Option<f64>,
    pub kelvin: Option<f64>,
    /// Seconds.
    pub transition: Option<f64>,
}

impl LightCommand {
    /// Switches off and changes nothing else.
    pub(crate) fn off() -> Self {
        Self {
            on: Some(false),
            ..Default::default()
        }
    }
}

/// State as reported by a device. Unset fields were not part of the report.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LightReport {
    pub on: Option<bool>,
    /// 0–100.
    pub brightness: Option<f64>,
    pub kelvin: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) enum AdapterEvent {
    Light {
        device: DeviceRef,
        report: LightReport,
    },
    /// `event` is one of [`DEVICE_EVENTS`]; `data` is the device's raw payload.
    Device {
        device: DeviceRef,
        event: String,
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DeviceClass {
    Light,
    Sensor,
}

/// A device an adapter knows about, for `knut discover`.
#[derive(Debug, Clone)]
pub(crate) struct DiscoveredDevice {
    pub device: DeviceRef,
    pub class: DeviceClass,
    /// The name the device has in its own ecosystem.
    pub name: String,
    pub description: String,
}

pub(crate) fn kelvin_to_mired(kelvin: f64) -> f64 {
    1_000_000.0 / kelvin.max(1.0)
}

pub(crate) fn mired_to_kelvin(mired: f64) -> f64 {
    1_000_000.0 / mired.max(1.0)
}

/// Supported colour temperature range of a light, in mireds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MiredRange {
    pub min: f64,
    pub max: f64,
}

impl MiredRange {
    pub(crate) fn clamp_kelvin(&self, kelvin: f64) -> f64 {
        kelvin_to_mired(kelvin).clamp(self.min, self.max)
    }
}

#[derive(Clone)]
pub(crate) enum Adapter {
    Hue(Arc<hue::Bridge>),
    Zigbee(Arc<zigbee::Zigbee>),
}

impl Adapter {
    pub(crate) async fn send(&self, device: &str, command: &LightCommand) -> Result<()> {
        match self {
            Self::Hue(bridge) => bridge.send(device, command).await,
            Self::Zigbee(zigbee) => zigbee.send(device, command).await,
        }
    }

    pub(crate) async fn discover(&self) -> Result<Vec<DiscoveredDevice>> {
        match self {
            Self::Hue(bridge) => bridge.discover().await,
            Self::Zigbee(zigbee) => zigbee.discover().await,
        }
    }
}

/// All configured adapters, by name.
#[derive(Clone)]
pub(crate) struct Adapters {
    adapters: HashMap<String, Adapter>,
}

impl Adapters {
    /// Connects every adapter in `model`. Adapters keep reconnecting in the
    /// background; state and events arrive on `events`.
    pub(crate) async fn connect(
        model: &Model,
        state_dir: &Path,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<Self> {
        let mut adapters = HashMap::new();
        for config in &model.adapters {
            let adapter = match config {
                AdapterConfig::Hue {
                    name, host, key, ..
                } => {
                    let key = match key {
                        Some(key) => key.clone(),
                        None => hue::load_key(state_dir, name)?.ok_or_else(|| {
                            anyhow!("hue bridge {name:?} is not paired yet; run `knut discover` and press the link button")
                        })?,
                    };
                    Adapter::Hue(hue::Bridge::connect(name, host, &key, events.clone()))
                }
                AdapterConfig::Zigbee {
                    name,
                    mqtt,
                    base_topic,
                    ..
                } => Adapter::Zigbee(zigbee::Zigbee::connect(
                    name,
                    mqtt,
                    base_topic,
                    events.clone(),
                )?),
            };
            adapters.insert(config.name().to_string(), adapter);
        }
        Ok(Self { adapters })
    }

    /// No adapters; commands fail and are logged. For tests of the logic above.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Adapter> {
        self.adapters.get(name)
    }

    pub(crate) async fn send(&self, device: &DeviceRef, command: &LightCommand) -> Result<()> {
        let adapter = self
            .get(&device.adapter)
            .ok_or_else(|| anyhow!("unknown adapter {:?}", device.adapter))?;
        adapter.send(&device.device, command).await
    }
}

/// Snippet to paste into the config for a discovered device.
pub(crate) fn dsl_snippet(device: &DiscoveredDevice, model: &Model) -> String {
    let several_of_kind = model
        .adapters
        .iter()
        .filter(|a| a.kind() == device.device.kind)
        .count()
        > 1;
    let adapter = if several_of_kind {
        format!(", adapter: {:?}", device.device.adapter)
    } else {
        String::new()
    };
    let keyword = match device.class {
        DeviceClass::Light => "light",
        DeviceClass::Sensor => "sensor",
    };
    let kind = match device.device.kind {
        AdapterKind::Hue => "hue",
        AdapterKind::Zigbee => "zigbee",
    };
    format!(
        "{keyword} {:?}, {kind}: {:?}{adapter}",
        suggested_name(&device.name),
        device.device.device
    )
}

fn suggested_name(name: &str) -> String {
    let name = name.replace('/', " ");
    let name = name.trim();
    if name.is_empty() {
        "unnamed".into()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_colour_temperature() {
        assert_eq!(kelvin_to_mired(2500.0), 400.0);
        assert_eq!(mired_to_kelvin(250.0), 4000.0);
        let range = MiredRange {
            min: 153.0,
            max: 454.0,
        };
        assert_eq!(range.clamp_kelvin(2000.0), 454.0);
    }
}
