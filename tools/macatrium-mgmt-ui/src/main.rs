//! MacAtrium Management UI — an egui front-end for the MacAtrium build tooling.
//!
//! Every action here goes through `atrium` — the exact code the CLI runs — so the
//! CLI stays the source of truth and this is just a nicer way to drive it. The UI
//! is organised around **jobs** a user actually does, not the pipeline stages of
//! the CLI:
//!
//!   * **Build** — pick a *Target* (a Mac profile), curate the list of titles the
//!     disk gets, write a fresh bootable MacAtrium disk. The saved collection IS
//!     that list: rows are removed with ✖ and flagged Recommended in place, and
//!     "Add titles…" opens the library in a modal. Plumbing lives behind
//!     **Advanced**.
//!   * **Edit disk** — change an already-built disk in place. Its own catalog is
//!     read off the volume as the baseline, removals and additions are staged, and
//!     Apply runs `add` / `remove` / `replace` — the same list model as Build, from
//!     a different starting point.
//!   * **Library** — browse the bundled catalogue and edit each title's
//!     compatibility facets (Colour/B&W, Mouse, launch hotkey).
//!   * **Attain** — acquire the *source software*: register the MacPack folder,
//!     run the Macintosh Garden downloader (gated on a valid MG-Archive).
//!   * **⚙ Settings** — Targets & Templates, Donors, tool paths, MacPack /
//!     MG-Archive / cache locations; persisted to `~/.macatrium.json`. A first-run
//!     wizard auto-detects `rb-cli` and prompts for the source folders.
//!
//! **What goes on a disk is always an explicit list** (`work_ids` when building,
//! `disk_ids` + staged edits when editing) — never a flag on a library row. The
//! library table is a catalogue you pull from, not a selection.
//!
//! Long operations run on a worker thread so the window stays responsive. The ones
//! that write a disk (build / add / remove / replace) run the **`atrium` CLI as a
//! child process**, shipped beside this executable, and stream its output into the
//! Build log: the library reports its progress and every warning with `eprintln!`,
//! and a `windows_subsystem = "windows"` app has no stderr to show — so a build
//! that skipped half its titles used to look exactly like a clean one. Running the
//! CLI makes that output a pipe we can display, and gives Cancel something to kill.
//! If the CLI isn't found the library is called in-process instead (correct, but
//! with no log).

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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    EditDisk,
    Library,
    Database,
    Attain,
    Settings,
}

/// One library row: identity + descriptive metadata plus the compatibility facets
/// a user edits. `raw`-free — we re-read the source on reload, and only facets are
/// written back (via the compatibility overlay), so dropping the other fields here
/// loses nothing. What goes ON a disk is a separate explicit list (`work_ids` /
/// `disk_ids`), not a flag per row.
#[derive(Default)]
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
    src: Src,               // where its files come from (drives the build preflight)
    dirty: bool,            // facet touched since last save
}

/// Which list the shared "Add titles…" modal is filling: the disk being built,
/// or the staged additions for an already-built disk being edited.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AddTo {
    #[default]
    List,
    Disk,
}

/// The Build screen's source preflight: how much of the disk's contents can
/// actually reach the volume, and what's missing if not.
#[derive(Default, Clone)]
struct SourceCheck {
    ok: usize,                              // titles with a usable source
    unsourced: Vec<String>,                 // ids whose donor isn't configured
    unknown: Vec<String>,                   // ids not in the library at all
    missing_donors: std::collections::BTreeSet<String>, // the donor keys to add
    donors_configured: bool,                // any donor at all in the registry
}

impl SourceCheck {
    fn is_clean(&self) -> bool {
        self.unsourced.is_empty() && self.unknown.is_empty()
    }
}

/// Where a title's bits come from — the three source kinds a build understands
/// (`selection::Plan`), as the library table sees them.
#[derive(Default, Clone, PartialEq, Eq)]
enum Src {
    /// `harvest_src.donor`: needs that donor key configured to reach a disk.
    Donor(String),
    /// `local_src`: an imported capture staged on this machine. No donor needed.
    Local,
    /// No source at all — the record can never put files on a disk.
    #[default]
    None,
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
    wizard_returning: bool, // they've completed an OLDER revision of setup
    macpack_dir: String,   // editor mirror of settings.macpack_dir
    cache_dir: String,     // editor mirror of settings.cache_dir
    output_dir: String,    // where built disks are written (settings.output_dir)
    sources_dir: String,   // root of the user's source library (settings.sources_dir)
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
    // ---- Templates editor (base-OS images), persisted to ~/.macatrium.json ----
    // The file registry (data/templates.json) is a *relative* path, so it only
    // resolves from a repo checkout — for an installed app these settings entries
    // are the only way to name a base disk.
    tpl_key: String,   // OS key, e.g. "7.1"
    tpl_hda: String,   // base bootable System image
    tpl_label: String,
    tpl_finder_replace: bool, // launcher AS the Finder (System 6) vs Startup Items
    tpl_startup: String,      // Startup Items folder when not finder_replace
    // ---- Donors editor (donor key -> image), same rationale ----
    dn_key: String,
    dn_path: String,
    dn_reservoir: bool, // verbatim-copy reservoir vs a harvested MacPack donor
    // ---- Import (.mar / .sit / .cpt captures of installed apps) ----
    imp_files: Vec<PathBuf>,     // captures queued for import
    imp_donor: String,           // "" = host staging (no donor image needed)
    imp_collection: String,      // "" = don't add to a collection
    imp_report: Vec<String>,     // per-title result lines from the last run
    // ---- the shared library (browse / pick / edit facets) ----
    library: Vec<LibRow>,
    library_loaded: bool,
    lib_search: String,
    lib_kind: String,  // "" = all kinds
    lib_genre: String, // "" = all genres
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
    // ---- Collections: the available lists (name -> where it lives) ----
    coll_names: Vec<String>,       // every collection name (user ⊕ bundled)
    coll_scanned: bool,            // whether reload_collections has run (an empty
                                   // result is a valid answer — keying the lazy
                                   // scan off `coll_names.is_empty()` re-read the
                                   // settings file + dirs on every repaint)
    coll_index: HashMap<String, (&'static str, PathBuf)>, // name -> (origin, backing file)
    // ---- The working set: the collection being edited IS the disk's contents ----
    // One model, not two. The Build screen edits this list directly (add / remove /
    // recommend) and builds from it; there is no separate "ticked titles" notion to
    // drift out of sync with the saved list. `recommended` only reaches a build via
    // a NAMED collection (image.rs tags `coll.recommended` into the catalog), which
    // is why Build saves the working set before building rather than passing a bare
    // id list — a nameless selection would silently ship an empty Recommended.
    work_name: String,              // collection name ("" = unsaved/untitled)
    work_ids: Vec<String>,          // the titles on the disk, in order
    work_rec: HashSet<String>,      // which of them are Recommended
    work_label: String,             // the collection's own description, preserved
    work_overrides: BTreeMap<String, Value>, // per-title overrides, preserved verbatim
    work_path: Option<PathBuf>,     // backing file (None = never saved)
    work_origin: &'static str,      // "user" / "bundled" / "new"
    work_dirty: bool,               // edited since load/save
    work_search: String,            // filter over the working table
    work_all: bool,                 // true = build every compatible title instead
    src_check: Option<SourceCheck>, // cached build preflight (see ensure_src_check)
    // ---- The "Add titles…" modal: its own ticks and its own filters, so opening
    // it never disturbs the Library tab's browse state or the working set. ----
    add_open: bool,
    add_sel: HashSet<String>, // ticked in the modal, not yet added
    add_search: String,
    add_kind: String,  // "" = all kinds
    add_genre: String, // "" = all genres
    add_to: AddTo,     // which list the modal is filling
    // ---- Edit disk: the contents of an already-built disk, staged edits ----
    // Mirrors the Build screen's model (a list you add to and remove from), but
    // the baseline comes from the disk's own catalog rather than a collection.
    disk_ids: Vec<String>,               // titles currently on the loaded disk
    disk_names: HashMap<String, String>, // id -> name as the disk's catalog has it
    disk_rm: HashSet<String>,            // staged removals
    disk_add: Vec<String>,               // staged additions (library ids)
    disk_loaded: Option<String>,         // the .hda the contents were read from
    #[allow(clippy::type_complexity)]
    disk_rx: Option<std::sync::mpsc::Receiver<Result<Vec<(String, String)>, String>>>,
    // ---- Build log: the CLI's own output, streamed from the child process ----
    log_lines: Vec<String>,
    log_rx: Option<std::sync::mpsc::Receiver<String>>,
    log_open: bool,
    /// The running child, so a long job can be cancelled.
    job_child: Option<Arc<Mutex<Option<std::process::Child>>>>,
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
    // Final bless: which System Folder the built disk BOOTS. "" = the ship default
    // (System Folder 7.1). The choices are read off the base disk, so a 6.0.8 B&W
    // build can actually ship blessed 6.0.8 instead of silently booting 7.1.
    final_bless: String,
    bless_folders: Vec<String>, // System Folders found on `base_system`
    bless_scanned: String,      // the base_system path bless_folders was read from
    disk_size_mb: String,
    sel_mode: u8, // 0 harvest-list, 1 All, 2 Manual list, 3 By category
    sel_text: String,
    launcher: String,
    out_image: String,
    disk_path: String,           // Edit-disk: the existing MacAtrium .hda
    disk_search: String,         // filter over the disk's contents
    disk_sync_name: String,      // saved list to apply the same edit to ("" = none)
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
    strip_quicktime: bool, // remove QuickTime + Apple Photo Access (compact/B&W target)
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
        // Show setup until the user has been through THIS revision of it. Earlier
        // rules were both wrong in the same way — they inferred "has this person
        // been set up?" from the settings themselves. Keying off rb-cli hid the
        // wizard from anyone whose rb-cli had been detected once; keying off "is
        // anything configured" re-opened it on every launch until a template or
        // donor existed, and Skip couldn't be remembered because nothing recorded
        // it. What the wizard needs to know is what the USER has already seen.
        let seen = settings.setup_seen.unwrap_or(0);
        let show_wizard = seen < WIZARD_REV;
        let wizard_returning = seen > 0;
        let rb_cli = settings
            .rb_cli
            .clone()
            .or_else(detect_rb_cli)
            .unwrap_or_else(|| RB_CLI_EXE.to_string());
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
        let output_dir = settings.output_dir().display().to_string();
        let sources_dir = settings.sources_dir().display().to_string();
        let out_image = default_out_image(&settings);
        Self {
            tab: Tab::Build,
            settings,
            show_wizard,
            wizard_returning,
            macpack_dir,
            cache_dir,
            output_dir,
            sources_dir,
            target_reg,
            target_name: String::new(),
            te_name: String::new(),
            te_base_os: String::new(),
            te_depths: String::new(),
            te_mem_pref: String::new(),
            te_mem_min: String::new(),
            te_label: String::new(),
            tpl_key: String::new(),
            tpl_hda: String::new(),
            tpl_label: String::new(),
            tpl_finder_replace: false,
            tpl_startup: "/System Folder/Startup Items".into(),
            dn_key: String::new(),
            dn_path: String::new(),
            dn_reservoir: true, // the common case here: an installed-content reservoir
            imp_files: Vec::new(),
            imp_donor: String::new(),
            imp_collection: String::new(),
            imp_report: Vec::new(),
            library: Vec::new(),
            library_loaded: false,
            lib_search: String::new(),
            lib_kind: String::new(),
            lib_genre: String::new(),
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
            coll_scanned: false,
            coll_index: HashMap::new(),
            work_name: String::new(),
            work_ids: Vec::new(),
            work_rec: HashSet::new(),
            work_label: String::new(),
            work_overrides: BTreeMap::new(),
            work_path: None,
            work_origin: "new",
            work_dirty: false,
            work_search: String::new(),
            work_all: false,
            src_check: None,
            add_open: false,
            add_sel: HashSet::new(),
            add_search: String::new(),
            add_kind: String::new(),
            add_genre: String::new(),
            add_to: AddTo::List,
            disk_ids: Vec::new(),
            disk_names: HashMap::new(),
            disk_rm: HashSet::new(),
            disk_add: Vec::new(),
            disk_loaded: None,
            disk_rx: None,
            log_lines: Vec::new(),
            log_rx: None,
            log_open: false,
            job_child: None,
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
            final_bless: String::new(),
            bless_folders: Vec::new(),
            bless_scanned: String::new(),
            disk_size_mb: String::new(),
            sel_mode: 2, // Pick titles
            sel_text: String::new(),
            launcher: String::new(),
            out_image,
            disk_path: String::new(),
            disk_search: String::new(),
            disk_sync_name: String::new(),
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
            strip_quicktime: false,
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
        path_row_inline(ui, value, kind, None);
    });
}

/// [`path_row`] whose Browse… dialog opens in `start` — used to land the user in
/// the right corner of their source library (Templates, Donors, Captures…)
/// instead of wherever the dialog was last time.
fn path_row_from(ui: &mut egui::Ui, label: &str, value: &mut String, kind: Pick, start: PathBuf) {
    ui.horizontal(|ui| {
        ui.label(label);
        path_row_inline(ui, value, kind, Some(start));
    });
}

/// The "text field · Browse…" half of [`path_row`], for use inside a `Grid` where
/// the label already occupies its own cell. `start` sets the dialog's initial
/// directory (created if missing — an absent one is ignored by the dialog).
fn path_row_inline(ui: &mut egui::Ui, value: &mut String, kind: Pick, start: Option<PathBuf>) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(value).desired_width(360.0));
        if ui.button("Browse…").clicked() {
            let mut dlg = rfd::FileDialog::new();
            // Prefer the value already in the box, else the caller's hint.
            let existing = PathBuf::from(value.trim());
            let seed = if !value.trim().is_empty() && existing.parent().is_some_and(Path::is_dir) {
                existing.parent().map(PathBuf::from)
            } else {
                start
            };
            if let Some(d) = seed {
                let _ = std::fs::create_dir_all(&d);
                dlg = dlg.set_directory(&d);
            }
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

/// The revision of the first-run setup this build ships.
///
/// Persisted per user as `setup_seen` once they finish *or* skip the wizard, and
/// compared against it on every launch: equal or higher means they've already been
/// through this version, so the wizard stays shut. **Bump this only when setup asks
/// for something it didn't before** — every user then gets it once more, with a
/// note saying why. Cosmetic edits don't count; the point is to re-ask when there's
/// genuinely a new answer needed, not to nag.
///
/// rev 1 — output + sources folders, rb-cli, MacPack, MG-Archive.
const WIZARD_REV: u32 = 1;

/// The rb-cli executable name for this platform (`rb-cli.exe` on Windows).
const RB_CLI_EXE: &str = if cfg!(windows) { "rb-cli.exe" } else { "rb-cli" };

/// The user's home directory. `HOME` first (POSIX), then `USERPROFILE` — Windows
/// normally sets only the latter, so probing `HOME` alone silently finds nothing
/// there. Mirrors `atrium::settings::home`.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The default output disk, as a **platform-native** path.
///
/// `/tmp/macatrium.hda` was hard-coded, which on Windows is neither a real
/// location nor recognisable — and a built disk is a deliverable, not scratch.
///
/// Built from the user's configured output folder
/// ([`Settings::output_dir`](atrium::settings::Settings::output_dir)), which
/// defaults to `<home>/MacAtrium/Images` — **not** the Documents root the user's
/// own small files go to: a built image runs to hundreds of MB, and Windows
/// Documents is often OneDrive-backed, so defaulting there would quietly push a
/// ~750 MB disk into cloud sync.
///
/// Only the *filename* is a placeholder: a build auto-names the output from the
/// collection it built.
fn default_out_image(settings: &Settings) -> String {
    settings.output_dir().join("macatrium.hda").to_string_lossy().into_owned()
}

/// The `atrium` CLI executable, which every package ships beside this GUI.
///
/// Search order mirrors [`detect_rb_cli`]: next to the running executable (an
/// installed app, or the staged zip), then the sibling target dirs a repo
/// checkout builds into, then `PATH`. `None` means run the library in-process
/// instead — correct but silent, since a windowed app can't show its stderr.
fn find_atrium_exe() -> Option<PathBuf> {
    const EXE: &str = if cfg!(windows) { "atrium.exe" } else { "atrium" };
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        cands.push(dir.join(EXE));
        // A checkout builds the two crates into their own target dirs.
        cands.push(dir.join("../../../atrium-tool/target/release").join(EXE));
    }
    cands.push(PathBuf::from("tools/atrium-tool/target/release").join(EXE));
    if let Some(paths) = std::env::var_os("PATH") {
        cands.extend(std::env::split_paths(&paths).map(|d| d.join(EXE)));
    }
    cands.into_iter().find(|p| p.is_file()).and_then(|p| p.canonicalize().ok())
}

