//! Exposes groups, individually exposed lights, scenes and the Auto switch
//! to Apple Home as one HAP bridge.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use hap::accessory::bridge::BridgeAccessory;
use hap::accessory::lightbulb::LightbulbAccessory;
use hap::accessory::switch::SwitchAccessory;
use hap::accessory::{AccessoryCategory, AccessoryInformation, HapAccessory};
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
/// HAP reserves accessory id 1 for the bridge itself.
const BRIDGE_ID: u64 = 1;
/// The HAP version the bridge announces over mDNS…
const PROTOCOL_VERSION: &str = "1.1";
/// …and the same version in its Protocol Information service.
const PROTOCOL_INFORMATION_VERSION: &str = "1.1.0";
/// Required by HAP, as plain numbers: the Home app rejects the bridge as
/// "out of compliance" for a version like "2.0.0-dev".
const FIRMWARE_REVISION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    ".",
    env!("CARGO_PKG_VERSION_PATCH")
);

/// Starts the HAP server. HomeKit writes arrive as [`Input`]s; engine
/// [`Update`]s are reflected in HomeKit.
///
/// # Errors
///
/// Fails when the setup code is invalid, the state directory cannot be
/// written or the HAP server cannot be set up.
pub(crate) async fn start(
    model: &Model,
    store: &mut Store,
    state_dir: &Path,
    inputs: mpsc::UnboundedSender<Input>,
    updates: mpsc::UnboundedReceiver<Update>,
) -> Result<()> {
    let setup_code = setup_code(model, store)?;
    let plan = plan_accessories(model, store)?;
    let mut storage = open_storage(state_dir).await?;

    let config = server_config(&mut storage, model, &setup_code, &plan.ids()).await?;
    log::info!(
        "HomeKit: {:?} on {}:{}, setup code {setup_code}",
        config.name,
        config.host,
        config.port,
    );
    log_existing_pairings(&storage).await?;

    let server = IpServer::new(config, storage).await.map_err(hap_error)?;
    let (resets, reset_requests) = mpsc::unbounded_channel();
    let accessories = add_accessories(&server, model, plan, &inputs, &resets).await?;
    tokio::spawn(serve(server));
    tokio::spawn(forward_updates(accessories, updates, reset_requests));
    Ok(())
}

/// Removes all stored controller pairings and the bridge identity. A bridge
/// with pairings is not offered for pairing, so one left behind by an aborted
/// pairing hides it from the Home app. The Home app also keeps hiding a
/// device id it has started pairing with, so the bridge gets a new one.
///
/// # Errors
///
/// Fails when a stored file cannot be removed.
pub(crate) fn unpair(state_dir: &Path) -> Result<()> {
    let homekit_dir = homekit_dir(state_dir);
    remove_bridge_identity(&homekit_dir)?;
    let removed = remove_pairings(&homekit_dir)?;
    println!("removed {removed} HomeKit pairing(s)");
    Ok(())
}

/// The eight-digit code that pairs the bridge. It always holds a code HAP
/// accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SetupCode {
    digits: [u8; 8],
}

impl SetupCode {
    /// Reads a code from its digits; anything between them, such as the
    /// dashes in "1112-2333", is ignored.
    ///
    /// # Errors
    ///
    /// Fails unless there are exactly 8 digits, or when HAP rejects the
    /// code as too easy to guess, such as 12345678.
    fn parse(code: &str) -> Result<Self> {
        let digits: Vec<u8> = code
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|digit| digit - b'0')
            .collect();
        let digits: [u8; 8] = digits
            .try_into()
            .map_err(|_| anyhow!("the HomeKit setup code needs 8 digits, got {code:?}"))?;
        Pin::new(digits).map_err(hap_error)?;
        Ok(Self { digits })
    }

    /// A random code that HAP accepts.
    fn random() -> Self {
        loop {
            let digits = std::array::from_fn(|_| random_byte() % 10);
            if Pin::new(digits).is_ok() {
                return Self { digits };
            }
        }
    }

    /// The code as hap's [`Pin`].
    fn pin(&self) -> Pin {
        Pin::new(self.digits).expect("a SetupCode only holds codes HAP accepts")
    }

    /// The digits without separator, as the store keeps them.
    fn to_digit_string(self) -> String {
        self.digits.iter().map(u8::to_string).collect()
    }
}

/// The code as the Home app asks for it, e.g. "1112-2333".
impl fmt::Display for SetupCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g, h, i] = self.digits;
        write!(f, "{a}{b}{c}{d}-{e}{g}{h}{i}")
    }
}

