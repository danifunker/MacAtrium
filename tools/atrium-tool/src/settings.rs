//! `~/.macatrium.json` — machine-local user settings (source locations + tool
//! paths), configured once rather than per-build. These are deliberately NOT in
//! `BuildConfig` (which is a portable, shareable build recipe): a MacPack folder
//! or rb-cli path is specific to one machine. The build reads `macpack_dir` to
//! resolve donor disks referenced by their original filename (e.g. `boot.vhd`).

use crate::config::Dependency;
use crate::donors::Donor;
use crate::targets::Target;
use crate::templates::Template;
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
    /// User-defined base-OS [Templates](crate::templates), keyed by OS key ("7.1").
    /// These overlay `data/templates.json` (a user entry wins on a key collision) —
    /// see [`templates::Registry::load_default`](crate::templates::Registry::load_default).
    ///
    /// This is the *only* place a template can live for an **installed** app: the
    /// file registry resolves a relative `data/templates.json`, which exists only
    /// when the tool runs from a repo checkout.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub templates: BTreeMap<String, Template>,
    /// User-defined [donor](crate::donors::Donor) images, keyed by donor key. These
    /// overlay `data/donors.json` (a user entry wins) for the same reason as
    /// [`Self::templates`] — see
    /// [`donors::Registry::load_default`](crate::donors::Registry::load_default).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub donors: BTreeMap<String, Donor>,
    /// Where the user's saved [collections](crate::collections) live. `None` =
    /// `<Documents>/MacAtrium/Collections`. Set it to a repo checkout's
    /// `data/collections` to edit the committed ones in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections_dir: Option<PathBuf>,
    /// The user's own library records — titles they imported from a capture
    /// ([`crate::import`]) rather than ones shipped in the compiled-in library.
    /// `None` = `<Documents>/MacAtrium/library.jsonl`. Layered OVER the embedded
    /// library so an imported title shows up without touching the repo dataset,
    /// and survives an app update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_library: Option<PathBuf>,
    /// Where built disk images are written. `None` = [`default_output_dir`].
    /// Kept out of Documents by default — see that function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,
    /// The root of the user's **source** library: base-OS templates, donor and
    /// reservoir images, hand-made `.mar` captures, the MacPack set. `None` =
    /// [`default_sources_dir`]. Conventional subfolders are [`SOURCE_SUBDIRS`];
    /// nothing enforces them, they just give the file pickers a sane starting
    /// point and the user one place to drop new material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources_dir: Option<PathBuf>,
    /// The revision of the Manager's first-run setup the user has been through
    /// (`None` = never). The GUI compares it against its own current revision and
    /// only shows the wizard when this is behind, so finishing *or* skipping setup
    /// makes it stay gone — while a later version that needs something new can ask
    /// once more by bumping its revision.
    ///
    /// Deliberately a revision rather than a bool: "seen it" is not the same as
    /// "seen the current one", and a plain flag can never re-prompt. Unused by the
    /// CLI, which has no wizard, but it belongs with the other machine-local state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_seen: Option<u32>,
}

/// The conventional subfolders under [`Settings::sources_dir`] — `(name, what
/// goes in it)`. Advisory: a template or donor may be configured to point
/// anywhere, but these are what the app creates and where its pickers open.
pub const SOURCE_SUBDIRS: &[(&str, &str)] = &[
    ("Templates", "base OS disk images (System 6.0.8 / 7.1 / 7.5.5 / 7.6.1)"),
    ("Donors", "donor + reservoir images titles are copied or harvested from"),
    ("Captures", "hand-made .mar captures of apps installed in an emulator"),
    ("MacPack", "the MacPack donor set"),
];

/// Default output root: `<home>/MacAtrium/Images`.
///
/// Deliberately **not** under [`user_root`]'s Documents folder: a built image is
/// hundreds of MB and Windows Documents is often OneDrive-backed, so defaulting
/// there would push every build into cloud sync.
pub fn default_output_dir() -> PathBuf {
    home().unwrap_or_else(std::env::temp_dir).join("MacAtrium").join("Images")
}

