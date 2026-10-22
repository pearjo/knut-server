//! Exposes groups, individually exposed lights, scenes and the Auto switch
//! to Apple Home as one HAP bridge.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use hap::accessory::bridge::BridgeAccessory;
use hap::accessory::lightbulb::LightbulbAccessory;
use hap::accessory::switch::SwitchAccessory;
use hap::accessory::{AccessoryCategory, AccessoryInformation};
use hap::characteristic::{CharacteristicCallbacks, HapCharacteristic};
use hap::server::{IpServer, Server};
use hap::storage::{FileStorage, Storage};
use hap::{Config, HapType, MacAddress, Pin};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::adapter::{kelvin_to_mired, mired_to_kelvin};
use crate::engine::{Change, Input, Update};
use crate::model::Model;
use crate::store::Store;

/// How long a scene switch stays on after being tapped.
const SCENE_SWITCH_ON_TIME: Duration = Duration::from_secs(1);
/// HomeKit's colour temperature range in mireds (about 7140 K to 2000 K).
const MIRED_MIN: u32 = 140;
const MIRED_MAX: u32 = 500;
const AUTO_SWITCH_KEY: &str = "switch:Auto";

/// An accessory and the flag that marks value changes as coming from Knut
/// rather than from a HomeKit controller.
#[derive(Clone)]
struct Exposed {
    accessory: hap::pointer::Accessory,
    pushing: Arc<AtomicBool>,
}

impl Exposed {
    async fn set(&self, service: HapType, characteristic: HapType, value: Value) {
        let mut accessory = self.accessory.lock().await;
        let Some(service) = accessory.get_mut_service(service) else { return };
        let Some(characteristic) = service.get_mut_characteristic(characteristic) else { return };
        // The accessory lock keeps HomeKit writes out while the flag is set.
        self.pushing.store(true, Ordering::SeqCst);
        let result = characteristic.set_value(value).await;
        self.pushing.store(false, Ordering::SeqCst);
        if let Err(error) = result {
            log::warn!("HomeKit: cannot update value: {error}");
        }
    }
}

