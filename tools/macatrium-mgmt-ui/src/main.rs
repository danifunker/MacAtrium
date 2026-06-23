//! MacAtrium Management UI — an egui front-end for the MacAtrium build tooling.
//!
//! Every action here calls the `atrium` **library** — the exact functions the
//! CLI exposes — so the CLI stays the source of truth and this is just a nicer
//! way to drive it. Workflow: pick an `.hda`, extract its catalog with rb-cli,
//! toggle the **Color/B&W** and **Mouse** facets LaunchBox can't provide (plus
//! fix metadata), enrich from LaunchBox, save your edits to `overrides.jsonl`,
//! and build a bootable image. The **Build image** panel exposes the full
//! `atrium image` config — every option the CLI's JSON config takes, including
//! the art-depth variants to bake (default 1/8/24; pick one for a single depth).
//!
//! GUI-specific concern only: it renders the table and shells the same atrium
//! calls. Long operations run inline for now (a brief freeze) — threading them
//! is a follow-up.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use atrium::{enrich, image, merge, rbcli::RbCli};
use eframe::egui;
use serde_json::{Map, Value};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MacAtrium Management UI",
        opts,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
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
    // shared paths / dataset editing
    rb_cli: String,
    metadata: String, // LaunchBox Metadata.xml
    image_path: String, // selected .hda (for Extract catalog)
    dataset: String, // working dataset / extracted catalog (JSONL)
    overrides: String,
    rows: Vec<Row>,
    status: String,
    // ---- build image config (mirrors atrium image's Config) ----
    base_system: String,
    launcher: String,
    out_image: String,
    startup_items: String,
    startup_sound: String, // optional WAV chime baked into the image
    shutdown_sound: String,
    platform: String,
    detect_color: bool,
    download_art: bool,
    art_dir: String,
    art_max: String,
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
    // a long op (extract / enrich / build) running on a worker thread, if any
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
            rb_cli: "rb-cli".into(),
            metadata: String::new(),
            image_path: String::new(),
            dataset: "data/library.jsonl".into(),
            overrides: "data/overrides.jsonl".into(),
            rows: Vec::new(),
            status: "Open a dataset, or pick an .hda and Extract its catalog.".into(),
            base_system: String::new(),
            launcher: "build/MacAtrium.bin".into(),
            out_image: "/tmp/macatrium.hda".into(),
            startup_items: "/System Folder/Startup Items".into(),
            startup_sound: String::new(),
            shutdown_sound: String::new(),
            platform: "Apple Mac OS".into(),
            detect_color: false,
            download_art: false,
            art_dir: String::new(),
            art_max: "256".into(),
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
        ui.add(egui::TextEdit::singleline(value).desired_width(380.0));
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