/// A machine-local file under `~/.macatrium/`, created on demand — where the GUI
/// writes data it must be able to find again regardless of the working directory.
fn user_data_file(name: &str) -> Option<PathBuf> {
    let dir = home_dir()?.join(".macatrium");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

/// Locate the rb-cli binary, **always as an absolute path**.
///
/// Absolute matters: `resolve_bin` prefers an absolute path precisely so a stale
/// `rb-cli` earlier on `$PATH` can't shadow the intended one and write a corrupt
/// catalog (see the code guidelines). Returning the bare name would re-open that.
///
/// Search order: next to this executable (the installer stages the binaries side
/// by side), then the user's `~/.local/bin`, then rb-cli's own per-user install
/// location, then each `PATH` entry. Home comes from `HOME` **or** `USERPROFILE` —
/// Windows sets only the latter, so a `HOME`-only probe finds nothing there.
fn detect_rb_cli() -> Option<String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    if let Some(h) = home_dir() {
        roots.push(h.join(".local").join("bin"));
    }
    // rb-cli's Windows installer is per-user under %LocalAppData%\Programs.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs").join("Rusty Backup").join("bin"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        roots.extend(std::env::split_paths(&paths));
    }
    for dir in roots {
        let p = dir.join(RB_CLI_EXE);
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
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
        // Where this title's bits come from, so the Build screen can tell the user
        // a title can't be sourced BEFORE the build quietly leaves it off the disk
        // (`filter_present_apps` drops any record whose files never landed).
        // `local_src` (an imported capture) needs no donor at all.
        let src = if m.get("local_src").and_then(Value::as_str).is_some() {
            Src::Local
        } else {
            match m.get("harvest_src").and_then(|h| h.get("donor")).and_then(Value::as_str) {
                Some(d) => Src::Donor(d.to_string()),
                None => Src::None,
            }
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
            src,
            dirty: false,
        });
    }
    rows
}

impl App {
    /// (Re)load the library table from the bundled data (or the override paths if
    /// set under Advanced), preserving the current selection by id.
    /// The library the GUI and a build should use: the compiled-in dataset with
    /// the user's imported titles layered over it, materialised to one file.
    /// `None` when there are no imports (the embedded library is used directly).
    ///
    /// Imports can't be a mere id list — a captured title has no record at all in
    /// the shipped library — so they have to arrive as dataset records, and this
    /// is where the bundled⊕user layering happens for them.
    fn merged_library_path(&self) -> Option<PathBuf> {
        let user = self.settings.user_library();
        if !user.is_file() || std::fs::metadata(&user).map(|m| m.len()).unwrap_or(0) == 0 {
            return None;
        }
        let dir = user_data_file("cache")?.parent()?.join("cache");
        let _ = std::fs::create_dir_all(&dir);
        let base = dir.join("library-embedded.jsonl");
        let out = dir.join("library-with-imports.jsonl");
        // Materialise the embedded library so merge::run (which works on files)
        // can overlay the user's records onto it.
        if std::fs::write(&base, atrium::config::EMBEDDED_LIBRARY).is_err() {
            return None;
        }
        match atrium::merge::run(&base, &user, &out, false) {
            Ok(()) => Some(out),
            Err(e) => {
                eprintln!("[library] merging imports failed: {e:#}");
                None
            }
        }
    }

