//! Zigbee lights and sensors through zigbee2mqtt.
//!
//! Devices are addressed by IEEE address (`0x00158d0001a2b3c4`), which stays
//! stable when a device is renamed in zigbee2mqtt; topics use the friendly
//! name, learned from the retained `bridge/devices` message.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, watch};

use super::{
    mired_to_kelvin, AdapterEvent, DeviceClass, DiscoveredDevice, LightCommand, LightReport,
    MiredRange,
};
use crate::model::{AdapterKind, DeviceRef};

/// zigbee2mqtt's brightness scale.
const MAX_BRIGHTNESS: f64 = 254.0;

pub(crate) struct Zigbee {
    name: String,
    base_topic: String,
    client: AsyncClient,
    devices: Mutex<HashMap<String, Device>>,
    /// Flips to true once `bridge/devices` arrived.
    devices_known: watch::Sender<bool>,
}

#[derive(Debug, Clone)]
struct Device {
    ieee: String,
    friendly_name: String,
    class: Option<DeviceClass>,
    description: String,
    supports_brightness: bool,
    colour_temperature: Option<MiredRange>,
    /// Last reported occupancy, to report `clear` only on a change.
    occupancy: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct BridgeDevice {
    ieee_address: String,
    friendly_name: String,
    #[serde(rename = "type")]
    device_type: String,
    definition: Option<Definition>,
}

#[derive(Debug, Deserialize)]
struct Definition {
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    exposes: Vec<Expose>,
}

#[derive(Debug, Deserialize)]
struct Expose {
    #[serde(rename = "type")]
    expose_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    features: Vec<Expose>,
    #[serde(default)]
    value_min: Option<f64>,
    #[serde(default)]
    value_max: Option<f64>,
}

impl Device {
    fn from_bridge(device: BridgeDevice) -> Self {
        let mut class = None;
        let mut supports_brightness = false;
        let mut colour_temperature = None;
        let mut description = String::new();
        if let Some(definition) = &device.definition {
            description = format!(
                "{} {} – {}",
                definition.vendor, definition.model, definition.description
            );
            for expose in &definition.exposes {
                if expose.expose_type == "light" {
                    class = Some(DeviceClass::Light);
                    let (brightness, range) = light_features(expose);
                    supports_brightness |= brightness;
                    colour_temperature = range.or(colour_temperature);
                } else if class.is_none() && is_sensor_value(expose) {
                    class = Some(DeviceClass::Sensor);
                }
            }
        }
        Self {
            ieee: device.ieee_address,
            friendly_name: device.friendly_name,
            class,
            description,
            supports_brightness,
            colour_temperature,
            occupancy: None,
        }
    }
}

/// Whether a `light` expose offers brightness, and its colour temperature
/// range in mireds if it has one.
fn light_features(expose: &Expose) -> (bool, Option<MiredRange>) {
    let mut supports_brightness = false;
    let mut colour_temperature = None;
    for feature in &expose.features {
        match feature.name.as_deref() {
            Some("brightness") => supports_brightness = true,
            Some("color_temp") => {
                colour_temperature = Some(MiredRange {
                    min: feature.value_min.unwrap_or(153.0),
                    max: feature.value_max.unwrap_or(500.0),
                })
            }
            _ => {}
        }
    }
    (supports_brightness, colour_temperature)
}

/// Whether an expose is something a sensor reports, so that the device can
/// trigger blocks.
fn is_sensor_value(expose: &Expose) -> bool {
    matches!(
        expose.name.as_deref(),
        Some("occupancy" | "action" | "contact" | "presence")
    )
}

impl Zigbee {
    pub(crate) fn connect(
        name: &str,
        url: &str,
        base_topic: &str,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<Arc<Self>> {
        let options = mqtt_options(url, name)?;
        let (client, mut event_loop) = AsyncClient::new(options, 256);
        let zigbee = Self::new(name, base_topic, client);

        let this = Arc::clone(&zigbee);
        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        log::info!("zigbee {}: connected to MQTT", this.name);
                        let topic = format!("{}/#", this.base_topic);
                        if let Err(error) = this.client.subscribe(topic, QoS::AtLeastOnce).await {
                            log::error!("zigbee {}: subscribe failed: {error}", this.name);
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        this.handle_message(&publish.topic, &publish.payload, &events);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        log::warn!(
                            "zigbee {}: MQTT connection error: {error}; retrying in 5 s",
                            this.name
                        );
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });
        Ok(zigbee)
    }

