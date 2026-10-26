//! The daemon's core: entity state, auto mode, scenes and triggers.
//!
//! Every HomeKit-controlled entity is either in auto (the sun handler and
//! `adapt` move it) or overridden (the user picked brightness or colour
//! temperature). See DESIGN.md, "Auto mode".
//!
//! The engine's methods are split by what they handle: `inputs` (HomeKit
//! and adapters), `triggers` (sun and clock), `actions` (what blocks ask
//! for), `lights` (commands and staging) and `state` (bookkeeping).

mod actions;
mod inputs;
mod lights;
mod state;
mod triggers;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Local, Timelike};
use tokio::sync::mpsc;

use crate::adapter::{AdapterEvent, Adapters, LightCommand, LightReport};
use crate::model::Model;
use crate::script::Script;
use crate::store::Store;
use crate::sun;

/// Transition used when the Auto switch moves entities back to sun values.
const AUTO_RESUME_TRANSITION: f64 = 2.0;
/// Scenes may activate scenes; this stops accidental loops.
const MAX_SCENE_DEPTH: usize = 4;
/// `adapt` never dims an entity below this value (0–100) while it is on:
/// very low light levels are the one robust discomfort finding.
const COMFORT_MINIMUM: f64 = 5.0;
/// Seconds a stage change takes at least, so stages cross-fade rather than
/// switch even for commands without a transition.
const MIN_STAGE_TRANSITION: f64 = 1.0;

/// What a light or group is doing, as far as the engine knows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntityState {
    pub on: bool,
    /// 1–100; kept while off so the entity comes back at the same level.
    pub brightness: f64,
    pub kelvin: Option<f64>,
}

impl Default for EntityState {
    fn default() -> Self {
        Self {
            on: false,
            brightness: 100.0,
            kelvin: None,
        }
    }
}

impl EntityState {
    /// Takes over what `command` sets, as the light will once it arrives.
    fn record(&mut self, command: &LightCommand) {
        if let Some(on) = command.on {
            self.on = on;
        }
        if let Some(brightness) = command.brightness {
            self.brightness = brightness;
        }
        if command.kelvin.is_some() {
            self.kelvin = command.kelvin;
        }
    }

    /// Takes over what a device reported. A reported brightness of 0 is
    /// ignored, so the entity comes back at its last level.
    fn record_report(&mut self, report: &LightReport) {
        if let Some(on) = report.on {
            self.on = on;
        }
        if let Some(brightness) = report.brightness.filter(|brightness| *brightness > 0.0) {
            self.brightness = brightness;
        }
        if report.kelvin.is_some() {
            self.kelvin = report.kelvin;
        }
    }
}

/// Requests from HomeKit.
#[derive(Debug, Clone)]
pub(crate) enum Input {
    Entity { path: String, change: Change },
    Scene(String),
    AutoSwitch(bool),
}

/// One characteristic the user changed in HomeKit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Change {
    On(bool),
    /// 0–100; 0 switches off.
    Brightness(f64),
    Kelvin(f64),
}

/// State HomeKit should show.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Update {
    Entity {
        path: String,
        state: EntityState,
    },
    /// Whether every entity is in auto.
    Auto(bool),
}

/// Who asked for an action; decides how it affects auto mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Scene { auto: bool },
    Automation,
}

/// What staging has switched on in a group that is on.
#[derive(Debug, Clone, PartialEq)]
enum ActiveStaging {
    /// The number of leading stages.
    Stages(usize),
    /// The lights a staging block picked.
    Lights(Vec<String>),
}

/// The values the last `adapt` asked for, applied when an auto entity
/// comes on.
#[derive(Debug, Clone, Default)]
struct AutoTarget {
    brightness: Option<f64>,
    kelvin: Option<f64>,
}

/// Owns the state of every light and group and decides what they do, from
/// HomeKit requests, device events, triggers and scenes.
pub(crate) struct Engine {
    model: Arc<Model>,
    script: Script,
    adapters: Adapters,
    store: Store,
    updates: mpsc::UnboundedSender<Update>,
    states: HashMap<String, EntityState>,
    auto_targets: HashMap<String, AutoTarget>,
    /// Groups each light belongs to.
    groups_of_light: HashMap<String, Vec<String>>,
    exposed: HashSet<String>,
    store_dirty: bool,
    /// As of the last tick; `None` without a location.
    sun: Option<sun::SunPosition>,
    /// Per staged group that is on.
    active_stages: HashMap<String, ActiveStaging>,
}

impl Engine {
    /// An engine for `model`. Every entity starts off; auto flags come from
    /// `store`, and entities exposed for the first time start in auto.
    /// `updates` receives what HomeKit should show.
    pub(crate) fn new(
        model: Arc<Model>,
        script: Script,
        adapters: Adapters,
        mut store: Store,
        updates: mpsc::UnboundedSender<Update>,
    ) -> Self {
        let states = model
            .lights
            .keys()
            .chain(model.groups.keys())
            .map(|path| (path.clone(), EntityState::default()))
            .collect();
        let exposed: HashSet<String> = model.exposed_entities().into_iter().collect();
        sync_auto_flags(&mut store, &exposed);
        Self {
            groups_of_light: groups_of_light(&model),
            model,
            script,
            adapters,
            store,
            updates,
            states,
            auto_targets: HashMap::new(),
            exposed,
            store_dirty: true,
            sun: None,
            active_stages: HashMap::new(),
        }
    }

    /// Whether every exposed entity is in auto, as the Auto switch shows.
    pub(crate) fn all_auto(&self) -> bool {
        self.store.state.auto.values().all(|auto| *auto)
    }

    /// Handles HomeKit `inputs`, adapter `events` and a tick every minute
    /// until both channels close.
    pub(crate) async fn run(
        mut self,
        mut inputs: mpsc::UnboundedReceiver<Input>,
        mut events: mpsc::UnboundedReceiver<AdapterEvent>,
    ) {
        self.publish_all();
        let mut last_tick = Local::now();
        // Give adapters a moment to report state before the first sun tick.
        let mut next_tick = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut first_tick = true;
        loop {
            tokio::select! {
                Some(input) = inputs.recv() => self.handle_input(input).await,
                Some(event) = events.recv() => self.handle_adapter_event(event).await,
                _ = tokio::time::sleep_until(next_tick) => {
                    let now = Local::now();
                    self.on_tick(last_tick, now, first_tick).await;
                    first_tick = false;
                    last_tick = now;
                    next_tick = tokio::time::Instant::now() + until_next_minute(now);
                }
                else => break,
            }
            self.save_if_dirty();
        }
    }
}

/// The groups each light belongs to.
fn groups_of_light(model: &Model) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for group in model.groups.values() {
        for member in &group.members {
            groups
                .entry(member.light.clone())
                .or_default()
                .push(group.path.clone());
        }
    }
    groups
}

/// Forgets the auto flags of entities that are no longer exposed and puts
/// newly exposed ones into auto.
fn sync_auto_flags(store: &mut Store, exposed: &HashSet<String>) {
    store.state.auto.retain(|path, _| exposed.contains(path));
    for path in exposed {
        store.state.auto.entry(path.clone()).or_insert(true);
    }
}

fn until_next_minute(now: DateTime<Local>) -> std::time::Duration {
    let into_minute = now.second() as u64 * 1000 + now.timestamp_subsec_millis() as u64;
    std::time::Duration::from_millis(60_000 - into_minute.min(59_999))
}

#[cfg(test)]
mod tests;