/// Starts the HAP server. HomeKit writes arrive as [`Input`]s; engine
/// [`Update`]s are reflected in HomeKit.
pub async fn start(
    model: &Model,
    store: &mut Store,
    state_dir: &Path,
    inputs: mpsc::UnboundedSender<Input>,
    mut updates: mpsc::UnboundedReceiver<Update>,
) -> Result<()> {
    let pin = setup_code(model, store)?;
    let homekit_dir = state_dir.join("homekit");
    std::fs::create_dir_all(&homekit_dir).context("cannot create the HomeKit state directory")?;
    let mut storage = FileStorage::new(&homekit_dir).await.map_err(hap_error)?;

    let mut accessory_ids = Vec::new();
    let mut lightbulbs = Vec::new();
    let mut switches = Vec::new();
    for path in model.exposed_entities() {
        let id = store.accessory_id(&path);
        accessory_ids.push(id);
        lightbulbs.push((id, path));
    }
    for scene in &model.scenes {
        let id = store.accessory_id(&format!("scene:{}", scene.name));
        accessory_ids.push(id);
        switches.push((id, Some(scene.name.clone())));
    }
    let auto_id = store.accessory_id(AUTO_SWITCH_KEY);
    accessory_ids.push(auto_id);
    switches.push((auto_id, None));
    store.save()?;

    let mut config = match storage.load_config().await {
        Ok(config) => config,
        Err(_) => Config {
            device_id: random_device_id(),
            category: AccessoryCategory::Bridge,
            ..Default::default()
        },
    };
    config.redetermine_local_ip();
    if let Some(host) = model.homekit.host {
        config.host = host;
    }
    config.port = model.homekit.port;
    config.name = model.homekit.name.clone();
    config.pin = Pin::new(pin_digits(&pin)?).map_err(hap_error)?;
    // Accessories that left the config would linger in the Home app as
    // "No Response"; dropping them from the cache and bumping the
    // configuration number makes controllers reload the accessory list.
    let cached = storage.load_aid_cache().await.unwrap_or_default();
    let stale: Vec<u64> = cached.iter().copied().filter(|id| !accessory_ids.contains(id) && *id != 1).collect();
    if !stale.is_empty() {
        let kept: Vec<u64> = cached.into_iter().filter(|id| !stale.contains(id)).collect();
        storage.save_aid_cache(&kept).await.map_err(hap_error)?;
        config.configuration_number += 1;
    }
    storage.save_config(&config).await.map_err(hap_error)?;
    log::info!("HomeKit: {:?} on {}:{}, setup code {}", config.name, config.host, config.port, format_pin(&pin));
    let pairings = storage.count_pairings().await.map_err(hap_error)?;
    if pairings > 0 {
        log::info!("HomeKit: paired with {pairings} controller(s), not offered for pairing (`knut unpair` resets)");
    }

    let server = IpServer::new(config, storage).await.map_err(hap_error)?;
    let bridge = BridgeAccessory::new(1, information(&model.homekit.name, "bridge")).map_err(hap_error)?;
    server.add_accessory(bridge).await.map_err(hap_error)?;

    let mut entities: HashMap<String, Exposed> = HashMap::new();
    for (id, path) in lightbulbs {
        let pushing = Arc::new(AtomicBool::new(false));
        let accessory = lightbulb(id, &path, &pushing, &inputs)?;
        let pointer = server.add_accessory(accessory).await.map_err(hap_error)?;
        entities.insert(path, Exposed { accessory: pointer, pushing });
    }

    let (reset_tx, mut reset_rx) = mpsc::unbounded_channel::<u64>();
    let mut scene_switches: HashMap<u64, Exposed> = HashMap::new();
    let mut auto_switch = None;
    for (id, scene) in switches {
        let pushing = Arc::new(AtomicBool::new(false));
        let accessory = switch(id, scene.as_deref(), &pushing, &inputs, &reset_tx)?;
        let pointer = server.add_accessory(accessory).await.map_err(hap_error)?;
        let exposed = Exposed { accessory: pointer, pushing };
        match scene {
            Some(_) => {
                scene_switches.insert(id, exposed);
            }
            None => auto_switch = Some(exposed),
        }
    }
    let auto_switch = auto_switch.expect("the Auto switch is always created");

    tokio::spawn(async move {
        if let Err(error) = server.run_handle().await {
            log::error!("HomeKit server stopped: {error}");
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(update) = updates.recv() => match update {
                    Update::Entity { path, state } => {
                        let Some(exposed) = entities.get(&path) else { continue };
                        exposed.set(HapType::Lightbulb, HapType::PowerState, json!(state.on)).await;
                        let brightness = state.brightness.round().clamp(1.0, 100.0) as i32;
                        exposed.set(HapType::Lightbulb, HapType::Brightness, json!(brightness)).await;
                        if let Some(kelvin) = state.kelvin {
                            let mired = (kelvin_to_mired(kelvin).round() as u32).clamp(MIRED_MIN, MIRED_MAX);
                            exposed.set(HapType::Lightbulb, HapType::ColorTemperature, json!(mired)).await;
                        }
                    }
                    Update::Auto(all_auto) => {
                        auto_switch.set(HapType::Switch, HapType::PowerState, json!(all_auto)).await;
                    }
                },
                Some(id) = reset_rx.recv() => {
                    if let Some(exposed) = scene_switches.get(&id).cloned() {
                        tokio::spawn(async move {
                            tokio::time::sleep(SCENE_SWITCH_ON_TIME).await;
                            exposed.set(HapType::Switch, HapType::PowerState, json!(false)).await;
                        });
                    }
                }
                else => break,
            }
        }
    });
    Ok(())
}

/// Removes all stored controller pairings and the bridge identity. A bridge
/// with pairings is not offered for pairing, so one left behind by an aborted
/// pairing hides it from the Home app. The Home app also keeps hiding a
/// device id it has started pairing with, so the bridge gets a new one.
pub fn unpair(state_dir: &Path) -> Result<()> {
    let homekit_dir = state_dir.join("homekit");
    let config = homekit_dir.join("config.json");
    if config.exists() {
        std::fs::remove_file(&config).with_context(|| format!("cannot remove {}", config.display()))?;
    }
    let pairings = homekit_dir.join("pairings");
    let mut removed = 0;
    for entry in std::fs::read_dir(&pairings).into_iter().flatten() {
        let path = entry?.path();
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        removed += 1;
    }
    println!("removed {removed} HomeKit pairing(s)");
    Ok(())
}

