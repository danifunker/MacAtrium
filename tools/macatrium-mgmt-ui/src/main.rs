//! MacAtrium Management UI — an egui front-end for the MacAtrium build tooling.
//!
//! Every action here calls the `atrium` **library** — the exact functions the
//! CLI exposes — so the CLI stays the source of truth and this is just a nicer
//! way to drive it. Workflow: pick an `.hda`, extract its catalog with rb-cli,
//! toggle the **Color/B&W** and **Mouse** facets LaunchBox can't provide (plus
//! fix metadata), enrich from LaunchBox, save your edits to `overrides.jsonl`,
//! and build a bootable image.
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
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 640.0]),
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
    dirty: bool, // touched since last save
}

struct App {
    rb_cli: String,
    metadata: String, // LaunchBox Metadata.xml
    image_path: String, // selected .hda
    dataset: String, // working dataset / extracted catalog (JSONL)
    overrides: String,
    rows: Vec<Row>,
    status: String,
    // build fields
    base_system: String,
    launcher: String,
    out_image: String,
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
        }
    }
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
            dirty: false,
        });
    }
    Ok(rows)
}

impl App {
    fn extract_catalog(&mut self) {
        if self.image_path.is_empty() {
            self.status = "Pick an .hda first.".into();
            return;
        }
        let rb = RbCli::new(&self.rb_cli);
        let tmp = std::env::temp_dir().join("macatrium-mgmt-catalog.jsonl");
        let _ = std::fs::remove_file(&tmp);
        match rb.get(
            PathBuf::from(&self.image_path).as_path(),
            "/MacAtrium/metadata/catalog.jsonl",
            &tmp,
            true,
        ) {
            Ok(()) => {
                self.dataset = tmp.to_string_lossy().into_owned();
                self.reload();
            }
            Err(e) => self.status = format!("Extract failed: {e}"),
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

    fn run_enrich(&mut self) {
        if self.metadata.is_empty() {
            self.status = "Set the LaunchBox Metadata.xml path first.".into();
            return;
        }
        let p = PathBuf::from(&self.dataset);
        match enrich::run(&p, PathBuf::from(&self.metadata).as_path(), &p, "Apple Mac OS", false, None, false, "curl") {
            Ok(()) => {
                self.reload();
            }
            Err(e) => self.status = format!("Enrich failed: {e}"),
        }
    }

    fn build_image(&mut self) {
        if self.base_system.is_empty() || self.out_image.is_empty() {
            self.status = "Set base system + output paths first.".into();
            return;
        }
        // Assemble a config and call the same orchestrator the CLI uses.
        let cfg = serde_json::json!({
            "system": self.base_system,
            "out": self.out_image,
            "launcher": self.launcher,
            "dataset": self.dataset,
            "overrides": self.overrides,
            "rb_cli": self.rb_cli,
        });
        let cfg_path = std::env::temp_dir().join("macatrium-mgmt-build.json");
        if let Err(e) = std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()) {
            self.status = format!("Config write failed: {e}");
            return;
        }
        match image::run(&cfg_path) {
            Ok(()) => self.status = format!("Built image -> {}", self.out_image),
            Err(e) => self.status = format!("Build failed: {e}"),
        }
    }
}

impl eframe::App for App {
    // eframe 0.34 hands us a root Ui (no panels); we lay out controls + table in it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
            if ui.button("Extract catalog").clicked() {
                self.extract_catalog();
            }
            ui.separator();
            ui.label("Dataset:");
            ui.text_edit_singleline(&mut self.dataset);
            if ui.button("Open").clicked() {
                self.reload();
            }
        });

        ui.separator();

        // Table — reserve room below for the action bar + status.
        let table_h = (ui.available_height() - 120.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(table_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("catalog")
                    .striped(true)
                    .num_columns(6)
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Year");
                        ui.strong("Vendor");
                        ui.strong("Genre");
                        ui.strong("Color");
                        ui.strong("Mouse");
                        ui.end_row();
                        for row in &mut self.rows {
                            ui.label(&row.name);
                            ui.label(&row.year);
                            ui.label(&row.vendor);
                            ui.label(&row.genre);
                            // labels computed before the &mut borrow
                            let clabel = if row.color { "Color" } else { "B&W" };
                            let c = ui.checkbox(&mut row.color, clabel);
                            let mlabel = if row.mouse { "Required" } else { "No mouse" };
                            let m = ui.checkbox(&mut row.mouse, mlabel);
                            if c.changed() || m.changed() {
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
            if ui.button("Save overrides").clicked() {
                self.save_overrides();
            }
            if ui.button("Enrich (LaunchBox)").clicked() {
                self.run_enrich();
            }
        });
        ui.horizontal(|ui| {
            ui.label("base system:");
            ui.text_edit_singleline(&mut self.base_system);
            ui.label("launcher:");
            ui.text_edit_singleline(&mut self.launcher);
            ui.label("out:");
            ui.text_edit_singleline(&mut self.out_image);
            if ui.button("Build image").clicked() {
                self.build_image();
            }
        });
        ui.separator();
        ui.label(&self.status);
    }
}
