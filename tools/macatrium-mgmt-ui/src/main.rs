//! MacAtrium Management UI — an egui front-end for the MacAtrium build tooling.
//!
//! Every action here calls the `atrium` **library** — the exact functions the
//! CLI exposes — so the CLI stays the source of truth and this is just a nicer
//! way to drive it. The UI is organised around **jobs** a user actually does,
//! not the pipeline stages of the CLI:
//!
//!   * **Build** — pick a *Target* (a Mac profile) + the titles to include, write
//!     a fresh bootable MacAtrium disk. Plumbing lives behind **Advanced**.
//!   * **Add to disk** — extend an already-built MacAtrium disk with more titles.
//!   * **Library** — browse the bundled catalogue and edit each title's
//!     compatibility facets (Colour/B&W, Mouse, launch hotkey).
//!   * **Attain** — acquire the *source software*: register the MacPack folder,
//!     run the Macintosh Garden downloader (gated on a valid MG-Archive).
//!   * **⚙ Settings** — Targets & Templates, tool paths, MacPack / MG-Archive /
//!     cache locations; persisted to `~/.macatrium.json`. A first-run wizard
//!     auto-detects `rb-cli` and prompts for the source folders.
//!
//! Long operations (extract / enrich / download / build) run on a worker thread
//! so the window stays responsive.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use atrium::{
    config::{BuildConfig, HarvestSrc, Selection},
    fetch, image, merge, mg,
    rbcli::RbCli,
    settings::{self, Settings},
    targets::{self, Target},
    templates,
};
use eframe::egui;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MacAtrium Manager",
        opts,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
}

/// The job-based screens (the top tab bar).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Build,
    AddToDisk,
    Library,
    Attain,
    Settings,
}

/// One library row: identity + descriptive metadata, the compatibility facets a
/// user edits, and the `selected` flag the title picker toggles. `raw`-free — we
/// re-read the source on reload, and only facets are written back (via the
/// compatibility overlay), so dropping the other fields here loses nothing.
struct LibRow {
    id: String,
    name: String,
    kind: String,
    year: String,
    color: bool,    // true = Colour, false = B&W
    mouse: bool,    // true = Mouse Required
    hotkey: String, // single-char launch hotkey (gamepad button map), "" = none
    selected: bool, // included by the title picker
    dirty: bool,    // facet touched since last save
}

/// One harvest source for `atrium image`: a donor disk image plus the app
/// folders to pull from it (one path per line), or a `scan` glob.
#[derive(Default)]
struct HarvestUi {
    image: String,
    apps: String, // one app/folder path per line
    scan: String, // optional glob, e.g. "/Games/**"
}

struct App {
    tab: Tab,
    // ---- machine-local settings (~/.macatrium.json) ----
    settings: Settings,    // loaded at startup, the source of truth for the editor
    show_wizard: bool,     // first-run overlay
    macpack_dir: String,   // editor mirror of settings.macpack_dir
    cache_dir: String,     // editor mirror of settings.cache_dir
    // ---- Targets ----
    target_reg: targets::Registry, // bundled ⊕ user targets
    target_name: String,           // selected Target on the Build screen
    // target editor (Settings screen)
    te_name: String,
    te_base_os: String,
    te_depths: String, // "1,8"
    te_mem_pref: String,
    te_mem_min: String,
    te_label: String,
    // ---- the shared library (browse / pick / edit facets) ----
    library: Vec<LibRow>,
    library_loaded: bool,
    lib_search: String,
    lib_kind: String, // "" = all kinds
    build_pick: bool, // Build: false = All compatible, true = Pick titles
    // ---- shared paths / dataset editing ----
    rb_cli: String,
    metadata: String,   // LaunchBox Metadata.xml
    mg_archive: String, // local Macintosh Garden archive root
    image_path: String, // selected .hda (Library: Load Existing MacAtrium Disk)
    dataset: String,    // blank = the library bundled in the tool
    overrides: String,  // blank = the compatibility overlay bundled in the tool
    status: String,
    // ---- build image config (mirrors atrium image's BuildConfig) ----
    base_system: String,
    base_os: String,        // template key ("" = custom .hda)
    templates: Vec<String>, // OS keys from the template registry (combo)
    disk_size_mb: String,
    sel_mode: u8, // 0 harvest-list, 1 All, 2 Manual list, 3 By category
    sel_text: String,
    launcher: String,
    out_image: String,
    add_disk_path: String, // Add-to-disk: the existing MacAtrium .hda
    startup_items: String,
    startup_sound: String,
    shutdown_sound: String,
    platform: String,
    detect_color: bool,
    download_art: bool,
    art_dir: String,
    max_art_size: String,
    bw_only: bool,
    app_mem_pref: String,
    app_mem_min: String,
    d1: bool,
    d4: bool,
    d8: bool,
    d16: bool,
    d24: bool,
    harvest: Vec<HarvestUi>,
    apps_root: String,
    metadata_dir: String,
    images_dir: String,
    stage: String,
    curl: String,
    // a long op on a worker thread, if any
    job: Option<std::sync::mpsc::Receiver<Done>>,
    busy: String, // label of the running job ("" = idle)
}

/// Result of a background job, applied on the UI thread when it arrives.
struct Done {
    status: String,
    dataset: Option<String>, // if set, switch the working dataset to this path
    reload: bool,            // re-read the library table after
}

impl Default for App {
    fn default() -> Self {
        let settings = Settings::load_default();
        let target_reg = targets::Registry::load_default();
        // First run = no source config saved yet. The wizard auto-detects rb-cli
        // and prompts for the MacPack / MG-Archive folders.
        let show_wizard = settings.macpack_dir.is_none() && settings.rb_cli.is_none();
        let rb_cli = settings
            .rb_cli
            .clone()
            .or_else(detect_rb_cli)
            .unwrap_or_else(|| "rb-cli".into());
        let macpack_dir = settings
            .macpack_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let cache_dir = settings
            .cache_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mg_archive = settings
            .mg_archive
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| mg::default_archive().display().to_string());
        Self {
            tab: Tab::Build,
            settings,
            show_wizard,
            macpack_dir,
            cache_dir,
            target_reg,
            target_name: String::new(),
            te_name: String::new(),
            te_base_os: String::new(),
            te_depths: String::new(),
            te_mem_pref: String::new(),
            te_mem_min: String::new(),
            te_label: String::new(),
            library: Vec::new(),
            library_loaded: false,
            lib_search: String::new(),
            lib_kind: String::new(),
            build_pick: true,
            rb_cli,
            metadata: String::new(),
            mg_archive,
            image_path: String::new(),
            dataset: String::new(),   // blank => bundled library
            overrides: String::new(), // blank => bundled compatibility overlay
            status: "Pick a Target and the titles to include, then Build.".into(),
            base_system: String::new(),
            base_os: String::new(),
            templates: templates::Registry::load_default().keys(),
            disk_size_mb: String::new(),
            sel_mode: 2, // Pick titles
            sel_text: String::new(),
            launcher: String::new(),
            out_image: "/tmp/macatrium.hda".into(),
            add_disk_path: String::new(),
            startup_items: "/System Folder/Startup Items".into(),
            startup_sound: String::new(),
            shutdown_sound: String::new(),
            platform: "Apple Mac OS".into(),
            detect_color: false,
            download_art: false,
            art_dir: String::new(),
            max_art_size: String::new(),
            bw_only: false,
            app_mem_pref: String::new(),
            app_mem_min: String::new(),
            d1: true,
            d4: false,
            d8: true,
            d16: false,
            d24: true,
            harvest: Vec::new(),
            apps_root: "/MacAtrium/Apps".into(),
            metadata_dir: "/MacAtrium/metadata".into(),
            images_dir: "/MacAtrium/images".into(),
            stage: String::new(),
            curl: "curl".into(),
            job: None,
            busy: String::new(),
        }
    }
}