    fn new(name: &str, base_topic: &str, client: AsyncClient) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            base_topic: base_topic.trim_end_matches('/').to_string(),
            client,
            devices: Mutex::new(HashMap::new()),
            devices_known: watch::Sender::new(false),
        })
    }

    fn handle_message(
        &self,
        topic: &str,
        payload: &[u8],
        events: &mpsc::UnboundedSender<AdapterEvent>,
    ) {
        let Some(subtopic) = topic
            .strip_prefix(&self.base_topic)
            .and_then(|t| t.strip_prefix('/'))
        else {
            return;
        };
        if subtopic == "bridge/devices" {
            self.on_device_list(payload);
            return;
        }
        if is_not_device_state(subtopic) {
            return;
        }
        let Ok(Value::Object(data)) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        self.on_device_message(subtopic, &data, events);
    }

    /// zigbee2mqtt's retained list of devices, sent at start and whenever a
    /// device joins, leaves or is renamed.
    fn on_device_list(&self, payload: &[u8]) {
        let list = match serde_json::from_slice::<Vec<BridgeDevice>>(payload) {
            Ok(list) => list,
            Err(error) => {
                log::warn!("zigbee {}: cannot parse bridge/devices: {error}", self.name);
                return;
            }
        };
        let mut devices = self.devices.lock().unwrap();
        let previous = std::mem::take(&mut *devices);
        for device in list.into_iter().filter(|d| d.device_type != "Coordinator") {
            let mut device = Device::from_bridge(device);
            device.occupancy = previous.get(&device.ieee).and_then(|d| d.occupancy);
            devices.insert(device.ieee.clone(), device);
        }
        log::info!("zigbee {}: {} devices known", self.name, devices.len());
        let first_list = !self.devices_known.send_replace(true);
        if first_list {
            self.request_light_states(devices.values());
        }
    }

    /// A device's state, on the topic of its friendly name.
    fn on_device_message(
        &self,
        friendly_name: &str,
        data: &Map<String, Value>,
        events: &mpsc::UnboundedSender<AdapterEvent>,
    ) {
        let mut devices = self.devices.lock().unwrap();
        let Some(device) = devices
            .values_mut()
            .find(|d| d.friendly_name == friendly_name)
        else {
            return;
        };
        let device_ref = self.device_ref(&device.ieee);
        if device.class == Some(DeviceClass::Light) || data.contains_key("state") {
            let _ = events.send(AdapterEvent::Light {
                device: device_ref.clone(),
                report: light_report(data),
            });
        }
        for event in device_events(device, data) {
            let _ = events.send(AdapterEvent::Device {
                device: device_ref.clone(),
                event: event.to_string(),
                data: Value::Object(data.clone()),
            });
        }
    }

    fn device_ref(&self, ieee: &str) -> DeviceRef {
        DeviceRef {
            adapter: self.name.clone(),
            kind: AdapterKind::Zigbee,
            device: ieee.to_string(),
        }
    }

    /// zigbee2mqtt does not retain device state by default, so ask each light.
    fn request_light_states<'a>(&self, devices: impl Iterator<Item = &'a Device>) {
        let payload = json!({ "state": "", "brightness": "", "color_temp": "" }).to_string();
        for device in devices.filter(|d| d.class == Some(DeviceClass::Light)) {
            let topic = format!("{}/{}/get", self.base_topic, device.friendly_name);
            if let Err(error) =
                self.client
                    .try_publish(topic, QoS::AtMostOnce, false, payload.clone())
            {
                log::debug!(
                    "zigbee {}: cannot request state of {}: {error}",
                    self.name,
                    device.friendly_name
                );
            }
        }
    }

    /// Waits until zigbee2mqtt has sent its device list, which maps IEEE
    /// addresses to topics.
    pub(crate) async fn ready(&self, timeout: Duration) -> Result<()> {
        let mut known = self.devices_known.subscribe();
        tokio::time::timeout(timeout, known.wait_for(|known| *known))
            .await
            .map_err(|_| {
                anyhow!(
                    "zigbee {}: no device list from zigbee2mqtt within {timeout:?}",
                    self.name
                )
            })??;
        Ok(())
    }

    pub(crate) async fn send(&self, ieee: &str, command: &LightCommand) -> Result<()> {
        if let Err(error) = self.ready(Duration::from_secs(5)).await {
            log::warn!("{error}; addressing {ieee} by its IEEE address");
        }
        let (topic, payload) = {
            let devices = self.devices.lock().unwrap();
            let device = devices.get(ieee);
            let friendly_name = device.map_or(ieee, |d| d.friendly_name.as_str());
            (
                format!("{}/{friendly_name}/set", self.base_topic),
                set_payload(device, command).to_string(),
            )
        };
        self.client
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .await
            .context("cannot queue MQTT message")
    }

    pub(crate) async fn discover(&self) -> Result<Vec<DiscoveredDevice>> {
        self.ready(Duration::from_secs(10)).await?;
        let devices = self.devices.lock().unwrap();
        let mut found: Vec<DiscoveredDevice> = devices
            .values()
            .filter_map(|device| {
                Some(DiscoveredDevice {
                    device: self.device_ref(&device.ieee),
                    class: device.class?,
                    name: device.friendly_name.clone(),
                    description: device.description.clone(),
                })
            })
            .collect();
        found.sort_by(|a, b| (a.class, &a.name).cmp(&(b.class, &b.name)));
        Ok(found)
    }
}

