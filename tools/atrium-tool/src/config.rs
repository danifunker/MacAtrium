//! `atrium::config` — the build **model**.
//!
//! `BuildConfig` is the single, shared description of an image build. Both views
//! construct it: the CLI deserializes it from `--config` JSON, the GUI builds it
//! field-by-field from its widgets. The controllers (`image`, `templates`,
//! `selection`, `preflight`) operate on it. Defining the schema **once** here is
//! what keeps the CLI and GUI in lock-step (no more re-encoding it as JSON in the
//! GUI). Serialize is derived too, so either view can write a build back to disk.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn d_startup() -> String { "/System Folder/Startup Items".into() }
pub fn d_platform() -> String { "Apple Mac OS".into() }
pub fn d_rbcli() -> String { "rb-cli".into() }
pub fn d_apps_root() -> String { "/MacAtrium/Apps".into() }
pub fn d_metadir() -> String { "/MacAtrium/metadata".into() }
pub fn d_imagesdir() -> String { "/MacAtrium/images".into() }
pub fn d_artdepth() -> String { "8".into() }
pub fn d_curl() -> String { "curl".into() }
pub fn d_sounds_dir() -> String { "/MacAtrium/sounds".into() }

/// Hard ceiling for a built image: classic HFS tops out at 2 GB in practice.
pub const MAX_DISK_MB: u64 = 2048;

/// One harvest source: a donor disk image plus the app paths to pull from it.
/// This is the low-level / manual selection path; `Selection` (below) is the
/// higher-level, dataset-driven way to pick apps.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct HarvestSrc {
    pub image: PathBuf,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub scan: Option<String>,
}

/// How to choose which dataset apps go into the image. Apps are matched against
/// the dataset and harvested from each record's `source` (donor image + path).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Selection {
    /// Everything in the dataset that is harvestable (has a `source`) and
    /// compatible with the target OS.
    All,
    /// An explicit list of dataset ids — the manual list (handy for testing).
    List {
        #[serde(default)]
        ids: Vec<String>,
    },
    /// Every app whose `categories`/`genre` intersects this list (optional facet).
    Categories {
        #[serde(default)]
        categories: Vec<String>,
    },
}

impl Default for Selection {
    fn default() -> Self {
        Selection::List { ids: Vec::new() }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct BuildConfig {
    /// Base bootable System image to build on top of. Either set this directly,
    /// or set `base_os` to resolve it (+ deploy mode) from the template registry.
    #[serde(default)]
    pub system: Option<PathBuf>,
    /// Template key (e.g. "6.0.8", "7.1") resolved against the template registry
    /// to fill in `system` + deploy mode when those aren't set explicitly.
    #[serde(default)]
    pub base_os: Option<String>,
    /// Output image to produce (overwritten).
    pub out: PathBuf,
    /// The launcher MacBinary (build/MacAtrium.bin) to install.
    pub launcher: PathBuf,
    /// Curated dataset (copied; never mutated by the build).
    pub dataset: PathBuf,
    /// Final image size in MB; the image is grown/kept to fit. Capped at
    /// `MAX_DISK_MB` (HFS 2 GB). None = leave the base image's size as-is.
    #[serde(default)]
    pub disk_size_mb: Option<u64>,
    /// Which dataset apps to include. None falls back to the `harvest` block.
    #[serde(default)]
    pub selection: Option<Selection>,
    #[serde(default = "d_startup")]
    pub startup_items: String,
    /// Manual overrides overlay (applied after enrich).
    #[serde(default)]
    pub overrides: Option<PathBuf>,
    /// LaunchBox Metadata.xml — if set, enrich the dataset.
    #[serde(default)]
    pub metadata: Option<PathBuf>,
    /// Macintosh Garden archive root — if set, enrich (68K-only) before LaunchBox
    /// and stage MG box-front/screenshot art for the art pass.
    #[serde(default)]
    pub mg_archive: Option<PathBuf>,
    #[serde(default = "d_platform")]
    pub platform: String,
    /// Auto-detect color/B&W from LaunchBox screenshots during enrich.
    #[serde(default)]
    pub detect_color: bool,
    #[serde(default = "d_curl")]
    pub curl: String,
    /// Apps to harvest from donor images into the output (low-level/manual path).
    #[serde(default)]
    pub harvest: Vec<HarvestSrc>,
    /// Directory of source artwork named `<id>.png` / `.jpg` — converted to PICT.
    #[serde(default)]
    pub art_dir: Option<PathBuf>,
    #[serde(default = "d_artdepth")]
    pub art_depth: String,
    /// Generate multiple depth variants (e.g. ["1","8"]) named `<id>.<depth>.pict`.
    #[serde(default)]
    pub art_depths: Vec<String>,
    /// Downscale art so its longest side is at most this many pixels.
    #[serde(default)]
    pub art_max: Option<u32>,
    /// Download Box-Front art from LaunchBox (needs `metadata`).
    #[serde(default)]
    pub download_art: bool,
    #[serde(default = "d_rbcli")]
    pub rb_cli: String,
    #[serde(default = "d_apps_root")]
    pub apps_root: String,
    #[serde(default = "d_metadir")]
    pub metadata_dir: String,
    #[serde(default = "d_imagesdir")]
    pub images_dir: String,
    /// Staging dir for intermediates (default: a temp dir).
    #[serde(default)]
    pub stage: Option<PathBuf>,
    /// Optional startup chime (PCM WAV) baked into the image.
    #[serde(default)]
    pub startup_sound: Option<PathBuf>,
    /// Optional shutdown chime (PCM WAV).
    #[serde(default)]
    pub shutdown_sound: Option<PathBuf>,
    /// Where the chimes live on the volume.
    #[serde(default = "d_sounds_dir")]
    pub sounds_dir: String,
    /// System 6 appliance: install the launcher *as* the Finder (FNDR/MACS) so the
    /// boot launches it as the shell. Leave false for the 7.x Startup-Items deploy.
    #[serde(default)]
    pub finder_replace: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            system: None,
            base_os: None,
            out: PathBuf::new(),
            launcher: PathBuf::new(),
            dataset: PathBuf::new(),
            disk_size_mb: None,
            selection: None,
            startup_items: d_startup(),
            overrides: None,
            metadata: None,
            mg_archive: None,
            platform: d_platform(),
            detect_color: false,
            curl: d_curl(),
            harvest: Vec::new(),
            art_dir: None,
            art_depth: d_artdepth(),
            art_depths: Vec::new(),
            art_max: None,
            download_art: false,
            rb_cli: d_rbcli(),
            apps_root: d_apps_root(),
            metadata_dir: d_metadir(),
            images_dir: d_imagesdir(),
            stage: None,
            startup_sound: None,
            shutdown_sound: None,
            sounds_dir: d_sounds_dir(),
            finder_replace: false,
        }
    }
}
