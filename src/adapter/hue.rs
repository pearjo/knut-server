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
/// How long to wait before reconnecting a broken event stream.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
/// Hue sensors and their description in `knut discover`.
const SENSOR_RESOURCES: [(&str, &str); 2] = [("motion", "motion sensor"), ("button", "button")];

/// One Hue bridge, reached over HTTPS with its application key.
pub(crate) struct Bridge {
    /// The adapter's name in the config.
    name: String,
    base_url: String,
    key: String,
    http: reqwest::Client,
    /// Colour temperature range per light id; `None` for lights without
    /// colour temperature. Filled on the first command to an unknown light.
    colour_ranges: Mutex<HashMap<String, Option<MiredRange>>>,
    /// Held while a command is sent and spaced, so commands queue up.
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

/// A device's name and product, by device id.
type DeviceNames<'a> = HashMap<&'a str, (&'a str, &'a str)>;

impl Bridge {
    /// Connects to the bridge at `host` and listens to its events in the
    /// background, reconnecting when the stream breaks.
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
        let listener = Arc::clone(&bridge);
        tokio::spawn(async move { listener.run_event_stream(events).await });
        bridge
    }

    /// Sends `command` to the light with resource id `light_id`.
    pub(crate) async fn send(&self, light_id: &str, command: &LightCommand) -> Result<()> {
        let range = self.colour_range(light_id).await;
        let body = request_body(command, range);

        let _spacing = self.send_lock.lock().await;
        self.put_light(light_id, &body).await?;
        tokio::time::sleep(COMMAND_SPACING).await;
        Ok(())
    }

    /// Every light and sensor the bridge knows.
    pub(crate) async fn discover(&self) -> Result<Vec<DiscoveredDevice>> {
        let mut found = self.discover_lights().await?;
        let devices: Vec<DeviceResource> = self.get("device").await?;
        let device_names = device_names(&devices);
        for (resource, label) in SENSOR_RESOURCES {
            let sensors = self
                .discover_sensors(resource, label, &device_names)
                .await?;
            found.extend(sensors);
        }
        Ok(found)
    }

    async fn discover_lights(&self) -> Result<Vec<DiscoveredDevice>> {
        let lights: Vec<LightResource> = self.get("light").await?;
        let found = lights
            .into_iter()
            .map(|light| DiscoveredDevice {
                device: hue_device(&self.name, &light.id),
                class: DeviceClass::Light,
                name: light
                    .metadata
                    .map(|metadata| metadata.name)
                    .unwrap_or_default(),
                description: "Hue light".into(),
            })
            .collect();
        Ok(found)
    }

    /// The sensors of one `resource` type. They are named after the device
    /// they belong to, and buttons also after their number on it.
    async fn discover_sensors(
        &self,
        resource: &str,
        label: &str,
        device_names: &DeviceNames<'_>,
    ) -> Result<Vec<DiscoveredDevice>> {
        let sensors: Vec<SensorResource> = self.get(resource).await?;
        let mut found = Vec::new();
        for sensor in sensors {
            let owner = sensor.owner.rid.as_str();
            let (name, product) = device_names.get(owner).copied().unwrap_or(("", ""));
            let button_number = sensor.metadata.and_then(|metadata| metadata.control_id);
            let name = match button_number {
                Some(number) => format!("{name} {number}"),
                None => name.to_string(),
            };
            found.push(DiscoveredDevice {
                device: hue_device(&self.name, &sensor.id),
                class: DeviceClass::Sensor,
                name,
                description: format!("{product} {label}"),
            });
        }
        Ok(found)
    }

    /// Reads every resource of type `resource`.
    async fn get<T: for<'de> Deserialize<'de>>(&self, resource: &str) -> Result<Vec<T>> {
        let url = format!("{}/clip/v2/resource/{resource}", self.base_url);
        let response = self
            .http
            .get(url)
            .header("hue-application-key", &self.key)
            .send()
            .await
            .with_context(|| format!("hue {}: cannot reach the bridge", self.name))?;
        let response = response.error_for_status()?;
        let body: Response<T> = response.json().await?;

        if let Some(error) = body.errors.first() {
            bail!("hue {}: {}", self.name, error.description);
        }
        Ok(body.data)
    }