fn put(m: &mut Map<String, Value>, k: &str, v: &str) {
    m.insert(k.to_string(), Value::String(v.to_string()));
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

    /// The checked art-depth variants, ascending (e.g. ["1","8","24"]).
    fn art_depths(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.d1 { v.push("1".to_string()); }
        if self.d4 { v.push("4".to_string()); }
        if self.d8 { v.push("8".to_string()); }
        if self.d16 { v.push("16".to_string()); }
        if self.d24 { v.push("24".to_string()); }
        v
    }

    fn build_image(&mut self, ctx: &egui::Context) {
        if self.base_system.trim().is_empty() || self.out_image.trim().is_empty() {
            self.status = "Set base system + output paths first.".into();
            return;
        }
        let depths = self.art_depths();
        if depths.is_empty() {
            self.status = "Select at least one art depth (1/4/8/16/24).".into();
            return;
        }

        // Assemble the same JSON config the CLI's `atrium image --config` takes.
        // Optional fields are only emitted when set, so defaults apply otherwise.
        let mut cfg = Map::new();
        put(&mut cfg, "system", &self.base_system);
        put(&mut cfg, "out", &self.out_image);
        put(&mut cfg, "launcher", &self.launcher);
        put(&mut cfg, "dataset", &self.dataset);
        if !self.overrides.trim().is_empty() { put(&mut cfg, "overrides", &self.overrides); }
        if !self.metadata.trim().is_empty() { put(&mut cfg, "metadata", &self.metadata); }
        put(&mut cfg, "platform", &self.platform);
        cfg.insert("detect_color".into(), Value::Bool(self.detect_color));
        cfg.insert("download_art".into(), Value::Bool(self.download_art));
        if !self.art_dir.trim().is_empty() { put(&mut cfg, "art_dir", &self.art_dir); }
        cfg.insert(
            "art_depths".into(),
            Value::Array(depths.iter().map(|d| Value::String(d.clone())).collect()),
        );
        if let Ok(m) = self.art_max.trim().parse::<u64>() {
            cfg.insert("art_max".into(), Value::from(m));
        }
        put(&mut cfg, "startup_items", &self.startup_items);
        if !self.startup_sound.trim().is_empty() { put(&mut cfg, "startup_sound", &self.startup_sound); }
        if !self.shutdown_sound.trim().is_empty() { put(&mut cfg, "shutdown_sound", &self.shutdown_sound); }
        put(&mut cfg, "rb_cli", &self.rb_cli);
        put(&mut cfg, "apps_root", &self.apps_root);
        put(&mut cfg, "metadata_dir", &self.metadata_dir);
        put(&mut cfg, "images_dir", &self.images_dir);
        if !self.stage.trim().is_empty() { put(&mut cfg, "stage", &self.stage); }
        put(&mut cfg, "curl", &self.curl);

        let harvest: Vec<Value> = self
            .harvest
            .iter()
            .filter(|h| !h.image.trim().is_empty())
            .map(|h| {
                let mut o = Map::new();
                o.insert("image".into(), Value::String(h.image.trim().to_string()));
                let apps: Vec<Value> = h
                    .apps
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                if !apps.is_empty() {
                    o.insert("apps".into(), Value::Array(apps));
                }
                if !h.scan.trim().is_empty() {
                    o.insert("scan".into(), Value::String(h.scan.trim().to_string()));
                }
                Value::Object(o)
            })
            .collect();
        if !harvest.is_empty() {
            cfg.insert("harvest".into(), Value::Array(harvest));
        }

        let cfg_path = std::env::temp_dir().join("macatrium-mgmt-build.json");
        if let Err(e) = std::fs::write(&cfg_path, serde_json::to_string_pretty(&Value::Object(cfg)).unwrap()) {
            self.status = format!("Config write failed: {e}");
            return;
        }
        let out = self.out_image.clone();
        let label = format!("Building image ({})", depths.join("/"));
        self.spawn_job(ctx, &label, move || match image::run(&cfg_path) {
            Ok(()) => Done { status: format!("Built image -> {out}"), dataset: None, reload: false },
            Err(e) => Done { status: format!("Build failed: {e}"), dataset: None, reload: false },
        });
    }
}

impl eframe::App for App {
    // eframe 0.34 hands us a root Ui (no panels). Layout is intentionally flat —
    // top-level sibling widgets/closures — so each closure borrows only the
    // fields it touches (no nested whole-`self` captures to fight the borrowck).
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job();                       // apply a finished background job
        let busy = !self.busy.is_empty();      // a long op is running -> disable actions
        let ctx = ui.ctx().clone();

        ui.heading("MacAtrium Management UI");
        ui.horizontal(|ui| {
            ui.label("rb-cli:");
            ui.text_edit_singleline(&mut self.rb_cli);
            ui.label("LaunchBox Metadata.xml:");
            ui.text_edit_singleline(&mut self.metadata);
        });
        ui.horizontal(|ui| {
            if ui.button("Pick .hda…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("disk image", &["hda", "img", "dsk", "vhd"])
                    .pick_file()
                {
                    self.image_path = p.to_string_lossy().into_owned();
                }
            }
            ui.monospace(&self.image_path);
            if ui.add_enabled(!busy, egui::Button::new("Extract catalog")).clicked() {
                self.extract_catalog(&ctx);
            }
            ui.separator();
            ui.label("Dataset:");
            ui.text_edit_singleline(&mut self.dataset);
            if ui.add_enabled(!busy, egui::Button::new("Open")).clicked() {
                self.reload();
            }
        });