    fn reload_library(&mut self) {
        // Explicit Advanced override wins; otherwise embedded ⊕ imported.
        let merged = self.dataset.trim().is_empty().then(|| self.merged_library_path()).flatten();
        let lib = if let Some(p) = merged {
            std::fs::read_to_string(p).unwrap_or_default()
        } else if self.dataset.trim().is_empty() {
            String::from_utf8_lossy(atrium::config::EMBEDDED_LIBRARY).into_owned()
        } else {
            std::fs::read_to_string(self.dataset.trim()).unwrap_or_default()
        };
        let compat = if self.overrides.trim().is_empty() {
            String::from_utf8_lossy(atrium::config::EMBEDDED_COMPAT).into_owned()
        } else {
            std::fs::read_to_string(self.overrides.trim()).unwrap_or_default()
        };
        let rows = parse_library(&lib, &compat);
        self.invalidate_src_check(); // rows carry each title's source
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

    /// The titles the Macintosh Garden downloader should fetch: the disk being
    /// built, else the titles staged for an existing disk.
    ///
    /// It used to read a per-row `selected` tick. Once both screens became explicit
    /// lists there was nothing left to set that tick, so "Download selected titles"
    /// was permanently empty and pointed the user at screens with no checkboxes.
    fn download_targets(&self) -> Vec<(String, String)> {
        let by_id: HashMap<&str, &LibRow> =
            self.library.iter().map(|r| (r.id.as_str(), r)).collect();
        let from_list: Vec<(String, String)> = self
            .work_ids
            .iter()
            .map(|id| {
                let name =
                    by_id.get(id.as_str()).map(|r| r.name.clone()).unwrap_or_else(|| id.clone());
                (id.clone(), name)
            })
            .collect();
        if !from_list.is_empty() {
            return from_list;
        }
        // Editing a disk rather than building one: the staged additions are what
        // the user is about to need the software for.
        self.disk_add
            .iter()
            .map(|id| {
                let name =
                    by_id.get(id.as_str()).map(|r| r.name.clone()).unwrap_or_else(|| id.clone());
                (id.clone(), name)
            })
            .collect()
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

    /// Run an `atrium` verb as a child process, streaming its output into the
    /// Build log. Returns false when the CLI isn't there, so the caller can fall
    /// back to calling the library in-process.
    ///
    /// Why a child process for the long disk jobs: the library reports every step
    /// and every warning with `eprintln!`, and a GUI built with
    /// `windows_subsystem = "windows"` has no stderr to print to — so a build that
    /// skipped half its titles, truncated a name past 31 chars or nearly overflowed
    /// the volume looked identical to a clean one. Running the CLI (the same code,
    /// shipped beside this app) makes that output a pipe we can show, and gives the
    /// user a Cancel that actually stops the work.
    fn spawn_cli(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        args: Vec<String>,
        ok_msg: String,
    ) -> bool {
        let Some(exe) = find_atrium_exe() else { return false };
        let (ltx, lrx) = std::sync::mpsc::channel::<String>();
        self.log_lines.clear();
        self.log_lines.push(format!("$ {} {}", exe.display(), args.join(" ")));
        self.log_rx = Some(lrx);
        self.log_open = true;
        let holder: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
        self.job_child = Some(holder.clone());
        let ctx2 = ctx.clone();
        let what = label.to_string();
        self.spawn_job(ctx, label, move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            let mut cmd = Command::new(&exe);
            cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped());
            if let Some(dir) = exe.parent() {
                cmd.current_dir(dir);
            }
            // CREATE_NO_WINDOW: without it every job flashes a console window.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = ltx.send(format!("could not run {}: {e}", exe.display()));
                    return Done {
                        status: format!("{what} failed: could not run {}", exe.display()),
                        dataset: None,
                        reload: false,
                    };
                }
            };
            // Take the pipes before parking the child where Cancel can reach it.
            let err = child.stderr.take();
            let out = child.stdout.take();
            if let Ok(mut g) = holder.lock() {
                *g = Some(child);
            }
            // stdout on its own thread: a full pipe on either stream would
            // otherwise block the child forever (classic pipe deadlock).
            let t_out = out.map(|o| {
                let tx = ltx.clone();
                let c = ctx2.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(o).lines().map_while(Result::ok) {
                        let _ = tx.send(line);
                        c.request_repaint();
                    }
                })
            });
            if let Some(e) = err {
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    let _ = ltx.send(line);
                    ctx2.request_repaint();
                }
            }
            if let Some(t) = t_out {
                let _ = t.join();
            }
            let status = holder.lock().ok().and_then(|mut g| g.as_mut().map(|c| c.wait()));
            if let Ok(mut g) = holder.lock() {
                *g = None;
            }
            match status {
                Some(Ok(st)) if st.success() => {
                    Done { status: ok_msg, dataset: None, reload: false }
                }
                Some(Ok(st)) => Done {
                    status: format!(
                        "{what} failed ({}) — see the Build log.",
                        match st.code() {
                            Some(c) => format!("exit {c}"),
                            None => "killed".to_string(),
                        }
                    ),
                    dataset: None,
                    reload: false,
                },
                Some(Err(e)) => Done {
                    status: format!("{what} failed: {e}"),
                    dataset: None,
                    reload: false,
                },
                None => Done {
                    status: format!("{what} cancelled."),
                    dataset: None,
                    reload: false,
                },
            }
        });
        true
    }

    /// Write the log to a file the user picks — what you attach to a bug report.
    fn save_log(&mut self) {
        let dir = atrium::settings::user_root().join("Logs");
        let _ = std::fs::create_dir_all(&dir);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("log", &["log", "txt"])
            .set_directory(&dir)
            .set_file_name("macatrium-build.log")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, self.log_lines.join("\n")) {
            Ok(()) => self.status = format!("Saved the log -> {}", path.display()),
            Err(e) => self.status = format!("Could not save the log: {e}"),
        }
    }

    /// Kill the running child job (the Cancel button). In-process jobs can't be
    /// interrupted, so this only appears for CLI-backed ones.
    fn cancel_job(&mut self) {
        if let Some(h) = &self.job_child {
            if let Ok(mut g) = h.lock() {
                if let Some(c) = g.as_mut() {
                    let _ = c.kill();
                    self.status = "Cancelling…".into();
                }
            }
        }
    }

    /// Write the config a CLI verb should run to a temp file, returning its path.
    fn write_temp_config(&mut self, name: &str, cfg: &BuildConfig) -> Option<PathBuf> {
        let path = std::env::temp_dir().join(name);
        match serde_json::to_string_pretty(cfg).map_err(|e| e.to_string()).and_then(|j| {
            std::fs::write(&path, j).map_err(|e| e.to_string()).map(|()| path.clone())
        }) {
            Ok(p) => Some(p),
            Err(e) => {
                self.status = format!("Could not stage the build config: {e}");
                None
            }
        }
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

    /// The most log lines kept in memory. A full build prints a few hundred; the
    /// cap only matters if something loops, and dropping the oldest keeps the tail
    /// (where the failure is) rather than the head.
    const LOG_MAX: usize = 4000;

    /// Pull whatever the running CLI has written into the log buffer. Called every
    /// frame — the channel is the only place those lines exist.
    fn drain_log(&mut self) {
        let Some(rx) = &self.log_rx else { return };
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(line) => self.log_lines.push(line),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if self.log_lines.len() > Self::LOG_MAX {
            let cut = self.log_lines.len() - Self::LOG_MAX;
            self.log_lines.drain(..cut);
        }
        if disconnected {
            self.log_rx = None;
        }
    }

    /// Apply a finished job's result (called at the top of each frame).
    fn poll_job(&mut self) {
        self.drain_log();
        let done = self.job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(done) = done {
            self.job = None;
            self.job_child = None;
            self.busy.clear();
            self.drain_log(); // the tail arrives with (or just after) the result
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
        // Apply imported title ids (migrate/clone) into the disk's contents. The
        // catalog's ids go in verbatim — including any this library doesn't carry,
        // which the build reports rather than the import silently dropping.
        if let Some(res) = self.import_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.import_rx = None;
            self.busy.clear();
            match res {
                Ok(ids) => {
                    self.ensure_library();
                    let n = self.work_add(ids);
                    self.work_all = false;
                    self.status = format!(
                        "Imported {n} title(s) into the list. Pick a Target, optionally Scrub, then Build to migrate/clone."
                    );
                }
                Err(e) => self.status = format!("Import failed: {e}"),
            }
        }
        // Adopt the disk's catalog (Edit disk). Its ids are the baseline the
        // staged edits apply to, so a failed read must leave nothing loaded.
        if let Some(res) = self.disk_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.disk_rx = None;
            self.busy.clear();
            match res {
                Ok(rows) => {
                    self.disk_ids = rows.iter().map(|(i, _)| i.clone()).collect();
                    self.disk_names = rows.into_iter().collect();
                    self.disk_loaded = Some(self.disk_path.trim().to_string());
                    self.disk_rm.clear();
                    self.disk_add.clear();
                    self.status = format!("This disk holds {} title(s).", self.disk_ids.len());
                }
                Err(e) => {
                    self.disk_loaded = None;
                    self.disk_ids.clear();
                    self.disk_names.clear();
                    self.status = e;
                }
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

    /// Where facet edits are written. The Advanced override wins; otherwise the
    /// repo's `data/compatibility.jsonl` **only when it actually exists** (i.e. the
    /// app is running from a checkout), else a user-owned overlay under
    /// `~/.macatrium/`.
    ///
    /// The old behaviour — writing the bare relative `data/compatibility.jsonl` —
    /// silently created a `data\` folder next to the installed `.exe`, so edits
    /// vanished from every later build. Never guess a relative path here.
    fn facet_overlay_path(&self) -> Option<PathBuf> {
        if !self.overrides.trim().is_empty() {
            return Some(PathBuf::from(self.overrides.trim()));
        }
        let in_repo = PathBuf::from("data/compatibility.jsonl");
        if in_repo.is_file() {
            return Some(in_repo.canonicalize().unwrap_or(in_repo));
        }
        user_data_file("compatibility.jsonl")
    }

    /// Save the edited compatibility facets (Colour/Mouse/hotkey) for the dirty
    /// rows into the compatibility overlay ([`Self::facet_overlay_path`]). The
    /// resolved path is pinned into the Advanced override so the very next build
    /// reads back what was just saved.
    fn save_facets(&mut self) {
        let Some(path) = self.facet_overlay_path() else {
            self.status =
                "Nowhere to save: set 'compatibility .jsonl' under Advanced (no home dir found)."
                    .into();
            return;
        };
        let target = path.to_string_lossy().into_owned();
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
            // Pin the resolved path so the next build reads these edits back
            // instead of falling through to the compiled-in overlay.
            if self.overrides.trim().is_empty() {
                self.overrides = target.clone();
            }
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
        let selected = self.download_targets();
        if selected.is_empty() {
            self.status =
                "Nothing to download — put some titles on the disk's list (Build) first.".into();
            return;
        }
        let archive = self.mg_archive.clone();
        let cache = self.cache_dir.clone();
        let rb = self.rb_cli.clone();
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
            // "" = the ship default (System Folder 7.1); the picker offers whatever
            // System Folders the base disk actually carries.
            final_bless: {
                let t = self.final_bless.trim();
                (!t.is_empty()).then(|| t.to_string())
            },
            disk_size_mb: self.disk_size_mb.trim().parse::<u64>().ok(),
            selection,
            out: PathBuf::from(self.out_image.trim()),
            launcher: opt(&self.launcher),
            // No explicit dataset? Use the embedded library WITH the user's
            // imported titles merged in, so an imported capture actually builds.
            dataset: opt(&self.dataset).or_else(|| self.merged_library_path()),
            startup_items: self.startup_items.trim().to_string(),
            overrides: opt(&self.overrides),
            metadata: opt(&self.metadata),
            mg_archive: opt(&self.mg_archive),
            platform: self.platform.trim().to_string(),
            detect_color: self.detect_color,
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
            strip_quicktime: Some(self.strip_quicktime),
            ..BuildConfig::default()
        }
    }

    /// Populate the GUI fields from a loaded [`BuildConfig`] — the inverse of
    /// [`Self::to_config`], so a `builds/*.json` opens straight into the form.
    fn apply_config(&mut self, c: BuildConfig) {
        let s = |o: &Option<PathBuf>| o.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.base_system = c.system.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.base_os = c.base_os.clone().unwrap_or_default();
        self.final_bless = c.final_bless.clone().unwrap_or_default();
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
        self.strip_quicktime = c.strip_quicktime.unwrap_or(!c.wants_color_art());
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
        // Default into <Documents>/MacAtrium/Builds — a build config is the user's
        // own small artefact, so it belongs with their collections rather than in
        // whatever directory the file dialog last happened to land on.
        let dir = atrium::settings::user_root().join("Builds");
        let _ = std::fs::create_dir_all(&dir);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("build config", &["json"])
            .set_directory(&dir)
            .set_file_name("build.json")
            .save_file()
        else { return };
        // The same config Build would run, so `atrium image --config` reproduces
        // this disk — collection and all.
        match serde_json::to_string_pretty(&self.disk_config()) {
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
                // A config names either a collection or an inline id list; both
                // land in the disk's contents so the screen shows what it builds.
                let coll = cfg.collection.clone();
                let sel = cfg.selection.clone();
                self.apply_config(cfg);
                match (coll, sel) {
                    (Some(name), _) => self.load_work(&name),
                    (None, Some(Selection::List { ids })) => {
                        self.new_work();
                        self.work_add(ids);
                        self.work_dirty = false;
                    }
                    (None, Some(Selection::All)) => self.work_all = true,
                    _ => {}
                }
                self.status = format!("Loaded build config {}", path.display());
            }
            Err(e) => self.status = format!("Load failed: {e}"),
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

    /// Drop working-set titles whose OS scope (minOS/maxOS) excludes the current
    /// Target's OS — the migration scrub, the same scope a build applies.
    ///
    /// A title the current library doesn't carry is kept rather than silently
    /// dropped: it may come from an imported capture, and the build reports an
    /// unresolvable id far more usefully than a scrub that quietly ate it.
    fn scrub_incompatible(&mut self) {
        let os = self.base_os.trim().to_string();
        if os.is_empty() {
            self.status = "Pick a Target first — its OS is what titles are scrubbed against.".into();
            return;
        }
        self.ensure_library();
        let keep: Vec<String> = {
            let by_id: HashMap<&str, &LibRow> =
                self.library.iter().map(|r| (r.id.as_str(), r)).collect();
            self.work_ids
                .iter()
                .filter(|id| match by_id.get(id.as_str()) {
                    Some(r) => {
                        atrium::selection::os_in_range(&os, r.min_os.as_deref(), r.max_os.as_deref())
                    }
                    None => true,
                })
                .cloned()
                .collect()
        };
        let scrubbed = self.work_ids.len() - keep.len();
        if scrubbed > 0 {
            self.work_ids = keep;
            // Recommended must stay a subset of the contents, or the launcher would
            // surface a title this build never installs.
            let ids: HashSet<&str> = self.work_ids.iter().map(String::as_str).collect();
            self.work_rec.retain(|r| ids.contains(r.as_str()));
            self.work_dirty = true;
            self.invalidate_src_check();
        }
        self.status = if scrubbed == 0 {
            format!("All {} title(s) are compatible with {os}.", self.work_ids.len())
        } else {
            format!("Scrubbed {scrubbed} title(s) incompatible with {os}.")
        };
    }

    /// The build config for the Build screen: the shared form fields plus the
    /// disk's contents as the selection.
    ///
    /// Building a list names the **collection**, not just its ids: `image::run`
    /// reads `coll.recommended` to tag the launcher's Recommended category
    /// (`add_recommended_to_cats` in image.rs), and a bare `Selection::List`
    /// carries no such thing — which is why a GUI build used to surface only the
    /// taxonomy's Recommended seeds and silently drop the collection's own list,
    /// while the CLI's `collection:` config shipped both. `build_image` therefore
    /// saves the working set before it builds.
    fn disk_config(&self) -> BuildConfig {
        let mut cfg = self.to_config();
        if self.work_all {
            cfg.selection = Some(Selection::All);
            cfg.collection = None;
        } else {
            cfg.selection = Some(Selection::List { ids: self.work_ids.clone() });
            cfg.collection = {
                let n = self.work_name.trim();
                (!n.is_empty()).then(|| n.to_string())
            };
        }
        cfg
    }

    /// The config for editing an already-built disk: the disk as `out`, the staged
    /// additions as the selection, and a collection ONLY if the user named one to
    /// keep in step.
    ///
    /// Deliberately not [`Self::disk_config`], which names whatever list the Build
    /// screen has open — with `--update-collection` that would rewrite a saved list
    /// having nothing to do with the disk being edited.
    fn edit_config(&self, disk: &Path, add: &[String]) -> BuildConfig {
        let mut cfg = self.to_config();
        cfg.out = disk.to_path_buf();
        cfg.selection = Some(Selection::List { ids: add.to_vec() });
        cfg.collection = {
            let n = self.disk_sync_name.trim();
            (!n.is_empty()).then(|| n.to_string())
        };
        cfg
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
        // The base disk has to be a real file. A Target names an OS key, the
        // registry maps it to a path, and nothing so far has checked that the
        // path exists — a stale or placeholder entry (the repo's shipped
        // templates.json points at "/path/to/your/…") otherwise fails several
        // seconds in with a raw copy error.
        if let Some(missing) = self.missing_base_image() {
            self.status = missing;
            return;
        }
        if detect_rb_cli().is_none() && !Path::new(self.rb_cli.trim()).is_file() {
            self.status = format!(
                "rb-cli not found ({}). Set it under ⚙ Settings — every disk operation runs through it.",
                if self.rb_cli.trim().is_empty() { "not configured" } else { self.rb_cli.trim() }
            );
            return;
        }
        if !self.work_all {
            if self.work_ids.is_empty() {
                self.status =
                    "Add at least one title (or switch to \"Every compatible title\").".into();
                return;
            }
            // Refuse a build that could only produce an empty disk. A partial one
            // still goes ahead — the warning is on screen — but "nothing at all
            // can be sourced" is always a configuration mistake, and finding out
            // after a ten-minute build is the worst way to learn it.
            self.ensure_src_check();
            if let Some(c) = self.src_check.clone() {
                if c.ok == 0 {
                    self.status = if c.missing_donors.is_empty() {
                        "None of these titles have a source, so the disk would come out empty."
                            .to_string()
                    } else {
                        let keys: Vec<&str> = c.missing_donors.iter().map(String::as_str).collect();
                        format!(
                            "The disk would come out empty: no donor configured for {}. \
                             Add it under Settings → Donors.",
                            keys.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")
                        )
                    };
                    return;
                }
            }
            // Build exactly what's on screen: an unsaved edit is written out
            // first, so the build can be driven by a named collection and carry
            // its Recommended set.
            if self.work_dirty || self.work_path.is_none() {
                self.work_name = self.work_save_name();
                if !self.save_work() {
                    return;
                }
            }
        }

        // A build overwrites its output from the first step (the base image is
        // copied over it), so an existing disk is gone before anything is
        // verified — including the disk the user just spent an hour on, if they
        // pointed Build at it instead of "Add to disk". Ask first.
        let out_path = PathBuf::from(self.out_image.trim());
        if out_path.is_file() {
            let mb = std::fs::metadata(&out_path).map(|m| m.len() / 1_048_576).unwrap_or(0);
            let go = rfd::MessageDialog::new()
                .set_title("Overwrite this disk?")
                .set_description(format!(
                    "{} already exists ({mb} MB) and will be replaced by this build.\n\n\
                     To add titles to it instead, cancel and use the \"Add to disk\" tab.",
                    out_path.display()
                ))
                .set_buttons(rfd::MessageButtons::OkCancel)
                .set_level(rfd::MessageLevel::Warning)
                .show();
            if go != rfd::MessageDialogResult::Ok {
                self.status = "Build cancelled — the existing disk was left alone.".into();
                return;
            }
        }

        // The output folder is the user's own choice and may never have been
        // created (only the wizard's "Create folders" makes the default tree), so
        // make it now instead of failing on the base copy.
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let out = self.out_image.clone();
        let label = format!("Building image ({})", depths.join("/"));
        // Preferred path: run the CLI so its seven stages, and every warning it
        // prints, land in the Build log. Falls back to the library in-process when
        // the CLI isn't beside the app (then the log stays empty, as before).
        let cli_cfg = self.disk_config();
        if let Some(cfg_path) = self.write_temp_config("macatrium-gui-build.json", &cli_cfg) {
            let args = vec!["image".into(), "--config".into(), cfg_path.display().to_string()];
            if self.spawn_cli(ctx, &label, args, format!("Built image -> {out}")) {
                return;
            }
        }
        let cfg = self.disk_config();
        self.spawn_job(ctx, &label, move || match image::run(&cfg) {
            Ok(()) => Done { status: format!("Built image -> {out}"), dataset: None, reload: false },
            Err(e) => Done { status: format!("Build failed: {e}"), dataset: None, reload: false },
        });
    }

    /// Close setup and record that this revision has been seen, so it doesn't come
    /// back next launch. Saves through [`Self::save_settings`], which writes every
    /// field the wizard just edited in the same pass.
    fn finish_wizard(&mut self) {
        self.settings.setup_seen = Some(WIZARD_REV);
        self.show_wizard = false;
        self.wizard_returning = false;
        self.save_settings();
    }

    /// Persist the Settings-screen fields to `~/.macatrium.json`.
    fn save_settings(&mut self) {
        // Cloning (not rebuilding) is what keeps fields the Settings screen doesn't
        // edit — `setup_seen`, the template/donor maps — through every save.
        let mut s = self.settings.clone();
        s.macpack_dir = opt_path(&self.macpack_dir);
        s.cache_dir = opt_path(&self.cache_dir);
        s.mg_archive = opt_path(&self.mg_archive);
        s.curated_overlay = opt_path(&self.curated);
        s.output_dir = opt_path(&self.output_dir);
        s.sources_dir = opt_path(&self.sources_dir);
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

    /// The disk's contents — the merged Build + Collections screen.
    ///
    /// The loaded list **is** what the disk gets: rows are dropped with ✖ and
    /// flagged Recommended in place, and "Add titles…" opens the whole library in
    /// a modal to pull more in. Previously these were two screens over two
    /// different states (the Build tab's ticks vs the Collections tab's loaded
    /// JSON), so a list round-tripped through both and neither owned it.
    fn disk_contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        self.ensure_library();
        self.ensure_collections();
        ui.group(|ui| {
            ui.strong("Titles on this disk");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.work_all, false, "This list");
                ui.radio_value(&mut self.work_all, true, "Every compatible title")
                    .on_hover_text("Ignore the list and include every title the Target's OS can run.");
            });
            if self.work_all {
                ui.label(
                    egui::RichText::new("Every title compatible with the Target's OS will be included.")
                        .small()
                        .weak(),
                );
                return;
            }

            // Which saved list is loaded, where it lives, whether it's edited.
            ui.horizontal(|ui| {
                ui.label("List:");
                let names = self.coll_names.clone();
                let cur = if self.work_name.is_empty() {
                    "(untitled)".to_string()
                } else {
                    self.work_name.clone()
                };
                let mut pick: Option<String> = None;
                egui::ComboBox::from_id_salt("work_pick")
                    .selected_text(cur)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for n in &names {
                            if ui.selectable_label(self.work_name == *n, n).clicked() {
                                pick = Some(n.clone());
                            }
                        }
                    });
                if let Some(n) = pick {
                    self.load_work(&n);
                }
                if ui.button("New").on_hover_text("Start an empty list.").clicked() {
                    self.new_work();
                }
                if ui
                    .button("Reload")
                    .on_hover_text("Re-scan for saved lists (yours ⊕ the ones shipped with the app).")
                    .clicked()
                {
                    self.reload_collections();
                }
                if self.work_origin != "new" {
                    let where_ = self
                        .work_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.label(egui::RichText::new(format!("({})", self.work_origin)).small().weak())
                        .on_hover_text(where_);
                }
                if self.work_dirty {
                    ui.label(
                        egui::RichText::new("• unsaved").small().color(ui.visuals().warn_fg_color),
                    );
                }
            });

            // Rename-on-save: type a different name and Save forks a new list
            // rather than rewriting the one you loaded.
            ui.horizontal(|ui| {
                ui.label("Name:");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.work_name)
                            .hint_text("list name")
                            .desired_width(220.0),
                    )
                    .changed()
                {
                    self.work_dirty = true;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} games · {} recommended",
                        self.work_ids.len(),
                        self.work_rec.len()
                    ))
                    .small()
                    .weak(),
                );
            });

            // Can these titles actually reach a disk? Say so here rather than let
            // the build skip them with a warning only stderr ever sees.
            self.ensure_src_check();
            if let Some(c) = self.src_check.clone() {
                if c.is_clean() {
                    if !self.work_ids.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("✓ all {} title(s) can be sourced", c.ok))
                                .small()
                                .weak(),
                        );
                    }
                } else {
                    let warn = ui.visuals().warn_fg_color;
                    let mut msg = format!(
                        "⚠ {} of {} title(s) have no source and would be left off the disk",
                        c.unsourced.len() + c.unknown.len(),
                        self.work_ids.len()
                    );
                    if !c.missing_donors.is_empty() {
                        let keys: Vec<&str> =
                            c.missing_donors.iter().map(String::as_str).collect();
                        msg.push_str(&format!(
                            " — no donor configured for {}",
                            keys.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")
                        ));
                    }
                    ui.label(egui::RichText::new(msg).small().color(warn));
                    ui.horizontal(|ui| {
                        if !c.missing_donors.is_empty()
                            && ui
                                .small_button("Add the donor…")
                                .on_hover_text("Open Settings with this donor key filled in.")
                                .clicked()
                        {
                            if let Some(k) = c.missing_donors.iter().next() {
                                self.dn_key = k.clone();
                            }
                            self.tab = Tab::Settings;
                        }
                        if !c.unknown.is_empty() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} id(s) aren't in this library: {}",
                                    c.unknown.len(),
                                    c.unknown.join(", ")
                                ))
                                .small()
                                .weak(),
                            );
                        }
                    });
                }
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("➕ Add titles…"))
                    .on_hover_text("Browse the whole library and add titles to this disk.")
                    .clicked()
                {
                    self.add_sel.clear();
                    self.add_to = AddTo::List; // the modal is shared with Edit disk
                    self.add_open = true;
                }
                ui.separator();
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.work_search)
                        .desired_width(160.0)
                        .hint_text("filter this list…"),
                );
            });

            // The rows to show, in disk order. Owned so the scroll closure doesn't
            // hold a borrow of the library while the buttons mutate the list.
            let rows: Vec<(String, String)> = {
                let q = self.work_search.to_lowercase();
                let by_id: HashMap<&str, &LibRow> =
                    self.library.iter().map(|r| (r.id.as_str(), r)).collect();
                self.work_ids
                    .iter()
                    .filter_map(|id| {
                        let name = by_id
                            .get(id.as_str())
                            .map(|r| r.name.clone())
                            .unwrap_or_else(|| id.clone());
                        (q.is_empty()
                            || name.to_lowercase().contains(&q)
                            || id.to_lowercase().contains(&q))
                        .then(|| (id.clone(), name))
                    })
                    .collect()
            };

            ui.horizontal(|ui| {
                if ui.small_button("Recommend all shown").clicked() {
                    for (id, _) in &rows {
                        self.work_rec.insert(id.clone());
                    }
                    self.work_dirty = true;
                }
                if ui.small_button("Clear recommended").clicked() {
                    self.work_rec.clear();
                    self.work_dirty = true;
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("{} shown", rows.len())).small().weak(),
                );
            });
            ui.separator();

            if self.work_ids.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "This list is empty — \"Add titles…\" to choose what goes on the disk.",
                    )
                    .weak(),
                );
            }

            // ✖ removes a row; applied after the scroll so we don't mutate the
            // list while it's being laid out.
            let mut remove: Option<String> = None;
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            egui::ScrollArea::vertical()
                .id_salt("work_rows")
                .auto_shrink([false, false])
                .max_height(300.0)
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    for vis in range {
                        let (id, name) = &rows[vis];
                        ui.horizontal(|ui| {
                            let mut rec = self.work_rec.contains(id.as_str());
                            if ui
                                .checkbox(&mut rec, "")
                                .on_hover_text("Recommended — surfaced in the launcher's Recommended category")
                                .changed()
                            {
                                if rec {
                                    self.work_rec.insert(id.clone());
                                } else {
                                    self.work_rec.remove(id.as_str());
                                }
                                self.work_dirty = true;
                            }
                            if ui
                                .small_button("✖")
                                .on_hover_text("Remove from this disk")
                                .clicked()
                            {
                                remove = Some(id.clone());
                            }
                            ui.label(name);
                            ui.label(egui::RichText::new(id.as_str()).small().weak());
                        });
                    }
                });
            if let Some(id) = remove {
                self.work_remove(&id);
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy && !self.work_ids.is_empty(), egui::Button::new("Save list"))
                    .on_hover_text(
                        "Save to your collections folder. A list shipped with the app is never \
                         overwritten — saving forks your own copy, which then shadows it by name.",
                    )
                    .clicked()
                {
                    self.save_work();
                }
                let deletable = self.work_origin == "user";
                if ui
                    .add_enabled(deletable, egui::Button::new("Delete"))
                    .on_hover_text("Delete this saved list (only your own copies).")
                    .clicked()
                {
                    self.delete_work();
                }
            });
        });
        self.add_titles_window(ctx);
    }

    /// The "Add titles…" modal: the whole library with its own search + filters
    /// and its own ticks, so opening it disturbs neither the disk's list nor the
    /// Library tab's browse state. Titles already on the disk are shown ticked
    /// and disabled rather than hidden — "why isn't it in the list?" is a worse
    /// question than a greyed row that answers itself.
    fn add_titles_window(&mut self, ctx: &egui::Context) {
        if !self.add_open {
            return;
        }
        self.ensure_art_index(ctx);
        let mut open = true;
        let mut do_add = false;
        egui::Window::new("Add titles")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_search)
                            .desired_width(220.0)
                            .hint_text("name or id…"),
                    );
                    if ui.small_button("✖").on_hover_text("Clear the search").clicked() {
                        self.add_search.clear();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Kind:");
                    let kinds = self.kinds();
                    let cur = if self.add_kind.is_empty() { "(all)".to_string() } else { self.add_kind.clone() };
                    egui::ComboBox::from_id_salt("add_kind").selected_text(cur).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.add_kind, String::new(), "(all)");
                        for k in &kinds {
                            ui.selectable_value(&mut self.add_kind, k.clone(), k.as_str());
                        }
                    });
                    ui.label("Genre:");
                    let genres = self.genres();
                    let curg = if self.add_genre.is_empty() { "(all)".to_string() } else { self.add_genre.clone() };
                    egui::ComboBox::from_id_salt("add_genre").selected_text(curg).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.add_genre, String::new(), "(all)");
                        for g in &genres {
                            ui.selectable_value(&mut self.add_genre, g.clone(), g.as_str());
                        }
                    });
                    ui.separator();
                    ui.checkbox(&mut self.thumbs, "thumbnails")
                        .on_hover_text("Box art from the Macintosh Garden archive (set it in Settings).");
                });

                // (id, name, meta, already on the disk)
                let rows: Vec<(String, String, String, bool)> = {
                    let q = self.add_search.to_lowercase();
                    // Which list is being filled decides what counts as "already
                    // there": the disk being built, or the one being edited (where
                    // a staged addition counts too).
                    let on_disk: HashSet<&str> = match self.add_to {
                        AddTo::List => self.work_ids.iter().map(String::as_str).collect(),
                        AddTo::Disk => self
                            .disk_ids
                            .iter()
                            .chain(self.disk_add.iter())
                            .map(String::as_str)
                            .collect(),
                    };
                    self.library
                        .iter()
                        .filter(|r| {
                            (self.add_kind.is_empty() || r.kind == self.add_kind)
                                && (self.add_genre.is_empty()
                                    || r.genres.iter().any(|g| *g == self.add_genre))
                                && (q.is_empty()
                                    || r.name.to_lowercase().contains(&q)
                                    || r.id.contains(&q))
                        })
                        .map(|r| {
                            let meta = [r.kind.as_str(), r.year.as_str()]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            (r.id.clone(), r.name.clone(), meta, on_disk.contains(r.id.as_str()))
                        })
                        .collect()
                };

                ui.horizontal(|ui| {
                    if ui.small_button("Select all shown").clicked() {
                        for (id, _, _, on) in &rows {
                            if !on {
                                self.add_sel.insert(id.clone());
                            }
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        self.add_sel.clear();
                    }
                    ui.separator();
                    let already = rows.iter().filter(|(_, _, _, on)| *on).count();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} shown · {} ticked · {already} already on the disk",
                            rows.len(),
                            self.add_sel.len()
                        ))
                        .small()
                        .weak(),
                    );
                });
                ui.separator();

                const THUMB: f32 = 40.0;
                let row_h = if self.thumbs {
                    THUMB + 6.0
                } else {
                    ui.text_style_height(&egui::TextStyle::Body) + 6.0
                };
                egui::ScrollArea::vertical()
                    .id_salt("add_rows")
                    .auto_shrink([false, false])
                    .max_height(340.0)
                    .show_rows(ui, row_h, rows.len(), |ui, range| {
                        for vis in range {
                            let (id, name, meta, on_disk) = &rows[vis];
                            let uri = if self.thumbs { self.thumb_uri(id, name) } else { None };
                            ui.horizontal(|ui| {
                                let mut ticked = *on_disk || self.add_sel.contains(id.as_str());
                                let cb = ui.add_enabled(
                                    !*on_disk,
                                    egui::Checkbox::new(&mut ticked, ""),
                                );
                                if cb.changed() {
                                    if ticked {
                                        self.add_sel.insert(id.clone());
                                    } else {
                                        self.add_sel.remove(id.as_str());
                                    }
                                }
                                if self.thumbs {
                                    match uri {
                                        Some(u) => {
                                            ui.add(
                                                egui::Image::from_uri(u)
                                                    .fit_to_exact_size(egui::vec2(THUMB, THUMB)),
                                            );
                                        }
                                        None => ui.add_space(THUMB),
                                    }
                                }
                                ui.label(name);
                                if !meta.is_empty() {
                                    ui.label(egui::RichText::new(meta.as_str()).small().weak());
                                }
                                if *on_disk {
                                    ui.label(
                                        egui::RichText::new("already on the disk").small().weak(),
                                    );
                                }
                            });
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    let n = self.add_sel.len();
                    if ui
                        .add_enabled(n > 0, egui::Button::new(format!("Add {n} selected")))
                        .clicked()
                    {
                        do_add = true;
                    }
                    if ui.button("Close").clicked() {
                        do_add = false;
                        self.add_open = false;
                    }
                });
            });

        if do_add {
            // Add in library order, not tick order, so the disk's list stays
            // stable regardless of the order rows happened to be clicked.
            let picked: Vec<String> = self
                .library
                .iter()
                .map(|r| r.id.clone())
                .filter(|id| self.add_sel.contains(id.as_str()))
                .collect();
            self.add_sel.clear();
            match self.add_to {
                AddTo::List => {
                    let n = self.work_add(picked);
                    self.status =
                        format!("Added {n} title(s) — {} on the disk.", self.work_ids.len());
                }
                AddTo::Disk => {
                    let n = self.stage_disk_add(picked);
                    self.status = format!(
                        "Staged {n} title(s) to add — press \"Apply changes\" to write them."
                    );
                }
            }
        }
        if !open {
            self.add_open = false;
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
        self.disk_contents(ui, ctx, busy);

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
                // Which System Folder the finished disk BOOTS. The choices are read off
                // the base .hda (cached per path), so a 6.0.8 B&W build can actually ship
                // blessed 6.0.8 instead of silently booting the 7.1 ship default.
                {
                    let base = self.base_system.trim().to_string();
                    if !base.is_empty() && base != self.bless_scanned {
                        let rb = RbCli::new(self.rb_cli.trim());
                        let found = image::system_folders(&rb, Path::new(&base));
                        self.bless_folders = found;
                        self.bless_scanned = base;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("boots:");
                    let cur = if self.final_bless.trim().is_empty() {
                        "(default: System Folder 7.1)".to_string()
                    } else {
                        self.final_bless.clone()
                    };
                    let folders = self.bless_folders.clone();
                    egui::ComboBox::from_id_salt("final_bless")
                        .selected_text(cur)
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.final_bless,
                                String::new(),
                                "(default: System Folder 7.1)",
                            );
                            for f in &folders {
                                ui.selectable_value(&mut self.final_bless, f.clone(), f.as_str());
                            }
                        });
                    if folders.is_empty() {
                        ui.label(
                            egui::RichText::new("pick a base .hda to list its System Folders")
                                .small()
                                .weak(),
                        );
                    }
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
                    if ui
                        .checkbox(&mut self.bw_only, "Mac Plus / SE (B&W only)")
                        .on_hover_text("1-bit artwork only — skips every colour PICT. Much smaller image.")
                        .changed()
                        && self.bw_only
                    {
                        self.strip_quicktime = true; // compact Macs can't load QuickTime
                    }
                    ui.separator();
                    ui.label("max art size:");
                    ui.add(egui::TextEdit::singleline(&mut self.max_art_size).hint_text("720x768").desired_width(80.0));
                });
                ui.checkbox(&mut self.strip_quicktime, "strip QuickTime base (Apple Photo Access, etc.)")
                    .on_hover_text(
                        "Remove QuickTime + Apple Photo Access from every System Folder — both the 7.x \
                         Extensions and the flat System 6 layout. They need Color QuickDraw / a 68020+, \
                         so they error at boot on a Mac Plus/SE. Leave off for colour machines.",
                    );
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

            // Two different kinds of path live here and must not be confused:
            // locations INSIDE the Mac disk being built (always `/`-separated, they
            // are HFS volume paths, not host paths) versus tools/folders on THIS
            // machine. Showing them in one undifferentiated list invites "fixing"
            // /System Folder/Startup Items into a Windows path, which breaks the build.
            ui.collapsing("Paths inside the built Mac disk", |ui| {
                ui.label(
                    egui::RichText::new(
                        "Locations on the Mac volume this build creates. These stay \
                         forward-slash Mac paths on every host OS — they are not \
                         folders on your PC. Defaults are almost always right.",
                    )
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| { ui.label("startup items:"); ui.add(egui::TextEdit::singleline(&mut self.startup_items).desired_width(260.0)); });
                ui.horizontal(|ui| { ui.label("apps root:"); ui.text_edit_singleline(&mut self.apps_root); });
                ui.horizontal(|ui| { ui.label("metadata dir:"); ui.text_edit_singleline(&mut self.metadata_dir); });
                ui.horizontal(|ui| { ui.label("images dir:"); ui.text_edit_singleline(&mut self.images_dir); });
            });

            ui.collapsing("Tools & folders on this machine", |ui| {
                ui.horizontal(|ui| { ui.label("platform:"); ui.add(egui::TextEdit::singleline(&mut self.platform).desired_width(160.0)); });
                ui.horizontal(|ui| {
                    ui.label("rb-cli:");
                    ui.text_edit_singleline(&mut self.rb_cli);
                    if ui.button("Detect").clicked() {
                        match detect_rb_cli() {
                            Some(p) => { self.rb_cli = p.clone(); self.status = format!("Found rb-cli: {p}"); }
                            None => self.status = format!("No {RB_CLI_EXE} found (PATH, ~/.local/bin, or next to this app)."),
                        }
                    }
                });
                path_row(ui, "stage dir:", &mut self.stage, Pick::Folder);
            });
        });
    }

    /// Stage titles to add to the loaded disk. Anything already on the disk, or
    /// already staged, is skipped; nothing touches the volume until Apply.
    /// Returns how many were newly staged.
    fn stage_disk_add(&mut self, picked: Vec<String>) -> usize {
        let mut n = 0;
        for id in picked {
            if !self.disk_ids.contains(&id) && !self.disk_add.contains(&id) {
                self.disk_add.push(id);
                n += 1;
            }
        }
        n
    }

    /// Read the loaded disk's own catalog - the baseline the Edit-disk screen
    /// edits, and the check that this .hda really is a MacAtrium disk.
    ///
    /// The catalog is the disk's source of truth about its contents (a build
    /// regenerates it from what actually landed), so it is also the honest answer
    /// to "what is on here?" - better than trusting the collection it was built
    /// from, which may have drifted since.
    fn load_disk_contents(&mut self, ctx: &egui::Context) {
        let disk = self.disk_path.trim().to_string();
        if disk.is_empty() {
            self.status = "Pick a MacAtrium disk first.".into();
            return;
        }
        if !Path::new(&disk).is_file() {
            self.status = format!("No such disk: {disk}");
            return;
        }
        let rb = self.rb_cli.clone();
        let meta_dir = self.metadata_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> Result<Vec<(String, String)>, String> {
                let rbc = RbCli::new(&rb);
                let tmp = std::env::temp_dir().join("macatrium-disk-catalog.jsonl");
                let _ = std::fs::remove_file(&tmp);
                let src = format!("{}/catalog.jsonl", meta_dir.trim_end_matches('/'));
                rbc.get(PathBuf::from(&disk).as_path(), &src, &tmp, true).map_err(|_| {
                    format!(
                        "That doesn't look like a MacAtrium disk - it has no {src}. \
                         Pick a disk this app built."
                    )
                })?;
                let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
                Ok(atrium::catalog::parse_compiled(&bytes)
                    .iter()
                    .filter_map(|v| {
                        let id = v.get("id").and_then(Value::as_str)?.to_string();
                        let name =
                            v.get("name").and_then(Value::as_str).unwrap_or(id.as_str()).to_string();
                        Some((id, name))
                    })
                    .collect())
            })();
            let _ = tx.send(res);
            ctx2.request_repaint();
        });
        self.disk_rx = Some(rx);
        self.busy = "Reading the disk".into();
        self.status = "Reading the disk's catalog...".into();
    }

    /// Apply the staged edits to the loaded disk: one `replace` when titles are
    /// going both ways, otherwise a plain `add` or `remove`. All three work in
    /// place - no rebuild, and the titles already there keep their art.
    fn apply_disk_changes(&mut self, ctx: &egui::Context) {
        let disk = self.disk_path.trim().to_string();
        if self.disk_loaded.as_deref() != Some(disk.as_str()) {
            self.status = "Load the disk first, so its contents are known.".into();
            return;
        }
        let rm: Vec<String> =
            self.disk_ids.iter().filter(|id| self.disk_rm.contains(id.as_str())).cloned().collect();
        let add = self.disk_add.clone();
        if rm.is_empty() && add.is_empty() {
            self.status = "Nothing staged - remove a title, or add some.".into();
            return;
        }
        // The same silent trap as a fresh build: a title whose donor isn't
        // configured is skipped with a warning and dropped from the catalog, so an
        // "add" can report success having changed nothing.
        if !add.is_empty() {
            let c = self.check_sources(&add);
            if c.ok == 0 {
                self.status = if c.missing_donors.is_empty() {
                    "None of the titles being added have a source, so nothing would change."
                        .to_string()
                } else {
                    let keys: Vec<&str> = c.missing_donors.iter().map(String::as_str).collect();
                    format!(
                        "Nothing would be added: no donor configured for {}. \
                         Add it under Settings -> Donors.",
                        keys.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")
                    )
                };
                return;
            }
        }
        // Removal deletes app folders and baked art in place: there is no undo
        // short of rebuilding, so name what goes and make the user say yes.
        if !rm.is_empty() {
            let names: Vec<&str> = rm
                .iter()
                .map(|id| self.disk_names.get(id).map(String::as_str).unwrap_or(id.as_str()))
                .collect();
            let go = rfd::MessageDialog::new()
                .set_title("Remove these titles?")
                .set_description(format!(
                    "{} will be deleted from {}:\n\n{}\n\nThis edits the disk in place - the only \
                     way back is to build it again.",
                    if rm.len() == 1 {
                        "1 title".to_string()
                    } else {
                        format!("{} titles", rm.len())
                    },
                    disk,
                    names.join(", ")
                ))
                .set_buttons(rfd::MessageButtons::OkCancel)
                .set_level(rfd::MessageLevel::Warning)
                .show();
            if go != rfd::MessageDialogResult::Ok {
                self.status = "Left the disk alone.".into();
                return;
            }
        }

        // Syncing is only meaningful when the user says WHICH list follows the edit.
        let sync = !self.disk_sync_name.trim().is_empty();
        let label = match (rm.is_empty(), add.is_empty()) {
            (false, false) => format!("Swapping {} out for {} on the disk", rm.len(), add.len()),
            (true, false) => format!("Adding {} title(s) to the disk", add.len()),
            (false, true) => format!("Removing {} title(s) from the disk", rm.len()),
            (true, true) => unreachable!("empty edit returns above"),
        };
        let ok_msg = format!("{label} - done. Reload to see the disk's new contents.");
        let disk_path = PathBuf::from(&disk);

        // Preferred path: the CLI, so each step and every warning lands in the
        // Build log. `add` has no --image flag, so the config carries the disk.
        let edit_cfg = self.edit_config(disk_path.as_path(), &add);
        if let Some(cfg_path) = self.write_temp_config("macatrium-gui-edit.json", &edit_cfg) {
            let mut args: Vec<String> = Vec::new();
            let verb_takes_image = !rm.is_empty();
            match (rm.is_empty(), add.is_empty()) {
                (false, false) => {
                    args.push("replace".into());
                    args.push("--config".into());
                    args.push(cfg_path.display().to_string());
                    args.push("--image".into());
                    args.push(disk.clone());
                    for id in &rm {
                        args.push("--remove".into());
                        args.push(id.clone());
                    }
                    for id in &add {
                        args.push("--add".into());
                        args.push(id.clone());
                    }
                }
                (true, false) => {
                    // `add` takes its titles from the config's selection.
                    args.push("add".into());
                    args.push("--config".into());
                    args.push(cfg_path.display().to_string());
                }
                (false, true) => {
                    args.push("remove".into());
                    args.push("--config".into());
                    args.push(cfg_path.display().to_string());
                    args.push("--image".into());
                    args.push(disk.clone());
                    for id in &rm {
                        args.push("--id".into());
                        args.push(id.clone());
                    }
                }
                (true, true) => unreachable!("empty edit returns above"),
            }
            // Only remove/replace can sync the collection; a plain add has no such
            // flag (its ids are already in the config's selection).
            if sync && verb_takes_image {
                args.push("--update-collection".into());
            }
            if self.spawn_cli(ctx, &label, args, ok_msg.clone()) {
                self.disk_rm.clear();
                self.disk_add.clear();
                return;
            }
        }

        // Fallback: the same operations in-process (no streamed log).
        let cfg = edit_cfg;
        let (rm2, add2) = (rm.clone(), add.clone());
        self.disk_rm.clear();
        self.disk_add.clear();
        self.spawn_job(ctx, &label, move || {
            let res = match (rm2.is_empty(), add2.is_empty()) {
                (false, false) => image::replace_on_disk(&cfg, &rm2, &add2, sync),
                (true, false) => image::add_to_disk(&cfg),
                (false, true) => image::remove_from_disk(&cfg, &rm2, false, sync),
                (true, true) => Ok(()),
            };
            match res {
                Ok(()) => Done { status: ok_msg, dataset: None, reload: false },
                Err(e) => Done { status: format!("Failed: {e}"), dataset: None, reload: false },
            }
        });
    }

    /// Edit an already-built disk: read what's on it, stage removals and
    /// additions, then apply them in place.
    ///
    /// Deliberately the same list model as Build (a table you add to and take
    /// from) rather than the old grid of library checkboxes - the two screens do
    /// the same job from different starting points, and the old one couldn't
    /// express "take this off the disk" at all.
    fn tab_edit_disk(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        self.ensure_library();
        ui.label(
            egui::RichText::new(
                "Change an already-built disk without rebuilding it: add titles, remove titles, or \
                 swap one for another. Everything else on the disk - including its artwork - is \
                 left as it is.",
            )
            .small()
            .weak(),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            path_row(ui, "MacAtrium disk (.hda):", &mut self.disk_path, Pick::File);
            if ui.add_enabled(!busy, egui::Button::new("Load")).clicked() {
                self.load_disk_contents(ctx);
            }
        });

        // Art depths and OS scope have to match what the disk was built with, or
        // an added title gets art in a depth the launcher won't use.
        ui.add_space(6.0);
        self.target_combo(ui);
        ui.label(
            egui::RichText::new(
                "Pick the Target this disk was built with, so added titles get matching art depths.",
            )
            .small()
            .weak(),
        );

        ui.add_space(6.0);
        if self.disk_loaded.as_deref() != Some(self.disk_path.trim()) {
            ui.group(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Load a disk to see what's on it. Its catalog is read straight off the \
                         volume, so it shows what the disk really contains.",
                    )
                    .weak(),
                );
            });
            return;
        }

        let staged_rm = self.disk_ids.iter().filter(|i| self.disk_rm.contains(i.as_str())).count();
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("On this disk");
                ui.label(
                    egui::RichText::new(format!(
                        "{} title(s){}{}",
                        self.disk_ids.len(),
                        if staged_rm > 0 {
                            format!(" \u{b7} {staged_rm} to remove")
                        } else {
                            String::new()
                        },
                        if self.disk_add.is_empty() {
                            String::new()
                        } else {
                            format!(" \u{b7} {} to add", self.disk_add.len())
                        }
                    ))
                    .small()
                    .weak(),
                );
            });

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("\u{2795} Add titles\u{2026}"))
                    .on_hover_text("Browse the library and stage titles to add to this disk.")
                    .clicked()
                {
                    self.add_sel.clear();
                    self.add_to = AddTo::Disk;
                    self.add_open = true;
                }
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.disk_search)
                        .hint_text("title...")
                        .desired_width(160.0),
                );
                if ui
                    .small_button("Reload")
                    .on_hover_text("Re-read the disk's catalog, dropping staged edits.")
                    .clicked()
                {
                    self.disk_rm.clear();
                    self.disk_add.clear();
                    self.load_disk_contents(ctx);
                }
            });
            ui.separator();

            // Staged additions first: they are the edit, so they shouldn't be
            // buried below a hundred rows of existing contents.
            let mut unstage: Option<String> = None;
            for id in &self.disk_add {
                let name = self
                    .library
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| r.name.clone())
                    .unwrap_or_else(|| id.clone());
                ui.horizontal(|ui| {
                    if ui
                        .small_button("\u{2716}")
                        .on_hover_text("Don't add this after all")
                        .clicked()
                    {
                        unstage = Some(id.clone());
                    }
                    ui.label(
                        egui::RichText::new("+").monospace().color(ui.visuals().hyperlink_color),
                    );
                    ui.label(name);
                    ui.label(egui::RichText::new(id.as_str()).small().weak());
                });
            }
            if let Some(id) = unstage {
                self.disk_add.retain(|x| x != &id);
            }

            let q = self.disk_search.to_lowercase();
            let rows: Vec<(String, String)> = self
                .disk_ids
                .iter()
                .filter_map(|id| {
                    let name = self.disk_names.get(id).cloned().unwrap_or_else(|| id.clone());
                    (q.is_empty()
                        || name.to_lowercase().contains(&q)
                        || id.to_lowercase().contains(&q))
                    .then_some((id.clone(), name))
                })
                .collect();
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            egui::ScrollArea::vertical()
                .id_salt("disk_rows")
                .auto_shrink([false, false])
                .max_height(300.0)
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    for vis in range {
                        let (id, name) = &rows[vis];
                        let staged = self.disk_rm.contains(id.as_str());
                        ui.horizontal(|ui| {
                            if staged {
                                if ui
                                    .small_button("undo")
                                    .on_hover_text("Keep this title after all")
                                    .clicked()
                                {
                                    self.disk_rm.remove(id.as_str());
                                }
                            } else if ui
                                .small_button("\u{2716}")
                                .on_hover_text("Remove this title from the disk")
                                .clicked()
                            {
                                self.disk_rm.insert(id.clone());
                            }
                            let txt = egui::RichText::new(name);
                            ui.label(if staged {
                                txt.strikethrough().color(ui.visuals().warn_fg_color)
                            } else {
                                txt
                            });
                            ui.label(egui::RichText::new(id.as_str()).small().weak());
                        });
                    }
                });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Also update saved list:");
            self.ensure_collections();
            let names = self.coll_names.clone();
            let cur = if self.disk_sync_name.trim().is_empty() {
                "(none)".to_string()
            } else {
                self.disk_sync_name.clone()
            };
            egui::ComboBox::from_id_salt("disk_sync_list")
                .selected_text(cur)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.disk_sync_name, String::new(), "(none)");
                    for n in &names {
                        ui.selectable_value(&mut self.disk_sync_name, n.clone(), n);
                    }
                })
                .response
                .on_hover_text(
                    "Apply the same additions and removals to a saved list, so a later full \
                     rebuild produces the disk you have now rather than the one you started with. \
                     Pick the list this disk was built from - nothing is guessed, because the \
                     disk itself doesn't record it.",
                );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let staged = staged_rm + self.disk_add.len();
            if ui
                .add_enabled(
                    !busy && staged > 0,
                    egui::Button::new(egui::RichText::new("Apply changes").strong()),
                )
                .clicked()
            {
                self.apply_disk_changes(ctx);
            }
            if ui.add_enabled(staged > 0, egui::Button::new("Revert")).clicked() {
                self.disk_rm.clear();
                self.disk_add.clear();
            }
            ui.label(
                egui::RichText::new(if staged == 0 {
                    "No changes staged.".to_string()
                } else {
                    format!("{staged} change(s) staged.")
                })
                .small()
                .weak(),
            );
        });
    }

    fn tab_library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        self.ensure_library();
        ui.label(
            egui::RichText::new("Browse the bundled catalogue and edit each title's compatibility facets (Colour/B&W, Mouse, launch hotkey). Save writes the compatibility overlay.")
                .small().weak(),
        );
        ui.add_space(4.0);
        self.import_panel(ui, busy);
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

    /// Re-scan for available collections via [`atrium::collections::list`] — **user
    /// and bundled**, a user file shadowing a bundled one of the same name.
    ///
    /// The previous scan read only `bundled_dir()`, a *relative* `data/collections`,
    /// so an installed app (whose working directory is its install folder) listed
    /// nothing and there was no way to reach a saved list.
    fn reload_collections(&mut self) {
        let listed = atrium::collections::list();
        self.coll_names = listed.iter().map(|l| l.collection.name.clone()).collect();
        self.coll_index = listed
            .into_iter()
            .map(|l| (l.collection.name.clone(), (l.origin, l.path)))
            .collect();
        self.coll_scanned = true;
    }

    /// Scan for collections once. Guarded by a flag, not by `coll_names.is_empty()`
    /// — "no collections" is a legitimate result, and re-deriving it every repaint
    /// would re-read the settings file and both directories continuously.
    fn ensure_collections(&mut self) {
        if !self.coll_scanned {
            self.reload_collections();
        }
    }

    /// Load a saved collection into the working set — its contents, its
    /// Recommended flags, and where it came from.
    fn load_work(&mut self, name: &str) {
        match atrium::collections::find(name) {
            Ok(c) => {
                self.work_rec = c.recommended.iter().cloned().collect();
                self.work_ids = c.ids;
                self.work_label = c.label;
                self.work_overrides = c.overrides;
                self.work_name = if c.name.is_empty() { name.to_string() } else { c.name };
                self.work_path = atrium::collections::find_path(name);
                self.work_origin = self.coll_index.get(name).map(|(o, _)| *o).unwrap_or("user");
                self.work_dirty = false;
                self.work_all = false;
                self.invalidate_src_check();
                self.status = format!(
                    "Loaded \"{}\" ({}): {} games · {} recommended.",
                    self.work_name,
                    self.work_origin,
                    self.work_ids.len(),
                    self.work_rec.len()
                );
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    /// Start an empty, untitled working set — "build me a disk from scratch".
    fn new_work(&mut self) {
        self.work_name.clear();
        self.work_ids.clear();
        self.work_rec.clear();
        self.work_label.clear();
        self.work_overrides.clear();
        self.work_path = None;
        self.work_origin = "new";
        self.work_dirty = false;
        self.work_all = false;
        self.invalidate_src_check();
        self.status = "New list — use \"Add titles…\" to fill it.".into();
    }

    /// The working set as a [`Collection`](atrium::collections::Collection).
    /// `recommended` is emitted in contents order and filtered to the contents, so
    /// the launcher can never surface a title this build doesn't install.
    fn work_collection(&self, name: &str) -> atrium::collections::Collection {
        atrium::collections::Collection {
            name: name.to_string(),
            label: self.work_label.clone(),
            ids: self.work_ids.clone(),
            overrides: self.work_overrides.clone(),
            recommended: self
                .work_ids
                .iter()
                .filter(|id| self.work_rec.contains(id.as_str()))
                .cloned()
                .collect(),
        }
    }

    /// The name an unnamed working set saves under: the output disk's filename
    /// stem (a build auto-names the disk from the collection, so this keeps the
    /// two in step), else "Untitled".
    fn work_save_name(&self) -> String {
        let n = self.work_name.trim();
        if !n.is_empty() {
            return n.to_string();
        }
        Path::new(self.out_image.trim())
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// Save the working set, returning whether it landed.
    ///
    /// Two rules decide *where*:
    /// * A **bundled** list (one shipped beside the app) is never written back to
    ///   — saving forks it into the user's collections folder, where it shadows
    ///   the shipped copy by name. Editing a curated list must not be revertible
    ///   by an app update.
    /// * A **renamed** list forks too: type a new name, press Save, and you get a
    ///   second list instead of silently rewriting the one you loaded.
    fn save_work(&mut self) -> bool {
        let name = self.work_save_name();
        if self.work_ids.is_empty() {
            self.status = "Nothing to save — the list is empty.".into();
            return false;
        }
        let c = self.work_collection(&name);
        let same_file = self
            .work_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy() == name.as_str())
            .unwrap_or(false);
        let in_place = self.work_origin == "user" && same_file;
        let written = match (&self.work_path, in_place) {
            (Some(p), true) => c.save(p).map(|()| p.clone()),
            _ => atrium::collections::save_user(&c),
        };
        match written {
            Ok(p) => {
                self.work_name = name.clone();
                self.work_path = Some(p.clone());
                self.work_origin = "user";
                self.work_dirty = false;
                self.status = format!(
                    "Saved \"{name}\" ({} games · {} recommended){} -> {}",
                    c.ids.len(),
                    c.recommended.len(),
                    if in_place { "" } else { ", your own copy" },
                    p.display()
                );
                self.reload_collections();
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e:#}");
                false
            }
        }
    }

    /// Delete the loaded collection — **user copies only**, so the GUI can't
    /// remove a list shipped with the app.
    fn delete_work(&mut self) {
        let name = self.work_name.clone();
        if name.is_empty() {
            self.status = "No saved list loaded.".into();
            return;
        }
        match atrium::collections::delete_user(&name) {
            Ok(p) => {
                self.status = format!("Deleted {}", p.display());
                self.new_work();
                self.reload_collections();
            }
            Err(e) => self.status = format!("Delete failed: {e:#}"),
        }
    }

    /// Check that every title on the disk can actually be sourced, against the
    /// configured donors — the preflight for this build's quietest failure.
    ///
    /// A selected title whose donor key isn't configured is skipped by
    /// `harvest_plan` with a warning printed to stderr, which a windowed GUI has
    /// nowhere to show; `filter_present_apps` then drops it from the catalog. The
    /// build therefore "succeeds" and hands back a bootable disk with nothing on
    /// it. Cached — this reads the settings file, so it must not run per repaint.
    fn ensure_src_check(&mut self) {
        if self.src_check.is_some() {
            return;
        }
        let ids = self.work_ids.clone();
        self.src_check = Some(self.check_sources(&ids));
    }

    /// The preflight itself, over any id list — the disk's contents on Build, the
    /// ticked titles on Add to disk.
    fn check_sources(&self, ids: &[String]) -> SourceCheck {
        self.check_sources_in(ids, &atrium::donors::Registry::load_default())
    }

    /// [`Self::check_sources`] against a given registry — the seam that keeps the
    /// rule testable without depending on whatever `~/.macatrium.json` happens to
    /// hold on the machine running the tests.
    fn check_sources_in(&self, ids: &[String], donors: &atrium::donors::Registry) -> SourceCheck {
        let by_id: HashMap<&str, &LibRow> =
            self.library.iter().map(|r| (r.id.as_str(), r)).collect();
        let mut c = SourceCheck::default();
        c.donors_configured = !donors.0.is_empty();
        for id in ids {
            match by_id.get(id.as_str()) {
                // Not in this library at all (a stale list, or a capture whose
                // record was removed) — the build can't source what it can't find.
                None => c.unknown.push(id.clone()),
                Some(r) => match &r.src {
                    Src::Local => c.ok += 1,
                    Src::None => c.unsourced.push(id.clone()),
                    Src::Donor(k) => {
                        if donors.get(k).is_some() {
                            c.ok += 1;
                        } else {
                            c.missing_donors.insert(k.clone());
                            c.unsourced.push(id.clone());
                        }
                    }
                },
            }
        }
        c
    }

    /// Invalidate the cached preflight — call whenever the contents, the library
    /// or the donor registry change.
    fn invalidate_src_check(&mut self) {
        self.src_check = None;
    }

    /// `Some(message)` when the base system image this build would copy isn't a
    /// file on disk — an unregistered OS key, or a registered one whose path is
    /// stale or a placeholder. `None` means the base is good.
    fn missing_base_image(&self) -> Option<String> {
        let os = self.base_os.trim();
        if os.is_empty() {
            let custom = self.base_system.trim();
            return (!Path::new(custom).is_file()).then(|| {
                format!("Base system image not found: {custom}. Pick one under Advanced → Custom base OS.")
            });
        }
        let reg = templates::Registry::load_default();
        match reg.get(os) {
            None => Some(format!(
                "This Target needs a base disk for System {os}, and none is configured. \
                 Add it under ⚙ Settings → Templates."
            )),
            Some(t) if !t.hda.is_file() => Some(format!(
                "The base disk registered for System {os} isn't there: {}. \
                 Fix it under ⚙ Settings → Templates.",
                t.hda.display()
            )),
            Some(_) => None,
        }
    }

    /// Append ids to the working set, skipping any already on it. Returns how
    /// many were actually added.
    fn work_add(&mut self, ids: impl IntoIterator<Item = String>) -> usize {
        let mut added = 0;
        for id in ids {
            if !self.work_ids.contains(&id) {
                self.work_ids.push(id);
                added += 1;
            }
        }
        if added > 0 {
            self.work_dirty = true;
            self.invalidate_src_check();
        }
        added
    }

    /// Drop one title from the working set (and from Recommended with it).
    fn work_remove(&mut self, id: &str) {
        self.work_ids.retain(|x| x != id);
        self.work_rec.remove(id);
        self.work_dirty = true;
        self.invalidate_src_check();
    }



    /// Import captures of installed apps (`.mar` and friends) into the library.
    fn import_panel(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.collapsing("Import captures (.mar)", |ui| {
            ui.label(
                egui::RichText::new(
                    "Add apps you installed yourself in an emulator and captured as .mar. \
                     They join your library as ordinary titles you can pick and build. \
                     No donor disk image is needed — the files are kept in your Sources \
                     folder and injected at build time.",
                )
                .small()
                .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("Add captures…").clicked() {
                    let start = self.settings.source_subdir("Captures");
                    let _ = std::fs::create_dir_all(&start);
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("Mac capture", &["mar", "sit", "sea", "cpt", "hqx"])
                        .set_directory(&start)
                        .pick_files()
                    {
                        for f in files {
                            if !self.imp_files.contains(&f) {
                                self.imp_files.push(f);
                            }
                        }
                    }
                }
                if ui.add_enabled(!self.imp_files.is_empty(), egui::Button::new("Clear")).clicked() {
                    self.imp_files.clear();
                    self.imp_report.clear();
                }
                ui.label(
                    egui::RichText::new(format!("{} queued", self.imp_files.len())).small().weak(),
                );
            });
            for f in &self.imp_files {
                ui.label(
                    egui::RichText::new(format!(
                        "    {}",
                        f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    ))
                    .small(),
                )
                .on_hover_text(f.display().to_string());
            }

            ui.add_space(4.0);
            // Optional destination. Blank = staged on the host, which is the
            // normal case; a reservoir donor is offered for exact-name fidelity.
            ui.horizontal(|ui| {
                ui.label("Store in:");
                let reg = atrium::donors::Registry::load_default();
                let reservoirs: Vec<String> =
                    reg.0.iter().filter(|(_, d)| d.reservoir()).map(|(k, _)| k.clone()).collect();
                let cur = if self.imp_donor.is_empty() {
                    "my Sources folder (no donor)".to_string()
                } else {
                    self.imp_donor.clone()
                };
                egui::ComboBox::from_id_salt("imp_donor")
                    .selected_text(cur)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.imp_donor,
                            String::new(),
                            "my Sources folder (no donor)",
                        );
                        for k in &reservoirs {
                            ui.selectable_value(&mut self.imp_donor, k.clone(), k);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Leave as-is unless you keep a reservoir disk image. A reservoir \
                         preserves Mac filenames exactly (including '/' , which the host \
                         filesystem can't store); only reservoir donors are listed, since a \
                         harvest donor would rename the imported folder.",
                    );
            });
            ui.horizontal(|ui| {
                ui.label("Also add to collection:");
                let names = self.coll_names.clone();
                let cur = if self.imp_collection.is_empty() {
                    "(none)".to_string()
                } else {
                    self.imp_collection.clone()
                };
                egui::ComboBox::from_id_salt("imp_coll").selected_text(cur).width(200.0).show_ui(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut self.imp_collection, String::new(), "(none)");
                        for n in &names {
                            ui.selectable_value(&mut self.imp_collection, n.clone(), n);
                        }
                    },
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.imp_collection)
                        .hint_text("or a new name")
                        .desired_width(150.0),
                );
            });

            ui.add_space(4.0);
            if ui
                .add_enabled(
                    !busy && !self.imp_files.is_empty(),
                    egui::Button::new(egui::RichText::new("Import").strong()),
                )
                .clicked()
            {
                self.run_import();
            }
            for line in &self.imp_report {
                ui.label(egui::RichText::new(line.as_str()).small());
            }
        });
    }

    /// Run the queued imports: expand each capture, record it in the user
    /// library, and (optionally) add the new ids to a collection.
    fn run_import(&mut self) {
        if self.imp_files.is_empty() {
            self.status = "Add one or more .mar captures first.".into();
            return;
        }
        let rb = RbCli::new(self.rb_cli.trim());
        let stage_root = self.settings.import_stage_dir();
        let dataset = self.settings.user_library();

        // A donor is optional. When chosen it must be a reservoir: a harvest
        // donor would re-pick the launch app and rename the folder, overriding
        // the `app` path the import just recorded.
        let donor = if self.imp_donor.trim().is_empty() {
            None
        } else {
            let reg = atrium::donors::Registry::load_default();
            match reg.get(self.imp_donor.trim()) {
                Some(d) if d.reservoir() => Some((self.imp_donor.trim().to_string(), d.path().to_path_buf())),
                Some(_) => {
                    self.status = format!(
                        "Donor {:?} is a harvest donor — it would rename the imported folder. \
                         Pick a reservoir, or leave the donor blank.",
                        self.imp_donor
                    );
                    return;
                }
                None => {
                    self.status = format!("Unknown donor {:?}.", self.imp_donor);
                    return;
                }
            }
        };

        let files = self.imp_files.clone();
        let res = atrium::import::run(
            &rb,
            &files,
            &stage_root,
            &dataset,
            donor.as_ref().map(|(k, p)| (k.as_str(), p.as_path())),
            &self.apps_root,
        );
        self.imp_report.clear();
        match res {
            Ok(report) => {
                for (id, name, n) in &report.imported {
                    self.imp_report.push(format!("✓ {name} ({id}) — {n} file(s)"));
                }
                for (f, e) in &report.failed {
                    self.imp_report.push(format!("✗ {}: {e}", f.display()));
                }
                // Surface name mangling: HFS allows '/' in a filename and the host
                // doesn't, so a data file can land under a different name than the
                // title expects. Rare, but silent and hard to diagnose later.
                for (id, mac, host) in &report.renamed {
                    self.imp_report
                        .push(format!("⚠ {id}: {mac:?} stored as {host:?} — a title that opens \
                                       this file by name may not find it; import via a reservoir \
                                       donor to keep the exact name"));
                }
                let ids: Vec<String> = report.imported.iter().map(|(i, _, _)| i.clone()).collect();
                if !ids.is_empty() {
                    self.imp_files.clear();
                    self.library_loaded = false; // pick the new titles up
                    self.ensure_library();
                    let target = self.imp_collection.trim().to_string();
                    self.add_ids_to_collection(&ids);
                    // If the import appended to the list Build currently has open,
                    // the in-memory copy is now stale — and the next Save would
                    // write it back WITHOUT the imported titles, silently undoing
                    // the import. Put them straight into the working set instead.
                    if !target.is_empty() && target == self.work_name {
                        let n = self.work_add(ids.clone());
                        self.work_dirty = false; // add_ids_to_collection already saved
                        self.imp_report.push(format!(
                            "added {n} title(s) to the open list \"{target}\""
                        ));
                    }
                }
                self.status = format!(
                    "Imported {} capture(s){}.",
                    report.imported.len(),
                    if report.failed.is_empty() { String::new() } else { format!(", {} failed", report.failed.len()) }
                );
            }
            Err(e) => self.status = format!("Import failed: {e:#}"),
        }
    }

    /// Add freshly imported ids to the chosen collection (creating it if new).
    /// A no-op when no collection is selected — importing shouldn't force one.
    fn add_ids_to_collection(&mut self, ids: &[String]) {
        let name = self.imp_collection.trim().to_string();
        if name.is_empty() {
            return;
        }
        let mut c = atrium::collections::find(&name).unwrap_or_default();
        c.name = name.clone();
        for id in ids {
            if !c.ids.contains(id) {
                c.ids.push(id.clone());
            }
        }
        match atrium::collections::save_user(&c) {
            Ok(p) => {
                self.imp_report.push(format!("added {} id(s) to \"{name}\" -> {}", ids.len(), p.display()));
                self.reload_collections();
            }
            Err(e) => self.imp_report.push(format!("could not update \"{name}\": {e:#}")),
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
                ui.label(
                    egui::RichText::new(format!("{} title(s) on the disk's list", self.download_targets().len()))
                        .small()
                        .weak(),
                );
            });
            if !archive_ok {
                ui.label(egui::RichText::new("Set a valid MG-Archive folder to enable the downloader.").small().weak());
            }
        });
    }

    fn tab_settings(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, _busy: bool) {
        ui.label(egui::RichText::new("Machine-local settings, persisted to ~/.macatrium.json.").small().weak());
        ui.add_space(6.0);

        // The two roots first: everything else is a pointer into one of them.
        ui.group(|ui| {
            ui.strong("Folders");
            ui.label(
                egui::RichText::new(
                    "Where finished disks are written, and where your source material lives. \
                     Both default under your home folder rather than Documents — disk images \
                     and donor sets are large, and Documents is often cloud-synced.",
                )
                .small()
                .weak(),
            );
            path_row(ui, "Output (built disks):", &mut self.output_dir, Pick::Folder);
            path_row(ui, "Sources:", &mut self.sources_dir, Pick::Folder);
            ui.label(egui::RichText::new("Inside Sources:").small());
            for (name, what) in atrium::settings::SOURCE_SUBDIRS {
                ui.label(egui::RichText::new(format!("    {name}/  — {what}")).small().weak());
            }
            ui.horizontal(|ui| {
                if ui.button("Save folders").clicked() {
                    self.save_settings();
                }
                if ui
                    .button("Run setup again")
                    .on_hover_text("Re-open the first-run setup with your current settings filled in.")
                    .clicked()
                {
                    self.wizard_returning = false;
                    self.show_wizard = true;
                }
                if ui
                    .button("Create folders")
                    .on_hover_text("Make the output folder and the source tree so you can drop files in.")
                    .clicked()
                {
                    self.save_settings();
                    match self.settings.ensure_dirs() {
                        Ok(made) => {
                            self.status = format!("Ready: {} folder(s) under {} and {}",
                                made.len(), self.output_dir, self.sources_dir);
                        }
                        Err(e) => self.status = format!("Could not create folders: {e:#}"),
                    }
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Source locations & tools");
            path_row_from(
                ui,
                "MacPack folder:",
                &mut self.macpack_dir,
                Pick::Folder,
                self.settings.source_subdir("MacPack"),
            );
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
        ui.group(|ui| {
            ui.strong("Templates (base OS images)");
            ui.label(
                egui::RichText::new(
                    "The bootable System disk each Target builds on, keyed by OS (\"7.1\"). \
                     A Target's base OS resolves against this list — without one, a build \
                     can't start. Entries you add here are saved to ~/.macatrium.json, so \
                     they work regardless of where the app is installed.",
                )
                .small()
                .weak(),
            );
            let reg = templates::Registry::load_default();
            if reg.0.is_empty() {
                ui.label(
                    egui::RichText::new("No templates configured yet — add one below.")
                        .small()
                        .weak(),
                );
            }
            let user_keys: Vec<String> = self.settings.templates.keys().cloned().collect();
            for (k, t) in &reg.0 {
                ui.horizontal(|ui| {
                    let is_user = user_keys.iter().any(|u| u == k);
                    if ui.button("Edit").clicked() {
                        self.load_template_into_editor(k);
                    }
                    ui.add_enabled(is_user, egui::Button::new("✖"))
                        .on_hover_text("Remove this template (yours only — a repo file entry is left alone)")
                        .clicked()
                        .then(|| self.remove_template(k));
                    ui.label(format!("{k}{}", if is_user { " (user)" } else { "" }));
                    let shown =
                        if t.label.is_empty() { t.hda.display().to_string() } else { t.label.clone() };
                    ui.label(egui::RichText::new(format!("— {shown}")).small().weak())
                        .on_hover_text(t.hda.display().to_string());
                    if !t.hda.is_file() {
                        ui.label(
                            egui::RichText::new("⚠ file not found")
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        )
                        .on_hover_text(t.hda.display().to_string());
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("Add / update a template:").small());
            // The key is not free text in practice: a Target resolves its
            // `base_os` against this registry by exact string, so "System 7.1"
            // instead of "7.1" silently yields "unknown base_os" at build time.
            // List the keys the Targets ask for and let the user click one.
            {
                let have: Vec<String> = templates::Registry::load_default().keys();
                let mut want: Vec<String> = self
                    .target_reg
                    .names()
                    .into_iter()
                    .filter_map(|n| self.target_reg.get(&n).map(|t| t.base_os.clone()))
                    .collect();
                want.sort();
                want.dedup();
                let missing: Vec<String> =
                    want.into_iter().filter(|k| !have.contains(k)).collect();
                if !missing.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("Targets still need a base disk for:")
                                .small()
                                .weak(),
                        );
                        for k in &missing {
                            if ui
                                .small_button(k)
                                .on_hover_text("Fill this OS key into the editor below.")
                                .clicked()
                            {
                                self.tpl_key = k.clone();
                            }
                        }
                    });
                }
            }
            egui::Grid::new("template_editor").num_columns(2).show(ui, |ui| {
                ui.label("OS key:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.tpl_key)
                        .hint_text("7.1")
                        .desired_width(120.0),
                );
                ui.end_row();
                ui.label("base .hda:");
                path_row_inline(
                    ui,
                    &mut self.tpl_hda,
                    Pick::File,
                    Some(self.settings.source_subdir("Templates")),
                );
                ui.end_row();
                ui.label("label:");
                ui.add(egui::TextEdit::singleline(&mut self.tpl_label).desired_width(300.0));
                ui.end_row();
                ui.label("launcher:");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.tpl_finder_replace, "install AS the Finder (System 6)")
                        .on_hover_text(
                            "System 6 has no Startup Items — the launcher replaces the Finder. \
                             Leave off for System 7+.",
                        );
                });
                ui.end_row();
                ui.label("Startup Items:");
                ui.add_enabled(
                    !self.tpl_finder_replace,
                    egui::TextEdit::singleline(&mut self.tpl_startup).desired_width(300.0),
                );
                ui.end_row();
            });
            if ui.button("Save template").clicked() {
                self.save_template_from_editor();
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Donors (source disk images)");
            ui.label(
                egui::RichText::new(
                    "Where a title's files are copied from. The dataset references a donor \
                     by key, so only this list holds machine paths — saved to ~/.macatrium.json.",
                )
                .small()
                .weak(),
            );
            let reg = atrium::donors::Registry::load_default();
            if reg.0.is_empty() {
                ui.label(egui::RichText::new("No donors configured yet.").small().weak());
            }
            // Which keys does the library actually ask for? Without this the key
            // is a guess, and getting it wrong builds a disk with no games on it.
            {
                self.ensure_library();
                let mut needed: Vec<&str> = self
                    .library
                    .iter()
                    .filter_map(|r| match &r.src {
                        Src::Donor(k) => Some(k.as_str()),
                        _ => None,
                    })
                    .collect();
                needed.sort();
                needed.dedup();
                let missing: Vec<&str> =
                    needed.iter().copied().filter(|k| reg.get(k).is_none()).collect();
                if !missing.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new("The library sources titles from:").small().weak(),
                        );
                        for k in &missing {
                            if ui
                                .small_button(*k)
                                .on_hover_text("Fill this donor key into the editor below.")
                                .clicked()
                            {
                                self.dn_key = (*k).to_string();
                            }
                        }
                        ui.label(
                            egui::RichText::new("— not configured yet").small().weak(),
                        );
                    });
                }
            }
            let user_keys: Vec<String> = self.settings.donors.keys().cloned().collect();
            for (k, d) in &reg.0 {
                ui.horizontal(|ui| {
                    let is_user = user_keys.iter().any(|u| u == k);
                    if ui.button("Edit").clicked() {
                        self.load_donor_into_editor(k);
                    }
                    ui.add_enabled(is_user, egui::Button::new("✖"))
                        .on_hover_text("Remove this donor (yours only)")
                        .clicked()
                        .then(|| self.remove_donor(k));
                    ui.label(format!("{k}{}", if is_user { " (user)" } else { "" }));
                    ui.label(
                        egui::RichText::new(format!(
                            "— {}{}",
                            d.path().display(),
                            if d.reservoir() { " · reservoir" } else { " · harvest" }
                        ))
                        .small()
                        .weak(),
                    );
                    if !d.path().is_file() {
                        ui.label(
                            egui::RichText::new("⚠ file not found")
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        );
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("Add / update a donor:").small());
            egui::Grid::new("donor_editor").num_columns(2).show(ui, |ui| {
                ui.label("key:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.dn_key)
                        .hint_text("macgarden")
                        .desired_width(160.0),
                );
                ui.end_row();
                ui.label("image:");
                path_row_inline(
                    ui,
                    &mut self.dn_path,
                    Pick::File,
                    Some(self.settings.source_subdir("Donors")),
                );
                ui.end_row();
                ui.label("kind:");
                ui.checkbox(&mut self.dn_reservoir, "reservoir (copy folders verbatim)")
                    .on_hover_text(
                        "On: already-installed content is copied as-is. Off: a MacPack-style \
                         harvest donor — the tool re-picks the launch app and renames the \
                         folder to it, which would override curated app paths.",
                    );
                ui.end_row();
            });
            if ui.button("Save donor").clicked() {
                self.save_donor_from_editor();
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.strong("Collections folder");
            ui.label(
                egui::RichText::new(
                    "Where the collections YOU save are written. Blank = \
                     <Documents>/MacAtrium/Collections. The lists shipped with the app are \
                     read separately and are never modified — saving a change to one writes \
                     your own copy here, which then takes precedence.",
                )
                .small()
                .weak(),
            );
            // Show what's actually in effect, and where the shipped lists came
            // from — the two answers you need when a name resolves unexpectedly.
            ui.label(
                egui::RichText::new(format!(
                    "in effect: {}",
                    atrium::collections::user_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(none)".into())
                ))
                .small()
                .weak(),
            );
            let shipped = atrium::collections::bundled_dirs();
            ui.label(
                egui::RichText::new(if shipped.is_empty() {
                    "shipped lists: none found".to_string()
                } else {
                    format!(
                        "shipped lists: {}",
                        shipped.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
                    )
                })
                .small()
                .weak(),
            );
            let mut dir = self
                .settings
                .collections_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            path_row(ui, "collections:", &mut dir, Pick::Folder);
            if ui.button("Save collections folder").clicked() {
                self.settings.collections_dir = opt_path(&dir);
                let path = settings::default_path();
                match self.settings.save(&path) {
                    Ok(()) => {
                        self.reload_collections();
                        self.status = format!("Saved collections folder -> {}", path.display());
                    }
                    Err(e) => self.status = format!("Save failed: {e}"),
                }
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

    fn load_template_into_editor(&mut self, key: &str) {
        if let Some(t) = templates::Registry::load_default().get(key) {
            self.tpl_key = key.to_string();
            self.tpl_hda = t.hda.display().to_string();
            self.tpl_label = t.label.clone();
            self.tpl_finder_replace = t.finder_replace;
            self.tpl_startup = t.startup_items.clone();
        }
    }

    /// Upsert a base-OS template into `~/.macatrium.json`. This is what makes a
    /// Target's `base_os` resolvable on a machine with no repo checkout.
    fn save_template_from_editor(&mut self) {
        let key = self.tpl_key.trim();
        if key.is_empty() || self.tpl_hda.trim().is_empty() {
            self.status = "A template needs an OS key (e.g. \"7.1\") and a base .hda.".into();
            return;
        }
        let hda = PathBuf::from(self.tpl_hda.trim());
        if !hda.exists() {
            // Not fatal — the disk may live on removable media — but a silent
            // "unknown base_os" at build time is worse than a nudge now.
            self.status = format!("Note: {} doesn't exist right now. ", hda.display());
        }
        let t = templates::Template {
            hda,
            label: self.tpl_label.trim().to_string(),
            finder_replace: self.tpl_finder_replace,
            startup_items: {
                let s = self.tpl_startup.trim();
                if s.is_empty() { "/System Folder/Startup Items".to_string() } else { s.to_string() }
            },
        };
        self.settings.templates.insert(key.to_string(), t);
        let path = settings::default_path();
        match self.settings.save(&path) {
            Ok(()) => {
                self.templates = templates::Registry::load_default().keys();
                self.status =
                    format!("{}Saved template \"{key}\" -> {}", self.status, path.display());
            }
            Err(e) => self.status = format!("Save template failed: {e}"),
        }
    }

    fn remove_template(&mut self, key: &str) {
        if self.settings.templates.remove(key).is_some() {
            let path = settings::default_path();
            match self.settings.save(&path) {
                Ok(()) => {
                    self.templates = templates::Registry::load_default().keys();
                    self.status = format!("Removed template \"{key}\".");
                }
                Err(e) => self.status = format!("Remove template failed: {e}"),
            }
        }
    }

    fn load_donor_into_editor(&mut self, key: &str) {
        if let Some(d) = atrium::donors::Registry::load_default().get(key) {
            self.dn_key = key.to_string();
            self.dn_path = d.path().display().to_string();
            self.dn_reservoir = d.reservoir();
        }
    }

    /// Upsert a donor image into `~/.macatrium.json`. `reservoir` matters: a
    /// reservoir donor's folders are copied **verbatim**, while a harvest donor is
    /// re-scanned for a launchable `APPL` and its folder renamed to it — wrong for
    /// already-installed content.
    fn save_donor_from_editor(&mut self) {
        let key = self.dn_key.trim().to_string();
        let key = key.as_str();
        if key.is_empty() || self.dn_path.trim().is_empty() {
            self.status = "A donor needs a key and an image path.".into();
            return;
        }
        let d = if self.dn_reservoir {
            atrium::donors::Donor::Full {
                path: PathBuf::from(self.dn_path.trim()),
                reservoir: true,
            }
        } else {
            atrium::donors::Donor::Path(PathBuf::from(self.dn_path.trim()))
        };
        self.settings.donors.insert(key.to_string(), d);
        let path = settings::default_path();
        match self.settings.save(&path) {
            Ok(()) => {
                self.invalidate_src_check(); // a new donor can source more titles
                self.status = format!("Saved donor \"{key}\" -> {}", path.display());
            }
            Err(e) => self.status = format!("Save donor failed: {e}"),
        }
    }

    fn remove_donor(&mut self, key: &str) {
        if self.settings.donors.remove(key).is_some() {
            let path = settings::default_path();
            match self.settings.save(&path) {
                Ok(()) => {
                    self.invalidate_src_check();
                    self.status = format!("Removed donor \"{key}\".");
                }
                Err(e) => self.status = format!("Remove donor failed: {e}"),
            }
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
    /// Either exit records [`WIZARD_REV`], so this is the last time it appears
    /// unless a later version needs a new answer.
    fn wizard(&mut self, ui: &mut egui::Ui) {
        if self.wizard_returning {
            ui.label(
                "Setup needs a couple of things it didn't ask for last time. Your existing \
                 settings are filled in below - check them over and continue.",
            );
        } else {
            ui.label(
                "Welcome! Pick where MacAtrium should keep your files, and point it at your tools.",
            );
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "Two folders do most of the work: finished disks are written to Output, and \
                 everything you build FROM — base OS images, donor disks, your own .mar \
                 captures, MacPack — lives under Sources. The defaults are fine; change them \
                 if you keep this material on another drive.",
            )
            .small()
            .weak(),
        );
        path_row(ui, "Output (built disks):", &mut self.output_dir, Pick::Folder);
        path_row(ui, "Sources:", &mut self.sources_dir, Pick::Folder);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("rb-cli:");
            ui.add(egui::TextEdit::singleline(&mut self.rb_cli).desired_width(300.0));
            if ui.button("Detect").clicked() {
                match detect_rb_cli() {
                    Some(p) => { self.rb_cli = p.clone(); self.status = format!("Found rb-cli: {p}"); }
                    None => self.status = format!("No {RB_CLI_EXE} found — install rusty-backup, or type the path."),
                }
            }
        });
        path_row_from(ui, "MacPack folder:", &mut self.macpack_dir, Pick::Folder,
                      self.settings.source_subdir("MacPack"));
        path_row(ui, "MG-Archive (optional):", &mut self.mg_archive, Pick::Folder);
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(self.status.as_str()).small().weak());
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "Setup rev {WIZARD_REV} \u{b7} MacAtrium Manager {}",
                env!("CARGO_PKG_VERSION")
            ))
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Create folders & continue").clicked() {
                self.finish_wizard();
                // Make the tree now so there's somewhere obvious to drop sources.
                match self.settings.ensure_dirs() {
                    Ok(made) => self.status = format!("Created {} folder(s).", made.len()),
                    Err(e) => self.status = format!("Could not create folders: {e:#}"),
                }
                self.out_image = default_out_image(&self.settings);
            }
            if ui
                .button("Skip for now")
                .on_hover_text(
                    "Closes setup for good. Re-open it any time from Settings; the Build screen \
                     will tell you what's still missing.",
                )
                .clicked()
            {
                self.finish_wizard();
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
            ui.selectable_value(&mut self.tab, Tab::EditDisk, "Edit disk");
            ui.selectable_value(&mut self.tab, Tab::Library, "Library");
            ui.selectable_value(&mut self.tab, Tab::Database, "Database");
            ui.selectable_value(&mut self.tab, Tab::Attain, "Attain");
            ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Settings");
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                if busy {
                    ui.spinner();
                }
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.log_lines.is_empty() {
                        let label = if self.log_open { "Hide log" } else { "Show log" };
                        if ui.small_button(label).clicked() {
                            self.log_open = !self.log_open;
                        }
                    }
                    // Only a child job can be interrupted; an in-process one has
                    // no safe cancellation point, so don't offer a dead button.
                    if busy && self.job_child.is_some() && ui.small_button("Cancel").clicked() {
                        self.cancel_job();
                    }
                });
            });
            if self.log_open && !self.log_lines.is_empty() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Build log — {} line(s)", self.log_lines.len()))
                            .small()
                            .weak(),
                    );
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(self.log_lines.join("
"));
                    }
                    if ui.small_button("Save…").clicked() {
                        self.save_log();
                    }
                    if ui.small_button("Clear").clicked() {
                        self.log_lines.clear();
                    }
                });
                let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
                egui::ScrollArea::vertical()
                    .id_salt("build_log")
                    .max_height(190.0)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show_rows(ui, row_h, self.log_lines.len(), |ui, range| {
                        for i in range {
                            let line = &self.log_lines[i];
                            // The library's own convention: WARNING / failure lines
                            // are what a user actually needs to spot in a wall of
                            // progress output.
                            let hot = line.contains("WARNING")
                                || line.contains("failed")
                                || line.contains("skipped");
                            let txt = egui::RichText::new(line).monospace().small();
                            ui.label(if hot { txt.color(ui.visuals().warn_fg_color) } else { txt });
                        }
                    });
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::Build => self.tab_build(ui, &ctx, busy),
                    Tab::EditDisk => self.tab_edit_disk(ui, &ctx, busy),
                    Tab::Library => self.tab_library(ui, &ctx, busy),
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

    /// Whatever `detect_rb_cli` finds must be an **absolute** path. Returning the
    /// bare name (the old PATH branch did) re-opens the stale-rb-cli-on-PATH trap:
    /// a different rb-cli earlier on PATH silently wins and can write a corrupt
    /// catalog. Also asserts we never hand back a directory.
    #[test]
    fn detected_rb_cli_is_always_an_absolute_file() {
        if let Some(found) = detect_rb_cli() {
            let p = Path::new(&found);
            assert!(p.is_absolute(), "detect_rb_cli returned a relative path: {found}");
            assert!(p.is_file(), "detect_rb_cli returned a non-file: {found}");
            assert_ne!(found, "rb-cli", "the bare name must never be returned");
        }
    }

    /// Host-path defaults must be platform-native. `/tmp/macatrium.hda` was
    /// hard-coded, which is meaningless on Windows — and the paths that *do*
    /// legitimately look POSIX (startup items, apps root) are Mac **volume** paths
    /// inside the built disk, so they must stay `/`-separated on every host.
    #[test]
    fn host_defaults_are_native_but_mac_volume_paths_are_not() {
        let out = default_out_image(&Settings::default());
        assert!(
            Path::new(&out).is_absolute(),
            "default output disk must be an absolute host path: {out}"
        );
        // …and it must sit under the configured output folder, not Documents.
        let mut custom = Settings::default();
        custom.output_dir = Some(PathBuf::from("/mnt/disks/macatrium-out"));
        assert!(
            default_out_image(&custom).starts_with("/mnt/disks/macatrium-out"),
            "the configured output folder must win"
        );
        if cfg!(windows) {
            assert!(!out.starts_with('/'), "POSIX-style default leaked onto Windows: {out}");
        }
        // The in-disk paths are NOT host paths and must not be platform-ised.
        let a = App::default();
        assert_eq!(a.startup_items, "/System Folder/Startup Items");
        assert!(a.apps_root.starts_with('/'), "apps root is an HFS volume path");
    }

    /// Facet edits must never target a bare relative path. From an installed app
    /// that silently creates a `data/` folder next to the executable, so the edits
    /// are invisible to every later build.
    #[test]
    fn facet_overlay_path_is_never_a_bare_relative_guess() {
        let mut a = App::default();
        a.overrides.clear();
        if let Some(p) = a.facet_overlay_path() {
            assert!(
                p.is_absolute(),
                "facet overlay resolved to a relative path: {}",
                p.display()
            );
        }
        // An explicit Advanced override is honoured verbatim.
        a.overrides = "/tmp/my-compat.jsonl".into();
        assert_eq!(a.facet_overlay_path(), Some(PathBuf::from("/tmp/my-compat.jsonl")));
    }

    // "Add titles…" appends to the disk's contents and never double-adds one
    // that's already there (the modal shows those ticked + disabled, but a stale
    // tick set must not be able to duplicate a row either).
    #[test]
    fn adding_titles_appends_without_duplicates() {
        let mut a = App::default();
        a.work_ids = vec!["alpha".into()];
        let added = a.work_add(vec!["beta".to_string(), "alpha".to_string(), "gamma".to_string()]);
        assert_eq!(added, 2, "alpha was already on the disk");
        assert_eq!(a.work_ids, vec!["alpha".to_string(), "beta".into(), "gamma".into()]);
        assert!(a.work_dirty);
    }

    // Removing a title takes its Recommended flag with it — otherwise the saved
    // collection would surface a title the build never installs.
    #[test]
    fn removing_a_title_drops_its_recommended_flag() {
        let mut a = App::default();
        a.work_ids = vec!["alpha".into(), "beta".into()];
        a.work_rec = ["alpha".to_string(), "beta".to_string()].into_iter().collect();
        a.work_remove("alpha");
        assert_eq!(a.work_ids, vec!["beta".to_string()]);
        assert!(!a.work_rec.contains("alpha"));
        let c = a.work_collection("list");
        assert_eq!(c.recommended, vec!["beta".to_string()]);
    }

    // `recommended` is emitted in disk order and filtered to the contents, so the
    // saved list can't reference a title that isn't on the disk.
    #[test]
    fn work_recommended_is_a_subset_in_disk_order() {
        let mut a = App::default();
        a.work_ids = vec!["one".into(), "two".into(), "three".into()];
        a.work_rec = ["three".to_string(), "one".to_string(), "ghost".to_string()]
            .into_iter()
            .collect();
        let c = a.work_collection("list");
        assert_eq!(c.recommended, vec!["one".to_string(), "three".to_string()]);
    }

    // The build must be driven by the NAMED collection: `image::run` reads
    // `coll.recommended` to fill the launcher's Recommended category, so a config
    // carrying only an id list keeps just the taxonomy seeds and drops the list's
    // own Recommended. Regression guard — that's what every GUI build did before
    // the screens merged.
    #[test]
    fn building_a_list_names_the_collection() {
        let mut a = App::default();
        a.out_image = "/tmp/out.hda".into();
        a.work_name = "Smoke_Test_6".into();
        a.work_ids = vec!["apeiron".into(), "bolo".into()];
        let cfg = a.disk_config();
        assert_eq!(cfg.collection.as_deref(), Some("Smoke_Test_6"));
        match cfg.selection {
            Some(Selection::List { ids }) => assert_eq!(ids, vec!["apeiron".to_string(), "bolo".into()]),
            other => panic!("expected an id list, got {other:?}"),
        }

        // "Every compatible title" drops both the list and the collection.
        a.work_all = true;
        let cfg = a.disk_config();
        assert!(cfg.collection.is_none());
        assert!(matches!(cfg.selection, Some(Selection::All)));
    }

    // The quietest build failure there is: a title whose donor key isn't
    // configured is skipped by harvest_plan (warning on stderr, which a windowed
    // app never shows) and then dropped from the catalog by filter_present_apps —
    // so the build "succeeds" and hands back a bootable disk with nothing on it.
    // The preflight has to name the missing donor key BEFORE the build runs.
    #[test]
    fn an_unconfigured_donor_is_caught_before_the_build() {
        let mut a = App::default();
        a.library = vec![
            LibRow { id: "bolo".into(), src: Src::Donor("macgarden".into()), ..Default::default() },
            LibRow { id: "mine".into(), src: Src::Local, ..Default::default() },
            LibRow { id: "orphan".into(), src: Src::None, ..Default::default() },
        ];
        a.library_loaded = true;
        let ids = vec!["bolo".to_string(), "mine".into(), "orphan".into(), "ghost".into()];

        // Nothing configured: only the imported capture can be sourced.
        let empty = atrium::donors::Registry::default();
        let c = a.check_sources_in(&ids, &empty);
        assert_eq!(c.ok, 1, "the local capture needs no donor");
        assert!(c.missing_donors.contains("macgarden"));
        assert_eq!(c.unsourced, vec!["bolo".to_string(), "orphan".into()]);
        assert_eq!(c.unknown, vec!["ghost".to_string()], "an id not in the library");
        assert!(!c.is_clean());

        // Configure that donor and the title becomes sourceable.
        let mut reg = atrium::donors::Registry::default();
        reg.0.insert(
            "macgarden".into(),
            atrium::donors::Donor::Full { path: PathBuf::from("/d.hfv"), reservoir: true },
        );
        let c = a.check_sources_in(&ids, &reg);
        assert_eq!(c.ok, 2);
        assert!(c.missing_donors.is_empty());
    }

    // The MG downloader follows the disk's list; with no list it falls back to
    // the Add-to-disk ticks. (It used to read the ticks only — which the merged
    // Build screen no longer sets, so it was permanently empty.)
    #[test]
    fn download_targets_follow_the_disk_list() {
        let mut a = App::default();
        a.library = vec![
            LibRow { id: "bolo".into(), name: "Bolo".into(), ..Default::default() },
            LibRow { id: "chiral".into(), name: "Chiral".into(), ..Default::default() },
        ];
        a.library_loaded = true;
        a.work_ids = vec!["bolo".into()];
        assert_eq!(a.download_targets(), vec![("bolo".to_string(), "Bolo".to_string())]);

        // No build list: the titles staged for an existing disk are next in line.
        a.work_ids.clear();
        a.disk_add = vec!["chiral".into()];
        assert_eq!(a.download_targets(), vec![("chiral".to_string(), "Chiral".to_string())]);
    }

    // Setup shows once per revision, not once per launch. Both earlier rules
    // inferred "is this person set up?" from the settings, so Skip was never
    // remembered and the wizard reappeared every time the app started.
    #[test]
    fn setup_shows_once_per_revision() {
        let fresh = Settings::default();
        assert!(fresh.setup_seen.is_none(), "a fresh install has never seen setup");

        // The rule the constructor applies.
        let shows = |seen: Option<u32>| seen.unwrap_or(0) < WIZARD_REV;
        assert!(shows(None), "first launch prompts");
        assert!(!shows(Some(WIZARD_REV)), "dismissing it is remembered");
        assert!(!shows(Some(WIZARD_REV + 1)), "a downgrade doesn't re-prompt");
        if WIZARD_REV > 1 {
            assert!(shows(Some(WIZARD_REV - 1)), "an older revision is asked again");
        }
        // And "asked again" is the returning case, which explains itself.
        assert!(WIZARD_REV >= 1, "revision 0 would mean nobody is ever prompted");
    }

    // The marker has to survive the round trip to ~/.macatrium.json, and survive
    // saves the Settings screen makes later — otherwise pressing "Save folders"
    // after setup would bring the wizard back on the next launch.
    #[test]
    fn the_setup_marker_persists() {
        let mut a = App::default();
        a.settings.setup_seen = Some(WIZARD_REV);
        // save_settings() starts from a clone, so unedited fields ride along.
        let s = a.settings.clone();
        assert_eq!(s.setup_seen, Some(WIZARD_REV));
        // And skip_serializing_if must not eat it on the way to disk.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("setup_seen"), "not written to the file: {json}");
        assert_eq!(
            serde_json::from_str::<Settings>(&json).unwrap().setup_seen,
            Some(WIZARD_REV)
        );
        // A settings file written before this field existed reads as "never seen".
        assert!(serde_json::from_str::<Settings>("{}").unwrap().setup_seen.is_none());
    }

    // Staging an addition never re-adds what the disk already holds, and never
    // stages the same title twice — the modal can be reopened any number of times.
    #[test]
    fn staging_skips_what_the_disk_already_has() {
        let mut a = App::default();
        a.disk_ids = vec!["bolo".into()];
        assert_eq!(a.stage_disk_add(vec!["bolo".into(), "chiral".into()]), 1);
        assert_eq!(a.disk_add, vec!["chiral".to_string()]);
        // Same picks again: nothing new.
        assert_eq!(a.stage_disk_add(vec!["bolo".into(), "chiral".into()]), 0);
        assert_eq!(a.disk_add, vec!["chiral".to_string()]);
    }

    // Editing a disk must not drag the Build screen's collection along: with
    // --update-collection that would rewrite a saved list belonging to a
    // different disk. Only a list the user names is touched.
    #[test]
    fn editing_a_disk_only_syncs_a_list_you_name() {
        let mut a = App::default();
        a.work_name = "Someone Elses List".into(); // open on the Build screen
        a.work_ids = vec!["zzz".into()];
        let disk = PathBuf::from("/tmp/some.hda");
        let add = vec!["bolo".to_string()];

        let cfg = a.edit_config(&disk, &add);
        assert!(cfg.collection.is_none(), "no list is synced unless one is named");
        assert_eq!(cfg.out, disk);
        match cfg.selection {
            Some(Selection::List { ids }) => assert_eq!(ids, add),
            other => panic!("the staged additions are the selection, got {other:?}"),
        }

        a.disk_sync_name = "This Disk's List".into();
        assert_eq!(a.edit_config(&disk, &add).collection.as_deref(), Some("This Disk's List"));
    }

    // The log keeps the tail: a failure is at the end of the output, so an
    // overlong log must drop its head rather than stop recording.
    #[test]
    fn the_log_keeps_the_end_not_the_beginning() {
        let mut a = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..App::LOG_MAX + 500 {
            tx.send(format!("line {i}")).unwrap();
        }
        drop(tx);
        a.log_rx = Some(rx);
        a.drain_log();
        assert_eq!(a.log_lines.len(), App::LOG_MAX);
        assert_eq!(a.log_lines.last().unwrap(), &format!("line {}", App::LOG_MAX + 499));
        assert!(a.log_rx.is_none(), "a closed channel is dropped");
    }

    // Whatever CLI we find must be an absolute file — the same rule as rb-cli:
    // a bare name would let a different `atrium` earlier on PATH run the build.
    #[test]
    fn a_found_atrium_cli_is_always_an_absolute_file() {
        if let Some(p) = find_atrium_exe() {
            assert!(p.is_absolute(), "not absolute: {}", p.display());
            assert!(p.is_file(), "not a file: {}", p.display());
        }
    }

    // A base disk that isn't on disk is caught up front, not as a raw copy error
    // several seconds into the build.
    #[test]
    fn a_missing_base_image_is_refused_up_front() {
        let mut a = App::default();
        a.base_os.clear();
        a.base_system = "/nope/not-a-real-base.hda".into();
        let msg = a.missing_base_image().expect("a missing custom base is reported");
        assert!(msg.contains("not-a-real-base.hda"), "names the file: {msg}");
    }

    // An unnamed list still saves under something meaningful: the output disk's
    // stem, so the list and the disk it builds share a name.
    #[test]
    fn unnamed_list_takes_the_disks_name() {
        let mut a = App::default();
        a.out_image = "/home/me/MacAtrium/Images/Colour_Games.hda".into();
        assert_eq!(a.work_save_name(), "Colour_Games");
        a.work_name = "  Chosen  ".into();
        assert_eq!(a.work_save_name(), "Chosen");
    }

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
        a.strip_quicktime = true;
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
        assert!(b.strip_quicktime);
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
    fn a_built_disk_carries_its_list_into_the_config() {
        let mut a = App::default();
        a.work_ids = vec!["x".into(), "y".into()];
        a.work_name = "Mine".into();
        let cfg = a.disk_config();
        assert_eq!(cfg.collection.as_deref(), Some("Mine"));
        match cfg.selection {
            Some(Selection::List { ids }) => assert_eq!(ids, vec!["x".to_string(), "y".into()]),
            other => panic!("expected an explicit id list, got {other:?}"),
        }
        // "Every compatible title" drops the list and the collection name.
        a.work_all = true;
        let cfg = a.disk_config();
        assert!(matches!(cfg.selection, Some(Selection::All)));
        assert!(cfg.collection.is_none());
    }

    // OS-migration scrub: titles the target OS can't run leave the disk's list.
    #[test]
    fn migration_scrub_drops_out_of_range() {
        let mut a = App::default();
        a.base_os = "7.5".into(); // migrating to System 7.5
        a.work_ids = vec!["old".into(), "any".into(), "imported".into()];
        a.work_rec = ["old".to_string(), "any".to_string()].into_iter().collect();
        a.library = vec![
            // playable only up to 7.1 -> dropped on 7.5
            LibRow { id: "old".into(), name: "Old".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: Some("7.1".into()), color: false, mouse: true, hotkey: String::new(), src: Src::None, dirty: false },
            // open OS range -> kept
            LibRow { id: "any".into(), name: "Any".into(), kind: "game".into(), year: String::new(), genres: vec![], min_os: None, max_os: None, color: false, mouse: true, hotkey: String::new(), src: Src::None, dirty: false },
        ];
        a.library_loaded = true;
        a.scrub_incompatible();
        // "imported" isn't in this library at all — kept, so the build reports it
        // rather than the scrub silently eating a capture.
        assert_eq!(a.work_ids, vec!["any".to_string(), "imported".into()]);
        assert!(!a.work_rec.contains("old"), "a scrubbed title can't stay Recommended");
        assert!(a.work_rec.contains("any"));
        assert!(a.work_dirty);
    }
}
