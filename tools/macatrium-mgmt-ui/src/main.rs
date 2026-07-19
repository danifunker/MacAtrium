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
use std::path::{Path, PathBuf};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MacAtrium Manager",
        opts,
        Box::new(|cc| {
            // Register the file:// + image loaders so the title picker can show
            // box-art thumbnails from the MG archive / a local art folder.
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<App>::default())
        }),
    )
}

/// The job-based screens (the top tab bar).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Build,
    AddToDisk,
    Library,
    Collections,
    Database,
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
    genres: Vec<String>,    // multi-valued tags (slice-and-dice filter)
    min_os: Option<String>, // OS scope (dotted), from the compatibility overlay
    max_os: Option<String>, // — used by the OS-migration scrub
    color: bool,            // true = Colour, false = B&W
    mouse: bool,            // true = Mouse Required
    hotkey: String,         // single-char launch hotkey (gamepad button map), "" = none
    selected: bool,         // included by the title picker
    dirty: bool,            // facet touched since last save
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
    lib_kind: String,  // "" = all kinds
    lib_genre: String, // "" = all genres
    build_pick: bool,  // Build: false = All compatible, true = Pick titles
    // box-art thumbnails: a Macintosh Garden art index (built lazily on a worker
    // when MG-Archive is set) + a per-id resolved thumbnail-URI cache.
    art_index: Option<atrium::mg::ArtIndex>,
    art_rx: Option<std::sync::mpsc::Receiver<Option<atrium::mg::ArtIndex>>>,
    art_requested: bool,
    thumbs: bool, // show box-art thumbnails in the picker
    thumb_cache: HashMap<String, Option<String>>, // id -> file:// URI (or None)
    // Database tab: the MG archive cross-referenced against MacPack (lazy worker).
    db: Option<Vec<atrium::mgdb::Entry>>,
    db_rx: Option<std::sync::mpsc::Receiver<Result<Vec<atrium::mgdb::Entry>, String>>>,
    db_requested: bool,
    db_archs: Vec<String>,
    db_systems: Vec<String>,
    db_cats: Vec<String>,
    db_detect_rx: Option<std::sync::mpsc::Receiver<atrium::mgdb::ColorCache>>,
    db_kind: String,     // "" | "game" | "app"
    db_arch: String,     // "" = any
    db_system: String,   // "" = any
    db_category: String, // "" = any
    db_min_year: String,
    db_max_year: String,
    db_color: u8,     // 0 any · 1 colour · 2 B&W
    db_missing: bool, // only titles not in MacPack
    db_search: String,
    db_selected: Option<usize>, // index into `db` of the detail-panel title
    db_shot: usize,             // which screenshot of the selected title is shown
    // MG download file-pick (Database detail): the selected title's download
    // options + the chosen file ("" = Auto), pinned into `curated` as mg.files.
    db_files: Vec<String>,      // the selected title's info.json downloads
    db_files_for: Option<i64>,  // the nid db_files was loaded for (refresh on change)
    db_file_pick: String,       // "" = Auto (smart pick), else an explicit filename
    // ---- Collections editor (curate each collection's Recommended set) ----
    coll_names: Vec<String>,       // collection names = *.json stems in bundled_dir()
    coll_selected: Option<String>, // the picked collection (name/stem)
    coll_loaded: Option<atrium::collections::Collection>, // its parsed JSON, the edit target
    coll_recommended: HashSet<String>, // ids currently toggled Recommended (edit buffer)
    coll_status: String,           // load/save status shown in the tab
    // ---- shared paths / dataset editing ----
    rb_cli: String,
    metadata: String,   // LaunchBox Metadata.xml
    mg_archive: String, // local Macintosh Garden archive root
    image_path: String, // selected .hda (Library: Load Existing MacAtrium Disk)
    dataset: String,    // blank = the library bundled in the tool
    overrides: String,  // blank = the compatibility overlay bundled in the tool
    curated: String,    // data/curated.jsonl overlay for pinning mg.files (blank = disabled)
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
    add_disk_path: String,     // Add-to-disk: the existing MacAtrium .hda
    migrate_disk: String,      // Build/migrate: import titles from this .hda
    // importing an existing disk's titles (migrate/clone) on a worker thread.
    import_rx: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
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
        let curated = settings
            .curated_overlay
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
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
            lib_genre: String::new(),
            build_pick: true,
            art_index: None,
            art_rx: None,
            art_requested: false,
            thumbs: false,
            thumb_cache: HashMap::new(),
            db: None,
            db_rx: None,
            db_requested: false,
            db_archs: Vec::new(),
            db_systems: Vec::new(),
            db_cats: Vec::new(),
            db_detect_rx: None,
            db_kind: String::new(),
            db_arch: "68k".into(), // the relevant default for a 68k appliance
            db_system: String::new(),
            db_category: String::new(),
            db_min_year: String::new(),
            db_max_year: String::new(),
            db_color: 0,
            db_missing: true, // default to the "what are we missing" view
            db_search: String::new(),
            db_selected: None,
            db_shot: 0,
            db_files: Vec::new(),
            db_files_for: None,
            db_file_pick: String::new(),
            coll_names: Vec::new(),
            coll_selected: None,
            coll_loaded: None,
            coll_recommended: HashSet::new(),
            coll_status: String::new(),
            rb_cli,
            metadata: String::new(),
            mg_archive,
            image_path: String::new(),
            dataset: String::new(),   // blank => bundled library
            overrides: String::new(), // blank => bundled compatibility overlay
            curated,
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
            migrate_disk: String::new(),
            import_rx: None,
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

