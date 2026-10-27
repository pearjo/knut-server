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
use crate::engine::{Change, EntityState, Input, Update};
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
    fn new(accessory: hap::pointer::Accessory, pushing: Arc<AtomicBool>) -> Self {
        Self { accessory, pushing }
    }

    async fn set(&self, service: HapType, characteristic: HapType, value: Value) {
        let mut accessory = self.accessory.lock().await;
        let Some(service) = accessory.get_mut_service(service) else {
            return;
        };
        let Some(characteristic) = service.get_mut_characteristic(characteristic) else {
            return;
        };
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
pub(crate) async fn start(
    model: &Model,
    store: &mut Store,
    state_dir: &Path,
    inputs: mpsc::UnboundedSender<Input>,
    updates: mpsc::UnboundedReceiver<Update>,
) -> Result<()> {
    let pin = setup_code(model, store)?;
    let plan = plan_accessories(model, store)?;
    let homekit_dir = state_dir.join("homekit");
    std::fs::create_dir_all(&homekit_dir).context("cannot create the HomeKit state directory")?;
    let mut storage = FileStorage::new(&homekit_dir).await.map_err(hap_error)?;

    let config = server_config(&mut storage, model, &pin, &plan.ids()).await?;
    log::info!(
        "HomeKit: {:?} on {}:{}, setup code {}",
        config.name,
        config.host,
        config.port,
        format_pin(&pin)
    );
    let pairings = storage.count_pairings().await.map_err(hap_error)?;
    if pairings > 0 {
        log::info!(
            "HomeKit: paired with {pairings} controller(s), not offered for pairing \
             (`knut unpair` resets)"
        );
    }

    let server = IpServer::new(config, storage).await.map_err(hap_error)?;
    let (resets, reset_requests) = mpsc::unbounded_channel();
    let accessories = add_accessories(&server, model, plan, &inputs, &resets).await?;
    tokio::spawn(async move {
        if let Err(error) = server.run_handle().await {
            log::error!("HomeKit server stopped: {error}");
        }
    });
    tokio::spawn(forward_updates(accessories, updates, reset_requests));
    Ok(())
}

/// The accessories HomeKit gets, each with the id the store keeps for it.
struct AccessoryPlan {
    /// Exposed groups and lights, by entity path.
    lightbulbs: Vec<(u64, String)>,
    /// Scene switches, by scene name.
    scenes: Vec<(u64, String)>,
    auto_switch: u64,
}

impl AccessoryPlan {
    fn ids(&self) -> Vec<u64> {
        let lightbulbs = self.lightbulbs.iter().map(|(id, _)| *id);
        let scenes = self.scenes.iter().map(|(id, _)| *id);
        lightbulbs.chain(scenes).chain([self.auto_switch]).collect()
    }
}

fn plan_accessories(model: &Model, store: &mut Store) -> Result<AccessoryPlan> {
    let lightbulbs = model
        .exposed_entities()
        .into_iter()
        .map(|path| (store.accessory_id(&path), path))
        .collect();
    let scenes = model
        .scenes
        .iter()
        .map(|scene| {
            let id = store.accessory_id(&format!("scene:{}", scene.name));
            (id, scene.name.clone())
        })
        .collect();
    let auto_switch = store.accessory_id(AUTO_SWITCH_KEY);
    store.save()?;
    Ok(AccessoryPlan {
        lightbulbs,
        scenes,
        auto_switch,
    })
}

/// The bridge's HAP configuration: the stored one with a new random device
/// id the first time, updated from the config and saved.
async fn server_config(
    storage: &mut FileStorage,
    model: &Model,
    pin: &str,
    accessory_ids: &[u64],
) -> Result<Config> {
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
    config.name = homekit_name(&model.homekit.name);
    // Matches the bridge's Protocol Information service (1.1.0).
    config.protocol_version = "1.1".into();
    config.pin = Pin::new(pin_digits(pin)?).map_err(hap_error)?;
    forget_removed_accessories(storage, &mut config, accessory_ids).await?;
    storage.save_config(&config).await.map_err(hap_error)?;
    Ok(config)
}

/// Accessories that left the config would linger in the Home app as "No
/// Response"; dropping them from the cache and bumping the configuration
/// number makes controllers reload the accessory list.
async fn forget_removed_accessories(
    storage: &mut FileStorage,
    config: &mut Config,
    accessory_ids: &[u64],
) -> Result<()> {
    const BRIDGE_ID: u64 = 1;
    let cached = storage.load_aid_cache().await.unwrap_or_default();
    let removed: Vec<u64> = cached
        .iter()
        .copied()
        .filter(|id| !accessory_ids.contains(id) && *id != BRIDGE_ID)
        .collect();
    if removed.is_empty() {
        return Ok(());
    }
    let kept: Vec<u64> = cached
        .into_iter()
        .filter(|id| !removed.contains(id))
        .collect();
    storage.save_aid_cache(&kept).await.map_err(hap_error)?;
    config.configuration_number += 1;
    Ok(())
}

/// The accessories Knut keeps updating, once they are on the server.
struct ExposedAccessories {
    /// Lightbulbs by entity path.
    entities: HashMap<String, Exposed>,
    /// Scene switches by accessory id.
    scenes: HashMap<u64, Exposed>,
    auto_switch: Exposed,
}

/// Adds the bridge and every planned accessory to `server`. HomeKit writes
/// go to `inputs`; a tapped scene switch asks `resets` to turn it off again.
async fn add_accessories(
    server: &IpServer,
    model: &Model,
    plan: AccessoryPlan,
    inputs: &mpsc::UnboundedSender<Input>,
    resets: &mpsc::UnboundedSender<u64>,
) -> Result<ExposedAccessories> {
    let mut bridge =
        BridgeAccessory::new(1, information(&model.homekit.name, "bridge")).map_err(hap_error)?;
    bridge
        .protocol_information
        .version
        .set_value(json!("1.1.0"))
        .await
        .map_err(hap_error)?;
    server.add_accessory(bridge).await.map_err(hap_error)?;

    let mut entities = HashMap::new();
    for (id, path) in plan.lightbulbs {
        let pushing = Arc::new(AtomicBool::new(false));
        let accessory = lightbulb(id, &path, &pushing, inputs)?;
        let pointer = server.add_accessory(accessory).await.map_err(hap_error)?;
        entities.insert(path, Exposed::new(pointer, pushing));
    }
    let mut scenes = HashMap::new();
    for (id, scene) in plan.scenes {
        let pushing = Arc::new(AtomicBool::new(false));
        let accessory = switch(id, Some(&scene), &pushing, inputs, resets)?;
        let pointer = server.add_accessory(accessory).await.map_err(hap_error)?;
        scenes.insert(id, Exposed::new(pointer, pushing));
    }
    let pushing = Arc::new(AtomicBool::new(false));
    let accessory = switch(plan.auto_switch, None, &pushing, inputs, resets)?;
    let pointer = server.add_accessory(accessory).await.map_err(hap_error)?;
    Ok(ExposedAccessories {
        entities,
        scenes,
        auto_switch: Exposed::new(pointer, pushing),
    })
}

/// Shows the engine's updates in HomeKit, and turns tapped scene switches
/// off again after a moment.
async fn forward_updates(
    accessories: ExposedAccessories,
    mut updates: mpsc::UnboundedReceiver<Update>,
    mut reset_requests: mpsc::UnboundedReceiver<u64>,
) {
    loop {
        tokio::select! {
            Some(update) = updates.recv() => match update {
                Update::Entity { path, state } => {
                    if let Some(exposed) = accessories.entities.get(&path) {
                        show_entity_state(exposed, &state).await;
                    }
                }
                Update::Auto(all_auto) => {
                    let switch = &accessories.auto_switch;
                    switch.set(HapType::Switch, HapType::PowerState, json!(all_auto)).await;
                }
            },
            Some(id) = reset_requests.recv() => {
                if let Some(exposed) = accessories.scenes.get(&id).cloned() {
                    tokio::spawn(async move {
                        tokio::time::sleep(SCENE_SWITCH_ON_TIME).await;
                        exposed.set(HapType::Switch, HapType::PowerState, json!(false)).await;
                    });
                }
            }
            else => break,
        }
    }
}

async fn show_entity_state(exposed: &Exposed, state: &EntityState) {
    let lightbulb = HapType::Lightbulb;
    exposed
        .set(lightbulb, HapType::PowerState, json!(state.on))
        .await;
    let brightness = state.brightness.round().clamp(1.0, 100.0) as i32;
    exposed
        .set(lightbulb, HapType::Brightness, json!(brightness))
        .await;
    if let Some(kelvin) = state.kelvin {
        let mired = (kelvin_to_mired(kelvin).round() as u32).clamp(MIRED_MIN, MIRED_MAX);
        exposed
            .set(lightbulb, HapType::ColorTemperature, json!(mired))
            .await;
    }
}

/// Removes all stored controller pairings and the bridge identity. A bridge
/// with pairings is not offered for pairing, so one left behind by an aborted
/// pairing hides it from the Home app. The Home app also keeps hiding a
/// device id it has started pairing with, so the bridge gets a new one.
pub(crate) fn unpair(state_dir: &Path) -> Result<()> {
    let homekit_dir = state_dir.join("homekit");
    let config = homekit_dir.join("config.json");
    if config.exists() {
        std::fs::remove_file(&config)
            .with_context(|| format!("cannot remove {}", config.display()))?;
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

fn lightbulb(
    id: u64,
    path: &str,
    pushing: &Arc<AtomicBool>,
    inputs: &mpsc::UnboundedSender<Input>,
) -> Result<LightbulbAccessory> {
    let mut accessory =
        LightbulbAccessory::new(id, information(&display_name(path), path)).map_err(hap_error)?;
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
                let _ = inputs.send(Input::Entity {
                    path: path.clone(),
                    change: change(value),
                });
            }
        }
    };
    let on = send(|value| Change::On(value > 0.5));
    service
        .power_state
        .on_update(Some(move |_: &bool, new: &bool| {
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
        colour_temperature
            .set_min_value(Some(json!(MIRED_MIN)))
            .map_err(hap_error)?;
        colour_temperature
            .set_max_value(Some(json!(MIRED_MAX)))
            .map_err(hap_error)?;
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
    accessory
        .switch
        .power_state
        .on_update(Some(move |_: &bool, new: &bool| {
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
        name: homekit_name(name),
        manufacturer: "Knut".into(),
        model: "Knut 2".into(),
        serial_number: serial.to_string(),
        // Required by HAP, as plain numbers: the Home app rejects the bridge
        // as "out of compliance" for a version like "2.0.0-dev".
        firmware_revision: Some(
            concat!(
                env!("CARGO_PKG_VERSION_MAJOR"),
                ".",
                env!("CARGO_PKG_VERSION_MINOR"),
                ".",
                env!("CARGO_PKG_VERSION_PATCH")
            )
            .into(),
        ),
        ..Default::default()
    }
}

/// "Kitchen/Spots" becomes "Kitchen Spots".
fn display_name(path: &str) -> String {
    path.replace('/', " ")
}

/// The Home app only accepts names of letters, digits, spaces and
/// apostrophes that start and end with a letter or digit.
fn homekit_name(name: &str) -> String {
    let allowed: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = allowed.split_whitespace().collect();
    let joined = words.join(" ");
    let trimmed = joined.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        "Knut".to_string()
    } else {
        trimmed.to_string()
    }
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
        let digits: String = (0..8)
            .map(|_| char::from(b'0' + (rand_byte() % 10)))
            .collect();
        if Pin::new(pin_digits(&digits)?).is_ok() {
            break digits;
        }
    };
    store.state.homekit_pin = Some(pin.clone());
    store.save()?;
    Ok(pin)
}

fn pin_digits(pin: &str) -> Result<[u8; 8]> {
    let digits: Vec<u8> = pin
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    digits
        .try_into()
        .map_err(|_| anyhow!("the HomeKit setup code needs 8 digits"))
}

/// The code as the Home app asks for it, e.g. "1112-2333".
fn format_pin(pin: &str) -> String {
    let digits: String = pin.chars().filter(char::is_ascii_digit).collect();
    format!("{}-{}", &digits[..4], &digits[4..])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_setup_code_from_its_digits() {
        assert_eq!(format_pin("11122333"), "1112-2333");
        assert_eq!(format_pin("111-22-333"), "1112-2333");
    }

    #[test]
    fn makes_names_acceptable_to_the_home_app() {
        assert_eq!(
            homekit_name("Café Spots"),
            "Café Spots",
            "letters beyond ASCII"
        );
        assert_eq!(homekit_name("Joe's Office Light"), "Joe's Office Light");
        assert_eq!(
            homekit_name("Living Room - Ceiling (old)"),
            "Living Room Ceiling old"
        );
        assert_eq!(homekit_name("'Hallway'"), "Hallway");
        assert_eq!(homekit_name("--"), "Knut");
    }
}