/// The configured setup code, or one generated once and kept in the store.
fn setup_code(model: &Model, store: &mut Store) -> Result<SetupCode> {
    if let Some(code) = &model.homekit.pin {
        return SetupCode::parse(code);
    }
    if let Some(code) = &store.state.homekit_pin {
        return SetupCode::parse(code);
    }

    let code = SetupCode::random();
    store.state.homekit_pin = Some(code.to_digit_string());
    store.save()?;
    Ok(code)
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

fn homekit_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("homekit")
}

async fn open_storage(state_dir: &Path) -> Result<FileStorage> {
    let homekit_dir = homekit_dir(state_dir);
    std::fs::create_dir_all(&homekit_dir).context("cannot create the HomeKit state directory")?;
    FileStorage::new(&homekit_dir).await.map_err(hap_error)
}

/// The bridge's HAP configuration: the stored one, updated from the config
/// and saved.
async fn server_config(
    storage: &mut FileStorage,
    model: &Model,
    setup_code: &SetupCode,
    accessory_ids: &[u64],
) -> Result<Config> {
    let mut stored = stored_config(storage).await;
    stored.redetermine_local_ip();
    let accessories_removed = forget_removed_accessories(storage, accessory_ids).await?;
    // A new configuration number makes controllers reload the accessories.
    let configuration_number = if accessories_removed {
        stored.configuration_number + 1
    } else {
        stored.configuration_number
    };

    let config = Config {
        host: model.homekit.host.unwrap_or(stored.host),
        port: model.homekit.port,
        name: homekit_name(&model.homekit.name),
        pin: setup_code.pin(),
        protocol_version: PROTOCOL_VERSION.into(),
        configuration_number,
        ..stored
    };
    storage.save_config(&config).await.map_err(hap_error)?;
    Ok(config)
}

/// The stored configuration, or a new bridge with a random device id the
/// first time.
async fn stored_config(storage: &FileStorage) -> Config {
    match storage.load_config().await {
        Ok(config) => config,
        Err(_) => Config {
            device_id: random_device_id(),
            category: AccessoryCategory::Bridge,
            ..Default::default()
        },
    }
}

/// Drops the accessories that left the config from the cache, and says
/// whether there were any. Until controllers reload, such accessories
/// linger in the Home app as "No Response".
async fn forget_removed_accessories(
    storage: &mut FileStorage,
    accessory_ids: &[u64],
) -> Result<bool> {
    let cached = storage.load_aid_cache().await.unwrap_or_default();
    let (kept, removed): (Vec<u64>, Vec<u64>) = cached
        .into_iter()
        .partition(|id| *id == BRIDGE_ID || accessory_ids.contains(id));
    if removed.is_empty() {
        return Ok(false);
    }

    storage.save_aid_cache(&kept).await.map_err(hap_error)?;
    Ok(true)
}

async fn log_existing_pairings(storage: &FileStorage) -> Result<()> {
    let pairings = storage.count_pairings().await.map_err(hap_error)?;
    if pairings > 0 {
        log::info!(
            "HomeKit: paired with {pairings} controller(s), not offered for pairing \
             (`knut unpair` resets)"
        );
    }
    Ok(())
}

async fn serve(server: IpServer) {
    if let Err(error) = server.run_handle().await {
        log::error!("HomeKit server stopped: {error}");
    }
}

/// An accessory on the server, and the flag that marks value changes as
/// coming from Knut rather than from a HomeKit controller.
#[derive(Clone)]
struct Exposed {
    accessory: hap::pointer::Accessory,
    pushing: Arc<AtomicBool>,
}

impl Exposed {
    /// Adds `accessory` to `server`. Its callbacks must check `pushing`.
    async fn add(
        server: &IpServer,
        accessory: impl HapAccessory + 'static,
        pushing: Arc<AtomicBool>,
    ) -> Result<Self> {
        let accessory = server.add_accessory(accessory).await.map_err(hap_error)?;
        Ok(Self { accessory, pushing })
    }

    /// Shows `value` in HomeKit without reporting it back as a write.
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
    add_bridge(server, &model.homekit.name).await?;

    let mut entities = HashMap::new();
    for (id, path) in plan.lightbulbs {
        let pushing = Arc::new(AtomicBool::new(false));
        let input = EntityInput {
            path: path.clone(),
            pushing: Arc::clone(&pushing),
            inputs: inputs.clone(),
        };
        let accessory = lightbulb(id, &path, input)?;
        entities.insert(path, Exposed::add(server, accessory, pushing).await?);
    }

