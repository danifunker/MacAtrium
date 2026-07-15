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

/// A donor: a disk-image path, optionally flagged as a read-only **reservoir**
/// (an already-installed content store, e.g. a hand-built `donor.hfv`). A build
/// copies a reservoir donor's selected folders **verbatim** (`rb-cli cp`),
/// whereas it *harvests* a MacPack donor (re-picking the launch `APPL` and
/// renaming the folder to it). Serde is `untagged`, so the legacy bare-string
/// form (`"key": "path"`) still parses as a harvest donor.
#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Donor {
    /// A bare path string — a MacPack-style harvest donor (the original format).
    Path(PathBuf),
    /// `{ "path": …, "reservoir": true }` — an installed-content reservoir.
    Full {
        path: PathBuf,
        #[serde(default)]
        reservoir: bool,
    },
}

impl Donor {
    /// The disk-image path, whichever form the entry took.
    pub fn path(&self) -> &Path {
        match self {
            Donor::Path(p) => p,
            Donor::Full { path, .. } => path,
        }
    }
    /// Whether this donor is a verbatim-copy reservoir (vs a harvest donor).
    pub fn reservoir(&self) -> bool {
        matches!(self, Donor::Full { reservoir: true, .. })
    }
}

/// Donor key -> [`Donor`].
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Registry(pub BTreeMap<String, Donor>);

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
    pub fn get(&self, key: &str) -> Option<&Donor> {
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
