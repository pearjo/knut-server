//! Running scene and trigger blocks and carrying out the actions they ask
//! for: `set`, `adapt`, `activate`, `log`.

use std::collections::VecDeque;

use super::{Engine, EntityState, Origin, COMFORT_MINIMUM, MAX_SCENE_DEPTH};
use crate::adapter::LightCommand;
use crate::script::{self, Action, AdaptValues, BlockArgs, SetValues};

/// An action waiting to run, with who asked for it and how deeply nested
/// in scenes it is.
#[derive(Debug)]
struct QueuedAction {
    action: Action,
    origin: Origin,
    depth: usize,
}

impl Engine {
    /// Runs the trigger block `proc_id`, defined at `line`, and its actions.
    pub(super) async fn run_trigger(&mut self, proc_id: usize, args: &BlockArgs, line: &str) {
        let snapshot = self.snapshot();
        match self.script.run(proc_id, args, &snapshot) {
            Ok(actions) => self.execute(actions, Origin::Automation).await,
            Err(error) => log::error!("trigger at {line}: {}", script::describe(&error)),
        }
    }

    pub(super) async fn activate_scene(&mut self, name: &str) {
        let activate = Action::Activate {
            scene: name.to_string(),
        };
        self.execute(vec![activate], Origin::Automation).await;
    }

    /// Carries out `actions` in order. An activated scene's own actions run
    /// right where it was activated, before the actions after it.
    pub(super) async fn execute(&mut self, actions: Vec<Action>, origin: Origin) {
        let mut queue: VecDeque<QueuedAction> = actions
            .into_iter()
            .map(|action| QueuedAction {
                action,
                origin,
                depth: 0,
            })
            .collect();
        while let Some(QueuedAction {
            action,
            origin,
            depth,
        }) = queue.pop_front()
        {
            match action {
                Action::Set { targets, values } => {
                    for target in &targets {
                        self.set(target, &values, origin).await;
                    }
                }
                Action::Adapt { targets, values } => {
                    for target in &targets {
                        self.adapt(target, &values).await;
                    }
                }
                Action::Activate { scene } => {
                    let scene_actions = self.run_scene(&scene, depth);
                    for (index, action) in scene_actions.into_iter().enumerate() {
                        queue.insert(index, action);
                    }
                }
                Action::Log { message } => log::info!("script: {message}"),
            }
        }
    }

    /// The actions of `scene`, activated at nesting `depth`. Nothing if it
    /// nests too deep, is unknown or fails; each is logged.
    fn run_scene(&mut self, scene: &str, depth: usize) -> Vec<QueuedAction> {
        if depth >= MAX_SCENE_DEPTH {
            log::error!("scene {scene:?}: scenes nest deeper than {MAX_SCENE_DEPTH}; stopping");
            return Vec::new();
        }
        let Some(found) = self.model.scene(scene).cloned() else {
            log::error!("unknown scene {scene:?}");
            return Vec::new();
        };

        log::info!("scene {:?}", found.name);
        let snapshot = self.snapshot();
        let actions = match self.script.run(found.proc_id, &BlockArgs::None, &snapshot) {
            Ok(actions) => actions,
            Err(error) => {
                let error = script::describe(&error);
                log::error!("scene {:?} at {}: {error}", found.name, found.line);
                return Vec::new();
            }
        };
        let origin = Origin::Scene { auto: found.auto };
        actions
            .into_iter()
            .map(|action| QueuedAction {
                action,
                origin,
                depth: depth + 1,
            })
            .collect()
    }

    async fn set(&mut self, path: &str, values: &SetValues, origin: Origin) {
        let SetValues {
            on,
            brightness,
            kelvin,
            transition,
        } = *values;
        if self.model.entity_kind(path).is_none() {
            log::error!("unknown light or group {path:?}");
            return;
        }
        if on == Some(false) {
            let off = LightCommand {
                transition,
                ..LightCommand::off()
            };
            self.apply(path, off).await;
            return;
        }
        let sets_values = brightness.is_some() || kelvin.is_some();
        let was_on = self.state(path).on;
        if let Some(auto) = auto_after_set(origin, sets_values, was_on) {
            self.set_auto(path, auto);
        }
        let mut command = LightCommand {
            on: Some(true),
            brightness,
            kelvin,
            transition,
        };
        if !was_on {
            let auto = self.auto_values(path);
            command.brightness = command.brightness.or(auto.brightness);
            command.kelvin = command.kelvin.or(auto.kelvin);
        }
        self.apply(path, command).await;
    }