/// Default sources root: `<home>/MacAtrium/Sources` — same reasoning as
/// [`default_output_dir`]; donor disks and MacPack are GB-scale.
pub fn default_sources_dir() -> PathBuf {
    home().unwrap_or_else(std::env::temp_dir).join("MacAtrium").join("Sources")
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

    /// The user's imported-title library: [`Self::user_library`], else
    /// `<Documents>/MacAtrium/library.jsonl`.
    pub fn user_library(&self) -> PathBuf {
        self.user_library.clone().unwrap_or_else(|| user_root().join("library.jsonl"))
    }

    /// Where imported captures are expanded: `<sources>/Captures/_staged`. Kept
    /// inside the source tree (it IS source material) but under a `_`-prefixed
    /// folder so it doesn't look like a capture the user dropped there.
    pub fn import_stage_dir(&self) -> PathBuf {
        self.source_subdir("Captures").join("_staged")
    }

    /// Where built disk images go: [`Self::output_dir`] or [`default_output_dir`].
    pub fn output_dir(&self) -> PathBuf {
        self.output_dir.clone().unwrap_or_else(default_output_dir)
    }

    /// The source-library root: [`Self::sources_dir`] or [`default_sources_dir`].
    pub fn sources_dir(&self) -> PathBuf {
        self.sources_dir.clone().unwrap_or_else(default_sources_dir)
    }

    /// A conventional subfolder of the source library (see [`SOURCE_SUBDIRS`]),
    /// e.g. `source_subdir("Templates")`. Not created by this call.
    pub fn source_subdir(&self, name: &str) -> PathBuf {
        self.sources_dir().join(name)
    }

    /// Create the output dir and the whole source tree, so the folders exist for
    /// the user to drop files into and for the pickers to open. Returns the dirs
    /// created or already present, in display order.
    pub fn ensure_dirs(&self) -> Result<Vec<PathBuf>> {
        let mut made = vec![self.output_dir(), self.sources_dir()];
        made.extend(SOURCE_SUBDIRS.iter().map(|(n, _)| self.source_subdir(n)));
        for d in &made {
            std::fs::create_dir_all(d)
                .with_context(|| format!("creating {}", d.display()))?;
        }
        Ok(made)
    }
}

/// `$MACATRIUM_CONFIG`, else `~/.macatrium.json`.
pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("MACATRIUM_CONFIG") {
        return PathBuf::from(p);
    }
    home().unwrap_or_else(|| PathBuf::from(".")).join(".macatrium.json")
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The user's Documents folder, or `None` if none of the usual spots exist.
///
/// Resolved without a platform dependency, first hit wins: `$XDG_DOCUMENTS_DIR`,
/// `<home>/Documents`, then `<home>/OneDrive/Documents` (Windows folder
/// redirection moves the real Documents there, leaving no plain `Documents`).
/// Only *existing* directories count — a guessed path that isn't there is worse
/// than admitting we don't know.
pub fn documents_dir() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(x) = std::env::var_os("XDG_DOCUMENTS_DIR") {
        cands.push(PathBuf::from(x));
    }
    if let Some(h) = home() {
        cands.push(h.join("Documents"));
        cands.push(h.join("OneDrive").join("Documents"));
    }
    cands.into_iter().find(|p| p.is_dir())
}

