//! `~/.macatrium.json` — machine-local user settings (source locations + tool
//! paths), configured once rather than per-build. These are deliberately NOT in
//! `BuildConfig` (which is a portable, shareable build recipe): a MacPack folder
//! or rb-cli path is specific to one machine. The build reads `macpack_dir` to
//! resolve donor disks referenced by their original filename (e.g. `boot.vhd`).

use crate::config::Dependency;
use crate::targets::Target;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct Settings {
    /// Folder holding the MacPack donor disks (`boot.vhd`, `Supplement.vhd`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macpack_dir: Option<PathBuf>,
    /// Macintosh Garden archive (MG-Archive) root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mg_archive: Option<PathBuf>,
    /// rb-cli binary path (HFS I/O); falls back to `rb-cli` on PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rb_cli: Option<String>,
    /// Download / work cache dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<PathBuf>,
    /// Curated overlay (`data/curated.jsonl`) the GUI pins per-title Macintosh
    /// Garden download picks (`mg.files`) into. Blank/None disables pinning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curated_overlay: Option<PathBuf>,
    /// User-defined build [Targets](crate::targets), keyed by display name. These
    /// overlay the bundled defaults (a user target wins on a name collision) — see
    /// [`targets::Registry::load_default`](crate::targets::Registry::load_default).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<String, Target>,
    /// User-defined runtime [dependencies](crate::config::Dependency), keyed by
    /// dep-id. These overlay the bundled registry (a user entry wins on an id
    /// collision) — see [`config::dependencies`](crate::config::dependencies).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, Dependency>,
}

impl Settings {
    /// Load from `path`; an absent or unparseable file yields defaults (not an
    /// error — first run has no config).
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn load_default() -> Settings {
        Settings::load(&default_path())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    }
}

/// `$MACATRIUM_CONFIG`, else `~/.macatrium.json`.
pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("MACATRIUM_CONFIG") {
        return PathBuf::from(p);
    }
    home().unwrap_or_else(|| PathBuf::from(".")).join(".macatrium.json")
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_omits_none() {
        let dir = std::env::temp_dir();
        let p = dir.join("atrium_settings_test.json");
        let mut s = Settings::default();
        s.macpack_dir = Some(PathBuf::from("/m/pack"));
        s.save(&p).unwrap();
        let txt = std::fs::read_to_string(&p).unwrap();
        assert!(txt.contains("macpack_dir"));
        assert!(!txt.contains("mg_archive"), "None fields are omitted");
        let back = Settings::load(&p);
        assert_eq!(back.macpack_dir, Some(PathBuf::from("/m/pack")));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn absent_file_is_default_not_error() {
        let s = Settings::load(Path::new("/no/such/macatrium.json"));
        assert!(s.macpack_dir.is_none());
    }
}
