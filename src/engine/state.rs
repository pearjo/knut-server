//! The engine's bookkeeping: entity state, auto flags, and what HomeKit and
//! scripts are told about them.

use serde_json::{json, Value};

use super::{Engine, EntityState, Update};
use crate::adapter::LightCommand;

impl Engine {
    pub(super) fn state(&self, path: &str) -> EntityState {
        self.states.get(path).cloned().unwrap_or_default()
    }

    pub(super) fn update_state(&mut self, path: &str, change: impl FnOnce(&mut EntityState)) {
        change(self.states.entry(path.to_string()).or_default());
    }

    /// A group shows on while any member is on.
    pub(super) fn refresh_group(&mut self, group: &str) {
        let Some(members) = self.model.groups.get(group).map(|g| g.members.clone()) else {
            return;
        };
        let any_on = members.iter().any(|m| self.state(&m.light).on);
        if self.state(group).on != any_on {
            self.update_state(group, |state| state.on = any_on);
            self.publish(group);
        }
    }

    /// Entities without an auto flag (hidden lights) always follow `adapt`.
    pub(super) fn is_auto(&self, path: &str) -> bool {
        self.store.state.auto.get(path).copied().unwrap_or(true)
    }

    pub(super) fn auto_values(&self, path: &str) -> LightCommand {
        if !self.is_auto(path) {
            return LightCommand::default();
        }
        let target = self.auto_targets.get(path).cloned().unwrap_or_default();
        LightCommand {
            brightness: target.brightness,
            kelvin: target.kelvin,
            ..Default::default()
        }
    }

    pub(super) fn set_auto(&mut self, path: &str, auto: bool) {
        if !self.exposed.contains(path) {
            return;
        }
        let was_all_auto = self.all_auto();
        let previous = self.store.state.auto.insert(path.to_string(), auto);
        if previous != Some(auto) {
            log::info!("{path}: {}", if auto { "auto" } else { "overridden" });
            self.store_dirty = true;
            if was_all_auto != self.all_auto() {
                let _ = self.updates.send(Update::Auto(self.all_auto()));
            }
        }
    }

    pub(super) fn publish(&self, path: &str) {
        if self.exposed.contains(path) {
            let _ = self.updates.send(Update::Entity {
                path: path.to_string(),
                state: self.state(path),
            });
        }
    }

    pub(super) fn publish_all(&self) {
        for path in &self.exposed {
            self.publish(path);
        }
        let _ = self.updates.send(Update::Auto(self.all_auto()));
    }

    /// Entity state for `state(path)` in scripts.
    pub(super) fn snapshot(&self) -> Value {
        let entries = self.states.iter().map(|(path, state)| {
            let fields = json!({
                "on": state.on,
                "brightness": state.brightness,
                "kelvin": state.kelvin,
                "auto": self.is_auto(path),
            });
            (path.clone(), fields)
        });
        Value::Object(entries.collect())
    }

    pub(super) fn save_if_dirty(&mut self) {
        if self.store_dirty {
            match self.store.save() {
                Ok(()) => self.store_dirty = false,
                Err(error) => log::error!("{error:#}"),
            }
        }
    }
}
