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
            Input::AutoSwitch(true) => self.put_all_into_auto().await,
            Input::AutoSwitch(false) => self.take_all_out_of_auto(),
        }
    }

    /// The Auto switch went on: every entity is in auto again, and those
    /// that are on fade to the auto values.
    async fn put_all_into_auto(&mut self) {
        log::info!("Auto switch on: every entity back into auto");
        let paths: Vec<String> = self.exposed.iter().cloned().collect();
        for path in paths {
            self.set_auto(&path, true);
            if !self.state(&path).on {
                continue;
            }
            let command = self.auto_values(&path);
            if command.sets_level() {
                let command = LightCommand {
                    transition: Some(AUTO_RESUME_TRANSITION),
                    ..command
                };
                self.apply(&path, command).await;
            }
        }
    }

    fn take_all_out_of_auto(&mut self) {
        log::info!("Auto switch off: every entity out of auto");
        let paths: Vec<String> = self.exposed.iter().cloned().collect();
        for path in paths {
            self.set_auto(&path, false);
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
                self.change_kelvin(path, kelvin, was_on).await;
            }
        }
    }

    /// A light that is off keeps the colour temperature for when it comes
    /// on; HomeKit lets the user pick it without switching on.
    async fn change_kelvin(&mut self, path: &str, kelvin: f64, was_on: bool) {
        if was_on {
            let command = LightCommand {
                kelvin: Some(kelvin),
                ..Default::default()
            };
            self.apply(path, command).await;
        } else {
            self.update_state(path, |state| state.kelvin = Some(kelvin));
            self.publish(path);
        }
    }

    pub(super) async fn handle_adapter_event(&mut self, event: AdapterEvent) {
        match event {
            AdapterEvent::Light { device, report } => self.on_light_report(&device, &report).await,
            AdapterEvent::Unavailable { device } => self.on_light_unavailable(&device),
            AdapterEvent::Device {
                device,
                event,
                data,
            } => self.on_device_event(&device, event, data).await,
        }
    }

    /// A light reported its state, e.g. after a wall switch or another app
    /// changed it. Reports never change auto mode, but a light in auto that
    /// comes on gets the current auto values. A light that reports on after
    /// being unavailable counts as coming on: a wall switch gave it power.
    async fn on_light_report(&mut self, device: &DeviceRef, report: &LightReport) {
        let Some(light) = self.model.light_by_device(device) else {
            return;
        };
        let path = light.path.clone();
        let before = self.state(&path);
        self.update_state(&path, |state| state.record_report(report));
        self.publish(&path);
        self.refresh_groups_of(&path);

        let was_dark = !before.on || !before.available;
        let came_on = was_dark && self.state(&path).on;
        if came_on {
            self.resume_auto(&path).await;
        }
    }

    /// A light lost power or restarted. Knut keeps its last known state but
    /// sends it nothing until it reports again.
    fn on_light_unavailable(&mut self, device: &DeviceRef) {
        let Some(light) = self.model.light_by_device(device) else {
            return;
        };
        let path = light.path.clone();
        if !self.state(&path).available {
            return;
        }

        log::info!("{path}: unavailable");
        self.update_state(&path, |state| state.available = false);
        self.publish(&path);
        self.refresh_groups_of(&path);
    }

    /// A light in auto that came on outside Knut gets the values the last
    /// `adapt` asked for: its own, or else those of its first group in auto,
    /// through that group's brightness mapping.
    async fn resume_auto(&mut self, light: &str) {
        if !self.is_auto(light) {
            return;
        }
        let own = self.auto_values(light);
        let command = if own.sets_level() {
            Some(own)
        } else {
            self.auto_values_through_group(light)
        };
        if let Some(command) = command {
            self.apply(light, command).await;
        }
    }

    /// What `light` gets from its first group in auto that has auto values.
    fn auto_values_through_group(&self, light: &str) -> Option<LightCommand> {
        let groups = self.groups_of_light.get(light)?;
        let (group, command) = groups
            .iter()
            .filter(|group| self.is_auto(group))
            .map(|group| (group, self.auto_values(group)))
            .find(|(_, command)| command.sets_level())?;

        let members = control::commands_for(&self.model, group, &command, None);
        let member = members.into_iter().find(|member| member.path == light)?;
        Some(member.command)
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
