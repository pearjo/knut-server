//! What reaches the engine from outside: HomeKit changes and adapter events.

use chrono::Local;

use super::{Change, Engine, Input, AUTO_RESUME_TRANSITION};
use crate::adapter::{AdapterEvent, LightCommand};
use crate::model::TriggerKind;
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
            AdapterEvent::Light { device, report } => {
                let Some(light) = self.model.light_by_device(&device) else {
                    return;
                };
                let path = light.path.clone();
                self.update_state(&path, |state| {
                    if let Some(on) = report.on {
                        state.on = on;
                    }
                    if let Some(brightness) = report.brightness.filter(|b| *b > 0.0) {
                        state.brightness = brightness;
                    }
                    if report.kelvin.is_some() {
                        state.kelvin = report.kelvin;
                    }
                });
                self.publish(&path);
                for group in self.groups_of_light.get(&path).cloned().unwrap_or_default() {
                    self.refresh_group(&group);
                }
            }
            AdapterEvent::Device {
                device,
                event,
                data,
            } => {
                let path = match (
                    self.model.sensor_by_device(&device),
                    self.model.light_by_device(&device),
                ) {
                    (Some(sensor), _) => sensor.path.clone(),
                    (None, Some(light)) => light.path.clone(),
                    (None, None) => return,
                };
                let triggers: Vec<(usize, String)> = self
                    .model
                    .triggers
                    .iter()
                    .filter(|trigger| matches!(&trigger.kind, TriggerKind::Device { path: p, events } if *p == path && events.contains(&event)))
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
        }
    }
}