/// Topics below the base topic that carry no device state: zigbee2mqtt's
/// own, the `set` and `get` requests Knut sends, and availability.
fn is_not_device_state(subtopic: &str) -> bool {
    subtopic.starts_with("bridge/")
        || subtopic.ends_with("/set")
        || subtopic.ends_with("/get")
        || subtopic.ends_with("/availability")
}

/// The `set` message for `command`, in zigbee2mqtt's units. Values the
/// device does not support are left out. Without a known `device` (no
/// device list yet), every value is sent.
fn set_payload(device: Option<&Device>, command: &LightCommand) -> Value {
    let mut payload = Map::new();
    if let Some(on) = command.on {
        payload.insert("state".into(), json!(if on { "ON" } else { "OFF" }));
    }
    let supports_brightness = device.is_none_or(|d| d.supports_brightness);
    if let (Some(brightness), true) = (command.brightness, supports_brightness) {
        let value = (brightness.clamp(1.0, 100.0) / 100.0 * MAX_BRIGHTNESS)
            .round()
            .max(1.0);
        payload.insert("brightness".into(), json!(value as u32));
    }
    if let Some(kelvin) = command.kelvin {
        let mired = match device.map(|d| d.colour_temperature) {
            Some(Some(range)) => Some(range.clamp_kelvin(kelvin)),
            // A device without colour temperature.
            Some(None) => None,
            None => Some(super::kelvin_to_mired(kelvin)),
        };
        if let Some(mired) = mired {
            payload.insert("color_temp".into(), json!(mired.round() as u32));
        }
    }
    if let Some(transition) = command.transition {
        payload.insert("transition".into(), json!(transition.max(0.0)));
    }
    Value::Object(payload)
}