    let mut scenes = HashMap::new();
    for (id, scene) in plan.scenes {
        let pushing = Arc::new(AtomicBool::new(false));
        let accessory = scene_switch(id, &scene, &pushing, inputs, resets)?;
        scenes.insert(id, Exposed::add(server, accessory, pushing).await?);
    }

    let pushing = Arc::new(AtomicBool::new(false));
    let accessory = auto_switch(plan.auto_switch, &pushing, inputs)?;
    let auto_switch = Exposed::add(server, accessory, pushing).await?;

    Ok(ExposedAccessories {
        entities,
        scenes,
        auto_switch,
    })
}

async fn add_bridge(server: &IpServer, name: &str) -> Result<()> {
    let information = information(name, "bridge");
    let mut bridge = BridgeAccessory::new(BRIDGE_ID, information).map_err(hap_error)?;
    let version = &mut bridge.protocol_information.version;
    version
        .set_value(json!(PROTOCOL_INFORMATION_VERSION))
        .await
        .map_err(hap_error)?;
    server.add_accessory(bridge).await.map_err(hap_error)?;
    Ok(())
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
            Some(update) = updates.recv() => show_update(&accessories, update).await,
            Some(id) = reset_requests.recv() => {
                if let Some(scene_switch) = accessories.scenes.get(&id) {
                    tokio::spawn(turn_off_later(scene_switch.clone()));
                }
            }
            else => break,
        }
    }
}

async fn show_update(accessories: &ExposedAccessories, update: Update) {
    match update {
        Update::Entity { path, state } => {
            if let Some(exposed) = accessories.entities.get(&path) {
                show_entity_state(exposed, &state).await;
            }
        }
        Update::Auto(all_auto) => {
            let switch = &accessories.auto_switch;
            switch
                .set(HapType::Switch, HapType::PowerState, json!(all_auto))
                .await;
        }
    }
}

async fn show_entity_state(exposed: &Exposed, state: &EntityState) {
    let lightbulb = HapType::Lightbulb;
    let brightness = homekit_brightness(state.brightness);
    exposed
        .set(lightbulb, HapType::PowerState, json!(state.on))
        .await;
    exposed
        .set(lightbulb, HapType::Brightness, json!(brightness))
        .await;

    if let Some(kelvin) = state.kelvin {
        let mired = homekit_mired(kelvin);
        exposed
            .set(lightbulb, HapType::ColorTemperature, json!(mired))
            .await;
    }
}

async fn turn_off_later(scene_switch: Exposed) {
    tokio::time::sleep(SCENE_SWITCH_ON_TIME).await;
    scene_switch
        .set(HapType::Switch, HapType::PowerState, json!(false))
        .await;
}

/// HomeKit brightness is a whole percentage, and 0 is not allowed.
fn homekit_brightness(brightness: f64) -> i32 {
    brightness.round().clamp(1.0, 100.0) as i32
}

fn homekit_mired(kelvin: f64) -> u32 {
    let mired = kelvin_to_mired(kelvin).round() as u32;
    mired.clamp(MIRED_MIN, MIRED_MAX)
}

/// Sends a HomeKit write on an entity to the engine, unless Knut itself is
/// showing a value.
#[derive(Clone)]
struct EntityInput {
    path: String,
    pushing: Arc<AtomicBool>,
    inputs: mpsc::UnboundedSender<Input>,
}

impl EntityInput {
    fn send(&self, change: Change) {
        if self.pushing.load(Ordering::SeqCst) {
            return;
        }
        let input = Input::Entity {
            path: self.path.clone(),
            change,
        };
        // Fails only while the daemon shuts down.
        let _ = self.inputs.send(input);
    }
}