/// The user-facing data root: `<Documents>/MacAtrium`, falling back to
/// `<home>/.macatrium` where there's no Documents folder.
///
/// This is for **small things the user authors** — saved collections, build
/// configs — which belong somewhere discoverable and backed up. Built disk
/// images deliberately do NOT live here: they run to hundreds of MB, and on
/// Windows Documents is frequently OneDrive-backed, so a default output path
/// under it would silently push a ~750 MB image into cloud sync.
pub fn user_root() -> PathBuf {
    documents_dir()
        .map(|d| d.join("MacAtrium"))
        .unwrap_or_else(|| home().unwrap_or_else(|| PathBuf::from(".")).join(".macatrium"))
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

    /// `user_root` must be absolute and must not sit inside the install dir —
    /// it's where the user's own files go, so it has to survive an app update
    /// and be somewhere they can actually find.
    #[test]
    fn user_root_is_an_absolute_user_owned_path() {
        let root = user_root();
        assert!(root.is_absolute() || home().is_none(), "user_root must be absolute: {}", root.display());
        assert!(
            root.ends_with("MacAtrium") || root.ends_with(".macatrium"),
            "unexpected user root: {}",
            root.display()
        );
        // documents_dir only ever reports a directory that exists.
        if let Some(d) = documents_dir() {
            assert!(d.is_dir(), "documents_dir returned a non-directory: {}", d.display());
        }
    }

    /// The two roots the user is asked for round-trip, and the source tree is
    /// laid out under the configured root rather than the default one.
    #[test]
    fn output_and_source_roots_round_trip_and_drive_the_tree() {
        let p = std::env::temp_dir().join("atrium_settings_roots_test.json");
        let mut s = Settings::default();
        // Unset: sensible per-user defaults, never a relative path.
        assert!(s.output_dir().is_absolute() || home().is_none());
        assert!(s.sources_dir().is_absolute() || home().is_none());

        s.output_dir = Some(PathBuf::from("/vol/builds"));
        s.sources_dir = Some(PathBuf::from("/vol/sources"));
        s.save(&p).unwrap();
        let back = Settings::load(&p);
        assert_eq!(back.output_dir(), PathBuf::from("/vol/builds"));
        assert_eq!(back.source_subdir("Templates"), PathBuf::from("/vol/sources/Templates"));
        assert_eq!(back.source_subdir("Captures"), PathBuf::from("/vol/sources/Captures"));
        // Every advertised subfolder is one `ensure_dirs` would create.
        for (name, _) in SOURCE_SUBDIRS {
            assert!(back.source_subdir(name).starts_with("/vol/sources"));
        }
        let _ = std::fs::remove_file(&p);
    }

    /// `ensure_dirs` creates the output folder and the whole source tree.
    #[test]
    fn ensure_dirs_creates_output_and_every_source_subfolder() {
        let root = std::env::temp_dir().join("atrium_ensure_dirs_test");
        let _ = std::fs::remove_dir_all(&root);
        let mut s = Settings::default();
        s.output_dir = Some(root.join("Images"));
        s.sources_dir = Some(root.join("Sources"));
        let made = s.ensure_dirs().unwrap();
        assert!(root.join("Images").is_dir());
        for (name, _) in SOURCE_SUBDIRS {
            assert!(root.join("Sources").join(name).is_dir(), "missing {name}");
        }
        assert_eq!(made.len(), 2 + SOURCE_SUBDIRS.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The machine-local registries an *installed* app can't reach on disk
    /// (templates / donors / collections dir) round-trip through the settings file,
    /// and stay omitted when empty so a first-run config isn't full of noise.
    #[test]
    fn machine_local_registries_round_trip() {
        let p = std::env::temp_dir().join("atrium_settings_registries_test.json");
        let mut s = Settings::default();
        s.templates.insert(
            "7.1".into(),
            Template {
                hda: PathBuf::from(r"C:\Temp\MacAtrium_Sys-QT_761.hda"),
                label: "System 7.1".into(),
                finder_replace: false,
                startup_items: "/System Folder/Startup Items".into(),
            },
        );
        s.donors.insert("macgarden".into(), Donor::Full {
            path: PathBuf::from(r"C:\Temp\macatrium-build\donor.hfv"),
            reservoir: true,
        });
        s.collections_dir = Some(PathBuf::from(r"C:\Temp\collections"));
        s.save(&p).unwrap();

        let back = Settings::load(&p);
        assert_eq!(back.templates.get("7.1").map(|t| t.hda.clone()),
                   Some(PathBuf::from(r"C:\Temp\MacAtrium_Sys-QT_761.hda")));
        assert!(back.donors.get("macgarden").is_some_and(Donor::reservoir),
                "the reservoir flag must survive the round trip — a harvest donor would \
                 re-pick the launch APPL and rename folders");
        assert_eq!(back.collections_dir, Some(PathBuf::from(r"C:\Temp\collections")));

        // Empty registries stay out of the file.
        let txt = std::fs::read_to_string(&p).unwrap();
        assert!(!txt.contains("dependencies"), "empty maps are skipped");
        let _ = std::fs::remove_file(&p);
    }
}