/// What a light reported, in Knut's units (brightness 0–100, kelvin).
fn light_report(data: &Map<String, Value>) -> LightReport {
    LightReport {
        on: data
            .get("state")
            .and_then(Value::as_str)
            .map(|state| state.eq_ignore_ascii_case("ON")),
        brightness: data
            .get("brightness")
            .and_then(Value::as_f64)
            .map(|brightness| brightness / MAX_BRIGHTNESS * 100.0),
        kelvin: data
            .get("color_temp")
            .and_then(Value::as_f64)
            .map(mired_to_kelvin),
    }
}

/// The trigger events a message means, in this order: `motion`, or `clear`
/// when occupancy ends; `action` for a button press; and always `change`.
/// Remembers the occupancy so that `clear` comes only once.
fn device_events(device: &mut Device, data: &Map<String, Value>) -> Vec<&'static str> {
    let mut events = Vec::new();
    if let Some(occupancy) = data.get("occupancy").and_then(Value::as_bool) {
        if occupancy {
            events.push("motion");
        } else if device.occupancy != Some(false) {
            events.push("clear");
        }
        device.occupancy = Some(occupancy);
    }
    // zigbee2mqtt follows each action with an empty one to reset it.
    let action = data.get("action").and_then(Value::as_str);
    if action.is_some_and(|action| !action.is_empty()) {
        events.push("action");
    }
    events.push("change");
    events
}