fn opt_str(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
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
        let genres: Vec<String> = m
            .get("genre")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        // OS scope (the overlay wins, else the base record): drives the migration scrub.
        let os_field = |k: &str| {
            ov.and_then(|o| o.get(k))
                .and_then(Value::as_str)
                .or_else(|| m.get(k).and_then(Value::as_str))
                .map(str::to_string)
        };
        rows.push(LibRow {
            id,
            name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            kind: m.get("kind").and_then(Value::as_str).unwrap_or("").to_string(),
            year: m.get("year").and_then(Value::as_i64).map(|y| y.to_string()).unwrap_or_default(),
            genres,
            min_os: os_field("minOS"),
            max_os: os_field("maxOS"),
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

    /// Distinct genre tags present in the library (for the genre filter combo).
    fn genres(&self) -> Vec<String> {
        let mut set: Vec<String> = self.library.iter().flat_map(|r| r.genres.clone()).collect();
        set.sort();
        set.dedup();
        set
    }

    /// Library row indices passing the current search + kind + genre filters.
    fn filtered_indices(&self) -> Vec<usize> {
        let q = self.lib_search.to_lowercase();
        let kind = &self.lib_kind;
        let genre = &self.lib_genre;
        self.library
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                (kind.is_empty() || &r.kind == kind)
                    && (genre.is_empty() || r.genres.iter().any(|g| g == genre))
                    && (q.is_empty() || r.name.to_lowercase().contains(&q) || r.id.contains(&q))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The search + kind + genre filter bar (shared by the picker and Library).
    fn filter_bar(&mut self, ui: &mut egui::Ui, id_salt: &str) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).desired_width(200.0).hint_text("name…"));
            ui.label("Kind:");
            let kinds = self.kinds();
            let cur = if self.lib_kind.is_empty() { "(all)".to_string() } else { self.lib_kind.clone() };
            egui::ComboBox::from_id_salt(format!("{id_salt}_kind"))
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lib_kind, String::new(), "(all)");
                    for k in &kinds {
                        ui.selectable_value(&mut self.lib_kind, k.clone(), k.as_str());
                    }
                });
            ui.label("Genre:");
            let genres = self.genres();
            let curg = if self.lib_genre.is_empty() { "(all)".to_string() } else { self.lib_genre.clone() };
            egui::ComboBox::from_id_salt(format!("{id_salt}_genre"))
                .selected_text(curg)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lib_genre, String::new(), "(all)");
                    for g in &genres {
                        ui.selectable_value(&mut self.lib_genre, g.clone(), g.as_str());
                    }
                });
        });
    }

    /// Kick off (once) loading the Macintosh Garden art index on a worker thread,
    /// when thumbnails are on and an MG-Archive is configured. Cheap to call every
    /// frame — it self-gates.
    fn ensure_art_index(&mut self, ctx: &egui::Context) {
        if self.art_index.is_some() || self.art_requested || !self.thumbs {
            return;
        }
        let archive = self.mg_archive.trim().to_string();
        if archive.is_empty() || !PathBuf::from(&archive).exists() {
            return;
        }
        self.art_requested = true;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let idx = atrium::mg::ArtIndex::load(PathBuf::from(&archive).as_path()).ok();
            let _ = tx.send(idx);
            ctx.request_repaint();
        });
        self.art_rx = Some(rx);
    }

    /// The thumbnail file:// URI for a row, resolved (and cached) from the art
    /// index (MG box-art) — `None` until the index is ready or if there's no art.
    fn thumb_uri(&mut self, id: &str, name: &str) -> Option<String> {
        if let Some(hit) = self.thumb_cache.get(id) {
            return hit.clone();
        }
        let idx = self.art_index.as_ref()?;
        let uri = idx.box_art(name).map(|p| format!("file://{}", p.display()));
        self.thumb_cache.insert(id.to_string(), uri.clone());
        uri
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
        // Adopt a finished Macintosh Garden art index (box-art thumbnails).
        if let Some(idx) = self.art_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.art_index = idx;
            self.art_rx = None;
            self.thumb_cache.clear();
        }
        // Apply imported title ids (migrate/clone): tick the matching rows.
        if let Some(res) = self.import_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.import_rx = None;
            self.busy.clear();
            match res {
                Ok(ids) => {
                    self.ensure_library();
                    let want: HashSet<String> = ids.into_iter().collect();
                    let mut n = 0;
                    for r in &mut self.library {
                        if want.contains(&r.id) {
                            r.selected = true;
                            n += 1;
                        }
                    }
                    self.build_pick = true;
                    self.sync_picker();
                    self.status = format!(
                        "Imported {n} title(s). Pick a Target, optionally Scrub, then Build to migrate/clone."
                    );
                }
                Err(e) => self.status = format!("Import failed: {e}"),
            }
        }
        // Adopt a finished MG database load (the Database tab).
        if let Some(res) = self.db_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.db_rx = None;
            match res {
                Ok(entries) => {
                    self.db_archs = atrium::mgdb::architectures(&entries);
                    self.db_systems = atrium::mgdb::systems(&entries);
                    self.db_cats = atrium::mgdb::categories(&entries);
                    self.status = format!("Loaded {} Macintosh Garden record(s).", entries.len());
                    self.db = Some(entries);
                }
                Err(e) => self.status = format!("MG load failed: {e}"),
            }
        }
        // Adopt finished colour detection (fill colour where it was unknown).
        if let Some(cache) = self.db_detect_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.db_detect_rx = None;
            self.busy.clear();
            if let Some(db) = &mut self.db {
                let mut n = 0;
                for e in db.iter_mut() {
                    if e.color.is_none() {
                        if let Some(&c) = cache.get(&e.nid) {
                            e.color = Some(c);
                            n += 1;
                        }
                    }
                }
                self.status = format!("Detected colour for {n} title(s).");
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
                None, // no global --file override: per-title picks ride in the dataset (mg.files)
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

    /// Read an existing MacAtrium disk's catalog and tick its titles in the
    /// picker — the seed of an OS-migration or a clone. Runs rb-cli on a worker.
    fn import_from_disk(&mut self, ctx: &egui::Context) {
        let disk = self.migrate_disk.trim().to_string();
        if disk.is_empty() {
            self.status = "Pick the disk to import titles from first.".into();
            return;
        }
        let rb = self.rb_cli.clone();
        let meta_dir = self.metadata_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> Result<Vec<String>, String> {
                let rbc = RbCli::new(&rb);
                let tmp = std::env::temp_dir().join("macatrium-import-catalog.jsonl");
                let _ = std::fs::remove_file(&tmp);
                let src = format!("{}/catalog.jsonl", meta_dir.trim_end_matches('/'));
                rbc.get(PathBuf::from(&disk).as_path(), &src, &tmp, true).map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
                Ok(atrium::catalog::parse_compiled(&bytes)
                    .iter()
                    .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect())
            })();
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        self.import_rx = Some(rx);
        self.busy = "Importing titles".into();
        self.status = "Importing titles from the disk…".into();
    }

    /// Un-tick selected titles whose OS scope (minOS/maxOS) excludes the current
    /// Target's OS — the migration scrub, the same scope a build applies.
    fn scrub_incompatible(&mut self) {
        let os = self.base_os.trim().to_string();
        if os.is_empty() {
            self.status = "Pick a Target first — its OS is what titles are scrubbed against.".into();
            return;
        }
        let mut scrubbed = 0;
        for r in &mut self.library {
            if r.selected
                && !atrium::selection::os_in_range(&os, r.min_os.as_deref(), r.max_os.as_deref())
            {
                r.selected = false;
                scrubbed += 1;
            }
        }
        self.sync_picker();
        self.status = if scrubbed == 0 {
            format!("All selected titles are compatible with {os}.")
        } else {
            format!("Scrubbed {scrubbed} title(s) incompatible with {os}.")
        };
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
        s.curated_overlay = opt_path(&self.curated);
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

    /// The shared title picker: search + kind/genre filters + a virtualised,
    /// tickable list with optional box-art thumbnails. Toggling a tick re-syncs
    /// the build `Selection`.
    fn title_picker(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ensure_library();
        self.ensure_art_index(ctx);

        self.filter_bar(ui, "pick");
        let filtered = self.filtered_indices();
        ui.horizontal(|ui| {
            if ui.small_button("Select all (filtered)").clicked() {
                for &i in &filtered { self.library[i].selected = true; }
                self.sync_picker();
            }
            if ui.small_button("Clear all").clicked() {
                for r in &mut self.library { r.selected = false; }
                self.sync_picker();
            }
            ui.separator();
            ui.checkbox(&mut self.thumbs, "thumbnails")
                .on_hover_text("Show box-art thumbnails from the Macintosh Garden archive (set it in Settings).");
            ui.separator();
            ui.label(egui::RichText::new(format!("{} shown · {} selected", filtered.len(), self.selected_count())).small().weak());
        });
        ui.separator();

        const THUMB: f32 = 40.0;
        let mut changed = false;
        let row_h = if self.thumbs { THUMB + 6.0 } else { ui.text_style_height(&egui::TextStyle::Body) + 6.0 };
        egui::ScrollArea::vertical()
            .id_salt("title_picker")
            .auto_shrink([false, false])
            .max_height(340.0)
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for vis in range {
                    let idx = filtered[vis];
                    // Resolve the thumbnail first (borrows art_index/thumb_cache),
                    // before taking a &mut to the row — avoids aliasing self.
                    let uri = if self.thumbs {
                        let (id, name) = {
                            let r = &self.library[idx];
                            (r.id.clone(), r.name.clone())
                        };
                        self.thumb_uri(&id, &name)
                    } else {
                        None
                    };
                    let r = &mut self.library[idx];
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut r.selected, "").changed() {
                            changed = true;
                        }
                        if self.thumbs {
                            if let Some(u) = uri {
                                ui.add(egui::Image::from_uri(u).fit_to_exact_size(egui::vec2(THUMB, THUMB)));
                            } else {
                                ui.add_space(THUMB);
                            }
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

    /// The Target picker combo + a one-line summary of its pinned settings.
    /// Shared by Build and Add-to-disk. Applies the first Target on first view so
    /// a fresh screen is ready, and re-applies on selection.
    fn target_combo(&mut self, ui: &mut egui::Ui) {
        if self.target_name.is_empty() {
            if let Some(first) = self.target_reg.names().into_iter().next() {
                self.apply_target(&first);
            }
        }
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
    }

    fn tab_build(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Pick a Target (the Mac you're building for), choose the titles, and Build a fresh bootable disk.")
                .small().weak(),
        );
        ui.add_space(6.0);
        self.target_combo(ui);

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
                self.title_picker(ui, ctx);
            } else {
                ui.label(egui::RichText::new("Every title compatible with the Target's OS will be included.").small().weak());
            }
        });

        ui.add_space(6.0);
        ui.collapsing("Migrate / clone from an existing disk", |ui| {
            ui.label(
                egui::RichText::new(
                    "Import another MacAtrium disk's titles, then pick a Target — a newer OS to \
                     migrate, or the same to clone — and Build. Scrub drops the titles the chosen \
                     OS can't run (minOS/maxOS), so a migration leaves them behind.",
                )
                .small().weak(),
            );
            path_row(ui, "Existing disk (.hda):", &mut self.migrate_disk, Pick::File);
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Import titles")).clicked() {
                    self.import_from_disk(ctx);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Scrub incompatible with Target"))
                    .on_hover_text("Un-tick selected titles the current Target's OS can't run.")
                    .clicked()
                {
                    self.scrub_incompatible();
                }
            });
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

    /// Add the selected titles to an existing MacAtrium disk via the shared
    /// `image::add_to_disk` controller (harvest-into + compiled-catalog merge).
    fn run_add_to_disk(&mut self, ctx: &egui::Context) {
        if self.add_disk_path.trim().is_empty() {
            self.status = "Pick the MacAtrium disk to add to first.".into();
            return;
        }
        if self.selected_count() == 0 {
            self.status = "Select at least one title to add.".into();
            return;
        }
        self.sync_picker(); // ensure the Selection reflects the ticked titles
        let mut cfg = self.to_config();
        cfg.out = PathBuf::from(self.add_disk_path.trim());
        let disk = self.add_disk_path.clone();
        let n = self.selected_count();
        self.spawn_job(ctx, &format!("Adding {n} title(s) to the disk"), move || {
            match image::add_to_disk(&cfg) {
                Ok(()) => Done { status: format!("Added {n} title(s) -> {disk}"), dataset: None, reload: false },
                Err(e) => Done { status: format!("Add failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    fn tab_add_to_disk(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Extend an already-built MacAtrium disk with more titles, without rebuilding from scratch. The titles already on the disk keep their artwork.")
                .small().weak(),
        );
        ui.add_space(6.0);
        path_row(ui, "MacAtrium disk (.hda):", &mut self.add_disk_path, Pick::File);
        ui.add_space(6.0);
        // Match the disk's original Target so OS-scoping + art depths line up.
        self.target_combo(ui);
        ui.label(
            egui::RichText::new("Pick the Target the disk was built with, so the new titles get matching art depths.")
                .small().weak(),
        );
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Titles to add");
            self.build_pick = true;
            self.title_picker(ui, ctx);
        });
        ui.add_space(8.0);
        if ui
            .add_enabled(!busy, egui::Button::new(egui::RichText::new("Add to disk").strong()))
            .clicked()
        {
            self.run_add_to_disk(ctx);
        }
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

        self.filter_bar(ui, "lib");
        let filtered = self.filtered_indices();

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

    /// Re-scan the collections folder (`collections::bundled_dir()`) for the
    /// available `*.json` collection names (their filename stems), sorted.
    fn reload_collections(&mut self) {
        let dir = atrium::collections::bundled_dir();
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        self.coll_names = names;
    }

    /// Load a collection by name into the editor, seeding the Recommended edit
    /// buffer from its saved `recommended` ids.
    fn load_collection(&mut self, name: &str) {
        let path = atrium::collections::bundled_dir().join(format!("{name}.json"));
        match atrium::collections::Collection::load(&path) {
            Ok(c) => {
                self.coll_recommended = c.recommended.iter().cloned().collect();
                self.coll_status = format!(
                    "Loaded \"{name}\": {} games · {} recommended.",
                    c.ids.len(),
                    c.recommended.len()
                );
                self.coll_loaded = Some(c);
                self.coll_selected = Some(name.to_string());
            }
            Err(e) => {
                self.coll_loaded = None;
                self.coll_selected = Some(name.to_string());
                self.coll_status = format!("Load failed: {e}");
            }
        }
    }

    /// Write the loaded collection back to `collections::bundled_dir()`, setting
    /// its `recommended` to the collection's own ids filtered to the toggled set
    /// (so the saved order matches build order).
    fn save_collection(&mut self) {
        let name = match &self.coll_selected {
            Some(n) => n.clone(),
            None => {
                self.coll_status = "Pick a collection first.".into();
                return;
            }
        };
        if self.coll_loaded.is_none() {
            self.coll_status = "No collection loaded.".into();
            return;
        }
        // Recommended = the collection's ids that are toggled on, in id order.
        let rec: Vec<String> = {
            let coll = self.coll_loaded.as_ref().unwrap();
            coll.ids
                .iter()
                .filter(|id| self.coll_recommended.contains(id.as_str()))
                .cloned()
                .collect()
        };
        let path = atrium::collections::bundled_dir().join(format!("{name}.json"));
        let coll = self.coll_loaded.as_mut().unwrap();
        coll.recommended = rec;
        let n = coll.recommended.len();
        match serde_json::to_string_pretty(&*coll) {
            Ok(json) => match std::fs::write(&path, json) {
                Ok(()) => self.coll_status = format!("Saved {n} recommended -> {}", path.display()),
                Err(e) => self.coll_status = format!("Save failed: {e}"),
            },
            Err(e) => self.coll_status = format!("Encode failed: {e}"),
        }
    }

    /// The Collections editor: pick a collection, toggle which of its games are
    /// **Recommended** (surfaced in the launcher's Recommended nav category), Save.
    fn tab_collections(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        self.ensure_library();
        if self.coll_names.is_empty() {
            self.reload_collections();
        }
        ui.label(
            egui::RichText::new(
                "Pick a collection, toggle which games are Recommended (surfaced in the launcher's \
                 Recommended nav category), then Save.",
            )
            .small()
            .weak(),
        );
        ui.add_space(6.0);

        // Collection picker + a re-scan button (mirrors the Target combo).
        ui.horizontal(|ui| {
            ui.label("Collection:");
            let names = self.coll_names.clone();
            let cur = self.coll_selected.clone().unwrap_or_else(|| "(choose)".to_string());
            let mut pick: Option<String> = None;
            egui::ComboBox::from_id_salt("coll_pick")
                .selected_text(cur)
                .width(340.0)
                .show_ui(ui, |ui| {
                    for n in &names {
                        if ui
                            .selectable_label(self.coll_selected.as_deref() == Some(n.as_str()), n)
                            .clicked()
                        {
                            pick = Some(n.clone());
                        }
                    }
                });
            if ui.button("Reload").on_hover_text("Re-scan the collections folder for *.json.").clicked() {
                self.reload_collections();
            }
            if let Some(n) = pick {
                self.load_collection(&n);
            }
        });

        // Nothing loaded yet — invite a pick and stop.
        let (label, ids) = match self.coll_loaded.as_ref() {
            Some(c) => (c.label.clone(), c.ids.clone()),
            None => {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Pick a collection above to edit its Recommended set.").weak());
                return;
            }
        };

        ui.add_space(6.0);
        if !label.is_empty() {
            ui.label(egui::RichText::new(label.as_str()).weak());
        }
        ui.label(
            egui::RichText::new(format!("{} games · {} recommended", ids.len(), self.coll_recommended.len()))
                .small()
                .weak(),
        );
        ui.add_space(4.0);

        // Shared search + kind/genre filter bar.
        self.filter_bar(ui, "coll");

        // The rows to show: (id, display name) in collection order, passing the
        // filter. Owned so the scroll closure only borrows `coll_recommended`.
        let rows: Vec<(String, String)> = {
            let q = self.lib_search.to_lowercase();
            let by_id: HashMap<&str, &LibRow> =
                self.library.iter().map(|r| (r.id.as_str(), r)).collect();
            ids.iter()
                .filter_map(|id| {
                    let row = by_id.get(id.as_str()).copied();
                    let name = row.map(|r| r.name.clone()).unwrap_or_else(|| id.clone());
                    let kind_ok =
                        self.lib_kind.is_empty() || row.map(|r| r.kind == self.lib_kind).unwrap_or(false);
                    let genre_ok = self.lib_genre.is_empty()
                        || row.map(|r| r.genres.iter().any(|g| *g == self.lib_genre)).unwrap_or(false);
                    let hit = q.is_empty()
                        || name.to_lowercase().contains(&q)
                        || id.to_lowercase().contains(&q);
                    (kind_ok && genre_ok && hit).then(|| (id.clone(), name))
                })
                .collect()
        };

        ui.horizontal(|ui| {
            if ui.small_button("Recommend all shown").clicked() {
                for (id, _) in &rows {
                    self.coll_recommended.insert(id.clone());
                }
            }
            if ui.small_button("Clear recommended").clicked() {
                self.coll_recommended.clear();
            }
            ui.separator();
            ui.label(
                egui::RichText::new(format!("{} shown · {} recommended", rows.len(), self.coll_recommended.len()))
                    .small()
                    .weak(),
            );
        });
        ui.separator();

        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        egui::ScrollArea::vertical()
            .id_salt("coll_edit")
            .auto_shrink([false, false])
            .max_height(380.0)
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for vis in range {
                    let (id, name) = &rows[vis];
                    let mut is_rec = self.coll_recommended.contains(id.as_str());
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut is_rec, "Recommended").changed() {
                            if is_rec {
                                self.coll_recommended.insert(id.clone());
                            } else {
                                self.coll_recommended.remove(id.as_str());
                            }
                        }
                        ui.label(name);
                        ui.label(egui::RichText::new(id.as_str()).small().weak());
                    });
                }
            });

        ui.separator();
        if ui.button(egui::RichText::new("Save collection").strong()).clicked() {
            self.save_collection();
        }
        if !self.coll_status.is_empty() {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(self.coll_status.as_str()).small().weak());
        }
    }

    /// Kick off (once) loading the MG archive cross-referenced against MacPack,
    /// on a worker thread (~21k records). Self-gates; needs a valid MG-Archive.
    fn ensure_db(&mut self, ctx: &egui::Context) {
        if self.db.is_some() || self.db_requested {
            return;
        }
        let archive = self.mg_archive.trim().to_string();
        if archive.is_empty() || !PathBuf::from(&archive).exists() {
            return;
        }
        self.db_requested = true;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let res = atrium::mgdb::load(
                PathBuf::from(&archive).as_path(),
                atrium::config::EMBEDDED_LIBRARY,
                atrium::config::EMBEDDED_COMPAT,
            )
            .map_err(|e| e.to_string());
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        self.db_rx = Some(rx);
        self.status = "Loading the Macintosh Garden archive…".into();
    }

    fn db_filter(&self) -> atrium::mgdb::Filter {
        use atrium::mgdb::{Filter, Kind};
        Filter {
            kind: match self.db_kind.as_str() {
                "game" => Some(Kind::Game),
                "app" => Some(Kind::App),
                _ => None,
            },
            arch: opt_str(&self.db_arch),
            system: opt_str(&self.db_system),
            min_year: self.db_min_year.trim().parse().ok(),
            max_year: self.db_max_year.trim().parse().ok(),
            category: opt_str(&self.db_category),
            color: match self.db_color {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            },
            mouse: None,
            in_macpack: if self.db_missing { Some(false) } else { None },
            search: opt_str(&self.db_search),
        }
    }

    /// Detect colour (offline, from screenshots) for the currently-filtered set on
    /// a worker thread, then fill it into the table.
    fn run_db_detect(&mut self, ctx: &egui::Context) {
        let Some(db) = &self.db else { return };
        let mut base = self.db_filter();
        base.color = None; // detect over everything matching the OTHER filters
        let subset: Vec<atrium::mgdb::Entry> = db.iter().filter(|e| base.matches(e)).cloned().collect();
        let n = subset.len();
        if n == 0 {
            self.status = "No titles in the current filter to detect.".into();
            return;
        }
        let archive = self.mg_archive.trim().to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let a = PathBuf::from(&archive);
            let mut cache = atrium::mgdb::load_color_cache(&a);
            atrium::mgdb::detect_color(&a, &subset, &mut cache, |_, _| {});
            let _ = atrium::mgdb::save_color_cache(&a, &cache);
            let _ = tx.send(cache);
            ctx2.request_repaint();
        });
        self.db_detect_rx = Some(rx);
        self.busy = format!("Detecting colour for {n} title(s)");
    }

    /// The Database filter bar (kind/arch/system/category combos + year range +
    /// colour + missing toggle + search), driven by the cached distinct lists.
    fn db_filter_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let combo = |ui: &mut egui::Ui, salt: &str, label: &str, cur: &mut String, opts: &[String]| {
                ui.label(label);
                let text = if cur.is_empty() { "(any)".to_string() } else { cur.clone() };
                egui::ComboBox::from_id_salt(salt).selected_text(text).show_ui(ui, |ui| {
                    ui.selectable_value(cur, String::new(), "(any)");
                    for o in opts {
                        ui.selectable_value(cur, o.clone(), o.as_str());
                    }
                });
            };
            ui.label("Type:");
            egui::ComboBox::from_id_salt("db_kind")
                .selected_text(match self.db_kind.as_str() { "game" => "Games", "app" => "Apps", _ => "(all)" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.db_kind, String::new(), "(all)");
                    ui.selectable_value(&mut self.db_kind, "game".into(), "Games");
                    ui.selectable_value(&mut self.db_kind, "app".into(), "Apps");
                });
            let (archs, systems, cats) = (self.db_archs.clone(), self.db_systems.clone(), self.db_cats.clone());
            combo(ui, "db_arch", "Arch:", &mut self.db_arch, &archs);
            combo(ui, "db_system", "OS:", &mut self.db_system, &systems);
            combo(ui, "db_category", "Category:", &mut self.db_category, &cats);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Year:");
            ui.add(egui::TextEdit::singleline(&mut self.db_min_year).desired_width(48.0).hint_text("min"));
            ui.label("–");
            ui.add(egui::TextEdit::singleline(&mut self.db_max_year).desired_width(48.0).hint_text("max"));
            ui.separator();
            ui.label("Colour:");
            ui.radio_value(&mut self.db_color, 0u8, "any");
            ui.radio_value(&mut self.db_color, 1u8, "colour");
            ui.radio_value(&mut self.db_color, 2u8, "B&W");
            ui.separator();
            ui.checkbox(&mut self.db_missing, "missing from MacPack only");
            ui.separator();
            ui.label("Search:");
            ui.add(egui::TextEdit::singleline(&mut self.db_search).desired_width(160.0).hint_text("title…"));
        });
    }

    fn tab_database(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Explore the Macintosh Garden archive cross-referenced against MacPack — to see what we're missing. Colour/B&W isn't in MG's data; Detect colour fills it offline from screenshots (cached).")
                .small().weak(),
        );
        ui.add_space(4.0);
        self.ensure_db(ctx);
        if self.db.is_none() {
            ui.add_space(8.0);
            if self.db_requested {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Loading ~21k records…"); });
            } else {
                ui.label(egui::RichText::new("Set a valid MG-Archive folder in ⚙ Settings to explore it.").weak());
            }
            return;
        }

        self.db_filter_bar(ui);

        // Filter (immutable borrow ends in this block) → indices + counts.
        let filter = self.db_filter();
        let (idxs, missing) = {
            let db = self.db.as_ref().unwrap();
            let idxs: Vec<usize> = db.iter().enumerate().filter(|(_, e)| filter.matches(e)).map(|(i, _)| i).collect();
            let missing = idxs.iter().filter(|&&i| !db[i].in_macpack).count();
            (idxs, missing)
        };
        let total = idxs.len();

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong(format!("{total} match"));
            ui.label(egui::RichText::new(format!("· {missing} missing from MacPack")).weak());
            ui.separator();
            if ui.add_enabled(!busy, egui::Button::new("Detect colour (filtered)"))
                .on_hover_text("Fill Colour/B&W for the filtered titles offline from their screenshots (cached).")
                .clicked()
            {
                self.run_db_detect(ctx);
            }
        });
        ui.label(egui::RichText::new("● = missing from MacPack. Click a title for details + screenshots.").small().weak());
        ui.separator();

        // Master (filtered title list) ⟷ detail (the selected title + screenshots).
        let archive = PathBuf::from(self.mg_archive.trim());
        let sel = self.db_selected;
        let shot = self.db_shot;
        let mut clicked: Option<usize> = None;
        let mut new_shot = shot;
        // MG download picker: (re)load the selected title's downloads when the
        // selection changes, then stage the pick + actions as locals — the detail
        // renders inside the `self.db` borrow below, so persist/fetch run after it.
        let sel_nid = sel.and_then(|i| self.db.as_ref().unwrap().get(i)).map(|e| e.nid);
        if sel_nid != self.db_files_for {
            self.db_files = sel_nid
                .map(|nid| atrium::fetch::list_downloads(&archive, nid))
                .unwrap_or_default();
            self.db_files_for = sel_nid;
            self.db_file_pick.clear();
        }
        let files = self.db_files.clone();
        let mut pick = self.db_file_pick.clone();
        let curated_set = !self.curated.trim().is_empty();
        let archive_set = !self.mg_archive.trim().is_empty();
        let mut pin_now = false;
        let mut fetch_now = false;
        {
            let db = self.db.as_ref().unwrap();
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    egui::ScrollArea::vertical()
                        .id_salt("db_list")
                        .auto_shrink([false, false])
                        .max_height(440.0)
                        .show_rows(ui, row_h, idxs.len(), |ui, range| {
                            for vis in range {
                                let gi = idxs[vis];
                                let e = &db[gi];
                                let dot = if e.in_macpack { "   " } else { "●  " };
                                let yr = e.year.map(|y| format!("   ·  {y}")).unwrap_or_default();
                                if ui
                                    .selectable_label(sel == Some(gi), format!("{dot}{}{yr}", e.title))
                                    .clicked()
                                {
                                    clicked = Some(gi);
                                }
                            }
                        });
                });
                ui.separator();
                ui.vertical(|ui| match sel.and_then(|i| db.get(i)) {
                    Some(e) => {
                        db_detail(ui, e, &archive, shot, &mut new_shot);
                        download_picker(
                            ui, &files, &mut pick, archive_set, curated_set,
                            &mut fetch_now, &mut pin_now,
                        );
                    }
                    None => {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Select a title on the left to see its details and screenshots.").weak());
                    }
                });
            });
        }
        if let Some(c) = clicked {
            self.db_selected = Some(c);
            self.db_shot = 0;
            self.db_file_pick.clear();
        } else {
            self.db_shot = new_shot;
            self.db_file_pick = pick;
            if let Some(nid) = self.db_files_for {
                if fetch_now {
                    let f = self.db_file_pick.clone();
                    self.run_db_fetch(ctx, nid, f);
                } else if pin_now {
                    self.pin_mg_download(nid);
                }
            }
        }
    }

    /// Fetch a single Database-tab title (by nid) into the cache with the chosen
    /// file (blank = the smart auto-pick), reusing the `atrium fetch` pipeline. We
    /// have the exact nid + file here, so no dataset name-matching is needed.
    fn run_db_fetch(&mut self, ctx: &egui::Context, nid: i64, file: String) {
        if self.mg_archive.trim().is_empty() {
            self.status = "Set the MG-Archive (Settings) first.".into();
            return;
        }
        let archive = self.mg_archive.clone();
        let cache = self.cache_dir.clone();
        let rb = self.rb_cli.clone();
        let curl = self.curl.clone();
        let file_opt = {
            let f = file.trim().to_string();
            (!f.is_empty()).then_some(f)
        };
        self.spawn_job(ctx, &format!("Downloading nid {nid} from Macintosh Garden"), move || {
            let downloads = opt_path(&cache);
            match fetch::run(
                PathBuf::from(&archive).as_path(),
                &[nid],
                file_opt.as_deref(),
                None, // no dataset src — a single explicit nid
                downloads.as_deref(),
                None, // cache only — no injection
                "/MacAtrium/Apps",
                None,
                &rb,
                &curl,
                None,
            ) {
                Ok(()) => Done { status: format!("Downloaded nid {nid} into the cache."), dataset: None, reload: false },
                Err(e) => Done { status: format!("MG download failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// Pin the current Database-tab file pick into the curated overlay as
    /// `mg.{nid,files}` for the selected title (keyed by its slug id). Auto (empty
    /// pick) pins just the durable nid; an explicit file adds `files:[<name>]`.
    fn pin_mg_download(&mut self, nid: i64) {
        let curated = self.curated.trim().to_string();
        if curated.is_empty() {
            self.status = "Set a Curated overlay (Settings) to pin a download.".into();
            return;
        }
        let Some(title) = self
            .db
            .as_ref()
            .and_then(|db| db.iter().find(|e| e.nid == nid))
            .map(|e| e.title.clone())
        else {
            return;
        };
        let id = atrium::harvest::slugify(&title);
        let mut mg: Map<String, Value> = Map::new();
        mg.insert("nid".into(), Value::from(nid));
        let pick = self.db_file_pick.trim().to_string();
        let picked_label = if pick.is_empty() {
            "Auto (nid only)".to_string()
        } else {
            mg.insert("files".into(), Value::from(vec![pick.clone()]));
            pick.clone()
        };
        let mut fields: Map<String, Value> = Map::new();
        fields.insert("mg".into(), Value::Object(mg));
        match merge::set(std::path::Path::new(&curated), &id, &fields) {
            Ok(()) => self.status = format!("Pinned \"{title}\" [{id}] download: {picked_label} -> {curated}"),
            Err(e) => self.status = format!("Pin failed: {e}"),
        }
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
            path_row(ui, "Curated overlay:", &mut self.curated, Pick::File);
            ui.label(egui::RichText::new("data/curated.jsonl — where the Database tab pins per-title MG download picks (mg.files).").small().weak());
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

/// The Database detail panel: the selected MG title's facts, description, MG
/// page link, and a screenshot carousel (◀ / ▶ over its on-disk images). Reads
/// `shot` (the current image index) and writes the new index to `new_shot`.
fn db_detail(ui: &mut egui::Ui, e: &atrium::mgdb::Entry, archive: &Path, shot: usize, new_shot: &mut usize) {
    ui.heading(&e.title);
    if e.in_macpack {
        ui.label(egui::RichText::new("✓ In MacPack").color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)));
    } else {
        ui.label(egui::RichText::new("● Missing from MacPack").color(egui::Color32::from_rgb(0xe0, 0x6c, 0x4c)).strong());
    }
    if let Some(url) = e.page_url() {
        ui.hyperlink_to("Macintosh Garden page ↗", url);
    }
    ui.add_space(4.0);

    egui::Grid::new("db_detail_grid").num_columns(2).striped(true).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            if !v.is_empty() {
                ui.label(egui::RichText::new(k).weak());
                ui.label(v);
                ui.end_row();
            }
        };
        row("Type", e.kind.label().to_string());
        row("Year", e.year.map(|y| y.to_string()).unwrap_or_default());
        row("Developer", e.developer.clone().unwrap_or_default());
        row("Architecture", e.arch.join(", "));
        row("Runs on", e.systems.join(", "));
        row("Category", e.categories.join(", "));
        row("Perspective", e.perspective.join(", "));
        row("Colour", match e.color {
            Some(true) => "Colour".into(),
            Some(false) => "B&W".into(),
            None => "unknown (Detect colour)".into(),
        });
        if let Some(m) = e.mouse {
            row("Mouse", if m { "required".into() } else { "not required".into() });
        }
    });

    if !e.desc.is_empty() {
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("db_desc").max_height(120.0).show(ui, |ui| {
            ui.label(&e.desc);
        });
    }

    // Screenshot carousel over the title's on-disk images.
    ui.add_space(6.0);
    let shots = e.image_paths(archive);
    if shots.is_empty() {
        ui.label(egui::RichText::new("(no screenshots on disk)").weak());
    } else {
        let idx = shot.min(shots.len() - 1);
        ui.horizontal(|ui| {
            if ui.add_enabled(shots.len() > 1, egui::Button::new("◀")).clicked() {
                *new_shot = (idx + shots.len() - 1) % shots.len();
            }
            ui.label(format!("{} / {}", idx + 1, shots.len()));
            if ui.add_enabled(shots.len() > 1, egui::Button::new("▶")).clicked() {
                *new_shot = (idx + 1) % shots.len();
            }
            if let Some(name) = shots[idx].file_name() {
                ui.label(egui::RichText::new(name.to_string_lossy()).small().weak());
            }
        });
        let uri = format!("file://{}", shots[idx].display());
        ui.add(egui::Image::from_uri(uri).max_width(420.0).max_height(300.0));
    }
}

/// The MG download picker under a Database-tab detail: choose which file `atrium
/// fetch` should pull for this title ("Auto" = the smart default), then either
/// fetch it now (into the cache) or pin it into the curated overlay as `mg.files`.
/// `fetch`/`pin` are set true on the respective button click (applied by the
/// caller, which has `&mut self`, once the `self.db` borrow ends).
fn download_picker(
    ui: &mut egui::Ui,
    files: &[String],
    pick: &mut String,
    archive_set: bool,
    curated_set: bool,
    fetch: &mut bool,
    pin: &mut bool,
) {
    ui.add_space(8.0);
    ui.separator();
    ui.strong("Download");
    if files.is_empty() {
        ui.label(
            egui::RichText::new("No download list for this title (its info.json isn't in the MG-Archive).")
                .small()
                .weak(),
        );
        return;
    }
    ui.horizontal(|ui| {
        ui.label("File:");
        let current = if pick.is_empty() { "Auto (smart pick)".to_string() } else { pick.clone() };
        egui::ComboBox::from_id_salt("mg_file_pick")
            .selected_text(current)
            .width(280.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(pick, String::new(), "Auto (smart pick)");
                for f in files {
                    ui.selectable_value(pick, f.clone(), f.as_str());
                }
            });
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(archive_set, egui::Button::new("Download now"))
            .on_hover_text("Fetch this file into the cache now (atrium fetch --nid).")
            .clicked()
        {
            *fetch = true;
        }
        if ui
            .add_enabled(curated_set, egui::Button::new("Pin to curated overlay"))
            .on_hover_text("Write mg.{nid,files} into curated.jsonl so a later fetch pulls this exact download.")
            .clicked()
        {
            *pin = true;
        }
    });
    if !curated_set {
        ui.label(
            egui::RichText::new("Set a Curated overlay (Settings) to enable pinning.")
                .small()
                .weak(),
        );
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
            ui.selectable_value(&mut self.tab, Tab::Collections, "Collections");
            ui.selectable_value(&mut self.tab, Tab::Database, "Database");
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
                    Tab::Collections => self.tab_collections(ui, &ctx),
                    Tab::Database => self.tab_database(ui, &ctx, busy),
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
            LibRow { id: "x".into(), name: "X".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: None, color: false, mouse: true, hotkey: String::new(), selected: true, dirty: false },
            LibRow { id: "y".into(), name: "Y".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: None, color: false, mouse: true, hotkey: String::new(), selected: false, dirty: false },
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

    // OS-migration scrub: titles the target OS can't run get un-ticked.
    #[test]
    fn migration_scrub_drops_out_of_range() {
        let mut a = App::default();
        a.base_os = "7.5".into(); // migrating to System 7.5
        a.build_pick = true;
        a.library = vec![
            // playable only up to 7.1 -> dropped on 7.5
            LibRow { id: "old".into(), name: "Old".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: Some("7.1".into()), color: false, mouse: true, hotkey: String::new(), selected: true, dirty: false },
            // open OS range -> kept
            LibRow { id: "any".into(), name: "Any".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: None, color: false, mouse: true, hotkey: String::new(), selected: true, dirty: false },
        ];
        a.scrub_incompatible();
        assert!(!a.library[0].selected, "7.1-max title scrubbed on 7.5");
        assert!(a.library[1].selected, "open-range title kept");
        assert_eq!(a.sel_text, "any");
    }
}