/// Kind of file dialog for a path field's Browse button.
enum Pick {
    File,
    Folder,
    Save,
}

/// A "label · text field · Browse…" row that fills `value` from a file dialog.
fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, kind: Pick) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(360.0));
        if ui.button("Browse…").clicked() {
            let dlg = rfd::FileDialog::new();
            let picked = match kind {
                Pick::File => dlg.pick_file(),
                Pick::Folder => dlg.pick_folder(),
                Pick::Save => dlg.save_file(),
            };
            if let Some(p) = picked {
                *value = p.to_string_lossy().into_owned();
            }
        }
    });
}

fn opt_path(s: &str) -> Option<PathBuf> {
    let t = s.trim();
    (!t.is_empty()).then(|| PathBuf::from(t))
}

/// Find an rb-cli binary without asking: `~/.local/bin/rb-cli` first (where this
/// project installs it), else the bare name if it's anywhere on `PATH`.
fn detect_rb_cli() -> Option<String> {
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".local/bin/rb-cli");
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join("rb-cli").exists() {
                return Some("rb-cli".into());
            }
        }
    }
    None
}

fn as_bool(m: &Map<String, Value>, k: &str, default: bool) -> bool {
    m.get(k).and_then(Value::as_bool).unwrap_or(default)
}

/// Parse a library JSONL (identity + descriptive metadata) and overlay the
/// compatibility facets (Colour/B&W, Mouse, hotkey) keyed by id, into editable
/// rows. The overlay wins, matching the build-time merge.
fn parse_library(lib: &str, compat: &str) -> Vec<LibRow> {
    let mut overlay: HashMap<String, Map<String, Value>> = HashMap::new();
    for line in compat.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(m) = serde_json::from_str::<Map<String, Value>>(t) {
            if let Some(id) = m.get("id").and_then(Value::as_str) {
                overlay.insert(id.to_string(), m);
            }
        }
    }
    let mut rows = Vec::new();
    for line in lib.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Ok(m) = serde_json::from_str::<Map<String, Value>>(t) else { continue };
        let id = m.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let ov = overlay.get(&id);
        // overlay facet wins, else the base record, else the default.
        let facet_bool = |k: &str, d: bool| -> bool {
            ov.and_then(|o| o.get(k))
                .and_then(Value::as_bool)
                .unwrap_or_else(|| as_bool(&m, k, d))
        };
        let hotkey = ov
            .and_then(|o| o.get("hotkey"))
            .and_then(Value::as_str)
            .or_else(|| m.get("hotkey").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        rows.push(LibRow {
            id,
            name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            kind: m.get("kind").and_then(Value::as_str).unwrap_or("").to_string(),
            year: m.get("year").and_then(Value::as_i64).map(|y| y.to_string()).unwrap_or_default(),
            color: facet_bool("color", false),
            mouse: facet_bool("mouse", true),
            hotkey,
            selected: false,
            dirty: false,
        });
    }
    rows
}

