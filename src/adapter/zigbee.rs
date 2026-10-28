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
/// How long to wait before reconnecting to the MQTT broker.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// One zigbee2mqtt instance, reached through its MQTT broker.
pub(crate) struct Zigbee {
    /// The adapter's name in the config.
    name: String,
    base_topic: String,
    client: AsyncClient,
    /// Every device but the coordinator, by IEEE address.
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

/// A message on `bridge/event`, such as `device_announce`.
#[derive(Debug, Deserialize)]
struct BridgeEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: BridgeEventData,
}

#[derive(Debug, Deserialize)]
struct BridgeEventData {
    ieee_address: String,
}

/// A message on `<friendly name>/availability`: `online` or `offline`.
#[derive(Debug, Deserialize)]
struct Availability {
    state: String,
}

impl Device {
    fn from_bridge(device: BridgeDevice) -> Self {
        let definition = device.definition.as_ref();
        let description = definition.map(describe).unwrap_or_default();
        let exposes = definition.map_or(&[][..], |definition| &definition.exposes);
        let capabilities = Capabilities::of(exposes);
        Self {
            ieee: device.ieee_address,
            friendly_name: device.friendly_name,
            class: capabilities.class,
            description,
            supports_brightness: capabilities.supports_brightness,
            colour_temperature: capabilities.colour_temperature,
            occupancy: None,
        }
    }
}

/// "IKEA LED1537R6 – GU10" for `knut discover`.
fn describe(definition: &Definition) -> String {
    format!(
        "{} {} – {}",
        definition.vendor, definition.model, definition.description
    )
}

/// What a device can do, read from its exposes.
#[derive(Debug, Default)]
struct Capabilities {
    /// A light if it exposes one, else a sensor if it reports something
    /// triggers can use, else neither.
    class: Option<DeviceClass>,
    supports_brightness: bool,
    colour_temperature: Option<MiredRange>,
}

impl Capabilities {
    fn of(exposes: &[Expose]) -> Self {
        let mut capabilities = Self::default();
        for expose in exposes {
            if expose.expose_type == "light" {
                let (brightness, range) = light_features(expose);
                capabilities.class = Some(DeviceClass::Light);
                capabilities.supports_brightness |= brightness;
                capabilities.colour_temperature = range.or(capabilities.colour_temperature);
            } else if capabilities.class.is_none() && is_sensor_value(expose) {
                capabilities.class = Some(DeviceClass::Sensor);
            }
        }
        capabilities
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
    /// Connects to the MQTT broker at `url` and handles zigbee2mqtt's
    /// messages in the background, reconnecting when the connection breaks.
    ///
    /// # Errors
    ///
    /// Fails for an invalid MQTT url; connection errors are retried.
    pub(crate) fn connect(
        name: &str,
        url: &str,
        base_topic: &str,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<Arc<Self>> {
        let options = mqtt_options(url, name)?;
        let (client, event_loop) = AsyncClient::new(options, 256);
        let zigbee = Self::new(name, base_topic, client);

        let listener = Arc::clone(&zigbee);
        tokio::spawn(listener.run_event_loop(event_loop, events));
        Ok(zigbee)
    }

    async fn run_event_loop(
        self: Arc<Self>,
        mut event_loop: rumqttc::EventLoop,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) {
        loop {
            match event_loop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => self.on_connected().await,
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    self.handle_message(&publish.topic, &publish.payload, &events);
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!(
                        "zigbee {}: MQTT connection error: {error}; retrying in 5 s",
                        self.name
                    );
                    tokio::time::sleep(RECONNECT_DELAY).await;
                }
            }
        }
    }

