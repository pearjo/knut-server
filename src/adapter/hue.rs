//! Philips Hue through the bridge's local CLIP v2 API.
//!
//! Lights are addressed by their v2 `light` resource id, sensors by their
//! `motion` or `button` resource id. State changes arrive on the bridge's
//! server-sent event stream.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{
    mired_to_kelvin, AdapterEvent, DeviceClass, DiscoveredDevice, LightCommand, LightReport,
    MiredRange,
};
use crate::model::{AdapterKind, DeviceRef};

/// The bridge handles about ten light commands per second; more get dropped.
const COMMAND_SPACING: Duration = Duration::from_millis(100);

pub(crate) struct Bridge {
    name: String,
    base_url: String,
    key: String,
    http: reqwest::Client,
    colour_ranges: Mutex<HashMap<String, Option<MiredRange>>>,
    send_lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Deserialize)]
struct Response<T> {
    #[serde(default)]
    errors: Vec<ApiError>,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    description: String,
}

#[derive(Debug, Deserialize)]
struct LightResource {
    id: String,
    #[serde(default)]
    metadata: Option<Metadata>,
    #[serde(default)]
    color_temperature: Option<ColourTemperature>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ColourTemperature {
    mirek_schema: Option<MirekSchema>,
}

#[derive(Debug, Deserialize)]
struct MirekSchema {
    mirek_minimum: f64,
    mirek_maximum: f64,
}

#[derive(Debug, Deserialize)]
struct SensorResource {
    id: String,
    owner: Owner,
    #[serde(default)]
    metadata: Option<ButtonMetadata>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    rid: String,
}

#[derive(Debug, Deserialize)]
struct ButtonMetadata {
    control_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DeviceResource {
    id: String,
    metadata: Metadata,
    product_data: Option<ProductData>,
}

#[derive(Debug, Deserialize)]
struct ProductData {
    product_name: String,
}

impl Bridge {
    pub(crate) fn connect(
        name: &str,
        host: &str,
        key: &str,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Arc<Self> {
        let bridge = Arc::new(Self {
            name: name.to_string(),
            base_url: format!("https://{host}"),
            key: key.to_string(),
            http: http_client(),
            colour_ranges: Mutex::new(HashMap::new()),
            send_lock: tokio::sync::Mutex::new(()),
        });
        let this = Arc::clone(&bridge);
        tokio::spawn(async move { this.run_event_stream(events).await });
        bridge
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, resource: &str) -> Result<Vec<T>> {
        let response: Response<T> = self
            .http
            .get(format!("{}/clip/v2/resource/{resource}", self.base_url))
            .header("hue-application-key", &self.key)
            .send()
            .await
            .with_context(|| format!("hue {}: cannot reach the bridge", self.name))?
            .error_for_status()?
            .json()
            .await?;
        if let Some(error) = response.errors.first() {
            bail!("hue {}: {}", self.name, error.description);
        }
        Ok(response.data)
    }

    async fn refresh_colour_ranges(&self) -> Result<()> {
        let lights: Vec<LightResource> = self.get("light").await?;
        let mut ranges = self.colour_ranges.lock().unwrap();
        for light in lights {
            let range = light
                .color_temperature
                .and_then(|ct| ct.mirek_schema)
                .map(|schema| MiredRange {
                    min: schema.mirek_minimum,
                    max: schema.mirek_maximum,
                });
            ranges.insert(light.id, range);
        }
        Ok(())
    }

    pub(crate) async fn send(&self, light_id: &str, command: &LightCommand) -> Result<()> {
        if !self.colour_ranges.lock().unwrap().contains_key(light_id) {
            if let Err(error) = self.refresh_colour_ranges().await {
                log::warn!("{error:#}");
            }
        }
        let range = self
            .colour_ranges
            .lock()
            .unwrap()
            .get(light_id)
            .copied()
            .flatten();
        let body = request_body(command, range);

        let _spacing = self.send_lock.lock().await;
        let response = self
            .http
            .put(format!(
                "{}/clip/v2/resource/light/{light_id}",
                self.base_url
            ))
            .header("hue-application-key", &self.key)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("hue {}: cannot reach the bridge", self.name))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            bail!("hue {}: light {light_id}: {status} {text}", self.name);
        }
        tokio::time::sleep(COMMAND_SPACING).await;
        Ok(())
    }

