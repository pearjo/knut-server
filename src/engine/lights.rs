//! Sending commands to the lights behind an entity, with staging.

use std::sync::Arc;

use chrono::Local;

use super::{ActiveStaging, Engine, MIN_STAGE_TRANSITION};
use crate::adapter::LightCommand;
use crate::control;
use crate::model::{Group, Staging};
use crate::script;

/// A command after staging, and which lights of a staged group it reaches.
#[derive(Debug)]
pub(super) struct StagedCommand {
    pub command: LightCommand,
    /// `None` for lights, groups without staging and groups switching off.
    pub lights: Option<control::StagedLights>,
}

impl Engine {
    /// For a staged group that stays on: picks the stages for `command`
    /// and remembers them. When the stages change, every light that is on
    /// gets brightness and colour temperature, and the change cross-fades.
    pub(super) fn stage(&mut self, path: &str, command: LightCommand) -> StagedCommand {
        let model = Arc::clone(&self.model);
        let staging = model
            .groups
            .get(path)
            .and_then(|group| Some((group, group.staging.as_ref()?)));
        let Some((group, staging)) = staging else {
            return StagedCommand {
                command,
                lights: None,
            };
        };
        if command.on == Some(false) {
            self.active_stages.remove(path);
            return StagedCommand {
                command,
                lights: None,
            };
        }

        let state = self.state(path);
        let value = command.brightness.unwrap_or(state.brightness);
        let previous = self.active_stages.get(path).cloned();
        let (active, lights) = match staging {
            Staging::Stages(stages) => by_stages(stages, value, previous.as_ref()),
            Staging::Custom { proc_id, line } => {
                let on = self.custom_staging(group, *proc_id, line, value);
                by_block(group, on, previous.as_ref())
            }
        };

        let stages_changed = previous.as_ref() != Some(&active);
        let command = if stages_changed {
            cross_fade(command, value, state.kelvin)
        } else {
            command
        };
        self.active_stages.insert(path.to_string(), active);
        StagedCommand {
            command,
            lights: Some(lights),
        }
    }

    /// The lights `group`'s staging block picks for `value` (0–100); all of
    /// them when the block fails.
    fn custom_staging(
        &mut self,
        group: &Group,
        proc_id: usize,
        line: &str,
        value: f64,
    ) -> Vec<String> {
        let need = value / 100.0;
        let now = Local::now().timestamp();
        let picked = self
            .script
            .staging(proc_id, need, now)
            .map_err(|error| script::describe(&error))
            .and_then(|names| control::picked_lights(group, &names));
        picked.unwrap_or_else(|error| {
            log::error!("staging block at {line}: {error}; using all lights");
            all_lights(group)
        })
    }

    /// Sends `command` to the lights behind `path` and records the result.
    /// Unavailable lights are left out: they could not receive it, and they
    /// get the auto values when they report again.
    pub(super) async fn apply(&mut self, path: &str, command: LightCommand) {
        if !self.state(path).available {
            return;
        }
        let StagedCommand { command, lights } = self.stage(path, command);
        let mut commands = control::commands_for(&self.model, path, &command, lights.as_ref());
        commands.retain(|light| self.state(&light.path).available);
        self.update_state(path, |state| state.record(&command));

        let mut touched_groups = Vec::new();
        for light in &commands {
            self.update_state(&light.path, |state| state.record(&light.command));
            if light.path != path {
                self.publish(&light.path);
            }
            let groups = self.groups_of_light.get(&light.path).cloned();
            touched_groups.extend(groups.unwrap_or_default());
        }
        self.publish(path);
        for group in touched_groups {
            if group != path {
                self.refresh_group(&group);
            }
        }
        control::send_all(&self.adapters, &commands).await;
    }
}

/// The leading stages that carry `value`, with hysteresis against the
/// stages that were on before.
fn by_stages(
    stages: &[Vec<String>],
    value: f64,
    previous: Option<&ActiveStaging>,
) -> (ActiveStaging, control::StagedLights) {
    let previous_count = match previous {
        Some(ActiveStaging::Stages(count)) => Some(*count),
        _ => None,
    };
    let count = control::stage_count(stages, value, previous_count);
    (
        ActiveStaging::Stages(count),
        control::staged_lights(stages, count, previous_count),
    )
}

/// The lights a staging block picked (`on`); the ones lit before and not
/// picked again go off. Before the first pick every light counts as lit.
fn by_block(
    group: &Group,
    on: Vec<String>,
    previous: Option<&ActiveStaging>,
) -> (ActiveStaging, control::StagedLights) {
    let lit_before = match previous {
        Some(ActiveStaging::Lights(lights)) => lights.clone(),
        _ => all_lights(group),
    };
    let off = lit_before
        .into_iter()
        .filter(|light| !on.contains(light))
        .collect();
    (
        ActiveStaging::Lights(on.clone()),
        control::StagedLights { on, off },
    )
}

fn all_lights(group: &Group) -> Vec<String> {
    group
        .members
        .iter()
        .map(|member| member.light.clone())
        .collect()
}

/// `command` as a cross-fade for a stage change: every light that is on
/// gets the group's brightness `value` and colour temperature, and the
/// change takes at least [`MIN_STAGE_TRANSITION`] seconds.
fn cross_fade(command: LightCommand, value: f64, kelvin: Option<f64>) -> LightCommand {
    let transition = command.transition.unwrap_or(0.0);
    LightCommand {
        on: Some(true),
        brightness: Some(value),
        kelvin: command.kelvin.or(kelvin),
        transition: Some(transition.max(MIN_STAGE_TRANSITION)),
    }
}
