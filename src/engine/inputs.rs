//! What reaches the engine from outside: HomeKit changes and adapter events.

use chrono::Local;
use serde_json::Value;

use super::{Change, Engine, Input, AUTO_RESUME_TRANSITION};
use crate::adapter::{AdapterEvent, LightCommand, LightReport};
use crate::control;
use crate::model::DeviceRef;
use crate::script::BlockArgs;

impl Engine {
    pub(super) async fn handle_input(&mut self, input: Input) {
        match input {
            Input::Entity { path, change } => self.handle_change(&path, change).await,
            Input::Scene(name) => self.activate_scene(&name).await,
            Input::AutoSwitch(true) => {
                log::info!("Auto switch on: every entity back into auto");
                let paths: Vec<String> = self.exposed.iter().cloned().collect();
                for path in paths {
                    self.set_auto(&path, true);
                    if self.state(&path).on {
                        let mut command = self.auto_values(&path);
                        if command.brightness.is_some() || command.kelvin.is_some() {
                            command.transition = Some(AUTO_RESUME_TRANSITION);
                            self.apply(&path, command).await;
                        }
                    }
                }
            }
            Input::AutoSwitch(false) => {
                log::info!("Auto switch off: every entity out of auto");
                let paths: Vec<String> = self.exposed.iter().cloned().collect();
                for path in paths {
                    self.set_auto(&path, false);
                }
            }
        }
    }

    async fn handle_change(&mut self, path: &str, change: Change) {
        if self.model.entity_kind(path).is_none() {
            log::warn!("HomeKit changed unknown entity {path}");
            return;
        }
        let was_on = self.state(path).on;
        match change {
            Change::On(true) if !was_on => {
                // Switching on is not an override; off-and-on again resumes auto.
                self.set_auto(path, true);
                let mut command = self.auto_values(path);
                command.on = Some(true);
                self.apply(path, command).await;
            }
            Change::On(true) => {}
            Change::On(false) => self.apply(path, LightCommand::off()).await,
            Change::Brightness(brightness) if brightness <= 0.0 => {
                self.apply(path, LightCommand::off()).await
            }
            Change::Brightness(brightness) => {
                self.set_auto(path, false);
                let command = LightCommand {
                    on: Some(true),
                    brightness: Some(brightness),
                    ..Default::default()
                };
                self.apply(path, command).await;
            }
            Change::Kelvin(kelvin) => {
                self.set_auto(path, false);
                if was_on {
                    self.apply(
                        path,
                        LightCommand {
                            kelvin: Some(kelvin),
                            ..Default::default()
                        },
                    )
                    .await;
                } else {
                    self.update_state(path, |state| state.kelvin = Some(kelvin));
                    self.publish(path);
                }
            }
        }
    }

    pub(super) async fn handle_adapter_event(&mut self, event: AdapterEvent) {
        match event {
            AdapterEvent::Light { device, report } => self.on_light_report(&device, &report).await,
            AdapterEvent::Device {
                device,
                event,
                data,
            } => self.on_device_event(&device, event, data).await,
        }
    }

    /// A light reported its state, e.g. after a wall switch or another app
    /// changed it. Reports never change auto mode, but a light in auto that
    /// comes on gets the current auto values.
    async fn on_light_report(&mut self, device: &DeviceRef, report: &LightReport) {
        let Some(light) = self.model.light_by_device(device) else {
            return;
        };
        let path = light.path.clone();
        let was_on = self.state(&path).on;
        self.update_state(&path, |state| state.record_report(report));
        self.publish(&path);
        for group in self.groups_of_light.get(&path).cloned().unwrap_or_default() {
            self.refresh_group(&group);
        }
        if !was_on && self.state(&path).on {
            self.resume_auto(&path).await;
        }
    }

    /// A light in auto that came on outside Knut gets the values the last
    /// `adapt` asked for: its own, or else those of its first group in auto,
    /// through that group's brightness mapping.
    async fn resume_auto(&mut self, light: &str) {
        if !self.is_auto(light) {
            return;
        }
        let sets_values =
            |command: &LightCommand| command.brightness.is_some() || command.kelvin.is_some();
        let own = self.auto_values(light);
        if sets_values(&own) {
            self.apply(light, own).await;
            return;
        }
        let groups = self.groups_of_light.get(light).cloned().unwrap_or_default();
        let group_values = groups
            .iter()
            .filter(|group| self.is_auto(group))
            .map(|group| (group, self.auto_values(group)))
            .find(|(_, command)| sets_values(command));
        let Some((group, command)) = group_values else {
            return;
        };
        let member = control::commands_for(&self.model, group, &command, None)
            .into_iter()
            .find(|member| member.path == light);
        if let Some(member) = member {
            self.apply(light, member.command).await;
        }
    }

    /// Runs the triggers listening for `event` from a sensor or light.
    async fn on_device_event(&mut self, device: &DeviceRef, event: String, data: Value) {
        let Some(path) = self.device_path(device) else {
            return;
        };
        let triggers: Vec<(usize, String)> = self
            .model
            .device_triggers(&path, &event)
            .map(|trigger| (trigger.proc_id, trigger.line.clone()))
            .collect();
        if triggers.is_empty() {
            return;
        }
        let args = BlockArgs::Device {
            path,
            event,
            data,
            now: Local::now(),
        };
        for (proc_id, line) in triggers {
            self.run_trigger(proc_id, &args, &line).await;
        }
    }

    /// The path of the sensor or light behind `device`.
    fn device_path(&self, device: &DeviceRef) -> Option<String> {
        let sensor = self
            .model
            .sensor_by_device(device)
            .map(|sensor| &sensor.path);
        let light = self.model.light_by_device(device).map(|light| &light.path);
        sensor.or(light).cloned()
    }
}
