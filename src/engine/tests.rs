use chrono::TimeZone;
use serde_json::json;

use super::*;
use crate::model::{AdapterKind, DeviceRef};
use crate::script::{Action, AdaptValues};

const HOME: &str = include_str!("../../tests/fixtures/engine.rb");

/// An engine for the test home, with its HomeKit updates and a state
/// directory that is removed afterwards.
struct Harness {
    engine: Engine,
    updates: mpsc::UnboundedReceiver<Update>,
    dir: std::path::PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn harness(name: &str) -> Harness {
    let (script, model) = Script::load_str(HOME, "config.rb").unwrap();
    let dir = std::env::temp_dir().join(format!("knut-engine-{name}-{}", std::process::id()));
    let store = Store::open(&dir).unwrap();
    let (tx, updates) = mpsc::unbounded_channel();
    let engine = Engine::new(Arc::new(model), script, Adapters::none(), store, tx);
    Harness {
        engine,
        updates,
        dir,
    }
}

impl Harness {
    fn last_auto_update(&mut self) -> Option<bool> {
        let mut last = None;
        while let Ok(update) = self.updates.try_recv() {
            if let Update::Auto(auto) = update {
                last = Some(auto);
            }
        }
        last
    }

    async fn homekit(&mut self, path: &str, change: Change) {
        self.engine
            .handle_input(Input::Entity {
                path: path.into(),
                change,
            })
            .await;
    }

