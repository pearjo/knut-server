//! Sending commands to the lights behind an entity, with staging.

use std::sync::Arc;

use chrono::Local;

use super::{ActiveStaging, Engine, MIN_STAGE_TRANSITION};
use crate::adapter::LightCommand;
use crate::control;
use crate::model::{Group, Staging};
use crate::script;

impl Engine {
    /// For a staged group that stays on: picks the stages for `command`
    /// and remembers them. When the stages change, every light that is on
    /// gets brightness and colour temperature, and the change cross-fades.
    pub(super) fn stage(
        &mut self,
        path: &str,
        command: &mut LightCommand,
    ) -> Option<control::StagedLights> {
        let model = Arc::clone(&self.model);
        let group = model.groups.get(path)?;
        let staging = group.staging.as_ref()?;
        if command.on == Some(false) {
            self.active_stages.remove(path);
            return None;
        }
        let state = self.state(path);
        let value = command.brightness.unwrap_or(state.brightness);
        let previous = self.active_stages.get(path).cloned();
        let (active, staged) = match staging {
            Staging::Stages(stages) => by_stages(stages, value, previous.as_ref()),
            Staging::Custom { proc_id, line } => {
                let on = self.custom_staging(group, *proc_id, line, value);
                by_block(group, on, previous.as_ref())
            }
        };
        if previous.as_ref() != Some(&active) {
            cross_fade(command, value, state.kelvin);
        }
        self.active_stages.insert(path.to_string(), active);
        Some(staged)
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
        let picked = self
            .script
            .staging(proc_id, value / 100.0, Local::now().timestamp())
            .map_err(|error| script::describe(&error))
            .and_then(|names| control::picked_lights(group, &names));
        picked.unwrap_or_else(|error| {
            log::error!("staging block at {line}: {error}; using all lights");
            group.members.iter().map(|m| m.light.clone()).collect()
        })
    }

    /// Sends `command` to the lights behind `path` and records the result.
    pub(super) async fn apply(&mut self, path: &str, mut command: LightCommand) {
        let staged = self.stage(path, &mut command);
        let commands = control::commands_for(&self.model, path, &command, staged.as_ref());
        self.update_state(path, |state| state.record(&command));
        let mut touched_groups = Vec::new();
        for (light, _, light_command) in &commands {
            self.update_state(light, |state| state.record(light_command));
            if light != path {
                self.publish(light);
            }
            touched_groups.extend(self.groups_of_light.get(light).cloned().unwrap_or_default());
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
        _ => group.members.iter().map(|m| m.light.clone()).collect(),
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

/// Turns `command` into a cross-fade for a stage change: every light that
/// is on gets the group's brightness `value` and colour temperature, and
/// the change takes at least `MIN_STAGE_TRANSITION` seconds.
fn cross_fade(command: &mut LightCommand, value: f64, kelvin: Option<f64>) {
    command.on = Some(true);
    command.brightness = Some(value);
    command.kelvin = command.kelvin.or(kelvin);
    let transition = command.transition.unwrap_or(0.0);
    command.transition = Some(transition.max(MIN_STAGE_TRANSITION));
}