fn lightbulb(id: u64, path: &str, input: EntityInput) -> Result<LightbulbAccessory> {
    let information = information(&display_name(path), path);
    let mut accessory = LightbulbAccessory::new(id, information).map_err(hap_error)?;
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

    let on = input.clone();
    service
        .power_state
        .on_update(Some(move |_: &bool, new: &bool| {
            on.send(Change::On(*new));
            Ok(())
        }));

    if let Some(brightness) = service.brightness.as_mut() {
        let input = input.clone();
        brightness.on_update(Some(move |_: &i32, new: &i32| {
            input.send(Change::Brightness(*new as f64));
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
        colour_temperature.on_update(Some(move |_: &i32, new: &i32| {
            let kelvin = mired_to_kelvin(*new as f64);
            input.send(Change::Kelvin(kelvin));
            Ok(())
        }));
    }

    Ok(accessory)
}

/// A momentary switch that runs `scene` when tapped.
fn scene_switch(
    id: u64,
    scene: &str,
    pushing: &Arc<AtomicBool>,
    inputs: &mpsc::UnboundedSender<Input>,
    resets: &mpsc::UnboundedSender<u64>,
) -> Result<SwitchAccessory> {
    let mut accessory = switch_accessory(id, scene)?;
    let pushing = Arc::clone(pushing);
    let inputs = inputs.clone();
    let resets = resets.clone();
    let scene = scene.to_owned();
    accessory
        .switch
        .power_state
        .on_update(Some(move |_: &bool, on: &bool| {
            let tapped = *on && !pushing.load(Ordering::SeqCst);
            if tapped {
                // Fails only while the daemon shuts down.
                let _ = inputs.send(Input::Scene(scene.clone()));
                let _ = resets.send(id);
            }
            Ok(())
        }));
    Ok(accessory)
}

/// The switch that puts every entity into auto, and shows whether all are.
fn auto_switch(
    id: u64,
    pushing: &Arc<AtomicBool>,
    inputs: &mpsc::UnboundedSender<Input>,
) -> Result<SwitchAccessory> {
    let mut accessory = switch_accessory(id, "Auto")?;
    let pushing = Arc::clone(pushing);
    let inputs = inputs.clone();
    accessory
        .switch
        .power_state
        .on_update(Some(move |_: &bool, on: &bool| {
            if !pushing.load(Ordering::SeqCst) {
                // Fails only while the daemon shuts down.
                let _ = inputs.send(Input::AutoSwitch(*on));
            }
            Ok(())
        }));
    Ok(accessory)
}

fn switch_accessory(id: u64, name: &str) -> Result<SwitchAccessory> {
    let mut accessory = SwitchAccessory::new(id, information(name, name)).map_err(hap_error)?;
    // As for lightbulbs: an empty service name would override the
    // accessory's.
    accessory.switch.name = None;
    Ok(accessory)
}

fn information(name: &str, serial: &str) -> AccessoryInformation {
    AccessoryInformation {
        name: homekit_name(name),
        manufacturer: "Knut".into(),
        model: "Knut 2".into(),
        serial_number: serial.to_string(),
        firmware_revision: Some(FIRMWARE_REVISION.into()),
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
    let is_allowed = |c: char| c.is_alphanumeric() || c == '\'';
    let allowed: String = name
        .chars()
        .map(|c| if is_allowed(c) { c } else { ' ' })
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

/// The stored configuration holds the device id and keys.
fn remove_bridge_identity(homekit_dir: &Path) -> Result<()> {
    let config = homekit_dir.join("config.json");
    if config.exists() {
        std::fs::remove_file(&config)
            .with_context(|| format!("cannot remove {}", config.display()))?;
    }
    Ok(())
}

/// Removes every stored pairing and returns how many there were.
fn remove_pairings(homekit_dir: &Path) -> Result<usize> {
    let pairings = homekit_dir.join("pairings");
    let mut removed = 0;
    for entry in std::fs::read_dir(&pairings).into_iter().flatten() {
        let path = entry?.path();
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        removed += 1;
    }
    Ok(removed)
}

fn random_device_id() -> MacAddress {
    let bytes: [u8; 6] = std::array::from_fn(|_| random_byte());
    MacAddress::from(bytes)
}

fn random_byte() -> u8 {
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
    fn formats_the_setup_code_from_its_digits() -> Result<()> {
        assert_eq!(SetupCode::parse("11122333")?.to_string(), "1112-2333");
        assert_eq!(SetupCode::parse("111-22-333")?.to_string(), "1112-2333");
        Ok(())
    }

    #[test]
    fn keeps_the_setup_code_as_plain_digits() -> Result<()> {
        let code = SetupCode::parse("1112-2333")?;
        assert_eq!(code.to_digit_string(), "11122333");
        Ok(())
    }

    #[test]
    fn rejects_setup_codes_without_8_digits() {
        assert!(SetupCode::parse("1112-233").is_err());
    }

    #[test]
    fn rejects_setup_codes_that_are_easy_to_guess() {
        assert!(SetupCode::parse("12345678").is_err());
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