        ui.separator();

        // Dataset table (its own scroll; kept compact to leave room for Build).
        egui::ScrollArea::vertical()
            .id_salt("rows")
            .max_height(200.0)
            .auto_shrink([false, false])
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
                            // A 1-char launch hotkey (gamepad button map); we keep
                            // only the first char on save.
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
            ui.text_edit_singleline(&mut self.overrides);
            if ui.add_enabled(!busy, egui::Button::new("Save overrides")).clicked() {
                self.save_overrides();
            }
            if ui.add_enabled(!busy, egui::Button::new("Enrich (LaunchBox)")).clicked() {
                self.run_enrich(&ctx);
            }
        });

        ui.separator();
        ui.heading("Build image");
        path_row(ui, "base system:", &mut self.base_system, Pick::File);
        path_row(ui, "launcher:", &mut self.launcher, Pick::File);
        path_row(ui, "output .hda:", &mut self.out_image, Pick::Save);
        path_row(ui, "dataset:", &mut self.dataset, Pick::File);
        path_row(ui, "overrides:", &mut self.overrides, Pick::File);
        path_row(ui, "metadata (LaunchBox):", &mut self.metadata, Pick::File);
        ui.horizontal(|ui| {
            ui.label("platform:");
            ui.add(egui::TextEdit::singleline(&mut self.platform).desired_width(160.0));
            ui.label("startup items:");
            ui.add(egui::TextEdit::singleline(&mut self.startup_items).desired_width(240.0));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.download_art, "download box art (LaunchBox)");
            ui.checkbox(&mut self.detect_color, "auto-detect Color / B&W");
        });
        path_row(ui, "startup sound (WAV):", &mut self.startup_sound, Pick::File);
        path_row(ui, "shutdown sound (WAV):", &mut self.shutdown_sound, Pick::File);
        ui.label(
            egui::RichText::new(
                "Optional PCM WAV chimes; leave blank for none. The launcher's \
                 Settings turns each on/off (default off). Clips are capped at 7 \
                 seconds — longer files are truncated.",
            )
            .small()
            .weak(),
        );
        path_row(ui, "local art dir:", &mut self.art_dir, Pick::Folder);
        ui.horizontal(|ui| {
            ui.label("art depths:");
            ui.checkbox(&mut self.d1, "1");
            ui.checkbox(&mut self.d4, "4");
            ui.checkbox(&mut self.d8, "8");
            ui.checkbox(&mut self.d16, "16");
            ui.checkbox(&mut self.d24, "24");
            ui.separator();
            ui.label("max px:");
            ui.add(egui::TextEdit::singleline(&mut self.art_max).desired_width(56.0));
        });
        ui.label(
            egui::RichText::new(
                "Default 1/8/24 = dithered B&W + 256-colour + Millions; a deeper variant \
                 down-converts to shallower screens. Tick a single box for one depth only.",
            )
            .small()
            .weak(),
        );

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

        ui.collapsing("Advanced", |ui| {
            ui.horizontal(|ui| {
                ui.label("apps root:");
                ui.text_edit_singleline(&mut self.apps_root);
            });
            ui.horizontal(|ui| {
                ui.label("metadata dir:");
                ui.text_edit_singleline(&mut self.metadata_dir);
            });
            ui.horizontal(|ui| {
                ui.label("images dir:");
                ui.text_edit_singleline(&mut self.images_dir);
            });
            path_row(ui, "stage dir:", &mut self.stage, Pick::Folder);
            ui.horizontal(|ui| {
                ui.label("curl:");
                ui.text_edit_singleline(&mut self.curl);
            });
        });

        if ui.add_enabled(!busy, egui::Button::new("Build image")).clicked() {
            self.build_image(&ctx);
        }

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