    pub(crate) async fn discover(&self) -> Result<Vec<DiscoveredDevice>> {
        let devices: Vec<DeviceResource> = self.get("device").await?;
        let device_names: HashMap<&str, (&str, &str)> = devices
            .iter()
            .map(|d| {
                let product = d
                    .product_data
                    .as_ref()
                    .map_or("", |p| p.product_name.as_str());
                (d.id.as_str(), (d.metadata.name.as_str(), product))
            })
            .collect();
        let device_ref = |id: &str| DeviceRef {
            adapter: self.name.clone(),
            kind: AdapterKind::Hue,
            device: id.to_string(),
        };

        let mut found = Vec::new();
        for light in self.get::<LightResource>("light").await? {
            let name = light.metadata.map(|m| m.name).unwrap_or_default();
            found.push(DiscoveredDevice {
                device: device_ref(&light.id),
                class: DeviceClass::Light,
                name,
                description: "Hue light".into(),
            });
        }
        for (resource, label) in [("motion", "motion sensor"), ("button", "button")] {
            for sensor in self.get::<SensorResource>(resource).await? {
                let (name, product) = device_names
                    .get(sensor.owner.rid.as_str())
                    .copied()
                    .unwrap_or(("", ""));
                let name = match sensor.metadata.and_then(|m| m.control_id) {
                    Some(control) => format!("{name} {control}"),
                    None => name.to_string(),
                };
                found.push(DiscoveredDevice {
                    device: device_ref(&sensor.id),
                    class: DeviceClass::Sensor,
                    name,
                    description: format!("{product} {label}"),
                });
            }
        }
        Ok(found)
    }

