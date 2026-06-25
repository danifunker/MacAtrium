//! `atrium::donors` — donor-image registry (model + controller).
//!
//! Maps a logical donor key ("supplement", "pop", …) to a disk-image path, kept
//! in a shared JSON file (`data/donors.json`). The dataset's per-app `source`
//! references a key, not a machine path, so the dataset stays portable; only this
//! registry holds local paths. Both views load the same file.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Donor key -> disk-image path.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Registry(pub BTreeMap<String, PathBuf>);

impl Registry {
    pub fn load(path: &Path) -> Result<Registry> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("reading donor registry {}", path.display()))?;
        serde_json::from_str(&txt)
            .with_context(|| format!("parsing donor registry {}", path.display()))
    }
    /// Load the default registry; empty if the file is absent.
    pub fn load_default() -> Registry {
        Registry::load(&default_registry_path()).unwrap_or_default()
    }
    pub fn get(&self, key: &str) -> Option<&PathBuf> {
        self.0.get(key)
    }
    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// Registry path: `$MACATRIUM_DONORS`, else `data/donors.json`.
pub fn default_registry_path() -> PathBuf {
    if let Ok(p) = std::env::var("MACATRIUM_DONORS") {
        return PathBuf::from(p);
    }
    PathBuf::from("data/donors.json")
}
