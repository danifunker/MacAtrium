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

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
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
    /// The file registry alone (`$MACATRIUM_TEMPLATES`, else `data/templates.json`);
    /// empty when the file is absent, which is the normal case for an **installed**
    /// app — that path is relative, so it only resolves from a repo checkout.
    pub fn bundled() -> Registry {
        Registry::load(&default_registry_path()).unwrap_or_default()
    }

    /// The file registry overlaid with the user's templates from `~/.macatrium.json`
    /// ([`Settings::templates`](crate::settings::Settings::templates)) — a user entry
    /// wins on a key collision. Mirrors [`targets::Registry::load_default`](crate::targets::Registry::load_default).
    ///
    /// An empty registry is not an error: the views show "no templates configured"
    /// and the Settings editor is how you add one.
    pub fn load_default() -> Registry {
        let mut reg = Registry::bundled();
        reg.0.extend(crate::settings::Settings::load_default().templates);
        reg
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
        let reg = Registry::load_default();
        let t = reg.get(&os).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown base_os {:?} — not in the template registry (have: {}). \
                 Add it under Settings → Templates, or set an explicit `system` path.",
                os,
                if reg.0.is_empty() { "none configured".to_string() } else { reg.keys().join(", ") }
            )
        })?;
        out.system = Some(t.hda.clone());
        out.finder_replace = t.finder_replace;
        out.startup_items = t.startup_items.clone();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user template from `~/.macatrium.json` wins over a file entry with the
    /// same key — the merge `load_default` performs. Exercised on the maps directly
    /// (like the Targets test) so it doesn't depend on the ambient settings file.
    #[test]
    fn user_templates_override_file_by_key() {
        let mut reg = Registry::default();
        reg.0.insert(
            "7.1".into(),
            Template {
                hda: "/repo/placeholder.hda".into(),
                label: "placeholder".into(),
                finder_replace: false,
                startup_items: d_startup(),
            },
        );
        let mine = Template {
            hda: r"C:\Temp\MacAtrium_Sys-QT_761.hda".into(),
            label: "System 7.1 (QuickTime 2.5)".into(),
            finder_replace: false,
            startup_items: d_startup(),
        };
        reg.0.extend([("7.1".to_string(), mine.clone())]);
        assert_eq!(reg.get("7.1"), Some(&mine));
    }

    /// An explicit `system` always wins, so a build can bypass the registry — the
    /// escape hatch when no template is configured.
    #[test]
    fn explicit_system_bypasses_the_registry() {
        let cfg = BuildConfig {
            system: Some("/explicit/base.hda".into()),
            base_os: Some("no-such-os".into()),
            ..BuildConfig::default()
        };
        let out = resolve(&cfg).expect("explicit system must not consult the registry");
        assert_eq!(out.system, Some("/explicit/base.hda".into()));
    }

    /// An empty registry is not an error — it's the normal state of an *installed*
    /// app, whose working directory has no `data/templates.json`. The failure must
    /// name the fix rather than surfacing a file-not-found.
    #[test]
    fn resolve_reports_an_empty_registry_helpfully() {
        let cfg = BuildConfig { base_os: Some("7.1".into()), ..BuildConfig::default() };
        // Only assert the message when nothing is configured; a dev running tests
        // from the repo root legitimately has templates on disk.
        if Registry::load_default().0.is_empty() {
            // Not `unwrap_err` — BuildConfig has no Debug impl.
            let err = match resolve(&cfg) {
                Ok(_) => panic!("an empty registry must not resolve a base_os"),
                Err(e) => e.to_string(),
            };
            assert!(err.contains("none configured"), "unexpected message: {err}");
            assert!(err.contains("Settings"), "the error should point at the fix: {err}");
        }
    }
}