    async fn put_light(&self, light_id: &str, body: &Value) -> Result<()> {
        let url = format!("{}/clip/v2/resource/light/{light_id}", self.base_url);
        let response = self
            .http
            .put(url)
            .header("hue-application-key", &self.key)
            .json(body)
            .send()
            .await
            .with_context(|| format!("hue {}: cannot reach the bridge", self.name))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            bail!("hue {}: light {light_id}: {status} {text}", self.name);
        }
        Ok(())
    }

    /// The light's colour temperature range, read from the bridge the first
    /// time it is needed. `None` for lights without colour temperature, or
    /// when the bridge cannot tell.
    async fn colour_range(&self, light_id: &str) -> Option<MiredRange> {
        let known = self.colour_ranges.lock().unwrap().contains_key(light_id);
        if !known {
            if let Err(error) = self.refresh_colour_ranges().await {
                log::warn!("{error:#}");
            }
        }
        let ranges = self.colour_ranges.lock().unwrap();
        ranges.get(light_id).copied().flatten()
    }

    async fn refresh_colour_ranges(&self) -> Result<()> {
        let lights: Vec<LightResource> = self.get("light").await?;
        let mut ranges = self.colour_ranges.lock().unwrap();
        for light in lights {
            let schema = light.color_temperature.and_then(|ct| ct.mirek_schema);
            let range = schema.map(|schema| MiredRange {
                min: schema.mirek_minimum,
                max: schema.mirek_maximum,
            });
            ranges.insert(light.id, range);
        }
        Ok(())
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
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }

    /// Reports the current light states, then every update until the
    /// stream ends.
    async fn read_event_stream(&self, events: &mpsc::UnboundedSender<AdapterEvent>) -> Result<()> {
        let response = self.open_event_stream().await?;
        log::info!("hue {}: listening to events", self.name);
        // The stream only carries changes, so start from the current state.
        self.report_current_lights(events).await?;

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            // Server-sent events end with an empty line.
            while let Some(end) = buffer.find("\n\n") {
                let message: String = buffer.drain(..end + 2).collect();
                self.handle_message(&message, events);
            }
        }
        Ok(())
    }

    async fn open_event_stream(&self) -> Result<reqwest::Response> {
        // The bridge keeps the stream open; only a day of silence is a
        // broken connection.
        let timeout = Duration::from_secs(24 * 3600);
        let response = self
            .http
            .get(format!("{}/eventstream/clip/v2", self.base_url))
            .header("hue-application-key", &self.key)
            .header("Accept", "text/event-stream")
            .timeout(timeout)
            .send()
            .await?;
        Ok(response.error_for_status()?)
    }

    async fn report_current_lights(
        &self,
        events: &mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<()> {
        let lights: Vec<Value> = self.get("light").await?;
        for light in &lights {
            send_events(events, parse_resource_update(&self.name, light));
        }
        Ok(())
    }

    /// One server-sent event: its `data:` lines hold batches of updates.
    fn handle_message(&self, message: &str, events: &mpsc::UnboundedSender<AdapterEvent>) {
        for line in message.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                self.handle_batches(data.trim(), events);
            }
        }
    }

    fn handle_batches(&self, data: &str, events: &mpsc::UnboundedSender<AdapterEvent>) {
        let Ok(batches) = serde_json::from_str::<Vec<Value>>(data) else {
            log::debug!("hue {}: unparseable event {data}", self.name);
            return;
        };
        for batch in batches {
            if batch["type"] != "update" {
                continue;
            }
            for resource in batch["data"].as_array().into_iter().flatten() {
                send_events(events, parse_resource_update(&self.name, resource));
            }
        }
    }
}

fn send_events(events: &mpsc::UnboundedSender<AdapterEvent>, batch: Vec<AdapterEvent>) {
    for event in batch {
        // Fails only while the daemon shuts down.
        let _ = events.send(event);
    }
}

fn device_names(devices: &[DeviceResource]) -> DeviceNames<'_> {
    devices
        .iter()
        .map(|device| {
            let product = device
                .product_data
                .as_ref()
                .map_or("", |product| product.product_name.as_str());
            let name = device.metadata.name.as_str();
            (device.id.as_str(), (name, product))
        })
        .collect()
}

fn hue_device(bridge: &str, id: &str) -> DeviceRef {
    DeviceRef {
        adapter: bridge.to_string(),
        kind: AdapterKind::Hue,
        device: id.to_string(),
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
        let brightness = brightness.clamp(1.0, 100.0);
        body.insert("dimming".into(), json!({ "brightness": brightness }));
    }
    if let (Some(kelvin), Some(range)) = (command.kelvin, range) {
        let mirek = range.clamp_kelvin(kelvin).round() as u32;
        body.insert("color_temperature".into(), json!({ "mirek": mirek }));
    }
    if let Some(transition) = command.transition {
        let milliseconds = (transition.max(0.0) * 1000.0).round() as u64;
        body.insert("dynamics".into(), json!({ "duration": milliseconds }));
    }
    Value::Object(body)
}

/// The events one resource of an `update` stands for.
fn parse_resource_update(bridge: &str, resource: &Value) -> Vec<AdapterEvent> {
    let Some(id) = resource["id"].as_str() else {
        return Vec::new();
    };
    let device = hue_device(bridge, id);
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
    let mirek = resource["color_temperature"]["mirek"].as_f64();
    LightReport {
        on: resource["on"]["on"].as_bool(),
        brightness: resource["dimming"]["brightness"].as_f64(),
        kelvin: mirek.map(mired_to_kelvin),
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

/// The application key `pair` stored for `bridge`, if there is one.
///
/// # Errors
///
/// Fails when the key file exists but cannot be read.
pub(crate) fn load_key(state_dir: &Path, bridge: &str) -> Result<Option<String>> {
    match std::fs::read_to_string(key_path(state_dir, bridge)) {
        Ok(key) => Ok(Some(key.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("cannot read hue key"),
    }
}

/// Asks the bridge for an application key and stores it.
///
/// # Errors
///
/// Fails until the link button on the bridge has been pressed (within the
/// last 30 seconds), and when the key cannot be stored.
pub(crate) async fn pair(state_dir: &Path, bridge: &str, host: &str) -> Result<String> {
    let request = json!({ "devicetype": "knut#server", "generateclientkey": true });
    let response = http_client()
        .post(format!("https://{host}/api"))
        .json(&request)
        .send()
        .await
        .with_context(|| format!("hue {bridge}: cannot reach {host}"))?;
    let answers: Vec<Value> = response.json().await?;
    let answer = answers
        .first()
        .ok_or_else(|| anyhow!("hue {bridge}: empty pairing response"))?;

    if let Some(key) = answer["success"]["username"].as_str() {
        std::fs::create_dir_all(state_dir).context("cannot create the state directory")?;
        std::fs::write(key_path(state_dir, bridge), key).context("cannot store hue key")?;
        return Ok(key.to_string());
    }
    let description = answer["error"]["description"]
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
            panic!("expected a light report, got {events:?}")
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
                AdapterEvent::Light { .. } | AdapterEvent::Unavailable { .. } => {
                    panic!("{event:?}")
                }
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