    /// Subscribes to everything below the base topic, again after every
    /// reconnect.
    async fn on_connected(&self) {
        log::info!("zigbee {}: connected to MQTT", self.name);
        let topic = format!("{}/#", self.base_topic);
        if let Err(error) = self.client.subscribe(topic, QoS::AtLeastOnce).await {
            log::error!("zigbee {}: subscribe failed: {error}", self.name);
        }
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
        if subtopic == "bridge/event" {
            self.on_bridge_event(payload, events);
            return;
        }
        if let Some(friendly_name) = subtopic.strip_suffix("/availability") {
            self.on_availability(friendly_name, payload, events);
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

    /// A light that announces itself has just got power back, for example
    /// from a wall switch. Its state is unknown until it answers.
    fn on_bridge_event(&self, payload: &[u8], events: &mpsc::UnboundedSender<AdapterEvent>) {
        let Ok(event) = serde_json::from_slice::<BridgeEvent>(payload) else {
            return;
        };
        if event.event_type != "device_announce" {
            return;
        }
        let devices = self.devices.lock().unwrap();
        let light = devices
            .get(&event.data.ieee_address)
            .filter(|device| device.class == Some(DeviceClass::Light));
        let Some(light) = light else {
            return;
        };

        // Sending fails only while the daemon shuts down.
        let _ = events.send(AdapterEvent::Unavailable {
            device: self.device_ref(&light.ieee),
        });
        self.request_light_states(std::iter::once(light));
    }

    /// zigbee2mqtt's availability check: `offline` once a light stops
    /// answering, `online` when it answers again.
    fn on_availability(
        &self,
        friendly_name: &str,
        payload: &[u8],
        events: &mpsc::UnboundedSender<AdapterEvent>,
    ) {
        let Ok(availability) = serde_json::from_slice::<Availability>(payload) else {
            return;
        };
        let devices = self.devices.lock().unwrap();
        let light = devices.values().find(|device| {
            device.friendly_name == friendly_name && device.class == Some(DeviceClass::Light)
        });
        let Some(light) = light else {
            return;
        };

        if availability.state == "online" {
            self.request_light_states(std::iter::once(light));
        } else {
            // Sending fails only while the daemon shuts down.
            let _ = events.send(AdapterEvent::Unavailable {
                device: self.device_ref(&light.ieee),
            });
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
        let is_light = device.class == Some(DeviceClass::Light) || data.contains_key("state");
        // Sending fails only while the daemon shuts down.
        if is_light {
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
        let lights = devices.filter(|device| device.class == Some(DeviceClass::Light));
        for device in lights {
            let topic = format!("{}/{}/get", self.base_topic, device.friendly_name);
            let request = self
                .client
                .try_publish(topic, QoS::AtMostOnce, false, payload.clone());
            if let Err(error) = request {
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
        let list_arrived = known.wait_for(|known| *known);
        let waited = tokio::time::timeout(timeout, list_arrived).await;
        let Ok(watched) = waited else {
            bail!(
                "zigbee {}: no device list from zigbee2mqtt within {timeout:?}",
                self.name
            );
        };
        watched?;
        Ok(())
    }

    /// Sends `command` to the device with IEEE address `ieee`.
    pub(crate) async fn send(&self, ieee: &str, command: &LightCommand) -> Result<()> {
        if let Err(error) = self.ready(Duration::from_secs(5)).await {
            log::warn!("{error}; addressing {ieee} by its IEEE address");
        }
        let (topic, payload) = self.set_message(ieee, command);
        self.client
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .await
            .context("cannot queue MQTT message")
    }

    /// The topic and payload that set `command` on `ieee`. Before the
    /// device list arrives, zigbee2mqtt also accepts the IEEE address as
    /// the topic.
    fn set_message(&self, ieee: &str, command: &LightCommand) -> (String, String) {
        let devices = self.devices.lock().unwrap();
        let device = devices.get(ieee);
        let friendly_name = device.map_or(ieee, |device| device.friendly_name.as_str());
        let topic = format!("{}/{friendly_name}/set", self.base_topic);
        let payload = set_payload(device, command).to_string();
        (topic, payload)
    }

    /// Every light and sensor zigbee2mqtt knows, lights first, by name.
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
/// own and the `set` and `get` requests Knut sends.
fn is_not_device_state(subtopic: &str) -> bool {
    subtopic.starts_with("bridge/") || subtopic.ends_with("/set") || subtopic.ends_with("/get")
}

/// The `set` message for `command`, in zigbee2mqtt's units. Values the
/// device does not support are left out. Without a known `device` (no
/// device list yet), every value is sent.
fn set_payload(device: Option<&Device>, command: &LightCommand) -> Value {
    let mut payload = Map::new();
    if let Some(on) = command.on {
        payload.insert("state".into(), json!(if on { "ON" } else { "OFF" }));
    }
    let supports_brightness = device.is_none_or(|device| device.supports_brightness);
    if let Some(brightness) = command.brightness {
        if supports_brightness {
            payload.insert("brightness".into(), json!(zigbee_brightness(brightness)));
        }
    }
    if let Some(kelvin) = command.kelvin {
        if let Some(mired) = zigbee_mired(device, kelvin) {
            payload.insert("color_temp".into(), json!(mired));
        }
    }
    if let Some(transition) = command.transition {
        payload.insert("transition".into(), json!(transition.max(0.0)));
    }
    Value::Object(payload)
}

/// zigbee2mqtt's 1–254 for a brightness of 0–100; 0 would switch off.
fn zigbee_brightness(brightness: f64) -> u32 {
    let scaled = brightness.clamp(1.0, 100.0) / 100.0 * MAX_BRIGHTNESS;
    scaled.round().max(1.0) as u32
}

/// The colour temperature in the device's range, `None` for a device
/// without colour temperature. An unknown device gets the plain value.
fn zigbee_mired(device: Option<&Device>, kelvin: f64) -> Option<u32> {
    let mired = match device {
        Some(device) => device.colour_temperature?.clamp_kelvin(kelvin),
        None => super::kelvin_to_mired(kelvin),
    };
    Some(mired.round() as u32)
}

/// What a light reported, in Knut's units (brightness 0–100, kelvin).
fn light_report(data: &Map<String, Value>) -> LightReport {
    let state = data.get("state").and_then(Value::as_str);
    let brightness = data.get("brightness").and_then(Value::as_f64);
    let mired = data.get("color_temp").and_then(Value::as_f64);
    LightReport {
        on: state.map(|state| state.eq_ignore_ascii_case("ON")),
        brightness: brightness.map(|brightness| brightness / MAX_BRIGHTNESS * 100.0),
        kelvin: mired.map(mired_to_kelvin),
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

    #[tokio::test]
    async fn a_light_that_announces_itself_or_goes_offline_is_unavailable() {
        let (requests_tx, requests) = flume::unbounded();
        let zigbee = Zigbee::new("z2m", "zigbee2mqtt", AsyncClient::from_senders(requests_tx));
        let (events_tx, mut events) = mpsc::unbounded_channel();
        zigbee.handle_message("zigbee2mqtt/bridge/devices", &bridge_devices(), &events_tx);
        published(&requests);

        let announce = json!({
            "type": "device_announce",
            "data": {"friendly_name": "kitchen/spot", "ieee_address": "0x01"}
        });
        zigbee.handle_message(
            "zigbee2mqtt/bridge/event",
            announce.to_string().as_bytes(),
            &events_tx,
        );
        let Ok(AdapterEvent::Unavailable { device }) = events.try_recv() else {
            panic!("an announce makes the light unavailable")
        };
        assert_eq!(device.device, "0x01");
        let state_requests = published(&requests);
        assert_eq!(state_requests[0].0, "zigbee2mqtt/kitchen/spot/get");

        zigbee.handle_message(
            "zigbee2mqtt/kitchen/spot/availability",
            br#"{"state":"offline"}"#,
            &events_tx,
        );
        assert!(matches!(
            events.try_recv(),
            Ok(AdapterEvent::Unavailable { .. })
        ));

        zigbee.handle_message(
            "zigbee2mqtt/kitchen/spot/availability",
            br#"{"state":"online"}"#,
            &events_tx,
        );
        assert!(events.try_recv().is_err(), "online waits for the state");
        let state_requests = published(&requests);
        assert_eq!(state_requests[0].0, "zigbee2mqtt/kitchen/spot/get");

        zigbee.handle_message(
            "zigbee2mqtt/hall motion/availability",
            br#"{"state":"offline"}"#,
            &events_tx,
        );
        assert!(events.try_recv().is_err(), "only lights become unavailable");
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
