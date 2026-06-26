//! MacAtrium Management UI — an egui front-end for the MacAtrium build tooling.
//!
//! Every action here calls the `atrium` **library** — the exact functions the
//! CLI exposes — so the CLI stays the source of truth and this is just a nicer
//! way to drive it. The UI is organised as a **three-step flow**:
//!
//!   1. **Library** — pick an `.hda` and extract its catalog (or open a dataset),
//!      then edit the Color/B&W + Mouse facets (and per-item hotkey) and save.
//!   2. **Enrich** — fill metadata from public sources (LaunchBox and/or the local
//!      **Macintosh Garden** archive), and optionally **fetch** a title's software
//!      from Macintosh Garden into the output image.
//!   3. **Build** — assemble a bootable `.hda`. The essentials are up front; every
//!      other `atrium image` option lives behind an **Advanced** disclosure.
//!
//! Long operations (extract / enrich / mg / fetch / build) run on a worker thread
//! so the window stays responsive.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use atrium::{config::{BuildConfig, HarvestSrc, Selection}, enrich, fetch, image, merge, mg, rbcli::RbCli, templates};
use eframe::egui;
use serde_json::{Map, Value};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MacAtrium Manager",
        opts,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
}

/// The three workflow steps (the top tab bar).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Library,
    Enrich,
    Build,
}

/// One editable dataset row. `raw` keeps every field so saving never drops data.
struct Row {
    id: String,
    name: String,
    year: String,
    vendor: String,
    genre: String,
    color: bool, // true = Color, false = B&W
    mouse: bool, // true = Mouse Required, false = No Mouse
    hotkey: String, // single-char launch hotkey (gamepad button map), "" = none
    dirty: bool, // touched since last save
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
    // shared paths / dataset editing
    rb_cli: String,
    metadata: String, // LaunchBox Metadata.xml
    mg_archive: String, // local Macintosh Garden archive root
    image_path: String, // selected .hda (for Extract catalog)
    dataset: String, // working dataset / extracted catalog (JSONL)
    overrides: String,
    rows: Vec<Row>,
    status: String,
    // ---- build image config (mirrors atrium image's Config) ----
    base_system: String,
    base_os: String,           // template key ("" = use base_system .hda directly)
    templates: Vec<String>,    // OS keys from the registry (combo box)
    disk_size_mb: String,      // target image size in MB ("" = base size)
    sel_mode: u8,              // 0 harvest-list, 1 All, 2 Manual list, 3 By category
    sel_text: String,          // ids (list) or categories, comma/space/newline separated
    launcher: String,
    out_image: String,
    startup_items: String,
    startup_sound: String, // optional WAV chime baked into the image
    shutdown_sound: String,
    platform: String,
    detect_color: bool,
    download_art: bool,
    art_dir: String,
    max_art_size: String, // "WxH" (e.g. 1280x854); empty = the 720px default
    // Mac Plus / SE target: 1-bit art only — skips every colour PICT (box art,
    // screenshots, icl8 icon), shrinking the image and the launcher's RAM use.
    bw_only: bool,
    // Launcher memory partition (SIZE -1) in KB, pref/min; empty leaves the
    // binary's built-in 2 MB / 1 MB (a B&W-only build auto-applies the compact
    // default when these are blank). See App::app_mem_kb.
    app_mem_pref: String,
    app_mem_min: String,
    // art-depth variants to bake; default 1/8/24
    d1: bool,
    d4: bool,
    d8: bool,
    d16: bool,
    d24: bool,
    harvest: Vec<HarvestUi>,
    // advanced (sensible defaults; rarely changed)
    apps_root: String,
    metadata_dir: String,
    images_dir: String,
    stage: String,
    curl: String,
    // a long op (extract / enrich / mg / fetch / build) on a worker thread, if any
    job: Option<std::sync::mpsc::Receiver<Done>>,
    busy: String, // label of the running job ("" = idle)
}

/// Result of a background job, applied on the UI thread when it arrives.
struct Done {
    status: String,
    dataset: Option<String>, // if set, switch the working dataset to this path
    reload: bool,            // re-read the dataset table after
}

