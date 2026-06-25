//! `atrium::templates` — base-OS template registry (model + controller).
//!
//! One place that defines the base-OS images: each entry maps an OS key
//! ("6.0.8", "7.1", …) to a base `.hda` and how the launcher is deployed on it
//! (finder_replace for System 6, Startup Items for System 7). A build's `base_os`
//! field is resolved against this registry, so the views (CLI/GUI) only pick a
//! key. The registry is data — `data/templates.json` — so it's configurable and
//! importable. An explicit `system` in the config always overrides the registry.

use crate::config::{d_startup, BuildConfig};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Clone)]
pub struct Template {
    /// Base bootable System image for this OS.
    pub hda: PathBuf,
    /// Human label (e.g. "System 6.0.8").
    #[serde(default)]
    pub label: String,
    /// Deploy the launcher AS the Finder (System 6) vs into Startup Items (Sys 7).
    #[serde(default)]
    pub finder_replace: bool,
    /// Startup Items folder used when not finder_replace.
    #[serde(default = "d_startup")]
    pub startup_items: String,
}

/// The registry: OS key -> template. A JSON object file keyed by OS string.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Registry(pub BTreeMap<String, Template>);

impl Registry {
    pub fn load(path: &Path) -> Result<Registry> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("reading template registry {}", path.display()))?;
        serde_json::from_str(&txt)
            .with_context(|| format!("parsing template registry {}", path.display()))
    }
    /// Load the default registry; an empty registry if the file is absent (so the
    /// views can show "no templates configured" rather than erroring).
    pub fn load_default() -> Registry {
        let p = default_registry_path();
        Registry::load(&p).unwrap_or_default()
    }
    pub fn get(&self, os: &str) -> Option<&Template> {
        self.0.get(os)
    }
    /// OS keys, sorted (BTreeMap order) — for a GUI dropdown / CLI listing.
    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// Registry path: `$MACATRIUM_TEMPLATES`, else `data/templates.json`.
pub fn default_registry_path() -> PathBuf {
    if let Ok(p) = std::env::var("MACATRIUM_TEMPLATES") {
        return PathBuf::from(p);
    }
    PathBuf::from("data/templates.json")
}

/// Resolve `base_os` against the registry, filling `system` + deploy mode. An
/// explicit `system` wins (registry untouched). Returns an owned, fully-resolved
/// config the controller can rely on (`system` guaranteed `Some`).
pub fn resolve(cfg: &BuildConfig) -> Result<BuildConfig> {
    let mut out = cfg.clone();
    if out.system.is_none() {
        let os = match &out.base_os {
            Some(os) => os.clone(),
            None => bail!("no base system: set `system`, or `base_os` with a template registry"),
        };
        let reg = Registry::load(&default_registry_path())?;
        let t = reg.get(&os).ok_or_else(|| {
            anyhow::anyhow!("unknown base_os {:?} — not in the template registry", os)
        })?;
        out.system = Some(t.hda.clone());
        out.finder_replace = t.finder_replace;
        out.startup_items = t.startup_items.clone();
    }
    Ok(out)
}
