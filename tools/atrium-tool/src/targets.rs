//! `atrium::targets` — build **Targets** (model + registry + controller).
//!
//! A **Target** is a named build *profile*: it references a base-OS
//! [`Template`](crate::templates) (by its `base_os` key) and pins the machine
//! settings a build for that hardware needs — art depths, the launcher `'SIZE'`
//! partition, and (future) screen size / disk size. Picking a Target fills those
//! [`BuildConfig`] fields, so a user chooses *"Mac Plus / SE (B&W)"* rather than
//! hand-setting `base_os = 6.0.8`, `art_depths = ["1"]`, `app_mem_kb = [512,384]`.
//!
//! Targets layer like everything else in this tool: a set of **bundled defaults**
//! ([`EMBEDDED_TARGETS`], the committed `data/targets.json`, baked in at compile
//! time) plus **user targets** stored in `~/.macatrium.json`
//! ([`Settings::targets`](crate::settings::Settings)). A user target wins over a
//! bundled one with the same name. [`Registry::load_default`] is the merge of the
//! two, the same shape the GUI's Target combo and the CLI `targets` verb read.

use crate::config::BuildConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The committed bundled default targets (`data/targets.json`), embedded so the
/// tool ships sensible profiles with no user config — mirrors the launcher /
/// library / compatibility embeds in [`crate::config`].
pub const EMBEDDED_TARGETS: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/targets.json"));

/// A named build profile. The fields it sets on a [`BuildConfig`] are exactly the
/// machine knobs a non-expert shouldn't have to reason about per build. All but
/// `base_os` are optional so a Target can pin only what differs from the build's
/// own defaults.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Target {
    /// Template key resolved against the [template registry](crate::templates)
    /// (e.g. "6.0.8", "7.1") — the base OS this profile builds on.
    pub base_os: String,
    /// Art-depth variants to bake (e.g. `["1"]` for B&W, `["1","8"]` for colour).
    /// Empty leaves the build's own `art_depths` untouched.
    #[serde(default)]
    pub art_depths: Vec<String>,
    /// Launcher memory partition `[preferred_kb, minimum_kb]` baked into `'SIZE'`
    /// (-1). `None` keeps the launcher binary's built-in default.
    #[serde(default)]
    pub app_mem_kb: Option<[u32; 2]>,
    /// Final image size in MB (grown via `rb-cli expand`). `None` keeps base size.
    #[serde(default)]
    pub disk_size_mb: Option<u64>,
    /// Art downscale box `"WxH"` (a future "screen size" knob). `None` = default.
    #[serde(default)]
    pub max_art_size: Option<String>,
    /// Human description shown in the picker (e.g. hardware + colour summary).
    #[serde(default)]
    pub label: String,
}

impl Target {
    /// Stamp this profile's pinned settings onto a [`BuildConfig`]. Sets `base_os`
    /// (and clears any explicit `system` so the template registry resolves it), and
    /// overwrites only the machine knobs the profile actually pins — leaving the
    /// build's selection, output, content sources, etc. alone.
    pub fn apply_to(&self, c: &mut BuildConfig) {
        c.base_os = Some(self.base_os.clone());
        c.system = None; // let templates::resolve fill it from base_os
        if !self.art_depths.is_empty() {
            c.art_depths = self.art_depths.clone();
        }
        if self.app_mem_kb.is_some() {
            c.app_mem_kb = self.app_mem_kb;
        }
        if self.disk_size_mb.is_some() {
            c.disk_size_mb = self.disk_size_mb;
        }
        if self.max_art_size.is_some() {
            c.max_art_size = self.max_art_size.clone();
        }
    }
}

/// The Targets registry: name → [`Target`]. A JSON object keyed by display name,
/// same shape as `data/targets.json` and `Settings::targets`.
#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct Registry(pub BTreeMap<String, Target>);

impl Registry {
    /// The bundled default targets baked into the tool ([`EMBEDDED_TARGETS`]).
    pub fn bundled() -> Registry {
        serde_json::from_slice(EMBEDDED_TARGETS)
            .expect("bundled data/targets.json is valid (checked by a unit test)")
    }

    /// Bundled defaults overlaid with the user's targets from `~/.macatrium.json`
    /// — the full registry the views (GUI combo, CLI `targets`) present. A user
    /// target replaces a bundled one of the same name.
    pub fn load_default() -> Registry {
        let mut reg = Registry::bundled();
        let settings = crate::settings::Settings::load_default();
        reg.0.extend(settings.targets);
        reg
    }

    pub fn get(&self, name: &str) -> Option<&Target> {
        self.0.get(name)
    }

    /// Target names, sorted (BTreeMap order) — for a GUI dropdown / CLI listing.
    pub fn names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_targets_parse_and_reference_real_templates() {
        let reg = Registry::bundled();
        assert!(!reg.0.is_empty(), "bundled targets.json is empty");
        // Every bundled target's base_os must be a key the template registry knows,
        // else picking it would fail at build time.
        let tmpls = crate::templates::Registry::load_default();
        for (name, t) in &reg.0 {
            assert!(!t.base_os.is_empty(), "{name}: empty base_os");
            // `load_default` reads data/templates.json which exists in-repo; if a
            // dev runs tests from elsewhere it may be empty — only assert when set.
            if !tmpls.0.is_empty() {
                assert!(
                    tmpls.get(&t.base_os).is_some(),
                    "{name}: base_os {:?} not in the template registry",
                    t.base_os
                );
            }
        }
    }

    #[test]
    fn apply_to_pins_machine_settings() {
        let t = Target {
            base_os: "6.0.8".into(),
            art_depths: vec!["1".into()],
            app_mem_kb: Some([512, 384]),
            disk_size_mb: Some(120),
            max_art_size: None,
            label: String::new(),
        };
        let mut c = BuildConfig {
            system: Some("/some/explicit.hda".into()),
            out: "/tmp/out.hda".into(),
            ..BuildConfig::default()
        };
        t.apply_to(&mut c);
        assert_eq!(c.base_os.as_deref(), Some("6.0.8"));
        assert!(c.system.is_none(), "apply clears explicit system so the template resolves");
        assert_eq!(c.art_depths, vec!["1".to_string()]);
        assert_eq!(c.app_mem_kb, Some([512, 384]));
        assert_eq!(c.disk_size_mb, Some(120));
        // out (and other build-owned fields) are untouched.
        assert_eq!(c.out, std::path::PathBuf::from("/tmp/out.hda"));
    }

    #[test]
    fn user_targets_override_bundled_by_name() {
        let mut reg = Registry::bundled();
        let name = reg.names()[0].clone();
        let custom = Target {
            base_os: "7.1".into(),
            art_depths: vec!["8".into()],
            app_mem_kb: None,
            disk_size_mb: None,
            max_art_size: None,
            label: "mine".into(),
        };
        reg.0.insert(name.clone(), custom.clone());
        assert_eq!(reg.get(&name), Some(&custom));
    }
}