    /// The brightness of a light that is on, `None` while it is off.
    fn lit(&self, path: &str) -> Option<f64> {
        let state = self.engine.state(path);
        state.on.then_some(state.brightness)
    }
}

/// An `adapt` action, as a trigger block would ask for it.
fn adapt(targets: &[&str], values: AdaptValues) -> Vec<Action> {
    vec![Action::Adapt {
        targets: targets.iter().map(|target| target.to_string()).collect(),
        values,
    }]
}

fn need(need: f64) -> AdaptValues {
    AdaptValues {
        need: Some(need),
        ..Default::default()
    }
}

#[tokio::test]
async fn brightness_overrides_and_off_on_resumes_auto() {
    let mut h = harness("override");
    assert!(h.engine.all_auto());

    h.homekit("Kitchen/Spots", Change::On(true)).await;
    assert!(
        h.engine.is_auto("Kitchen/Spots"),
        "switching on alone is not an override"
    );

    h.homekit("Kitchen/Spots", Change::Brightness(60.0)).await;
    assert!(!h.engine.is_auto("Kitchen/Spots"));
    assert_eq!(h.last_auto_update(), Some(false));
    assert_eq!(
        h.engine.state("Kitchen/spot2").brightness,
        30.0,
        "mapping applies to members"
    );

    h.homekit("Kitchen/Spots", Change::On(false)).await;
    assert!(
        !h.engine.is_auto("Kitchen/Spots"),
        "switching off alone keeps the override"
    );
    h.homekit("Kitchen/Spots", Change::On(true)).await;
    assert!(h.engine.is_auto("Kitchen/Spots"));
    assert_eq!(h.last_auto_update(), Some(true));
}

#[tokio::test]
async fn adapt_moves_only_auto_entities_and_is_used_when_switching_on() {
    let mut h = harness("adapt");
    let warmth = |kelvin| {
        let values = AdaptValues {
            kelvin: Some(kelvin),
            transition: Some(60.0),
            ..Default::default()
        };
        adapt(&["Kitchen/Spots", "Hallway/Light"], values)
    };

    h.homekit("Hallway/Light", Change::On(true)).await;
    h.homekit("Hallway/Light", Change::Kelvin(3000.0)).await;
    h.engine.execute(warmth(2400.0), Origin::Automation).await;
    assert_eq!(
        h.engine.state("Hallway/Light").kelvin,
        Some(3000.0),
        "overridden entity keeps its value"
    );
    assert!(
        !h.engine.state("Kitchen/Spots").on,
        "adapt does not switch anything on"
    );

    h.homekit("Kitchen/Spots", Change::On(true)).await;
    assert_eq!(
        h.engine.state("Kitchen/Spots").kelvin,
        Some(2400.0),
        "comes on at the current sun values"
    );
    h.engine.execute(warmth(4000.0), Origin::Automation).await;
    assert_eq!(h.engine.state("Kitchen/spot1").kelvin, Some(4000.0));
}

#[tokio::test]
async fn adapt_subtracts_room_daylight_from_the_need() {
    let mut h = harness("need");
    // Full sky, sun behind the south window: the kitchen gets 0.5 × 1.0 daylight.
    h.engine.sun = Some(sun::SunPosition {
        elevation: 30.0,
        azimuth: 0.0,
    });
    let both = |value| adapt(&["Kitchen/Spots", "Hallway/Light"], need(value));
    let brightness = |h: &Harness, path| h.engine.state(path).brightness;
    h.homekit("Kitchen/Spots", Change::On(true)).await;
    h.homekit("Hallway/Light", Change::On(true)).await;

    h.engine.execute(both(0.8), Origin::Automation).await;
    assert!(
        (brightness(&h, "Kitchen/Spots") - 30.0).abs() < 1e-9,
        "daylight subtracted"
    );
    assert!(
        (brightness(&h, "Kitchen/spot2") - 15.0).abs() < 1e-9,
        "then the mapping"
    );
    assert_eq!(
        brightness(&h, "Hallway/Light"),
        80.0,
        "no daylight in the hallway"
    );

    h.engine.execute(both(0.3), Origin::Automation).await;
    assert_eq!(
        brightness(&h, "Kitchen/Spots"),
        COMFORT_MINIMUM,
        "daylight exceeds the need"
    );
    h.engine.execute(both(0.0), Origin::Automation).await;
    assert_eq!(brightness(&h, "Hallway/Light"), COMFORT_MINIMUM);

    h.homekit("Kitchen/Spots", Change::Brightness(40.0)).await;
    assert_eq!(
        brightness(&h, "Kitchen/Spots"),
        40.0,
        "an override is the need itself"
    );
    h.engine.execute(both(0.8), Origin::Automation).await;
    assert_eq!(
        brightness(&h, "Kitchen/Spots"),
        40.0,
        "and adapt leaves it alone"
    );
}

#[tokio::test]
async fn staging_follows_overrides_and_adapt() {
    let mut h = harness("staging");
    let lights = |h: &Harness| {
        (
            h.lit("Bathroom/a"),
            h.lit("Bathroom/b"),
            h.lit("Bathroom/c"),
        )
    };

    h.homekit("Bathroom/Light", Change::On(true)).await;
    assert_eq!(lights(&h), (Some(100.0), Some(100.0), Some(100.0)));

    h.homekit("Bathroom/Light", Change::Brightness(20.0)).await;
    assert_eq!(
        h.engine.state("Bathroom/Light").brightness,
        20.0,
        "HomeKit shows the need"
    );
    assert_eq!(
        lights(&h),
        (None, Some(60.0), None),
        "an override is staged too"
    );

    h.homekit("Bathroom/Light", Change::On(false)).await;
    h.homekit("Bathroom/Light", Change::On(true)).await;
    assert!(h.engine.is_auto("Bathroom/Light"));
    let slowly = |value| {
        let values = AdaptValues {
            transition: Some(60.0),
            ..need(value)
        };
        adapt(&["Bathroom/Light"], values)
    };
    h.engine.execute(slowly(0.5), Origin::Automation).await;
    assert_eq!(lights(&h), (Some(50.0), Some(50.0), Some(50.0)));
    h.engine.execute(slowly(0.26), Origin::Automation).await;
    assert_eq!(
        h.lit("Bathroom/a"),
        Some(26.0),
        "hysteresis keeps both stages above 75 per light"
    );
    h.engine.execute(slowly(0.2), Origin::Automation).await;
    assert_eq!(lights(&h), (None, Some(60.0), None));
}

#[tokio::test]
async fn stage_changes_cross_fade() {
    let mut h = harness("cross-fade");
    h.homekit("Bathroom/Light", Change::Brightness(20.0)).await;

    let command = LightCommand {
        brightness: Some(50.0),
        ..Default::default()
    };
    let staged = h.engine.stage("Bathroom/Light", command);
    let lights = staged.lights.expect("the group is staged");
    assert_eq!(lights.on.len(), 3, "the second stage comes on");
    assert_eq!(staged.command.transition, Some(MIN_STAGE_TRANSITION));
    assert_eq!(staged.command.on, Some(true));

    let command = LightCommand {
        brightness: Some(45.0),
        transition: Some(60.0),
        ..Default::default()
    };
    let staged = h.engine.stage("Bathroom/Light", command);
    let lights = staged.lights.expect("the group is staged");
    assert!(lights.off.is_empty());
    assert_eq!(
        staged.command.transition,
        Some(60.0),
        "longer transitions are kept"
    );

    let off = h.engine.stage("Bathroom/Light", LightCommand::off());
    assert!(off.lights.is_none());
    assert!(
        !h.engine.active_stages.contains_key("Bathroom/Light"),
        "switching off forgets the stages"
    );
}

#[tokio::test]
async fn staging_blocks_pick_the_lights() {
    let mut h = harness("staging-block");
    h.homekit("Bathroom/Mirror", Change::On(true)).await;

    let mirror = |value| adapt(&["Bathroom/Mirror"], need(value));
    h.engine.execute(mirror(0.3), Origin::Automation).await;
    assert_eq!(
        (h.lit("Bathroom/a"), h.lit("Bathroom/b")),
        (Some(60.0), None),
        "one of two lights carries 0.3 · 2"
    );
    h.engine.execute(mirror(0.8), Origin::Automation).await;
    assert_eq!(
        (h.lit("Bathroom/a"), h.lit("Bathroom/b")),
        (Some(80.0), Some(80.0))
    );
    assert_eq!(
        h.engine.active_stages["Bathroom/Mirror"],
        ActiveStaging::Lights(vec!["Bathroom/a".into(), "Bathroom/b".into()])
    );

    h.homekit("Bathroom/Tub", Change::Brightness(30.0)).await;
    assert_eq!(
        (h.lit("Bathroom/b"), h.lit("Bathroom/c")),
        (Some(30.0), Some(30.0)),
        "a failing block lights everything"
    );
}

#[tokio::test]
async fn auto_switch_sets_every_entity() {
    let mut h = harness("switch");
    h.engine.handle_input(Input::AutoSwitch(false)).await;
    assert!(h.engine.store.state.auto.values().all(|auto| !auto));
    h.engine.handle_input(Input::AutoSwitch(true)).await;
    assert!(h.engine.all_auto());
    assert_eq!(h.last_auto_update(), Some(true));
}

#[tokio::test]
async fn scenes_override_or_keep_auto() {
    let mut h = harness("scenes");
    h.engine.handle_input(Input::Scene("Bright".into())).await;
    assert!(!h.engine.is_auto("Kitchen/Spots"));
    assert_eq!(h.engine.state("Kitchen/Spots").brightness, 90.0);

    h.engine.handle_input(Input::Scene("Morning".into())).await;
    assert!(
        h.engine.is_auto("Kitchen/Spots"),
        "auto scenes put their entities back into auto"
    );

    h.engine.handle_input(Input::Scene("Chain".into())).await;
    assert!(
        !h.engine.is_auto("Kitchen/Spots"),
        "nested scenes run with their own mode"
    );
}

#[tokio::test]
async fn device_events_fire_triggers_and_reports_update_groups() {
    let mut h = harness("events");
    let zigbee = |device: &str| DeviceRef {
        adapter: "z2m".into(),
        kind: AdapterKind::Zigbee,
        device: device.into(),
    };
    let motion = AdapterEvent::Device {
        device: zigbee("0x11"),
        event: "motion".into(),
        data: json!({"occupancy": true}),
    };
    h.engine.handle_adapter_event(motion).await;
    assert!(h.engine.state("Hallway/Light").on);

    let report = AdapterEvent::Light {
        device: zigbee("0x01"),
        report: LightReport {
            on: Some(true),
            brightness: Some(50.0),
            kelvin: None,
        },
    };
    h.engine.handle_adapter_event(report).await;
    assert!(
        h.engine.state("Kitchen/Spots").on,
        "a group is on while any member is"
    );
    assert!(
        h.engine.is_auto("Kitchen/Spots"),
        "device reports never change auto mode"
    );
}

#[tokio::test]
async fn light_switched_on_outside_knut_gets_auto_values() {
    let mut h = harness("switched-outside");
    let switched_on = |device: &str| AdapterEvent::Light {
        device: DeviceRef {
            adapter: "z2m".into(),
            kind: AdapterKind::Zigbee,
            device: device.into(),
        },
        report: LightReport {
            on: Some(true),
            brightness: Some(80.0),
            kelvin: Some(4000.0),
        },
    };
    let values = AdaptValues {
        need: Some(0.4),
        kelvin: Some(2400.0),
        transition: Some(60.0),
    };
    h.engine
        .execute(adapt(&["Hallway/Light"], values), Origin::Automation)
        .await;
    h.engine
        .execute(adapt(&["Kitchen/Spots"], need(0.6)), Origin::Automation)
        .await;

    h.engine.handle_adapter_event(switched_on("0x10")).await;
    let hallway = h.engine.state("Hallway/Light");
    assert_eq!(
        (hallway.brightness, hallway.kelvin),
        (40.0, Some(2400.0)),
        "its own adapt values, not where it came on"
    );

    h.engine.handle_adapter_event(switched_on("0x02")).await;
    assert_eq!(
        h.lit("Kitchen/spot2"),
        Some(30.0),
        "the group's value through the member's mapping"
    );
    assert_eq!(h.lit("Kitchen/spot1"), None, "other members stay off");

    h.engine.handle_adapter_event(switched_on("0x02")).await;
    assert_eq!(
        h.lit("Kitchen/spot2"),
        Some(80.0),
        "a report while on only records the state"
    );

    h.homekit("Hallway/Light", Change::Kelvin(3000.0)).await;
    h.homekit("Hallway/Light", Change::On(false)).await;
    h.engine.handle_adapter_event(switched_on("0x10")).await;
    let hallway = h.engine.state("Hallway/Light");
    assert_eq!(
        (hallway.brightness, hallway.kelvin),
        (80.0, Some(4000.0)),
        "an overridden light keeps what it came on with"
    );
    assert!(!h.engine.is_auto("Hallway/Light"));
}

#[tokio::test]
async fn clock_triggers_fire_once_in_their_minute() {
    let mut h = harness("clock");
    let at = |h, m, s| Local.with_ymd_and_hms(2026, 10, 22, h, m, s).unwrap();
    h.engine.on_tick(at(6, 28, 0), at(6, 29, 0), false).await;
    assert!(!h.engine.state("Kitchen/Spots").on);
    h.engine.on_tick(at(6, 29, 0), at(6, 30, 0), false).await;
    assert!(h.engine.state("Kitchen/Spots").on);
    h.homekit("Kitchen/Spots", Change::On(false)).await;
    h.engine.on_tick(at(6, 30, 0), at(6, 31, 0), false).await;
    assert!(!h.engine.state("Kitchen/Spots").on, "fired only once");
}

#[test]
fn waits_until_the_next_minute() {
    let now = Local.with_ymd_and_hms(2026, 10, 22, 6, 29, 45).unwrap();
    assert_eq!(until_next_minute(now), std::time::Duration::from_secs(15));
}