    async fn adapt(&mut self, path: &str, values: &AdaptValues) {
        let AdaptValues {
            need,
            kelvin,
            transition,
        } = *values;
        if self.model.entity_kind(path).is_none() {
            log::error!("adapt: unknown light or group {path:?}");
            return;
        }
        if !self.is_auto(path) {
            return;
        }
        let brightness = need.map(|need| self.value_for_need(path, need));
        let target = self.auto_targets.entry(path.to_string()).or_default();
        target.brightness = brightness.or(target.brightness);
        target.kelvin = kelvin.or(target.kelvin);

        let state = self.state(path);
        if !state.on || already_at(&state, brightness, kelvin) {
            return;
        }
        let command = LightCommand {
            on: None,
            brightness,
            kelvin,
            transition,
        };
        self.apply(path, command).await;
    }

    /// The value (0–100) for `need` (0–1) after subtracting the daylight of
    /// the entity's room.
    fn value_for_need(&self, path: &str, need: f64) -> f64 {
        let daylight = self.room_daylight(path);
        (100.0 * (need - daylight)).clamp(COMFORT_MINIMUM, 100.0)
    }

    /// Daylight (0–1) in the room of the entity at `path`; 0 for a room
    /// without a daylight profile, or before the sun is known.
    fn room_daylight(&self, path: &str) -> f64 {
        let room_name = path.split('/').next().unwrap_or(path);
        let profile = self
            .model
            .room(room_name)
            .and_then(|room| room.daylight.as_ref());
        match (profile, self.sun) {
            (Some(profile), Some(sun)) => profile.at(sun),
            _ => 0.0,
        }
    }
}

/// Whether `state` is close enough to the asked-for values that sending
/// them would only make the light flicker.
fn already_at(state: &EntityState, brightness: Option<f64>, kelvin: Option<f64>) -> bool {
    let same_brightness =
        brightness.is_none_or(|brightness| (brightness - state.brightness).abs() < 0.5);
    let same_kelvin = kelvin.is_none_or(|kelvin| {
        state
            .kelvin
            .is_some_and(|current| (kelvin - current).abs() < 10.0)
    });
    same_brightness && same_kelvin
}

/// Whether a `set` puts its target into auto (`Some(true)`), takes it out
/// (`Some(false)`) or leaves auto mode alone (DESIGN.md, "Auto mode"):
///
/// - An `auto: true` scene sets a starting point; its entities stay in auto.
/// - An `auto: false` scene that sets brightness or colour temperature is an
///   override. One that only switches an entity on acts like switching on in
///   HomeKit: an entity that was off goes back into auto.
/// - Triggers never change auto mode.
fn auto_after_set(origin: Origin, sets_values: bool, was_on: bool) -> Option<bool> {
    match origin {
        Origin::Scene { auto: true } => Some(true),
        Origin::Scene { auto: false } if sets_values => Some(false),
        Origin::Scene { auto: false } if !was_on => Some(true),
        Origin::Scene { auto: false } | Origin::Automation => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scenes_change_auto_mode() {
        let manual = Origin::Scene { auto: false };
        let auto = Origin::Scene { auto: true };
        // (origin, sets values, was on) → auto afterwards
        let cases = [
            (auto, true, true, Some(true)),
            (auto, false, false, Some(true)),
            (manual, true, true, Some(false)),
            (manual, true, false, Some(false)),
            (manual, false, false, Some(true)),
            (manual, false, true, None),
            (Origin::Automation, true, false, None),
            (Origin::Automation, false, false, None),
        ];
        for (origin, sets_values, was_on, expected) in cases {
            assert_eq!(
                auto_after_set(origin, sets_values, was_on),
                expected,
                "{origin:?}, sets values: {sets_values}, was on: {was_on}"
            );
        }
    }
}
