//! State that survives restarts, kept as JSON under the state directory
//! (`/var/lib/knut` by default). HomeKit pairing keys live next to it in
//! `homekit/`, managed by the hap crate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct State {
    /// Auto (true) or overridden (false), per HomeKit-controlled entity.
    #[serde(default)]
    pub auto: BTreeMap<String, bool>,
    /// HomeKit accessory id per entity path or scene. They must stay stable,
    /// otherwise the Home app forgets rooms and favourites.
    #[serde(default)]
    pub accessory_ids: BTreeMap<String, u64>,
    #[serde(default)]
    pub homekit_pin: Option<String>,
}

pub(crate) struct Store {
    path: PathBuf,
    pub state: State,
}

impl Store {
    pub(crate) fn open(state_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("cannot create state directory {}", state_dir.display()))?;
        let path = state_dir.join("state.json");
        let state = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("{} is corrupt", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
        };
        Ok(Self { path, state })
    }

    /// Writes atomically, so a crash never leaves a half-written file.
    pub(crate) fn save(&self) -> Result<()> {
        let temporary = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&temporary, text).with_context(|| format!("cannot write {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path).with_context(|| format!("cannot replace {}", self.path.display()))
    }

    /// The accessory id for `key`, allocating a new one if needed. Id 1 is
    /// the bridge itself.
    pub(crate) fn accessory_id(&mut self, key: &str) -> u64 {
        if let Some(id) = self.state.accessory_ids.get(key) {
            return *id;
        }
        let id = self.state.accessory_ids.values().copied().max().unwrap_or(1) + 1;
        self.state.accessory_ids.insert(key.to_string(), id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_accessory_ids_stable() {
        let dir = std::env::temp_dir().join(format!("knut-store-test-{}", std::process::id()));
        let mut store = Store::open(&dir).unwrap();
        let a = store.accessory_id("Küche/Spots");
        let b = store.accessory_id("Flur/Licht");
        assert_eq!((a, b), (2, 3));
        store.state.auto.insert("Küche/Spots".into(), false);
        store.save().unwrap();

        let mut reopened = Store::open(&dir).unwrap();
        assert_eq!(reopened.accessory_id("Küche/Spots"), 2);
        assert_eq!(reopened.state.auto["Küche/Spots"], false);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