impl App {
    /// (Re)load the library table from the bundled data (or the override paths if
    /// set under Advanced), preserving the current selection by id.
    fn reload_library(&mut self) {
        let lib = if self.dataset.trim().is_empty() {
            String::from_utf8_lossy(atrium::config::EMBEDDED_LIBRARY).into_owned()
        } else {
            std::fs::read_to_string(self.dataset.trim()).unwrap_or_default()
        };
        let compat = if self.overrides.trim().is_empty() {
            String::from_utf8_lossy(atrium::config::EMBEDDED_COMPAT).into_owned()
        } else {
            std::fs::read_to_string(self.overrides.trim()).unwrap_or_default()
        };
        let keep: HashSet<String> = self
            .library
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.id.clone())
            .collect();
        let mut rows = parse_library(&lib, &compat);
        for r in &mut rows {
            r.selected = keep.contains(&r.id);
        }
        self.status = format!("Loaded {} title(s).", rows.len());
        self.library = rows;
        self.library_loaded = true;
    }

    /// Ensure the library table is populated (lazy — first time a screen needs it).
    fn ensure_library(&mut self) {
        if !self.library_loaded {
            self.reload_library();
        }
    }

    /// Distinct `kind` buckets present in the library (for the filter combo).
    fn kinds(&self) -> Vec<String> {
        let mut set: Vec<String> = self
            .library
            .iter()
            .map(|r| r.kind.clone())
            .filter(|k| !k.is_empty())
            .collect();
        set.sort();
        set.dedup();
        set
    }

    /// Sync the title-picker selection into the `Selection` fields the build reads.
    /// Pick mode => an explicit id list; All mode => `Selection::All`.
    fn sync_picker(&mut self) {
        if self.build_pick {
            self.sel_mode = 2;
            let ids: Vec<&str> = self
                .library
                .iter()
                .filter(|r| r.selected)
                .map(|r| r.id.as_str())
                .collect();
            self.sel_text = ids.join(", ");
        } else {
            self.sel_mode = 1;
            self.sel_text.clear();
        }
    }

    fn selected_count(&self) -> usize {
        self.library.iter().filter(|r| r.selected).count()
    }

    /// Extract a built disk's catalog into the Library table (Load Existing
    /// MacAtrium Disk). The catalog *is* a dataset, so we point the table at it.
    fn extract_catalog(&mut self, ctx: &egui::Context) {
        if self.image_path.is_empty() {
            self.status = "Pick a MacAtrium .hda first.".into();
            return;
        }
        let rb_cli = self.rb_cli.clone();
        let image_path = self.image_path.clone();
        self.spawn_job(ctx, "Extracting catalog", move || {
            let rb = RbCli::new(&rb_cli);
            let tmp = std::env::temp_dir().join("macatrium-mgmt-catalog.jsonl");
            let _ = std::fs::remove_file(&tmp);
            match rb.get(
                PathBuf::from(&image_path).as_path(),
                "/MacAtrium/metadata/catalog.jsonl",
                &tmp,
                true,
            ) {
                Ok(()) => Done { status: String::new(), dataset: Some(tmp.to_string_lossy().into_owned()), reload: true },
                Err(e) => Done { status: format!("Extract failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// Run `f` on a worker thread; its `Done` is applied by poll_job() when the
    /// thread wakes the UI. Keeps the window responsive during long ops.
    fn spawn_job<F>(&mut self, ctx: &egui::Context, label: &str, f: F)
    where
        F: FnOnce() -> Done + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let done = f();
            let _ = tx.send(done);
            ctx.request_repaint();
        });
        self.job = Some(rx);
        self.busy = label.to_string();
        self.status = format!("{label}…");
    }

    /// Apply a finished job's result (called at the top of each frame).
    fn poll_job(&mut self) {
        let done = self.job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(done) = done {
            self.job = None;
            self.busy.clear();
            if let Some(ds) = done.dataset {
                self.dataset = ds;
            }
            if done.reload {
                self.reload_library();
            } else {
                self.status = done.status;
            }
        }
    }

    /// Save the edited compatibility facets (Colour/Mouse/hotkey) for the dirty
    /// rows into the compatibility overlay (the bundled `data/compatibility.jsonl`
    /// by default, or the override path under Advanced).
    fn save_facets(&mut self) {
        let target = if self.overrides.trim().is_empty() {
            "data/compatibility.jsonl".to_string()
        } else {
            self.overrides.trim().to_string()
        };
        let mut n = 0;
        for row in self.library.iter_mut().filter(|r| r.dirty) {
            let mut f = Map::new();
            f.insert("color".into(), Value::Bool(row.color));
            f.insert("mouse".into(), Value::Bool(row.mouse));
            if let Some(c) = row.hotkey.trim().chars().next() {
                f.insert("hotkey".into(), Value::String(c.to_string()));
            }
            if let Err(e) = merge::set(PathBuf::from(&target).as_path(), &row.id, &f) {
                self.status = format!("Save failed for {}: {e}", row.id);
                return;
            }
            row.dirty = false;
            n += 1;
        }
        self.status = if n == 0 {
            "Nothing changed.".into()
        } else {
            format!("Saved {n} compatibility edit(s) -> {target}")
        };
    }

    /// Download the *selected* titles' software from the Macintosh Garden mirror
    /// into the cache (Attain). Caches once; the bits may need a manual install.
    fn run_mg_download(&mut self, ctx: &egui::Context) {
        if self.mg_archive.trim().is_empty() {
            self.status = "Set the Macintosh Garden archive (Settings) first.".into();
            return;
        }
        let selected: Vec<(String, String)> = self
            .library
            .iter()
            .filter(|r| r.selected)
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect();
        if selected.is_empty() {
            self.status = "Select titles (Build or Library) to download first.".into();
            return;
        }
        let archive = self.mg_archive.clone();
        let cache = self.cache_dir.clone();
        let rb = self.rb_cli.clone();
        let curl = self.curl.clone();
        self.spawn_job(ctx, &format!("Downloading {} title(s) from Macintosh Garden", selected.len()), move || {
            // fetch matches dataset records to MG titles by name — write a minimal
            // dataset of just the selected titles, then fetch into the cache only.
            let tmp = std::env::temp_dir().join("macatrium-mg-download.jsonl");
            let body: String = selected
                .iter()
                .map(|(id, name)| {
                    let m: Map<String, Value> = [
                        ("id".to_string(), Value::from(id.as_str())),
                        ("name".to_string(), Value::from(name.as_str())),
                    ]
                    .into_iter()
                    .collect();
                    serde_json::to_string(&Value::Object(m)).unwrap()
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Err(e) = std::fs::write(&tmp, body) {
                return Done { status: format!("MG download failed: {e}"), dataset: None, reload: false };
            }
            let downloads = opt_path(&cache);
            match fetch::run(
                PathBuf::from(&archive).as_path(),
                &[],
                Some(tmp.as_path()),
                downloads.as_deref(),
                None, // cache only — no injection
                "/MacAtrium/Apps",
                None,
                &rb,
                &curl,
                None,
            ) {
                Ok(()) => Done { status: "Downloaded selected software into the cache.".into(), dataset: None, reload: false },
                Err(e) => Done { status: format!("MG download failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// The checked art-depth variants, ascending (e.g. ["1","8","24"]).
    fn art_depths(&self) -> Vec<String> {
        if self.bw_only {
            return vec!["1".to_string()];
        }
        let mut v = Vec::new();
        if self.d1 { v.push("1".to_string()); }
        if self.d4 { v.push("4".to_string()); }
        if self.d8 { v.push("8".to_string()); }
        if self.d16 { v.push("16".to_string()); }
        if self.d24 { v.push("24".to_string()); }
        v
    }

    /// The launcher memory partition `[preferred_kb, minimum_kb]`, or `None` to
    /// keep the binary's built-in 2 MB / 1 MB.
    fn app_mem_kb(&self) -> Option<[u32; 2]> {
        let pref = self.app_mem_pref.trim().parse::<u32>().ok().filter(|&p| p > 0);
        let min = self.app_mem_min.trim().parse::<u32>().ok().filter(|&m| m > 0);
        if let Some(p) = pref {
            return Some([p, min.unwrap_or(p)]);
        }
        if self.bw_only {
            let (p, m) = atrium::config::COMPACT_APP_MEM_KB;
            return Some([p, m]);
        }
        None
    }

    /// Assemble the shared [`BuildConfig`] from the GUI fields — the single mapping
    /// used by Build *and* Save, so the GUI and the `builds/*.json` the CLI reads
    /// stay byte-compatible. (The schema lives once in `atrium::config`.)
    fn to_config(&self) -> BuildConfig {
        let opt = |s: &str| -> Option<PathBuf> {
            let t = s.trim();
            if t.is_empty() { None } else { Some(PathBuf::from(t)) }
        };
        let harvest: Vec<HarvestSrc> = self
            .harvest
            .iter()
            .filter(|h| !h.image.trim().is_empty())
            .map(|h| HarvestSrc {
                image: PathBuf::from(h.image.trim()),
                apps: h
                    .apps
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                scan: {
                    let t = h.scan.trim();
                    if t.is_empty() { None } else { Some(t.to_string()) }
                },
            })
            .collect();

        let base_os = {
            let b = self.base_os.trim();
            if b.is_empty() { None } else { Some(b.to_string()) }
        };
        let system = if base_os.is_some() { None } else { Some(PathBuf::from(self.base_system.trim())) };
        let words = |s: &str| -> Vec<String> {
            s.split(|c| c == ',' || c == '\n' || c == ' ' || c == '\t')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect()
        };
        let selection = match self.sel_mode {
            1 => Some(Selection::All),
            2 => Some(Selection::List { ids: words(&self.sel_text) }),
            3 => Some(Selection::Categories { categories: words(&self.sel_text) }),
            _ => None,
        };

        BuildConfig {
            system,
            base_os,
            disk_size_mb: self.disk_size_mb.trim().parse::<u64>().ok(),
            selection,
            out: PathBuf::from(self.out_image.trim()),
            launcher: opt(&self.launcher),
            dataset: opt(&self.dataset),
            startup_items: self.startup_items.trim().to_string(),
            overrides: opt(&self.overrides),
            metadata: opt(&self.metadata),
            mg_archive: opt(&self.mg_archive),
            platform: self.platform.trim().to_string(),
            detect_color: self.detect_color,
            curl: self.curl.trim().to_string(),
            harvest,
            art_dir: opt(&self.art_dir),
            art_depths: self.art_depths(),
            art_max: None,
            max_art_size: {
                let s = self.max_art_size.trim();
                (!s.is_empty()).then(|| s.to_string())
            },
            download_art: self.download_art,
            rb_cli: self.rb_cli.trim().to_string(),
            apps_root: self.apps_root.trim().to_string(),
            metadata_dir: self.metadata_dir.trim().to_string(),
            images_dir: self.images_dir.trim().to_string(),
            stage: opt(&self.stage),
            startup_sound: opt(&self.startup_sound),
            shutdown_sound: opt(&self.shutdown_sound),
            app_mem_kb: self.app_mem_kb(),
            ..BuildConfig::default()
        }
    }

    /// Populate the GUI fields from a loaded [`BuildConfig`] — the inverse of
    /// [`Self::to_config`], so a `builds/*.json` opens straight into the form.
    fn apply_config(&mut self, c: BuildConfig) {
        let s = |o: &Option<PathBuf>| o.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.base_system = c.system.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.base_os = c.base_os.clone().unwrap_or_default();
        self.out_image = c.out.display().to_string();
        self.launcher = s(&c.launcher);
        self.dataset = s(&c.dataset);
        self.disk_size_mb = c.disk_size_mb.map(|n| n.to_string()).unwrap_or_default();
        self.overrides = s(&c.overrides);
        self.metadata = s(&c.metadata);
        self.mg_archive = s(&c.mg_archive);
        self.platform = c.platform.clone();
        self.detect_color = c.detect_color;
        self.download_art = c.download_art;
        self.art_dir = s(&c.art_dir);
        self.startup_items = c.startup_items.clone();
        self.startup_sound = s(&c.startup_sound);
        self.shutdown_sound = s(&c.shutdown_sound);
        self.rb_cli = c.rb_cli.clone();
        self.curl = c.curl.clone();
        self.apps_root = c.apps_root.clone();
        self.metadata_dir = c.metadata_dir.clone();
        self.images_dir = c.images_dir.clone();
        self.stage = s(&c.stage);
        match &c.selection {
            Some(Selection::All) => { self.sel_mode = 1; self.sel_text.clear(); }
            Some(Selection::List { ids }) => { self.sel_mode = 2; self.sel_text = ids.join(", "); }
            Some(Selection::Categories { categories }) => { self.sel_mode = 3; self.sel_text = categories.join(", "); }
            None => { self.sel_mode = 0; self.sel_text.clear(); }
        }
        self.bw_only = c.art_depths == ["1"];
        let has = |d: &str| c.art_depths.iter().any(|x| x == d);
        self.d1 = has("1"); self.d4 = has("4"); self.d8 = has("8");
        self.d16 = has("16"); self.d24 = has("24");
        self.max_art_size = c.max_art_size.clone().unwrap_or_default();
        match c.app_mem_kb {
            Some([p, m]) => { self.app_mem_pref = p.to_string(); self.app_mem_min = m.to_string(); }
            None => { self.app_mem_pref.clear(); self.app_mem_min.clear(); }
        }
        self.harvest = c.harvest.iter().map(|h| HarvestUi {
            image: h.image.display().to_string(),
            apps: h.apps.join("\n"),
            scan: h.scan.clone().unwrap_or_default(),
        }).collect();
    }

    /// Apply a Target's pinned machine settings onto the form. Reuses the tested
    /// controller both ways: `to_config` (current form) -> `Target::apply_to`
    /// (stamp the profile) -> `apply_config` (push back into the widgets).
    fn apply_target(&mut self, name: &str) {
        if let Some(t) = self.target_reg.get(name).cloned() {
            let mut c = self.to_config();
            t.apply_to(&mut c);
            self.apply_config(c);
            self.target_name = name.to_string();
        }
    }

    /// Serialize the current form to a `builds/*.json` via a save dialog.
    fn save_config(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("build config", &["json"])
            .set_file_name("build.json")
            .save_file()
        else { return };
        match serde_json::to_string_pretty(&self.to_config()) {
            Ok(json) => match std::fs::write(&path, json) {
                Ok(()) => self.status = format!("Saved build config -> {}", path.display()),
                Err(e) => self.status = format!("Save failed: {e}"),
            },
            Err(e) => self.status = format!("Encode failed: {e}"),
        }
    }

    /// Load a `builds/*.json` into the form via a file dialog.
    fn load_config(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("build config", &["json"])
            .pick_file()
        else { return };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<BuildConfig>(&t).map_err(|e| e.to_string()))
        {
            Ok(cfg) => {
                self.apply_config(cfg);
                self.build_pick = self.sel_mode == 2;
                self.reflect_selection();
                self.status = format!("Loaded build config {}", path.display());
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    /// Mirror the `Selection::List` ids in `sel_text` into the library rows'
    /// `selected` flags, so the picker shows a loaded config's titles ticked.
    fn reflect_selection(&mut self) {
        if self.sel_mode != 2 {
            return;
        }
        let want: HashSet<&str> = self.sel_text.split([',', ' ', '\n', '\t']).map(str::trim).filter(|s| !s.is_empty()).collect();
        for r in &mut self.library {
            r.selected = want.contains(r.id.as_str());
        }
    }

    fn build_image(&mut self, ctx: &egui::Context) {
        if self.out_image.trim().is_empty()
            || (self.base_os.trim().is_empty() && self.base_system.trim().is_empty())
        {
            self.status = "Pick a Target (or set a custom base OS under Advanced) and an output path.".into();
            return;
        }
        let depths = self.art_depths();
        if depths.is_empty() {
            self.status = "This Target bakes no art depths — pick a Target or set depths under Advanced.".into();
            return;
        }
        if self.build_pick && self.selected_count() == 0 {
            self.status = "Select at least one title to include (or switch to \"All compatible\").".into();
            return;
        }

        let cfg = self.to_config();
        let out = self.out_image.clone();
        let label = format!("Building image ({})", depths.join("/"));
        self.spawn_job(ctx, &label, move || match image::run(&cfg) {
            Ok(()) => Done { status: format!("Built image -> {out}"), dataset: None, reload: false },
            Err(e) => Done { status: format!("Build failed: {e}"), dataset: None, reload: false },
        });
    }

    /// Persist the Settings-screen fields to `~/.macatrium.json`.
    fn save_settings(&mut self) {
        let mut s = self.settings.clone();
        s.macpack_dir = opt_path(&self.macpack_dir);
        s.cache_dir = opt_path(&self.cache_dir);
        s.mg_archive = opt_path(&self.mg_archive);
        s.rb_cli = {
            let t = self.rb_cli.trim();
            (!t.is_empty()).then(|| t.to_string())
        };
        let path = settings::default_path();
        match s.save(&path) {
            Ok(()) => {
                self.settings = s;
                self.status = format!("Saved settings -> {}", path.display());
            }
            Err(e) => self.status = format!("Save settings failed: {e}"),
        }
    }

    // ---- the job screens -----------------------------------------------------

    /// The shared title picker: search + kind filter + a virtualised, tickable
    /// list. Toggling a tick re-syncs the build `Selection`.
    fn title_picker(&mut self, ui: &mut egui::Ui) {
        self.ensure_library();
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).desired_width(220.0).hint_text("name…"));
            ui.label("Kind:");
            let kinds = self.kinds();
            let cur = if self.lib_kind.is_empty() { "(all)".to_string() } else { self.lib_kind.clone() };
            egui::ComboBox::from_id_salt("pick_kind")
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lib_kind, String::new(), "(all)");
                    for k in &kinds {
                        ui.selectable_value(&mut self.lib_kind, k.clone(), k.as_str());
                    }
                });
            ui.separator();
            ui.label(format!("{} selected", self.selected_count()));
        });

        let q = self.lib_search.to_lowercase();
        let kind = self.lib_kind.clone();
        let filtered: Vec<usize> = self
            .library
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (kind.is_empty() || r.kind == kind)
                    && (q.is_empty() || r.name.to_lowercase().contains(&q) || r.id.contains(&q))
            })
            .map(|(i, _)| i)
            .collect();

        ui.horizontal(|ui| {
            if ui.small_button("Select all (filtered)").clicked() {
                for &i in &filtered { self.library[i].selected = true; }
                self.sync_picker();
            }
            if ui.small_button("Clear all").clicked() {
                for r in &mut self.library { r.selected = false; }
                self.sync_picker();
            }
            ui.label(egui::RichText::new(format!("{} shown", filtered.len())).small().weak());
        });
        ui.separator();

        let mut changed = false;
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        egui::ScrollArea::vertical()
            .id_salt("title_picker")
            .auto_shrink([false, false])
            .max_height(340.0)
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for vis in range {
                    let idx = filtered[vis];
                    let r = &mut self.library[idx];
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut r.selected, "").changed() {
                            changed = true;
                        }
                        let meta = [r.kind.as_str(), r.year.as_str()]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · ");
                        ui.label(&r.name);
                        if !meta.is_empty() {
                            ui.label(egui::RichText::new(meta).small().weak());
                        }
                    });
                }
            });
        if changed {
            self.sync_picker();
        }
    }

    fn tab_build(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        // First view: default to the first Target so a fresh Build is ready.
        if self.target_name.is_empty() {
            if let Some(first) = self.target_reg.names().into_iter().next() {
                self.apply_target(&first);
            }
        }
        ui.label(
            egui::RichText::new("Pick a Target (the Mac you're building for), choose the titles, and Build a fresh bootable disk.")
                .small().weak(),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("Target:");
            let names = self.target_reg.names();
            let cur = if self.target_name.is_empty() { "(choose)".to_string() } else { self.target_name.clone() };
            let mut pick: Option<String> = None;
            egui::ComboBox::from_id_salt("target")
                .selected_text(cur)
                .width(320.0)
                .show_ui(ui, |ui| {
                    for n in &names {
                        if ui.selectable_label(self.target_name == *n, n).clicked() {
                            pick = Some(n.clone());
                        }
                    }
                });
            if let Some(n) = pick {
                self.apply_target(&n);
            }
        });
        if let Some(t) = self.target_reg.get(&self.target_name) {
            if !t.label.is_empty() {
                ui.label(egui::RichText::new(format!("    {}", t.label)).small().weak());
            }
            let depths = t.art_depths.join("/");
            let mem = t.app_mem_kb.map(|[p, m]| format!("{p}/{m} KB")).unwrap_or_else(|| "default".into());
            ui.label(egui::RichText::new(format!("    base OS {} · art {} · launcher RAM {}", t.base_os, depths, mem)).small().weak());
        }

        ui.add_space(6.0);
        path_row(ui, "Output disk (.hda):", &mut self.out_image, Pick::Save);

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Titles to include");
            ui.horizontal(|ui| {
                if ui.radio_value(&mut self.build_pick, true, "Pick titles").clicked() { self.sync_picker(); }
                if ui.radio_value(&mut self.build_pick, false, "All compatible").clicked() { self.sync_picker(); }
            });
            if self.build_pick {
                self.title_picker(ui);
            } else {
                ui.label(egui::RichText::new("Every title compatible with the Target's OS will be included.").small().weak());
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Build disk").strong())).clicked() {
                self.build_image(ctx);
            }
            ui.separator();
            if ui.button("Save config…").on_hover_text(
                "Write these settings to a builds/*.json the `atrium image --config` CLI can run."
            ).clicked() {
                self.save_config();
            }
            if ui.button("Load config…").clicked() {
                self.load_config();
            }
        });

        ui.add_space(6.0);
        self.build_advanced(ui);
    }

    /// The plumbing a normal user shouldn't see: custom base OS, data overrides,
    /// content sources, art depths, launcher RAM, harvest donors, tool paths.
    fn build_advanced(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Advanced", |ui| {
            ui.horizontal(|ui| {
                ui.label("disk size MB:");
                ui.add(egui::TextEdit::singleline(&mut self.disk_size_mb).desired_width(64.0));
                ui.label(egui::RichText::new("≤2048; blank = base size").small().weak());
            });
            ui.collapsing("Custom base OS / launcher", |ui| {
                ui.horizontal(|ui| {
                    ui.label("base OS:");
                    let cur = if self.base_os.is_empty() { "(custom .hda)".to_string() } else { self.base_os.clone() };
                    let tmpls = self.templates.clone();
                    egui::ComboBox::from_id_salt("base_os")
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.base_os, String::new(), "(custom .hda)");
                            for k in &tmpls {
                                ui.selectable_value(&mut self.base_os, k.clone(), k.as_str());
                            }
                        });
                });
                if self.base_os.trim().is_empty() {
                    path_row(ui, "base system .hda:", &mut self.base_system, Pick::File);
                }
                path_row(ui, "launcher (.bin):", &mut self.launcher, Pick::File);
                ui.label(egui::RichText::new("blank = the launcher bundled in this app (no Retro68 needed)").small().weak());
            });

            ui.collapsing("Content sources (optional)", |ui| {
                path_row(ui, "Macintosh Garden archive:", &mut self.mg_archive, Pick::Folder);
                path_row(ui, "LaunchBox Metadata.xml:", &mut self.metadata, Pick::File);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.download_art, "download box art (LaunchBox)");
                    ui.checkbox(&mut self.detect_color, "auto-detect Colour / B&W");
                });
                path_row(ui, "local art dir:", &mut self.art_dir, Pick::Folder);
            });

            ui.collapsing("Art & launcher RAM", |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.bw_only, "Mac Plus / SE (B&W only)")
                        .on_hover_text("1-bit artwork only — skips every colour PICT. Much smaller image.");
                    ui.separator();
                    ui.label("max art size:");
                    ui.add(egui::TextEdit::singleline(&mut self.max_art_size).hint_text("720x768").desired_width(80.0));
                });
                ui.add_enabled_ui(!self.bw_only, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("art depths:");
                        ui.checkbox(&mut self.d1, "1");
                        ui.checkbox(&mut self.d4, "4");
                        ui.checkbox(&mut self.d8, "8");
                        ui.checkbox(&mut self.d16, "16");
                        ui.checkbox(&mut self.d24, "24");
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("launcher RAM KB:");
                    ui.add(egui::TextEdit::singleline(&mut self.app_mem_pref).hint_text("pref").desired_width(56.0));
                    ui.add(egui::TextEdit::singleline(&mut self.app_mem_min).hint_text("min").desired_width(56.0));
                    let (cp, cm) = atrium::config::COLOR_APP_MEM_KB;
                    let (bp, bm) = atrium::config::COMPACT_APP_MEM_KB;
                    if ui.small_button("Colour").clicked() { self.app_mem_pref = cp.to_string(); self.app_mem_min = cm.to_string(); }
                    if ui.small_button("Compact B&W").clicked() { self.app_mem_pref = bp.to_string(); self.app_mem_min = bm.to_string(); }
                    if ui.small_button("Default").clicked() { self.app_mem_pref.clear(); self.app_mem_min.clear(); }
                });
                path_row(ui, "startup sound (WAV):", &mut self.startup_sound, Pick::File);
                path_row(ui, "shutdown sound (WAV):", &mut self.shutdown_sound, Pick::File);
            });

            ui.collapsing("Data overrides", |ui| {
                ui.label(egui::RichText::new("Blank = the library + compatibility overlay bundled in this app.").small().weak());
                path_row(ui, "library .jsonl:", &mut self.dataset, Pick::File);
                path_row(ui, "compatibility .jsonl:", &mut self.overrides, Pick::File);
                if ui.button("Reload library table").clicked() {
                    self.reload_library();
                }
            });

            ui.collapsing("Harvest sources (donor disks)", |ui| {
                let mut remove = None;
                for (i, h) in self.harvest.iter_mut().enumerate() {
                    ui.group(|ui| {
                        path_row(ui, "donor image:", &mut h.image, Pick::File);
                        ui.label("apps (one path per line):");
                        ui.add(egui::TextEdit::multiline(&mut h.apps).desired_rows(2).desired_width(440.0));
                        ui.horizontal(|ui| {
                            ui.label("scan glob (optional):");
                            ui.text_edit_singleline(&mut h.scan);
                            if ui.button("Remove").clicked() { remove = Some(i); }
                        });
                    });
                }
                if let Some(i) = remove { self.harvest.remove(i); }
                if ui.button("Add harvest source").clicked() { self.harvest.push(HarvestUi::default()); }
            });

            ui.collapsing("Tool paths", |ui| {
                ui.horizontal(|ui| { ui.label("platform:"); ui.add(egui::TextEdit::singleline(&mut self.platform).desired_width(160.0)); });
                ui.horizontal(|ui| { ui.label("startup items:"); ui.add(egui::TextEdit::singleline(&mut self.startup_items).desired_width(260.0)); });
                ui.horizontal(|ui| { ui.label("rb-cli:"); ui.text_edit_singleline(&mut self.rb_cli); });
                ui.horizontal(|ui| { ui.label("curl:"); ui.text_edit_singleline(&mut self.curl); });
                ui.horizontal(|ui| { ui.label("apps root:"); ui.text_edit_singleline(&mut self.apps_root); });
                ui.horizontal(|ui| { ui.label("metadata dir:"); ui.text_edit_singleline(&mut self.metadata_dir); });
                ui.horizontal(|ui| { ui.label("images dir:"); ui.text_edit_singleline(&mut self.images_dir); });
                path_row(ui, "stage dir:", &mut self.stage, Pick::Folder);
            });
        });
    }

    fn tab_add_to_disk(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, _busy: bool) {
        ui.label(
            egui::RichText::new("Extend an already-built MacAtrium disk with more titles, without rebuilding from scratch.")
                .small().weak(),
        );
        ui.add_space(6.0);
        path_row(ui, "MacAtrium disk (.hda):", &mut self.add_disk_path, Pick::File);
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Titles to add");
            self.build_pick = true;
            self.title_picker(ui);
        });
        ui.add_space(8.0);
        ui.add_enabled(false, egui::Button::new("Add to disk")).on_hover_text(
            "Injects the selected titles' forks into the existing disk and regenerates its \
             catalog. The inject backend (harvest --into + catalog regen) is the next build step.",
        );
        ui.label(
            egui::RichText::new("Inject backend coming next — pick the disk + titles now; the wiring lands in the following commit.")
                .small().weak(),
        );
    }

    fn tab_library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        self.ensure_library();
        ui.label(
            egui::RichText::new("Browse the bundled catalogue and edit each title's compatibility facets (Colour/B&W, Mouse, launch hotkey). Save writes the compatibility overlay.")
                .small().weak(),
        );
        ui.add_space(4.0);
        ui.collapsing("Load Existing MacAtrium Disk", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Pick .hda…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("disk image", &["hda", "img", "dsk", "vhd"])
                        .pick_file()
                    {
                        self.image_path = p.to_string_lossy().into_owned();
                    }
                }
                if ui.add_enabled(!busy, egui::Button::new("Extract catalog")).clicked() {
                    self.extract_catalog(ctx);
                }
                ui.monospace(&self.image_path);
            });
        });

        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).desired_width(220.0).hint_text("name…"));
            ui.label("Kind:");
            let kinds = self.kinds();
            let cur = if self.lib_kind.is_empty() { "(all)".to_string() } else { self.lib_kind.clone() };
            egui::ComboBox::from_id_salt("lib_kind")
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lib_kind, String::new(), "(all)");
                    for k in &kinds {
                        ui.selectable_value(&mut self.lib_kind, k.clone(), k.as_str());
                    }
                });
        });

        let q = self.lib_search.to_lowercase();
        let kind = self.lib_kind.clone();
        let filtered: Vec<usize> = self
            .library
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (kind.is_empty() || r.kind == kind)
                    && (q.is_empty() || r.name.to_lowercase().contains(&q) || r.id.contains(&q))
            })
            .map(|(i, _)| i)
            .collect();

        ui.separator();
        // header row
        ui.horizontal(|ui| {
            ui.add_sized([280.0, 18.0], egui::Label::new(egui::RichText::new("Name").strong()));
            ui.add_sized([46.0, 18.0], egui::Label::new(egui::RichText::new("Year").strong()));
            ui.add_sized([90.0, 18.0], egui::Label::new(egui::RichText::new("Colour").strong()));
            ui.add_sized([90.0, 18.0], egui::Label::new(egui::RichText::new("Mouse").strong()));
            ui.add_sized([40.0, 18.0], egui::Label::new(egui::RichText::new("Key").strong()));
        });
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
        egui::ScrollArea::vertical()
            .id_salt("lib_edit")
            .auto_shrink([false, false])
            .max_height(380.0)
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for vis in range {
                    let idx = filtered[vis];
                    let r = &mut self.library[idx];
                    ui.horizontal(|ui| {
                        ui.add_sized([280.0, 18.0], egui::Label::new(&r.name).truncate());
                        ui.add_sized([46.0, 18.0], egui::Label::new(&r.year));
                        let clabel = if r.color { "Colour" } else { "B&W" };
                        let c = ui.add_sized([90.0, 18.0], egui::Checkbox::new(&mut r.color, clabel));
                        let mlabel = if r.mouse { "Required" } else { "No mouse" };
                        let m = ui.add_sized([90.0, 18.0], egui::Checkbox::new(&mut r.mouse, mlabel));
                        let h = ui.add_sized(
                            [40.0, 18.0],
                            egui::TextEdit::singleline(&mut r.hotkey).char_limit(1).hint_text("key"),
                        );
                        if c.changed() || m.changed() || h.changed() {
                            r.dirty = true;
                        }
                    });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} shown · {} total", filtered.len(), self.library.len())).small().weak());
            if ui.add_enabled(!busy, egui::Button::new("Save compatibility")).clicked() {
                self.save_facets();
            }
        });
    }

    fn tab_attain(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Acquire the source software MacAtrium builds from. These locations are saved to ~/.macatrium.json.")
                .small().weak(),
        );
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("MacPack (primary source)");
            ui.label(egui::RichText::new("The folder holding the MacPack donor disks (boot.vhd, Supplement.vhd, …). Required to harvest MacPack titles into a build.").small().weak());
            path_row(ui, "MacPack folder:", &mut self.macpack_dir, Pick::Folder);
            if ui.button("Save MacPack location").clicked() {
                self.save_settings();
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Macintosh Garden downloader");
            ui.label(egui::RichText::new("Downloads the selected titles' software from the Macintosh Garden mirror into the cache. Caches once; some titles need a manual install afterwards.").small().weak());
            path_row(ui, "MG-Archive:", &mut self.mg_archive, Pick::Folder);
            path_row(ui, "cache dir:", &mut self.cache_dir, Pick::Folder);
            let archive_ok = !self.mg_archive.trim().is_empty() && PathBuf::from(self.mg_archive.trim()).exists();
            ui.horizontal(|ui| {
                ui.add_enabled(archive_ok && !busy, egui::Button::new("Download selected titles"))
                    .clicked()
                    .then(|| self.run_mg_download(ctx));
                ui.label(egui::RichText::new(format!("{} selected", self.selected_count())).small().weak());
            });
            if !archive_ok {
                ui.label(egui::RichText::new("Set a valid MG-Archive folder to enable the downloader.").small().weak());
            }
        });
    }

    fn tab_settings(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, _busy: bool) {
        ui.label(egui::RichText::new("Machine-local settings, persisted to ~/.macatrium.json.").small().weak());
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Source locations & tools");
            path_row(ui, "MacPack folder:", &mut self.macpack_dir, Pick::Folder);
            path_row(ui, "MG-Archive:", &mut self.mg_archive, Pick::Folder);
            path_row(ui, "cache dir:", &mut self.cache_dir, Pick::Folder);
            ui.horizontal(|ui| {
                ui.label("rb-cli:");
                ui.add(egui::TextEdit::singleline(&mut self.rb_cli).desired_width(300.0));
                if ui.button("Detect").clicked() {
                    if let Some(p) = detect_rb_cli() {
                        self.rb_cli = p;
                        self.status = format!("Found rb-cli: {}", self.rb_cli);
                    } else {
                        self.status = "rb-cli not found on PATH or in ~/.local/bin.".into();
                    }
                }
            });
            if ui.button("Save settings").clicked() {
                self.save_settings();
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Targets");
            ui.label(egui::RichText::new("Named build profiles (base OS + art depths + launcher RAM). Bundled defaults plus your own.").small().weak());
            let names = self.target_reg.names();
            let bundled = targets::Registry::bundled();
            for n in &names {
                ui.horizontal(|ui| {
                    let is_user = bundled.get(n).is_none();
                    let tag = if is_user { " (user)" } else { "" };
                    if ui.button("Edit").clicked() {
                        self.load_target_into_editor(n);
                    }
                    ui.add_enabled(is_user, egui::Button::new("✖"))
                        .on_hover_text("Remove this user target")
                        .clicked()
                        .then(|| self.remove_target(n));
                    if let Some(t) = self.target_reg.get(n) {
                        ui.label(format!("{n}{tag}"));
                        ui.label(egui::RichText::new(format!("— {} · {}", t.base_os, t.art_depths.join("/"))).small().weak());
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("Add / update a target:").small());
            egui::Grid::new("target_editor").num_columns(2).show(ui, |ui| {
                ui.label("name:"); ui.add(egui::TextEdit::singleline(&mut self.te_name).desired_width(300.0)); ui.end_row();
                ui.label("base OS:");
                let tmpls = self.templates.clone();
                egui::ComboBox::from_id_salt("te_base_os")
                    .selected_text(if self.te_base_os.is_empty() { "(pick)".into() } else { self.te_base_os.clone() })
                    .show_ui(ui, |ui| {
                        for k in &tmpls { ui.selectable_value(&mut self.te_base_os, k.clone(), k.as_str()); }
                    });
                ui.end_row();
                ui.label("art depths:"); ui.add(egui::TextEdit::singleline(&mut self.te_depths).hint_text("1,8").desired_width(120.0)); ui.end_row();
                ui.label("RAM pref/min KB:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.te_mem_pref).hint_text("pref").desired_width(56.0));
                    ui.add(egui::TextEdit::singleline(&mut self.te_mem_min).hint_text("min").desired_width(56.0));
                });
                ui.end_row();
                ui.label("label:"); ui.add(egui::TextEdit::singleline(&mut self.te_label).desired_width(300.0)); ui.end_row();
            });
            if ui.button("Save target").clicked() {
                self.save_target_from_editor();
            }
        });

        ui.add_space(8.0);
        ui.collapsing("Templates (base OS images)", |ui| {
            let reg = templates::Registry::load_default();
            if reg.0.is_empty() {
                ui.label(egui::RichText::new("No templates configured (data/templates.json).").small().weak());
            }
            for (k, t) in &reg.0 {
                ui.label(format!("{k} — {}", if t.label.is_empty() { t.hda.display().to_string() } else { t.label.clone() }));
            }
        });
    }

    fn load_target_into_editor(&mut self, name: &str) {
        if let Some(t) = self.target_reg.get(name) {
            self.te_name = name.to_string();
            self.te_base_os = t.base_os.clone();
            self.te_depths = t.art_depths.join(",");
            match t.app_mem_kb {
                Some([p, m]) => { self.te_mem_pref = p.to_string(); self.te_mem_min = m.to_string(); }
                None => { self.te_mem_pref.clear(); self.te_mem_min.clear(); }
            }
            self.te_label = t.label.clone();
        }
    }

    fn save_target_from_editor(&mut self) {
        let name = self.te_name.trim();
        if name.is_empty() || self.te_base_os.trim().is_empty() {
            self.status = "A target needs a name and a base OS.".into();
            return;
        }
        let depths: Vec<String> = self.te_depths.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        let pref = self.te_mem_pref.trim().parse::<u32>().ok();
        let app_mem_kb = pref.map(|p| [p, self.te_mem_min.trim().parse::<u32>().unwrap_or(p)]);
        let t = Target {
            base_os: self.te_base_os.trim().to_string(),
            art_depths: depths,
            app_mem_kb,
            disk_size_mb: None,
            max_art_size: None,
            label: self.te_label.trim().to_string(),
        };
        self.settings.targets.insert(name.to_string(), t);
        let path = settings::default_path();
        match self.settings.save(&path) {
            Ok(()) => {
                self.target_reg = targets::Registry::load_default();
                self.status = format!("Saved target \"{name}\" -> {}", path.display());
            }
            Err(e) => self.status = format!("Save target failed: {e}"),
        }
    }

    fn remove_target(&mut self, name: &str) {
        if self.settings.targets.remove(name).is_some() {
            let path = settings::default_path();
            match self.settings.save(&path) {
                Ok(()) => {
                    self.target_reg = targets::Registry::load_default();
                    if self.target_name == name { self.target_name.clear(); }
                    self.status = format!("Removed target \"{name}\".");
                }
                Err(e) => self.status = format!("Remove target failed: {e}"),
            }
        }
    }

    /// The first-run wizard: auto-detect rb-cli, prompt for the source folders.
    fn wizard(&mut self, ui: &mut egui::Ui) {
        ui.label("Welcome! Point MacAtrium at your source software and tools.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("rb-cli:");
            ui.add(egui::TextEdit::singleline(&mut self.rb_cli).desired_width(300.0));
            if ui.button("Detect").clicked() {
                if let Some(p) = detect_rb_cli() { self.rb_cli = p; }
            }
        });
        path_row(ui, "MacPack folder:", &mut self.macpack_dir, Pick::Folder);
        path_row(ui, "MG-Archive (optional):", &mut self.mg_archive, Pick::Folder);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save & continue").clicked() {
                self.save_settings();
                self.show_wizard = false;
            }
            if ui.button("Skip for now").clicked() {
                self.show_wizard = false;
            }
        });
    }
}

