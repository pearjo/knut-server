//! Running scene and trigger blocks and carrying out the actions they ask
//! for: `set`, `adapt`, `activate`, `log`.

use std::collections::VecDeque;

use super::{Engine, Origin, COMFORT_MINIMUM, MAX_SCENE_DEPTH};
use crate::adapter::LightCommand;
use crate::script::{self, Action, AdaptValues, BlockArgs, SetValues};

impl Engine {
    pub(super) async fn run_trigger(&mut self, proc_id: usize, args: &BlockArgs, line: &str) {
        let snapshot = self.snapshot();
        match self.script.run(proc_id, args, &snapshot) {
            Ok(actions) => self.execute(actions, Origin::Automation).await,
            Err(error) => log::error!("trigger at {line}: {}", script::describe(&error)),
        }
    }

    pub(super) async fn activate_scene(&mut self, name: &str) {
        self.execute(
            vec![Action::Activate {
                scene: name.to_string(),
            }],
            Origin::Automation,
        )
        .await;
    }

    pub(super) async fn execute(&mut self, actions: Vec<Action>, origin: Origin) {
        let mut queue: VecDeque<(Action, Origin, usize)> =
            actions.into_iter().map(|a| (a, origin, 0)).collect();
        while let Some((action, origin, depth)) = queue.pop_front() {
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
                    if depth >= MAX_SCENE_DEPTH {
                        log::error!(
                            "scene {scene:?}: scenes nest deeper than {MAX_SCENE_DEPTH}; stopping"
                        );
                        continue;
                    }
                    let Some(found) = self.model.scene(&scene).cloned() else {
                        log::error!("unknown scene {scene:?}");
                        continue;
                    };
                    log::info!("scene {:?}", found.name);
                    let snapshot = self.snapshot();
                    match self.script.run(found.proc_id, &BlockArgs::None, &snapshot) {
                        Ok(actions) => {
                            let origin = Origin::Scene { auto: found.auto };
                            for (i, action) in actions.into_iter().enumerate() {
                                queue.insert(i, (action, origin, depth + 1));
                            }
                        }
                        Err(error) => log::error!(
                            "scene {:?} at {}: {}",
                            found.name,
                            found.line,
                            script::describe(&error)
                        ),
                    }
                }
                Action::Log { message } => log::info!("script: {message}"),
            }
        }
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
            self.apply(
                path,
                LightCommand {
                    on: Some(false),
                    transition,
                    ..Default::default()
                },
            )
            .await;
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
        if !state.on {
            return;
        }
        let unchanged = brightness.is_none_or(|b| (b - state.brightness).abs() < 0.5)
            && kelvin.is_none_or(|k| {
                state
                    .kelvin
                    .is_some_and(|current| (k - current).abs() < 10.0)
            });
        if unchanged {
            return;
        }
        self.apply(
            path,
            LightCommand {
                on: None,
                brightness,
                kelvin,
                transition,
            },
        )
        .await;
    }

    /// The value (0–100) for `need` (0–1) after subtracting the daylight of
    /// the entity's room.
    fn value_for_need(&self, path: &str, need: f64) -> f64 {
        let room = path
            .split('/')
            .next()
            .and_then(|name| self.model.room(name));
        let daylight = match (room.and_then(|room| room.daylight.as_ref()), self.sun) {
            (Some(daylight), Some(sun)) => daylight.at(sun),
            _ => 0.0,
        };
        (100.0 * (need - daylight)).clamp(COMFORT_MINIMUM, 100.0)
    }
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