    async fn run_event_stream(&self, events: mpsc::UnboundedSender<AdapterEvent>) {
        loop {
            match self.read_event_stream(&events).await {
                Ok(()) => log::warn!("hue {}: event stream ended; reconnecting", self.name),
                Err(error) => log::warn!(
                    "hue {}: event stream failed: {error:#}; reconnecting in 5 s",
                    self.name
                ),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn read_event_stream(&self, events: &mpsc::UnboundedSender<AdapterEvent>) -> Result<()> {
        let response = self
            .http
            .get(format!("{}/eventstream/clip/v2", self.base_url))
            .header("hue-application-key", &self.key)
            .header("Accept", "text/event-stream")
            .timeout(Duration::from_secs(24 * 3600))
            .send()
            .await?
            .error_for_status()?;
        log::info!("hue {}: listening to events", self.name);
        // The stream only carries changes, so start from the current state.
        let lights: Vec<Value> = self.get("light").await?;
        for light in &lights {
            for event in parse_resource_update(&self.name, light) {
                let _ = events.send(event);
            }
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buffer.find("\n\n") {
                let message: String = buffer.drain(..end + 2).collect();
                for line in message.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        self.handle_events(data.trim(), events);
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_events(&self, data: &str, events: &mpsc::UnboundedSender<AdapterEvent>) {
        let Ok(batches) = serde_json::from_str::<Vec<Value>>(data) else {
            log::debug!("hue {}: unparseable event {data}", self.name);
            return;
        };
        for batch in batches {
            if batch["type"] != "update" {
                continue;
            }
            for resource in batch["data"].as_array().into_iter().flatten() {
                for event in parse_resource_update(&self.name, resource) {
                    let _ = events.send(event);
                }
            }
        }
    }
}

/// The CLIP v2 body that sets `command` on a light. Colour temperature is
/// only sent to lights with a known `range`.
fn request_body(command: &LightCommand, range: Option<MiredRange>) -> Value {
    let mut body = serde_json::Map::new();
    if let Some(on) = command.on {
        body.insert("on".into(), json!({ "on": on }));
    }
    if let Some(brightness) = command.brightness {
        body.insert(
            "dimming".into(),
            json!({ "brightness": brightness.clamp(1.0, 100.0) }),
        );
    }
    if let (Some(kelvin), Some(range)) = (command.kelvin, range) {
        body.insert(
            "color_temperature".into(),
            json!({ "mirek": range.clamp_kelvin(kelvin).round() as u32 }),
        );
    }
    if let Some(transition) = command.transition {
        body.insert(
            "dynamics".into(),
            json!({ "duration": (transition.max(0.0) * 1000.0).round() as u64 }),
        );
    }
    Value::Object(body)
}

/// The events one resource of an `update` stands for.
fn parse_resource_update(bridge: &str, resource: &Value) -> Vec<AdapterEvent> {
    let Some(id) = resource["id"].as_str() else {
        return Vec::new();
    };
    let device = DeviceRef {
        adapter: bridge.to_string(),
        kind: AdapterKind::Hue,
        device: id.to_string(),
    };
    match resource["type"].as_str() {
        Some("light") => vec![AdapterEvent::Light {
            device,
            report: light_report(resource),
        }],
        Some("motion") => motion_events(device, resource),
        Some("button") => button_events(device, resource),
        _ => Vec::new(),
    }
}

fn light_report(resource: &Value) -> LightReport {
    LightReport {
        on: resource["on"]["on"].as_bool(),
        brightness: resource["dimming"]["brightness"].as_f64(),
        kelvin: resource["color_temperature"]["mirek"]
            .as_f64()
            .map(mired_to_kelvin),
    }
}

/// `motion` or `clear`, then `change`.
fn motion_events(device: DeviceRef, resource: &Value) -> Vec<AdapterEvent> {
    let event = match resource["motion"]["motion"].as_bool() {
        Some(true) => "motion",
        Some(false) => "clear",
        None => return Vec::new(),
    };
    device_events(&device, &[event, "change"], resource)
}

/// `action` and `change`, with the button's event as `data["action"]`.
fn button_events(device: DeviceRef, resource: &Value) -> Vec<AdapterEvent> {
    let action = resource["button"]["button_report"]["event"]
        .as_str()
        .or_else(|| resource["button"]["last_event"].as_str());
    let Some(action) = action else {
        return Vec::new();
    };
    let mut data = resource.clone();
    data["action"] = json!(action);
    device_events(&device, &["action", "change"], &data)
}

fn device_events(device: &DeviceRef, events: &[&str], data: &Value) -> Vec<AdapterEvent> {
    events
        .iter()
        .map(|event| AdapterEvent::Device {
            device: device.clone(),
            event: event.to_string(),
            data: data.clone(),
        })
        .collect()
}

/// The bridge presents a certificate signed by Signify's own CA for its
/// bridge id, which no system trust store contains.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("the HTTP client configuration is valid")
}

fn key_path(state_dir: &Path, bridge: &str) -> PathBuf {
    state_dir.join(format!("hue-{bridge}.key"))
}

pub(crate) fn load_key(state_dir: &Path, bridge: &str) -> Result<Option<String>> {
    match std::fs::read_to_string(key_path(state_dir, bridge)) {
        Ok(key) => Ok(Some(key.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("cannot read hue key"),
    }
}

/// Asks the bridge for an application key. Fails until the link button on
/// the bridge has been pressed (within the last 30 seconds).
pub(crate) async fn pair(state_dir: &Path, bridge: &str, host: &str) -> Result<String> {
    let response: Vec<Value> = http_client()
        .post(format!("https://{host}/api"))
        .json(&json!({ "devicetype": "knut#server", "generateclientkey": true }))
        .send()
        .await
        .with_context(|| format!("hue {bridge}: cannot reach {host}"))?
        .json()
        .await?;
    let first = response
        .first()
        .ok_or_else(|| anyhow!("hue {bridge}: empty pairing response"))?;
    if let Some(key) = first["success"]["username"].as_str() {
        std::fs::create_dir_all(state_dir).context("cannot create the state directory")?;
        std::fs::write(key_path(state_dir, bridge), key).context("cannot store hue key")?;
        return Ok(key.to_string());
    }
    let description = first["error"]["description"]
        .as_str()
        .unwrap_or("unknown error");
    bail!("hue {bridge}: {description}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::kelvin_to_mired;

    #[test]
    fn parses_light_and_motion_updates() {
        let light = json!({
            "id": "abc", "type": "light",
            "on": {"on": true}, "dimming": {"brightness": 42.0}
        });
        let events = parse_resource_update("bridge", &light);
        let AdapterEvent::Light { report, .. } = &events[0] else {
            panic!()
        };
        assert_eq!(report.on, Some(true));
        assert_eq!(report.brightness, Some(42.0));

        let motion = json!({"id": "m", "type": "motion", "motion": {"motion": true}});
        let events = parse_resource_update("bridge", &motion);
        assert!(matches!(&events[0], AdapterEvent::Device { event, .. } if event == "motion"));
    }

    #[test]
    fn button_presses_report_their_action() {
        let button = json!({
            "id": "b", "type": "button",
            "button": {"button_report": {"event": "short_release"}}
        });
        let events = parse_resource_update("bridge", &button);
        let names: Vec<&str> = events
            .iter()
            .map(|event| match event {
                AdapterEvent::Device { event, data, .. } => {
                    assert_eq!(data["action"], "short_release");
                    event.as_str()
                }
                AdapterEvent::Light { .. } => panic!("{event:?}"),
            })
            .collect();
        assert_eq!(names, ["action", "change"]);
    }

    #[test]
    fn request_body_sends_colour_only_to_lights_that_have_it() {
        let command = LightCommand {
            on: Some(true),
            brightness: Some(120.0),
            kelvin: Some(2700.0),
            transition: Some(1.5),
        };
        let range = MiredRange {
            min: 153.0,
            max: 454.0,
        };
        assert_eq!(
            request_body(&command, Some(range)),
            json!({
                "on": {"on": true},
                "dimming": {"brightness": 100.0},
                "color_temperature": {"mirek": 370},
                "dynamics": {"duration": 1500}
            })
        );
        assert_eq!(
            request_body(&command, None),
            json!({
                "on": {"on": true},
                "dimming": {"brightness": 100.0},
                "dynamics": {"duration": 1500}
            })
        );
    }

    #[test]
    fn kelvin_round_trips_through_mirek() {
        assert!((mired_to_kelvin(kelvin_to_mired(2700.0)) - 2700.0).abs() < 1e-6);
    }
}