impl Default for App {
    fn default() -> Self {
        Self {
            tab: Tab::Library,
            rb_cli: "rb-cli".into(),
            metadata: String::new(),
            // The MG data store: $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive.
            mg_archive: mg::default_archive().display().to_string(),
            image_path: String::new(),
            dataset: "data/library.jsonl".into(),
            overrides: "data/overrides.jsonl".into(),
            rows: Vec::new(),
            status: "Step 1: open a dataset, or pick an .hda and extract its catalog.".into(),
            base_system: String::new(),
            base_os: String::new(),
            templates: templates::Registry::load_default().keys(),
            disk_size_mb: String::new(),
            sel_mode: 0,
            sel_text: String::new(),
            launcher: "build/MacAtrium.bin".into(),
            out_image: "/tmp/macatrium.hda".into(),
            startup_items: "/System Folder/Startup Items".into(),
            startup_sound: String::new(),
            shutdown_sound: String::new(),
            platform: "Apple Mac OS".into(),
            detect_color: false,
            download_art: false,
            art_dir: String::new(),
            max_art_size: String::new(), // empty => atrium's 720px default
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

fn as_bool(m: &Map<String, Value>, k: &str, default: bool) -> bool {
    m.get(k).and_then(Value::as_bool).unwrap_or(default)
}

fn load_rows(path: &str) -> anyhow::Result<Vec<Row>> {
    let text = std::fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let m: Map<String, Value> = serde_json::from_str(t)?;
        let genre = m
            .get("genre")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        rows.push(Row {
            id: m.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            year: m.get("year").and_then(Value::as_i64).map(|y| y.to_string()).unwrap_or_default(),
            vendor: m.get("vendor").and_then(Value::as_str).unwrap_or("").to_string(),
            genre,
            color: as_bool(&m, "color", false),
            mouse: as_bool(&m, "mouse", true),
            hotkey: m.get("hotkey").and_then(Value::as_str).unwrap_or("").to_string(),
            dirty: false,
        });
    }
    Ok(rows)
}

impl App {
    fn extract_catalog(&mut self, ctx: &egui::Context) {
        if self.image_path.is_empty() {
            self.status = "Pick an .hda first.".into();
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
    /// thread wakes the UI (`request_repaint`). Keeps the window responsive
    /// during long ops (enrich streams ~500 MB; a full build harvests + downloads).
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
                self.reload();
            } else {
                self.status = done.status;
            }
        }
    }

    fn reload(&mut self) {
        match load_rows(&self.dataset) {
            Ok(r) => {
                self.status = format!("Loaded {} item(s) from {}", r.len(), self.dataset);
                self.rows = r;
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    fn save_overrides(&mut self) {
        let mut n = 0;
        for row in self.rows.iter_mut().filter(|r| r.dirty) {
            let mut f = Map::new();
            f.insert("color".into(), Value::Bool(row.color));
            f.insert("mouse".into(), Value::Bool(row.mouse));
            // Hotkey: only the first character; written when set (gamepad map).
            if let Some(c) = row.hotkey.trim().chars().next() {
                f.insert("hotkey".into(), Value::String(c.to_string()));
            }
            if let Err(e) = merge::set(PathBuf::from(&self.overrides).as_path(), &row.id, &f) {
                self.status = format!("Save failed for {}: {e}", row.id);
                return;
            }
            row.dirty = false;
            n += 1;
        }
        self.status = if n == 0 {
            "Nothing changed.".into()
        } else {
            format!("Saved {n} override(s) to {}", self.overrides)
        };
    }

    fn run_enrich(&mut self, ctx: &egui::Context) {
        if self.metadata.is_empty() {
            self.status = "Set the LaunchBox Metadata.xml path first.".into();
            return;
        }
        let dataset = self.dataset.clone();
        let metadata = self.metadata.clone();
        let platform = self.platform.clone();
        let detect = self.detect_color;
        let curl = self.curl.clone();
        self.spawn_job(ctx, "Enriching from LaunchBox", move || {
            let p = PathBuf::from(&dataset);
            match enrich::run(&p, PathBuf::from(&metadata).as_path(), &p, &platform, false, None, detect, &curl) {
                Ok(()) => Done { status: String::new(), dataset: None, reload: true },
                Err(e) => Done { status: format!("Enrich failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// Enrich from the local Macintosh Garden archive (68K-only; fills gaps + the
    /// `source` attribution; colour detected offline from a scraped screenshot).
    fn run_mg(&mut self, ctx: &egui::Context) {
        if self.mg_archive.trim().is_empty() {
            self.status = "Set the Macintosh Garden archive path first.".into();
            return;
        }
        let dataset = self.dataset.clone();
        let archive = self.mg_archive.clone();
        self.spawn_job(ctx, "Enriching from Macintosh Garden", move || {
            let p = PathBuf::from(&dataset);
            match mg::run(&p, PathBuf::from(&archive).as_path(), &p, false, None) {
                Ok(()) => Done { status: String::new(), dataset: None, reload: true },
                Err(e) => Done { status: format!("MG enrich failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// Fetch each dataset title's software from the Macintosh Garden mirror,
    /// extract it with rb-cli, inject into the output `.hda`, and append a stub so
    /// it shows in the catalog. Needs the output image to exist (build first).
    fn run_fetch(&mut self, ctx: &egui::Context) {
        if self.mg_archive.trim().is_empty() {
            self.status = "Set the Macintosh Garden archive path first.".into();
            return;
        }
        if self.out_image.trim().is_empty() {
            self.status = "Set the output .hda to inject into first (build it on the Build step).".into();
            return;
        }
        let archive = self.mg_archive.clone();
        let dataset = self.dataset.clone();
        let out = self.out_image.clone();
        let apps_root = self.apps_root.clone();
        let rb = self.rb_cli.clone();
        let curl = self.curl.clone();
        self.spawn_job(ctx, "Fetching software from Macintosh Garden", move || {
            let ds = PathBuf::from(&dataset);
            match fetch::run(
                PathBuf::from(&archive).as_path(),
                &[],
                Some(ds.as_path()),
                None,
                Some(PathBuf::from(&out).as_path()),
                &apps_root,
                Some(ds.as_path()),
                &rb,
                &curl,
                None,
            ) {
                Ok(()) => Done { status: format!("Fetched MG software into {out}"), dataset: None, reload: true },
                Err(e) => Done { status: format!("MG fetch failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// The checked art-depth variants, ascending (e.g. ["1","8","24"]).
    fn art_depths(&self) -> Vec<String> {
        // Mac Plus / SE: 1-bit only, regardless of the depth checkboxes.
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

    /// The launcher memory partition `[preferred_kb, minimum_kb]` to bake into the
    /// `'SIZE'` (-1) resource, or `None` to keep the binary's built-in 2 MB / 1 MB.
    /// An explicit pref wins (min defaults to pref); otherwise a B&W-only (Mac
    /// Plus/SE) build gets the small compact default so 2 MB doesn't starve a 4 MB
    /// machine.
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

        // base OS: a template key wins; otherwise the explicit .hda.
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
            launcher: PathBuf::from(self.launcher.trim()),
            dataset: PathBuf::from(self.dataset.trim()),
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
        self.launcher = c.launcher.display().to_string();
        self.dataset = c.dataset.display().to_string();
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
        // selection mode/text
        match &c.selection {
            Some(Selection::All) => { self.sel_mode = 1; self.sel_text.clear(); }
            Some(Selection::List { ids }) => { self.sel_mode = 2; self.sel_text = ids.join(", "); }
            Some(Selection::Categories { categories }) => { self.sel_mode = 3; self.sel_text = categories.join(", "); }
            None => { self.sel_mode = 0; self.sel_text.clear(); }
        }
        // art depths -> checkboxes / B&W-only (["1"] alone is the Mac Plus/SE mode)
        self.bw_only = c.art_depths == ["1"];
        let has = |d: &str| c.art_depths.iter().any(|x| x == d);
        self.d1 = has("1"); self.d4 = has("4"); self.d8 = has("8");
        self.d16 = has("16"); self.d24 = has("24");
        self.max_art_size = c.max_art_size.clone().unwrap_or_default();
        // launcher RAM (SIZE) pref/min
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
            Ok(cfg) => { self.apply_config(cfg); self.status = format!("Loaded build config {}", path.display()); }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    fn build_image(&mut self, ctx: &egui::Context) {
        if self.out_image.trim().is_empty()
            || (self.base_os.trim().is_empty() && self.base_system.trim().is_empty())
        {
            self.status = "Set an output path + a base OS (template or custom .hda).".into();
            return;
        }
        let depths = self.art_depths();
        if depths.is_empty() {
            self.status = "Select at least one art depth (1/4/8/16/24) under Advanced.".into();
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

    // ---- the three workflow steps -------------------------------------------

    fn tab_library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Pick a built .hda and extract its catalog, or open a dataset directly. Edit the Color/B&W + Mouse facets (and an optional launch hotkey) below, then Save.")
                .small().weak(),
        );
        ui.add_space(4.0);
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
        ui.horizontal(|ui| {
            ui.label("Dataset:");
            ui.add(egui::TextEdit::singleline(&mut self.dataset).desired_width(360.0));
            if ui.add_enabled(!busy, egui::Button::new("Open")).clicked() {
                self.reload();
            }
        });

        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("rows")
            .auto_shrink([false, false])
            .max_height(380.0)
            .show(ui, |ui| {
                egui::Grid::new("catalog")
                    .striped(true)
                    .num_columns(7)
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Year");
                        ui.strong("Vendor");
                        ui.strong("Genre");
                        ui.strong("Color");
                        ui.strong("Mouse");
                        ui.strong("Hotkey");
                        ui.end_row();
                        for row in &mut self.rows {
                            ui.label(&row.name);
                            ui.label(&row.year);
                            ui.label(&row.vendor);
                            ui.label(&row.genre);
                            let clabel = if row.color { "Color" } else { "B&W" };
                            let c = ui.checkbox(&mut row.color, clabel);
                            let mlabel = if row.mouse { "Required" } else { "No mouse" };
                            let m = ui.checkbox(&mut row.mouse, mlabel);
                            let h = ui.add(
                                egui::TextEdit::singleline(&mut row.hotkey)
                                    .char_limit(1)
                                    .desired_width(24.0)
                                    .hint_text("key"),
                            );
                            if c.changed() || m.changed() || h.changed() {
                                row.dirty = true;
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("overrides:");
            ui.add(egui::TextEdit::singleline(&mut self.overrides).desired_width(300.0));
            if ui.add_enabled(!busy, egui::Button::new("Save overrides")).clicked() {
                self.save_overrides();
            }
        });
    }

    fn tab_enrich(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Fill year / vendor / genre / description from public databases. Curated values are kept (gaps-only). Run on the open dataset.")
                .small().weak(),
        );
        ui.add_space(6.0);

        ui.group(|ui| {
            ui.strong("LaunchBox Games Database");
            path_row(ui, "Metadata.xml:", &mut self.metadata, Pick::File);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.detect_color, "auto-detect Color / B&W (downloads screenshots)");
            });
            if ui.add_enabled(!busy, egui::Button::new("Enrich from LaunchBox")).clicked() {
                self.run_enrich(ctx);
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Macintosh Garden  (68K-only)");
            ui.label(
                egui::RichText::new("Local scrape archive (metadata/*.ndjson + per-title images). Fills metadata + adds the \"Macintosh Garden\" attribution; colour is detected offline from a scraped screenshot.")
                    .small().weak(),
            );
            path_row(ui, "MG archive:", &mut self.mg_archive, Pick::Folder);
            if ui.add_enabled(!busy, egui::Button::new("Enrich from Macintosh Garden")).clicked() {
                self.run_mg(ctx);
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.strong("Fetch software from Macintosh Garden");
            ui.label(
                egui::RichText::new("Downloads each matched title's software from the MG mirror, extracts it (rb-cli), injects it into the OUTPUT .hda under Apps/, and appends a catalog stub. Build the image on the Build step first, then fetch into it.")
                    .small().weak(),
            );
            ui.horizontal(|ui| {
                ui.label("into output:");
                ui.monospace(&self.out_image);
            });
            if ui.add_enabled(!busy, egui::Button::new("Fetch software from Macintosh Garden")).clicked() {
                self.run_fetch(ctx);
            }
        });
    }

    fn tab_build(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        ui.label(
            egui::RichText::new("Assemble a bootable .hda. Fill the three essentials and Build — everything else has sensible defaults under Advanced.")
                .small().weak(),
        );
        ui.add_space(4.0);
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
            ui.separator();
            ui.label("disk size MB:");
            ui.add(egui::TextEdit::singleline(&mut self.disk_size_mb).desired_width(64.0));
            ui.label(egui::RichText::new("≤2048; blank = base size").small().weak());
        });
        if self.base_os.trim().is_empty() {
            path_row(ui, "base system .hda:", &mut self.base_system, Pick::File);
        }
        path_row(ui, "launcher (.bin):", &mut self.launcher, Pick::File);
        path_row(ui, "output .hda:", &mut self.out_image, Pick::Save);

        ui.add_space(4.0);
        ui.group(|ui| {
            ui.strong("Apps to include");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.sel_mode, 0u8, "Harvest list (Advanced)");
                ui.radio_value(&mut self.sel_mode, 1u8, "All");
                ui.radio_value(&mut self.sel_mode, 2u8, "Manual list");
                ui.radio_value(&mut self.sel_mode, 3u8, "By category");
            });
            match self.sel_mode {
                2 => {
                    ui.label("dataset ids (comma / space / newline separated):");
                    ui.add(egui::TextEdit::multiline(&mut self.sel_text).desired_rows(2).desired_width(440.0));
                }
                3 => {
                    ui.label("categories (comma separated):");
                    ui.add(egui::TextEdit::singleline(&mut self.sel_text).desired_width(440.0));
                }
                _ => {}
            }
        });

        ui.add_space(4.0);
        ui.group(|ui| {
            ui.strong("Content sources (optional)");
            path_row(ui, "Macintosh Garden archive:", &mut self.mg_archive, Pick::Folder);
            path_row(ui, "LaunchBox Metadata.xml:", &mut self.metadata, Pick::File);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.download_art, "download box art (LaunchBox)");
                ui.checkbox(&mut self.detect_color, "auto-detect Color / B&W");
            });
            ui.label(
                egui::RichText::new("With an MG archive set, the build enriches + bakes MG art (preferred), with LaunchBox as fallback.")
                    .small().weak(),
            );
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Build image").strong())).clicked() {
                self.build_image(ctx);
            }
            ui.separator();
            if ui.button("Save config…").on_hover_text(
                "Write these settings to a builds/*.json the `atrium image --config` CLI can run."
            ).clicked() {
                self.save_config();
            }
            if ui.button("Load config…").on_hover_text(
                "Open a builds/*.json into the form to review or tweak, then Build."
            ).clicked() {
                self.load_config();
            }
        });

        ui.add_space(6.0);
        ui.collapsing("Advanced", |ui| {
            path_row(ui, "dataset:", &mut self.dataset, Pick::File);
            path_row(ui, "overrides:", &mut self.overrides, Pick::File);
            ui.horizontal(|ui| {
                ui.label("platform:");
                ui.add(egui::TextEdit::singleline(&mut self.platform).desired_width(160.0));
                ui.label("startup items:");
                ui.add(egui::TextEdit::singleline(&mut self.startup_items).desired_width(220.0));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.bw_only, "Mac Plus / SE (B&W only)")
                    .on_hover_text(
                        "1-bit artwork only — skips every colour PICT (box art, \
                         screenshots, icl8 icons). Much smaller image; the only art \
                         a compact Mac without Color QuickDraw can use.",
                    );
                ui.separator();
                ui.label("max art size:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.max_art_size)
                        .hint_text("720x768")
                        .desired_width(80.0),
                )
                .on_hover_text("Downscale art to fit WxH px (aspect kept). Empty = 720px default.");
            });
            ui.horizontal(|ui| {
                ui.label("launcher RAM KB:");
                ui.add(egui::TextEdit::singleline(&mut self.app_mem_pref)
                    .hint_text("pref").desired_width(56.0));
                ui.add(egui::TextEdit::singleline(&mut self.app_mem_min)
                    .hint_text("min").desired_width(56.0));
                // Presets fill pref/min from the measured per-target values.
                let (cp, cm) = atrium::config::COLOR_APP_MEM_KB;
                let (bp, bm) = atrium::config::COMPACT_APP_MEM_KB;
                if ui.small_button("Colour").on_hover_text(
                    format!("{cp}/{cm} KB — measured 7.x colour peak ~472 KB (GWorld in temp mem)")
                ).clicked() {
                    self.app_mem_pref = cp.to_string(); self.app_mem_min = cm.to_string();
                }
                if ui.small_button("Compact B&W").on_hover_text(
                    format!("{bp}/{bm} KB — Mac Plus/SE 6.0.8 1-bit (no GWorld)")
                ).clicked() {
                    self.app_mem_pref = bp.to_string(); self.app_mem_min = bm.to_string();
                }
                if ui.small_button("Default").on_hover_text(
                    "Clear — keep the launcher binary's built-in 2 MB / 1 MB"
                ).clicked() {
                    self.app_mem_pref.clear(); self.app_mem_min.clear();
                }
            });
            ui.label(
                egui::RichText::new("launcher RAM = the SIZE (-1) partition; blank = 2MB/1MB, or B&W-only auto-applies Compact")
                    .small().weak(),
            );
            // Per-depth variants — overridden (and greyed) when B&W-only is set.
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
            path_row(ui, "local art dir:", &mut self.art_dir, Pick::Folder);
            path_row(ui, "startup sound (WAV):", &mut self.startup_sound, Pick::File);
            path_row(ui, "shutdown sound (WAV):", &mut self.shutdown_sound, Pick::File);

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
                            if ui.button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    self.harvest.remove(i);
                }
                if ui.button("Add harvest source").clicked() {
                    self.harvest.push(HarvestUi::default());
                }
            });

            ui.collapsing("Paths & tools", |ui| {
                ui.horizontal(|ui| { ui.label("rb-cli:"); ui.text_edit_singleline(&mut self.rb_cli); });
                ui.horizontal(|ui| { ui.label("curl:"); ui.text_edit_singleline(&mut self.curl); });
                ui.horizontal(|ui| { ui.label("apps root:"); ui.text_edit_singleline(&mut self.apps_root); });
                ui.horizontal(|ui| { ui.label("metadata dir:"); ui.text_edit_singleline(&mut self.metadata_dir); });
                ui.horizontal(|ui| { ui.label("images dir:"); ui.text_edit_singleline(&mut self.images_dir); });
                path_row(ui, "stage dir:", &mut self.stage, Pick::Folder);
            });
        });
    }
}

impl eframe::App for App {
    // eframe 0.34 hands us a root Ui (no panels). The body is a three-step flow:
    // a tab bar, the active step's content, then a persistent status/progress bar.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job();                       // apply a finished background job
        let busy = !self.busy.is_empty();      // a long op is running -> disable actions
        let ctx = ui.ctx().clone();

        ui.horizontal(|ui| {
            ui.heading("MacAtrium Manager");
            ui.separator();
            ui.selectable_value(&mut self.tab, Tab::Library, "1 · Library");
            ui.selectable_value(&mut self.tab, Tab::Enrich, "2 · Enrich");
            ui.selectable_value(&mut self.tab, Tab::Build, "3 · Build");
        });
        ui.separator();

        match self.tab {
            Tab::Library => self.tab_library(ui, &ctx, busy),
            Tab::Enrich => self.tab_enrich(ui, &ctx, busy),
            Tab::Build => self.tab_build(ui, &ctx, busy),
        }

        // Persistent status / progress line at the bottom of every step.
        ui.separator();
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&self.status);
            });
        } else {
            ui.label(&self.status);
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
}