impl eframe::App for App {
    // eframe 0.34 hands us a root Ui (no panels). The body is a tab bar, the
    // active job's content, and a persistent status/progress bar — with a
    // first-run wizard floating over the top.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job();
        let busy = !self.busy.is_empty();
        let ctx = ui.ctx().clone();

        ui.horizontal_wrapped(|ui| {
            ui.heading("MacAtrium");
            ui.separator();
            ui.selectable_value(&mut self.tab, Tab::Build, "Build");
            ui.selectable_value(&mut self.tab, Tab::AddToDisk, "Add to disk");
            ui.selectable_value(&mut self.tab, Tab::Library, "Library");
            ui.selectable_value(&mut self.tab, Tab::Attain, "Attain");
            ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Settings");
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.separator();
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.status);
                });
            } else {
                ui.label(&self.status);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::Build => self.tab_build(ui, &ctx, busy),
                    Tab::AddToDisk => self.tab_add_to_disk(ui, &ctx, busy),
                    Tab::Library => self.tab_library(ui, &ctx, busy),
                    Tab::Attain => self.tab_attain(ui, &ctx, busy),
                    Tab::Settings => self.tab_settings(ui, &ctx, busy),
                });
        });

        if self.show_wizard {
            egui::Window::new("First-run setup")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| self.wizard(ui));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The full GUI Save -> JSON -> Load path: fields a user sets must survive the
    // round-trip through BuildConfig (and stay byte-compatible with the CLI).
    #[test]
    fn config_round_trips_through_gui() {
        let mut a = App::default();
        a.base_os = "6.0.8".into();
        a.out_image = "/tmp/out.hda".into();
        a.launcher = "build/MacAtrium.bin".into();
        a.dataset = "data/library.jsonl".into();
        a.disk_size_mb = "120".into();
        a.sel_mode = 3;
        a.sel_text = "Action, Puzzle".into();
        a.bw_only = true; // -> art_depths ["1"]
        a.app_mem_pref = "512".into();
        a.app_mem_min = "384".into();
        a.harvest = vec![HarvestUi {
            image: "/d.vhd".into(),
            apps: "/A\n/B".into(),
            scan: String::new(),
        }];

        let json = serde_json::to_string(&a.to_config()).unwrap();
        let cfg: BuildConfig = serde_json::from_str(&json).unwrap();
        let mut b = App::default();
        b.apply_config(cfg);

        assert_eq!(b.base_os, "6.0.8");
        assert_eq!(b.out_image, "/tmp/out.hda");
        assert_eq!(b.disk_size_mb, "120");
        assert_eq!(b.sel_mode, 3);
        assert_eq!(b.sel_text, "Action, Puzzle");
        assert!(b.bw_only);
        assert_eq!(b.app_mem_pref, "512");
        assert_eq!(b.app_mem_min, "384");
        assert_eq!(b.harvest.len(), 1);
        assert_eq!(b.harvest[0].apps, "/A\n/B");
    }

    // No launcher-RAM override -> app_mem_kb is None (keep the binary default).
    #[test]
    fn blank_launcher_ram_is_none() {
        let a = App::default();
        assert_eq!(a.app_mem_kb(), None);
    }

    // Picking a Target stamps its machine settings onto the form (via the shared
    // to_config -> Target::apply_to -> apply_config controller).
    #[test]
    fn target_pins_form_fields() {
        let mut a = App::default();
        // The bundled B&W target: 6.0.8, art ["1"], 512/384.
        let bw = a
            .target_reg
            .names()
            .into_iter()
            .find(|n| a.target_reg.get(n).map(|t| t.art_depths == ["1"]).unwrap_or(false))
            .expect("a bundled B&W target exists");
        a.apply_target(&bw);
        assert_eq!(a.base_os, "6.0.8");
        assert!(a.bw_only);
        assert_eq!(a.app_mem_pref, "512");
        assert_eq!(a.app_mem_min, "384");
    }

    // The picker selection syncs into the Selection::List the build reads.
    #[test]
    fn picker_selection_syncs_to_list() {
        let mut a = App::default();
        a.library = vec![
            LibRow { id: "x".into(), name: "X".into(), kind: "game".into(), year: String::new(), color: false, mouse: true, hotkey: String::new(), selected: true, dirty: false },
            LibRow { id: "y".into(), name: "Y".into(), kind: "game".into(), year: String::new(), color: false, mouse: true, hotkey: String::new(), selected: false, dirty: false },
        ];
        a.build_pick = true;
        a.sync_picker();
        assert_eq!(a.sel_mode, 2);
        assert_eq!(a.sel_text, "x");
        // reflect the other way
        a.sel_text = "x, y".into();
        a.reflect_selection();
        assert!(a.library.iter().all(|r| r.selected));
    }
}