fn mqtt_options(url: &str, adapter: &str) -> Result<MqttOptions> {
    let url = url::Url::parse(url).with_context(|| format!("invalid MQTT url {url:?}"))?;
    if url.scheme() != "mqtt" && url.scheme() != "tcp" {
        bail!("MQTT url must start with mqtt://, got {url}");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("MQTT url {url} has no host"))?;
    let client_id = format!("knut-{adapter}-{}", std::process::id());
    let mut options = MqttOptions::new(client_id, host, url.port().unwrap_or(1883));
    options.set_keep_alive(Duration::from_secs(30));
    options.set_max_packet_size(4 * 1024 * 1024, 64 * 1024);
    if !url.username().is_empty() {
        options.set_credentials(url.username(), url.password().unwrap_or_default());
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumqttc::Request;

    fn bridge_devices() -> Vec<u8> {
        json!([
            {"ieee_address": "0x00", "friendly_name": "Coordinator", "type": "Coordinator"},
            {
                "ieee_address": "0x01", "friendly_name": "kitchen/spot", "type": "Router",
                "definition": {"vendor": "IKEA", "model": "LED1537R6", "description": "GU10", "exposes": [
                    {"type": "light", "features": [
                        {"type": "binary", "name": "state"},
                        {"type": "numeric", "name": "brightness", "value_max": 254},
                        {"type": "numeric", "name": "color_temp", "value_min": 250, "value_max": 454}
                    ]}
                ]}
            },
            {
                "ieee_address": "0x11", "friendly_name": "hall motion", "type": "EndDevice",
                "definition": {"vendor": "Aqara", "model": "RTCGQ11LM", "description": "Motion", "exposes": [
                    {"type": "binary", "name": "occupancy"}
                ]}
            }
        ])
        .to_string()
        .into_bytes()
    }

    fn published(requests: &flume::Receiver<Request>) -> Vec<(String, Value)> {
        requests
            .try_iter()
            .filter_map(|request| match request {
                Request::Publish(publish) => Some((
                    publish.topic.clone(),
                    serde_json::from_slice(&publish.payload).unwrap(),
                )),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn talks_zigbee2mqtt() {
        let (requests_tx, requests) = flume::unbounded();
        let zigbee = Zigbee::new("z2m", "zigbee2mqtt", AsyncClient::from_senders(requests_tx));
        let (events_tx, mut events) = mpsc::unbounded_channel();

        zigbee.handle_message("zigbee2mqtt/bridge/devices", &bridge_devices(), &events_tx);
        let state_requests = published(&requests);
        assert_eq!(state_requests.len(), 1, "asks each light for its state");
        assert_eq!(state_requests[0].0, "zigbee2mqtt/kitchen/spot/get");

        let discovered = zigbee.discover().await.unwrap();
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].class, DeviceClass::Light);

        let command = LightCommand {
            on: Some(true),
            brightness: Some(50.0),
            kelvin: Some(2000.0),
            transition: Some(2.0),
        };
        zigbee.send("0x01", &command).await.unwrap();
        let sent = published(&requests);
        assert_eq!(
            sent[0].0, "zigbee2mqtt/kitchen/spot/set",
            "addressed by friendly name"
        );
        assert_eq!(
            sent[0].1,
            json!({"state": "ON", "brightness": 127, "color_temp": 454, "transition": 2.0})
        );

        zigbee.handle_message(
            "zigbee2mqtt/kitchen/spot",
            br#"{"state":"ON","brightness":254,"color_temp":250}"#,
            &events_tx,
        );
        let Some(AdapterEvent::Light { device, report }) = events.recv().await else {
            panic!()
        };
        assert_eq!(device.device, "0x01");
        assert_eq!(
            report,
            LightReport {
                on: Some(true),
                brightness: Some(100.0),
                kelvin: Some(4000.0)
            }
        );

        zigbee.handle_message(
            "zigbee2mqtt/hall motion",
            br#"{"occupancy":true}"#,
            &events_tx,
        );
        zigbee.handle_message(
            "zigbee2mqtt/hall motion",
            br#"{"occupancy":false}"#,
            &events_tx,
        );
        zigbee.handle_message(
            "zigbee2mqtt/hall motion",
            br#"{"occupancy":false}"#,
            &events_tx,
        );
        let mut names = Vec::new();
        while let Ok(AdapterEvent::Device { event, .. }) = events.try_recv() {
            names.push(event);
        }
        assert_eq!(
            names,
            ["change", "motion", "change", "clear", "change", "change"],
            "clear only on a change"
        );
    }

    #[test]
    fn reads_capabilities_from_bridge_devices() {
        let device: BridgeDevice = serde_json::from_value(json!({
            "ieee_address": "0x01",
            "friendly_name": "Kitchen Spot",
            "type": "Router",
            "definition": {
                "vendor": "IKEA", "model": "LED1537R6", "description": "TRADFRI GU10",
                "exposes": [{"type": "light", "features": [
                    {"type": "binary", "name": "state"},
                    {"type": "numeric", "name": "brightness", "value_min": 0, "value_max": 254},
                    {"type": "numeric", "name": "color_temp", "value_min": 250, "value_max": 454}
                ]}]
            }
        }))
        .unwrap();
        let device = Device::from_bridge(device);
        assert_eq!(device.class, Some(DeviceClass::Light));
        assert!(device.supports_brightness);
        assert_eq!(
            device.colour_temperature,
            Some(MiredRange {
                min: 250.0,
                max: 454.0
            })
        );
    }

    #[test]
    fn set_payload_leaves_out_what_a_device_cannot_do() {
        let command = LightCommand {
            on: Some(true),
            brightness: Some(50.0),
            kelvin: Some(2700.0),
            transition: None,
        };
        let device = |supports_brightness, colour_temperature| Device {
            ieee: "0x01".into(),
            friendly_name: "lamp".into(),
            class: Some(DeviceClass::Light),
            description: String::new(),
            supports_brightness,
            colour_temperature,
            occupancy: None,
        };

        let dimmable = device(true, None);
        assert_eq!(
            set_payload(Some(&dimmable), &command),
            json!({"state": "ON", "brightness": 127})
        );
        let switch = device(false, None);
        assert_eq!(set_payload(Some(&switch), &command), json!({"state": "ON"}));
        assert_eq!(
            set_payload(None, &command),
            json!({"state": "ON", "brightness": 127, "color_temp": 370}),
            "an unknown device gets every value"
        );
    }
}