fn lightbulb(id: u64, path: &str, pushing: &Arc<AtomicBool>, inputs: &mpsc::UnboundedSender<Input>) -> Result<LightbulbAccessory> {
    let mut accessory = LightbulbAccessory::new(id, information(&display_name(path), path)).map_err(hap_error)?;
    let service = &mut accessory.lightbulb;
    // Without these characteristics the Home app cannot offer Adaptive
    // Lighting, which would fight Knut's own auto mode.
    service.characteristic_value_active_transition_count = None;
    service.characteristic_value_transition_control = None;
    service.supported_characteristic_value_transition_configuration = None;
    service.hue = None;
    service.saturation = None;
    // The accessory information already carries the name; an empty one here
    // would override it.
    service.name = None;

    let send = |change: fn(f64) -> Change| {
        let pushing = Arc::clone(pushing);
        let inputs = inputs.clone();
        let path = path.to_string();
        move |value: f64| {
            if !pushing.load(Ordering::SeqCst) {
                let _ = inputs.send(Input::Entity { path: path.clone(), change: change(value) });
            }
        }
    };
    let on = send(|value| Change::On(value > 0.5));
    service.power_state.on_update(Some(move |_: &bool, new: &bool| {
        on(if *new { 1.0 } else { 0.0 });
        Ok(())
    }));
    if let Some(brightness) = service.brightness.as_mut() {
        let send_brightness = send(Change::Brightness);
        brightness.on_update(Some(move |_: &i32, new: &i32| {
            send_brightness(*new as f64);
            Ok(())
        }));
    }
    if let Some(colour_temperature) = service.color_temperature.as_mut() {
        colour_temperature.set_min_value(Some(json!(MIRED_MIN))).map_err(hap_error)?;
        colour_temperature.set_max_value(Some(json!(MIRED_MAX))).map_err(hap_error)?;
        let send_kelvin = send(Change::Kelvin);
        colour_temperature.on_update(Some(move |_: &i32, new: &i32| {
            send_kelvin(mired_to_kelvin(*new as f64));
            Ok(())
        }));
    }
    Ok(accessory)
}

/// A scene switch (`scene` set) or the Auto switch.
fn switch(
    id: u64,
    scene: Option<&str>,
    pushing: &Arc<AtomicBool>,
    inputs: &mpsc::UnboundedSender<Input>,
    reset: &mpsc::UnboundedSender<u64>,
) -> Result<SwitchAccessory> {
    let name = scene.unwrap_or("Auto");
    let mut accessory = SwitchAccessory::new(id, information(name, name)).map_err(hap_error)?;
    accessory.switch.name = None;
    let pushing = Arc::clone(pushing);
    let inputs = inputs.clone();
    let reset = reset.clone();
    let scene = scene.map(str::to_owned);
    accessory.switch.power_state.on_update(Some(move |_: &bool, new: &bool| {
        if pushing.load(Ordering::SeqCst) {
            return Ok(());
        }
        match &scene {
            Some(scene) if *new => {
                let _ = inputs.send(Input::Scene(scene.clone()));
                let _ = reset.send(id);
            }
            Some(_) => {}
            None => {
                let _ = inputs.send(Input::AutoSwitch(*new));
            }
        }
        Ok(())
    }));
    Ok(accessory)
}

fn information(name: &str, serial: &str) -> AccessoryInformation {
    AccessoryInformation {
        name: name.to_string(),
        manufacturer: "Knut".into(),
        model: "Knut 2".into(),
        serial_number: serial.to_string(),
        // Required by HAP; the Home app stalls while adding accessories
        // without it.
        firmware_revision: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    }
}

/// "Küche/Spots" becomes "Küche Spots".
fn display_name(path: &str) -> String {
    path.replace('/', " ")
}

/// The configured setup code, or one generated once and kept in the store.
fn setup_code(model: &Model, store: &mut Store) -> Result<String> {
    if let Some(pin) = &model.homekit.pin {
        return Ok(pin.clone());
    }
    if let Some(pin) = &store.state.homekit_pin {
        return Ok(pin.clone());
    }
    let pin = loop {
        let digits: String = (0..8).map(|_| char::from(b'0' + (rand_byte() % 10))).collect();
        if Pin::new(pin_digits(&digits)?).is_ok() {
            break digits;
        }
    };
    store.state.homekit_pin = Some(pin.clone());
    store.save()?;
    Ok(pin)
}

fn pin_digits(pin: &str) -> Result<[u8; 8]> {
    let digits: Vec<u8> = pin.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    digits.try_into().map_err(|_| anyhow!("the HomeKit setup code needs 8 digits"))
}

fn format_pin(pin: &str) -> String {
    format!("{}-{}-{}", &pin[0..3], &pin[3..5], &pin[5..8])
}

fn random_device_id() -> MacAddress {
    let bytes: [u8; 6] = std::array::from_fn(|_| rand_byte());
    MacAddress::from(bytes)
}

fn rand_byte() -> u8 {
    let mut byte = [0u8; 1];
    getrandom::fill(&mut byte).expect("the OS provides randomness");
    byte[0]
}

fn hap_error(error: hap::Error) -> anyhow::Error {
    anyhow!("HomeKit: {error}")
}
