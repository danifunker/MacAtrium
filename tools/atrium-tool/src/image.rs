//! `atrium image` — the one-command bootable build (docs/13 Priority 1).
//!
//! Config-driven orchestrator that composes the verified verbs into a bootable
//! appliance `.hda`, retiring the bash `assemble.sh`:
//!
//!   base system → copy → `harvest` apps (donor disks) → `enrich` (LaunchBox)
//!   → `merge` manual overrides → optional art (`pict`) → `catalog` → install
//!   launcher.
//!
//! It works on a throwaway copy of the dataset, so a build never mutates the
//! curated `data/library.jsonl`. Run with `atrium image --config build.json`.

use crate::rbcli::RbCli;
use crate::{catalog, enrich, harvest, icons, merge, mg, pict, snd};
use anyhow::{Context, Result};
use crate::config::BuildConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Set the Finder `hasBundle` flag (and clear `hasBeenInited`) in a MacBinary
/// header, so the Finder reads the app's `BNDL` and shows its real icon instead of
/// the generic one. fdFlags' high byte is at MacBinary offset 73; hasBundle = 0x20,
/// hasBeenInited = 0x01. The launcher already ships `ICN#`/`icl8`/`BNDL`/`FREF` —
/// only this flag was missing (so a normal-app deploy showed the generic icon).
fn set_bundle_bit(bytes: &mut [u8]) {
    if bytes.len() > 73 {
        bytes[73] = (bytes[73] | 0x20) & !0x01;
    }
}

/// Patch the launcher's `'SIZE'` (-1) memory partition for this build, if the
/// config asks for one (`app_mem_kb`). A failure to find/patch the resource only
/// warns — the launcher keeps its built-in 2 MB / 1 MB rather than failing the
/// build. Both install paths call this on the launcher bytes before injection.
fn apply_app_mem(cfg: &BuildConfig, bytes: &mut [u8]) {
    let Some((pref_kb, min_kb)) = cfg.effective_app_mem() else { return };
    match crate::size_rsrc::patch_app_mem(bytes, pref_kb * 1024, min_kb * 1024) {
        Ok((old_p, old_m)) => eprintln!(
            "[size] launcher partition -> {pref_kb} KB / {min_kb} KB (was {} KB / {} KB)",
            old_p / 1024,
            old_m / 1024
        ),
        Err(e) => eprintln!("[size] WARNING: keeping launcher default ({e:#})"),
    }
}

/// Install the launcher *as* `/System Folder/Finder` (typed FNDR/MACS) so a
/// System-6 boot launches it as the shell.
fn install_as_finder(rb: &RbCli, cfg: &BuildConfig, stage: &Path) -> Result<()> {
    install_as_finder_in(rb, cfg, stage, "/System Folder")
}

/// Install the launcher *as* `<sysfolder>/Finder` (typed FNDR/MACS) so booting that
/// System launches it as the shell. Patches the MacBinary internal name to "Finder",
/// injects it (overwriting the real Finder), and retypes it. Parameterised on the
/// System Folder so the multi-System path can finder-replace each System 6 folder.
fn install_as_finder_in(rb: &RbCli, cfg: &BuildConfig, stage: &Path, sysfolder: &str) -> Result<()> {
    let mut bytes = cfg.launcher_bytes()?;
    anyhow::ensure!(bytes.len() > 128, "launcher .bin too small to be MacBinary");
    let name = b"Finder";
    bytes[1] = name.len() as u8;            // MacBinary filename length
    for k in 0..63 {                        // filename field (63 bytes)
        bytes[2 + k] = if k < name.len() { name[k] } else { 0 };
    }
    set_bundle_bit(&mut bytes);
    apply_app_mem(cfg, &mut bytes);
    let patched = stage.join("Finder.bin");
    std::fs::write(&patched, &bytes)?;
    rb.put_macbinary(&cfg.out, &patched, sysfolder)?;   // lands as "Finder" (--force)
    rb.chmeta(&cfg.out, &format!("{sysfolder}/Finder"), "FNDR", "MACS")?;
    Ok(())
}

/// Put the launcher (bundle-bit + `SIZE` patched, browsable-app icon) into a System
/// Folder's `Startup Items` so a System-7+ boot auto-launches it while the real
/// Finder stays installed. `startup_dir` is the full `.../Startup Items` path.
fn install_startup_item(rb: &RbCli, cfg: &BuildConfig, stage: &Path, startup_dir: &str) -> Result<()> {
    rb.mkdir_p(&cfg.out, startup_dir)?;
    // Set the bundle bit so the Finder shows the app's real icon (it's a browsable
    // app here, unlike the finder_replace appliance).
    let mut bytes = cfg.launcher_bytes()?;
    anyhow::ensure!(bytes.len() > 128, "launcher .bin too small to be MacBinary");
    set_bundle_bit(&mut bytes);
    apply_app_mem(cfg, &mut bytes);
    let patched = stage.join("MacAtrium.bundle.bin");
    std::fs::write(&patched, &bytes)?;
    rb.put_macbinary(&cfg.out, &patched, startup_dir)?;
    Ok(())
}

/// Strip appliance-inappropriate quick-launch control panels (the Mac OS 8 Control
/// Strip + Launcher) from `<sysfolder>/Control Panels` so they don't float over the
/// full-screen launcher. Case-insensitive; no-op where absent (System 6 / 7.1 don't
/// ship them). docs/36.
fn strip_control_panels_in(rb: &RbCli, cfg: &BuildConfig, sysfolder: &str) -> Result<()> {
    if cfg.disable_control_panels.is_empty() {
        return Ok(());
    }
    let cp_dir = format!("{sysfolder}/Control Panels");
    if let Ok(entries) = rb.ls_exact(&cfg.out, &cp_dir) {
        for want in &cfg.disable_control_panels {
            if let Some(e) = entries.iter().find(|e| e.name.eq_ignore_ascii_case(want)) {
                rb.rm(&cfg.out, &format!("{cp_dir}/{}", e.name))?;
                eprintln!("[appliance]   disabled Control Panel: {} ({sysfolder})", e.name);
            }
        }
    }
    Ok(())
}

/// A folder name that denotes an *installable* System 6 (6.0.4–6.0.8, at or above
/// MacAtrium's 6.0.4 Gestalt floor). Used only when a System Folder has no `Startup
/// Items`, to tell System 6 (finder-replace) from pre-6 System 4/5 (skip).
fn is_system6_folder_name(name: &str) -> bool {
    ["6.0.4", "6.0.5", "6.0.6", "6.0.7", "6.0.8"]
        .iter()
        .any(|v| name.contains(v))
}

/// Install the launcher into **every** System Folder on the volume so a bless-swap
/// between Systems always boots back into MacAtrium (docs/36 Phase 2): Startup Items
/// for System 7+ (folders that have a `Startup Items`), as the Finder for System
/// 6.0.x (named folders with no Startup Items). Pre-6 (System 4/5) and unrecognised
/// no-Startup-Items folders are logged and skipped. Returns the install count.
fn install_into_all_systems(rb: &RbCli, cfg: &BuildConfig, stage: &Path) -> Result<usize> {
    let roots = rb.ls(&cfg.out, "/")?;
    let mut n = 0usize;
    for e in roots.iter().filter(|e| e.is_dir) {
        let folder = format!("/{}", e.name);
        if !rb.exists(&cfg.out, &format!("{folder}/System")) {
            continue; // not a System Folder (no `System` file)
        }
        if rb.exists(&cfg.out, &format!("{folder}/Startup Items")) {
            install_startup_item(rb, cfg, stage, &format!("{folder}/Startup Items"))?;
            strip_control_panels_in(rb, cfg, &folder)?;
            eprintln!("[all-systems] {}: Startup Items", e.name);
            n += 1;
        } else if is_system6_folder_name(&e.name) {
            install_as_finder_in(rb, cfg, stage, &folder)?;
            eprintln!("[all-systems] {}: as Finder (System 6)", e.name);
            n += 1;
        } else {
            eprintln!(
                "[all-systems] {}: skipped (System file but no Startup Items and not a System 6.0.x name)",
                e.name
            );
        }
    }
    anyhow::ensure!(
        n > 0,
        "install_all_systems: found no installable System Folders on {}",
        cfg.out.display()
    );
    Ok(n)
}

/// Standalone: run the all-systems install on an existing disk image (no rebuild) —
/// retrofit a hand-assembled multi-System disk so a bless-swap always boots back
/// into MacAtrium. Loads machine settings for the rb-cli path; `launcher` overrides
/// the default `build/MacAtrium.bin`. Returns the number of System Folders installed.
pub fn install_all_systems_on_image(image: &Path, launcher: Option<PathBuf>) -> Result<usize> {
    let settings = crate::settings::Settings::load(&crate::settings::default_path());
    let mut cfg = BuildConfig::default();
    cfg.out = image.to_path_buf();
    cfg.launcher = launcher;
    let rb_bin = crate::rbcli::resolve_bin(&cfg.rb_cli, settings.rb_cli.as_deref());
    let rb = RbCli::new(&rb_bin);
    let stage = std::env::temp_dir().join("atrium-install-all");
    std::fs::create_dir_all(&stage)?;
    install_into_all_systems(&rb, &cfg, &stage)
}

/// A System Folder needs a dependency when any of the dep's `install_os` version
/// strings is a **substring** of the folder's name — e.g. "7.1" matches "System
/// Folder 7.1" (and 7.1.x) but neither "System Folder 6.0.8" nor "System Folder
/// 7.5.5". An empty `install_os` never matches: it's a deliberate no-op (the dep
/// isn't staged, or no OS on this disk needs it).
fn dep_installs_on(install_os: &[String], folder_name: &str) -> bool {
    install_os.iter().any(|v| folder_name.contains(v.as_str()))
}

/// The distinct set of `requires:[<dep-id>]` across every record in a dataset file.
/// `requires` is an optional compatibility facet read **generically** (records flow
/// through this pipeline as `serde_json::Value`, like `id`/`app` in
/// [`dataset_records`]/[`filter_present_apps`]) — a title lists the runtime
/// Extensions its OS needs but doesn't bundle. Sorted, for a stable build log.
fn required_dep_ids(dataset: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(dataset)?;
    let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if let Some(arr) = v.get("requires").and_then(Value::as_array) {
                for dep in arr.iter().filter_map(Value::as_str) {
                    let dep = dep.trim();
                    if !dep.is_empty() {
                        ids.insert(dep.to_string());
                    }
                }
            }
        }
    }
    Ok(ids.into_iter().collect())
}

/// Install each required runtime dependency into the System Folders that need it —
/// run right after the launcher's all-systems install so every bootable System is
/// covered. For each dep-id in the union of the installed titles' `requires`, and
/// each System Folder whose name matches the dep's `install_os`, the dep's files are
/// copied **verbatim** from its reservoir donor into `<folder>/Extensions` — the
/// same image-to-image `rb-cli cp` reservoir games use. System Folders are found
/// exactly as [`install_into_all_systems`] finds them (a root dir holding a `System`
/// file), and donor keys resolve through the same [`resolve_donor`] the reservoir
/// game-copy uses.
///
/// Fail-soft throughout: an empty union is a quiet no-op; an unknown dep-id, an
/// unresolvable donor, or a copy failure each warn and continue — a missing
/// Extension must never abort an otherwise-good build.
fn install_dependencies(rb: &RbCli, cfg: &BuildConfig, present_dataset: &Path) -> Result<()> {
    // (a) union of required dep-ids across the installed titles; (b) quiet no-op.
    let want = required_dep_ids(present_dataset)?;
    if want.is_empty() {
        return Ok(());
    }
    // Registry (bundled ⊕ user) + the SAME donor resolution the reservoir game-copy
    // uses (donors.json first, else a filename under the MacPack folder).
    let registry = crate::config::dependencies();
    let donors = crate::donors::Registry::load_default();
    let macpack = crate::settings::Settings::load_default().macpack_dir;

    // (c) iterate System Folders exactly like install_into_all_systems: a root dir
    // is a System Folder iff it holds a `System` file.
    let roots = rb.ls(&cfg.out, "/")?;
    for e in roots.iter().filter(|e| e.is_dir) {
        let folder = format!("/{}", e.name);
        if !rb.exists(&cfg.out, &format!("{folder}/System")) {
            continue;
        }
        for dep_id in &want {
            let Some(dep) = registry.get(dep_id) else {
                eprintln!(
                    "[deps] WARNING: unknown dependency {dep_id:?} (declared in a title's `requires`) — skipping"
                );
                continue;
            };
            // Empty install_os (no-op) or a folder that doesn't match → skip silently.
            if !dep_installs_on(&dep.install_os, &e.name) {
                continue;
            }
            // Already satisfied? The base System (or an earlier step) may already ship
            // this dep — often a newer build (the QuickTime base bundles a modern Sound
            // Manager). Never downgrade: if a `satisfied_if` file is present here, skip.
            if let Some(hit) = dep
                .satisfied_if
                .iter()
                .find(|rel| rb.exists(&cfg.out, &format!("{folder}/{rel}")))
            {
                eprintln!(
                    "[deps]    {} already satisfied in {} ({hit} present) — skipping",
                    dep.name, e.name
                );
                continue;
            }
            // Resolve the donor key → disk-image path (fail-soft on an unknown donor).
            let Some((donor_img, _reservoir)) =
                crate::selection::resolve_donor(&dep.source.donor, &donors, macpack.as_deref())
            else {
                eprintln!(
                    "[deps] WARNING: {}: donor {:?} did not resolve (donors.json / MacPack dir) — skipping",
                    dep.name, dep.source.donor
                );
                continue;
            };
            // Copy the dep's files verbatim into this System Folder's Extensions.
            let ext_dir = format!("{folder}/Extensions");
            if let Err(err) = rb.mkdir_p(&cfg.out, &ext_dir) {
                eprintln!("[deps] WARNING: {}: could not create {ext_dir}: {err:#} — skipping", dep.name);
                continue;
            }
            let src = format!("{}/*", dep.source.path.trim_end_matches('/'));
            match rb.cp(&donor_img, &src, &cfg.out, &ext_dir) {
                Ok(()) => eprintln!("[deps]    {} -> {ext_dir}", dep.name),
                Err(err) => eprintln!(
                    "[deps] WARNING: {}: copy {} -> {ext_dir} failed: {err:#} — skipping",
                    dep.name,
                    donor_img.display()
                ),
            }
        }
    }
    Ok(())
}

/// (id, app-path) for every dataset record with an id. `app` is the launcher
/// path relative to /MacAtrium (e.g. "Apps/Foo/Foo"), used for icon harvest.
fn dataset_records(path: &Path) -> Result<Vec<(String, Option<String>)>> {
    let text = std::fs::read_to_string(path)?;
    let mut recs = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if let Some(id) = v.get("id").and_then(Value::as_str) {
                let app = v.get("app").and_then(Value::as_str).map(str::to_string);
                recs.push((id.to_string(), app));
            }
        }
    }
    Ok(recs)
}

/// Keep only dataset records whose `app` file is actually present on the volume,
/// writing the survivors to `dst`. The dataset is the full curated library, but
/// a build only harvests a subset of those titles — listing the rest in the
/// catalog means selecting one launches nothing and the launcher reports a
/// File-System error -43 (fnfErr). Filtering here keeps the on-screen catalog in
/// lockstep with what's on disk. Records with no `app` are left in place (the
/// catalog generator requires `app` and skips them anyway). Returns the display
/// names of the dropped titles, for the build log.
fn filter_present_apps(rb: &RbCli, image: &Path, src: &Path, dst: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(src)?;
    let mut out = String::new();
    let mut dropped = Vec::new();
    // De-dup by install path: the curated library can hold two records for one game
    // (e.g. `dark-castle` + `dark-castle-1-2`) that resolve to the same installed
    // app — keep the first so the catalog doesn't list it twice.
    let mut seen_apps: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in text.lines() {
        let t = line.trim();
        let keep = if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            true
        } else if let Ok(v) = serde_json::from_str::<Value>(t) {
            match v.get("app").and_then(Value::as_str) {
                Some(a) if !a.is_empty() => {
                    let full = format!("/MacAtrium/{}", a.trim_start_matches('/'));
                    if !rb.exists(image, &full) {
                        dropped.push(
                            v.get("name").and_then(Value::as_str).unwrap_or(a).to_string(),
                        );
                        false
                    } else {
                        seen_apps.insert(a.to_string()) // false = a dup of a kept record
                    }
                }
                _ => true, // no app field — catalog::run will skip it
            }
        } else {
            true // unparseable line — leave it for catalog::run to handle
        };
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    std::fs::write(dst, out)?;
    Ok(dropped)
}

use crate::config::fs_id;

fn find_art(dir: &Path, id: &str) -> Option<PathBuf> {
    for ext in ["png", "jpg", "jpeg", "PNG", "JPG"] {
        let p = dir.join(format!("{id}.{ext}"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Harvest the app's own Finder icon from the injected app into icon variants
/// and return the catalog base path (`<images_rel>/<id>.icon`) the launcher
/// resolves to them, or `None`. Bakes the 1-bit `ICN#` (`<id>.icon.raw`, every
/// screen) and, when present, the 8-bit colour `icl8` as a small PICT
/// (`<id>.icon.8.pict`, picked on colour screens). The launcher draws this in
/// the list-row gutter and falls back to it for the big art pane when an item
/// has no box art. Never fails the build — a missing or odd app yields `None`.
fn bake_icon(
    rb: &RbCli,
    cfg: &BuildConfig,
    stage: &Path,
    images_rel: &str,
    id: &str,
    app: &str,
) -> Result<Option<String>> {
    let hqx = stage.join(format!("{id}.icon.hqx"));
    let src = format!("/MacAtrium/{}", app.trim_start_matches('/'));
    if rb.get_binhex(&cfg.out, &src, &hqx).is_err() {
        return Ok(None); // not extractable (e.g. path moved/aliased)
    }
    let hqx_bytes = match std::fs::read(&hqx) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    // 1-bit ICN# — required; without it there's no usable icon.
    let raw = match icons::app_icon_raw1(&hqx_bytes).ok().flatten() {
        Some(r) => r,
        None => return Ok(None),
    };
    let rawfile = stage.join(format!("{id}.icon.raw"));
    std::fs::write(&rawfile, &raw)?;
    let dst = format!("{}/{}.icon.raw", cfg.images_dir.trim_end_matches('/'), id);
    rb.put_typed(&cfg.out, &rawfile, &dst, "ABMP", "ttxt")?;

    // 8-bit colour icl8 (optional): bake a small 8-bit PICT injected as the
    // `.8.pict` depth variant, so colour screens get a colour icon (1-bit
    // screens keep the ICN# .raw). The icl8 indexes the standard Mac palette,
    // which `icons::app_icl8_png` resolves into a PNG for `pict`. Skipped for a
    // black-&-white build (Mac Plus / SE): those screens never read it.
    let pngfile = stage.join(format!("{id}.icon.png"));
    if cfg.wants_color_art() && matches!(icons::app_icl8_png(&hqx_bytes, &pngfile), Ok(true)) {
        let pictfile = stage.join(format!("{id}.icon.8.pict"));
        // The icon is already tiny (≤32px); no art bound applies.
        if pict::run(&pngfile, &pictfile, pict::Depth::Eight, true, u32::MAX, u32::MAX).is_ok() {
            let pdst = format!("{}/{}.icon.8.pict", cfg.images_dir.trim_end_matches('/'), id);
            let _ = rb.put_typed(&cfg.out, &pictfile, &pdst, "PICT", "ttxt");
        }
    }

    Ok(Some(format!("{images_rel}/{id}.icon")))
}

/// Resource id for a depth's art variant: `128 + bits`, so 1-bit → 129 (`ABMP`),
/// 4 → 132, 8 → 136, 16 → 144, 24 → 152 (`PICT`). IDs stay ≥128 (Apple reserves
/// 0–127). **Must match the launcher's `art.c` resource loader.**
fn art_res_id(bits: u16) -> i16 {
    (128 + bits) as i16
}

/// Per-item art fork (docs/36 Phase 1): bake each requested depth and pack them
/// all into one `images/<name>.rsrc` — a 1-bit `ABMP` + one `PICT` per colour
/// depth — instead of loose files. The data fork is empty; the resources go in
/// the resource fork via `rb-cli setrsrc` (the `bake_sound` pattern). A `PICT`
/// resource is the picture data with the 512-byte file header stripped; the raw
/// 1-bit `ABMP` body is used as-is. Returns `<images_rel>/<name>.rsrc`, or None.
/// Build the resource-fork BYTES for one item's depth variants: a 1-bit `ABMP` +
/// a `PICT` per colour depth (id 128+bits), staging intermediate `.pict`/`.raw`
/// under `stage`. A `PICT` resource is the picture data with the 512-byte file
/// header stripped; the raw 1-bit `ABMP` body is used as-is. `None` if nothing
/// decoded. Shared by the image pipeline and the `atrium pict-rsrc` command.
pub fn art_rsrc_bytes(
    src: &Path,
    depths: &[pict::Depth],
    aw: u32,
    ah: u32,
    stage: &Path,
) -> Result<Option<Vec<u8>>> {
    let mut bodies: Vec<(crate::resfork::OsType, i16, Vec<u8>)> = Vec::new();
    for d in depths {
        let raw = *d == pict::Depth::One;
        let ext = if raw { "raw" } else { "pict" };
        let f = stage.join(format!("art-rsrc-{}.{ext}", d.bits()));
        let ok = if raw {
            pict::run_raw1(src, &f, aw, ah).is_ok()
        } else {
            pict::run(src, &f, *d, true, aw, ah).is_ok()
        };
        if !ok {
            continue; // skip a depth that won't decode rather than fail
        }
        let bytes = std::fs::read(&f)?;
        let (tag, body): (crate::resfork::OsType, Vec<u8>) = if raw {
            (*b"ABMP", bytes)
        } else {
            let cut = bytes.len().min(512);
            (*b"PICT", bytes[cut..].to_vec())
        };
        bodies.push((tag, art_res_id(d.bits()), body));
    }
    if bodies.is_empty() {
        return Ok(None);
    }
    let resources: Vec<crate::resfork::Res> = bodies
        .iter()
        .map(|(t, i, b)| crate::resfork::Res::new(*t, *i, b))
        .collect();
    Ok(Some(crate::resfork::build(&resources)))
}

fn bake_variants_rsrc(
    rb: &RbCli,
    cfg: &BuildConfig,
    stage: &Path,
    images_rel: &str,
    name: &str,
    src: &Path,
    depths: &[pict::Depth],
    aw: u32,
    ah: u32,
) -> Result<Option<String>> {
    let fork = match art_rsrc_bytes(src, depths, aw, ah, stage)? {
        Some(f) => f,
        None => return Ok(None),
    };
    let rfile = stage.join(format!("{name}.rsrc.bin"));
    std::fs::write(&rfile, &fork)?;
    let empty = stage.join(format!("{name}.rsrc.empty"));
    std::fs::write(&empty, b"")?;
    let dst = format!("{}/{}.rsrc", cfg.images_dir.trim_end_matches('/'), name);
    rb.put_typed(&cfg.out, &empty, &dst, "rsrc", "ttxt")?; // empty data fork
    rb.set_rsrc(&cfg.out, &dst, &rfile)?; // depth variants in the resource fork
    Ok(Some(format!("{images_rel}/{name}.rsrc")))
}

/// Bake the depth variants for one source image under `name` (e.g. "prince" for
/// box art or "prince.shot" for the screenshot), inject them, and return the
/// catalog path to record — a base path for multi-variant, an explicit `.ext`
/// for the single-depth case. `None` if nothing decoded.
fn bake_variants(
    rb: &RbCli,
    cfg: &BuildConfig,
    stage: &Path,
    images_rel: &str,
    name: &str,
    src: &Path,
    depths: &[pict::Depth],
    multi: bool,
) -> Result<Option<String>> {
    let (aw, ah) = cfg.art_bounds();
    if cfg.art_forks {
        return bake_variants_rsrc(rb, cfg, stage, images_rel, name, src, depths, aw, ah);
    }
    let mut any = false;
    for d in depths {
        let sfx = if multi { format!(".{}", d.bits()) } else { String::new() };
        // 1-bit ships as a raw CopyBits bitmap (.raw); colour depths as PICT.
        let raw = *d == pict::Depth::One;
        let ext = if raw { "raw" } else { "pict" };
        let stagefile = stage.join(format!("{name}{sfx}.{ext}"));
        let ok = if raw {
            pict::run_raw1(src, &stagefile, aw, ah).is_ok()
        } else {
            pict::run(src, &stagefile, *d, true, aw, ah).is_ok()
        };
        if !ok {
            continue; // skip art that won't decode rather than fail the build
        }
        let dst = format!("{}/{}{}.{}", cfg.images_dir.trim_end_matches('/'), name, sfx, ext);
        let (ftype, creator) = if raw { ("ABMP", "ttxt") } else { ("PICT", "ttxt") };
        rb.put_typed(&cfg.out, &stagefile, &dst, ftype, creator)?;
        any = true;
    }
    if !any {
        return Ok(None);
    }
    Ok(Some(if multi {
        format!("{images_rel}/{name}")
    } else {
        let ext = if depths[0] == pict::Depth::One { "raw" } else { "pict" };
        format!("{images_rel}/{name}.{ext}")
    }))
}


/// Bake one WAV chime into a sound file (`<sounds_dir>/<name>`) on the volume:
/// an empty data fork plus a resource fork holding a `snd ` resource. Warns when
/// the clip is over the 7-second cap (it's truncated).
fn bake_sound(rb: &RbCli, cfg: &BuildConfig, stage: &Path, wav: &Path, name: &str) -> Result<()> {
    let (rsrc, secs) = snd::build_resfork_from_wav(wav)
        .with_context(|| format!("encoding sound {}", wav.display()))?;
    if secs > snd::MAX_SECS {
        eprintln!(
            "  warning: {} is {:.1}s; truncated to {:.0}s",
            wav.display(),
            secs,
            snd::MAX_SECS
        );
    }
    let rfile = stage.join(format!("{name}.snd.rsrc"));
    std::fs::write(&rfile, &rsrc)?;
    let empty = stage.join(format!("{name}.snd.empty"));
    std::fs::write(&empty, b"")?;
    let dst = format!("{}/{}", cfg.sounds_dir.trim_end_matches('/'), name);
    rb.put_typed(&cfg.out, &empty, &dst, "sfil", "movr")?; // data fork (type: System sound)
    rb.set_rsrc(&cfg.out, &dst, &rfile)?; // then the snd resource fork
    Ok(())
}

/// Stage: bake artwork for every record in `work` and merge the resulting
/// `image`/`shot`/`icon` catalog paths back into `work`. Sources, in precedence:
/// an explicit `art_dir` > Macintosh Garden art (`<stage>/mg-art`, staged by the
/// `mg` pass) > a LaunchBox download. No-op unless an art source is configured.
/// Shared by [`run`] (fresh build) and [`add_to_disk`] so both bake art the same.
fn bake_art(cfg: &BuildConfig, rb: &RbCli, stage: &Path, work: &Path) -> Result<()> {
    if !(cfg.art_dir.is_some() || cfg.download_art || cfg.mg_archive.is_some()) {
        return Ok(());
    }
    let mg_art_dir = stage.join("mg-art");
    let depth = pict::Depth::parse(&cfg.art_depth)?;
    rb.mkdir_p(&cfg.out, &cfg.images_dir)?;

    // Downloaded art, id -> local file (from the enrich manifest): Box-Front
    // ("art") and the gameplay Screenshot ("shot").
    let mut downloaded: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    let mut downloaded_shot: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    if cfg.download_art {
        let manifest = stage.join("art-manifest.jsonl");
        let dl_dir = stage.join("art-dl");
        std::fs::create_dir_all(&dl_dir)?;
        for line in std::fs::read_to_string(&manifest).unwrap_or_default().lines() {
            let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
            let Some(id) = v.get("id").and_then(Value::as_str) else { continue };
            if let Some(url) = v.get("art").and_then(Value::as_str) {
                let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img");
                let dst = dl_dir.join(format!("{id}.{ext}"));
                if enrich::download(url, &dst).is_ok() {
                    downloaded.insert(id.to_string(), dst);
                }
            }
            if let Some(url) = v.get("shot").and_then(Value::as_str) {
                let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img");
                let dst = dl_dir.join(format!("{id}.shot.{ext}"));
                if enrich::download(url, &dst).is_ok() {
                    downloaded_shot.insert(id.to_string(), dst);
                }
            }
        }
    }

    // One depth (single `<id>.pict`) or several variants (`<id>.<d>.pict`).
    let multi = !cfg.art_depths.is_empty();
    let depths: Vec<pict::Depth> = if multi {
        cfg.art_depths.iter().map(|s| pict::Depth::parse(s)).collect::<Result<_>>()?
    } else {
        vec![depth]
    };
    let images_rel = cfg
        .images_dir
        .strip_prefix("/MacAtrium/")
        .unwrap_or(&cfg.images_dir)
        .trim_end_matches('/');

    let mut overlay = String::new();
    let mut n = 0;
    let mut n_shot = 0;
    let mut n_icon = 0;
    for (id, app) in dataset_records(work)? {
        // Box-Front (catalog `image`) and Screenshot (catalog `shot`); a local
        // art_dir wins, else the downloaded file. art_dir screenshots are named
        // `<id>.shot.<ext>`. Baked files use `fid` (HFS 31-char safe), not `id`.
        let fid = fs_id(&id);
        // Source precedence: explicit art_dir (user override) > MacGarden art >
        // LaunchBox download. (MG art is era-accurate Mac art.)
        let mg_art = cfg.mg_archive.as_ref().map(|_| mg_art_dir.as_path());
        let box_src = cfg.art_dir.as_ref().and_then(|adir| find_art(adir, &id))
            .or_else(|| mg_art.and_then(|adir| find_art(adir, &id)))
            .or_else(|| downloaded.get(&id).cloned());
        let shot_name = format!("{id}.shot");
        let shot_vol = format!("{fid}.shot");
        let shot_src = cfg.art_dir.as_ref().and_then(|adir| find_art(adir, &shot_name))
            .or_else(|| mg_art.and_then(|adir| find_art(adir, &shot_name)))
            .or_else(|| downloaded_shot.get(&id).cloned());

        let mut fields = String::new();
        let mut has_box = false;
        if let Some(src) = &box_src {
            if let Some(rel) = bake_variants(rb, cfg, stage, images_rel, &fid, src, &depths, multi)? {
                fields.push_str(&format!(",\"image\":{rel:?}"));
                n += 1;
                has_box = true;
            }
        }
        if let Some(src) = &shot_src {
            if let Some(rel) = bake_variants(rb, cfg, stage, images_rel, &shot_vol, src, &depths, multi)? {
                fields.push_str(&format!(",\"shot\":{rel:?}"));
                n_shot += 1;
            }
        }
        // App's own Finder icon (ICN#/icl8) for the list-row gutter — baked
        // for every item with an app. When the item has no box art, reuse it
        // as the big art-pane fallback (the old behaviour, now for all rows).
        if let Some(app) = app.as_deref().filter(|a| !a.is_empty()) {
            if let Some(icon_rel) = bake_icon(rb, cfg, stage, images_rel, &fid, app)? {
                fields.push_str(&format!(",\"icon\":{icon_rel:?}"));
                n_icon += 1;
                if !has_box {
                    fields.push_str(&format!(",\"image\":{icon_rel:?}"));
                }
            }
        }
        if !fields.is_empty() {
            overlay.push_str(&format!("{{\"id\":{id:?}{fields}}}\n"));
        }
    }
    let depth_label = if multi { cfg.art_depths.join("/") } else { cfg.art_depth.clone() };
    eprintln!(
        "[art]    {n} box-art + {n_shot} screenshot + {n_icon} app-icon item(s) at {depth_label}-bit"
    );
    if n + n_shot + n_icon > 0 {
        let ovf = stage.join("art-overlay.jsonl");
        std::fs::write(&ovf, overlay)?;
        merge::run(work, &ovf, work, false)?;
    }
    Ok(())
}

/// Copy `src` to `dst` preserving sparse holes, WITHOUT shelling out (portable —
/// Windows included). Streams the source in blocks and seeks over all-zero blocks
/// instead of writing them, so the mostly-empty regions of a base image stay holes:
/// a real sparse hole on Unix; on Windows the gap reads back as zeros (correct, just
/// not physically sparse unless the volume makes it so). A final `set_len` pins the
/// exact source length so a trailing zero run is preserved. `std::fs::copy` is not
/// used because its `copy_file_range` fast-path de-sparsifies on some filesystems.
fn copy_sparse(src: &Path, dst: &Path) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut r = std::fs::File::open(src)
        .with_context(|| format!("opening base image {}", src.display()))?;
    let mut w = std::fs::File::create(dst)
        .with_context(|| format!("creating output image {}", dst.display()))?;
    let total = r.metadata()?.len();
    const BLK: usize = 1 << 20; // 1 MiB
    let mut buf = vec![0u8; BLK];
    loop {
        // A single File::read may return short; fill the block (or hit EOF) first.
        let mut got = 0usize;
        while got < BLK {
            match r.read(&mut buf[got..])? {
                0 => break,
                n => got += n,
            }
        }
        if got == 0 {
            break;
        }
        if buf[..got].iter().all(|&b| b == 0) {
            w.seek(SeekFrom::Current(got as i64))?; // leave a hole
        } else {
            w.write_all(&buf[..got])?;
        }
        if got < BLK {
            break; // short read == EOF
        }
    }
    w.set_len(total)?; // exact length; preserves a trailing hole
    Ok(())
}

/// CLI convenience (a *view* helper): load a [`BuildConfig`] from a JSON file and
/// run it. The GUI builds the `BuildConfig` directly and calls [`run`].
pub fn run_from_path(config: &Path) -> Result<()> {
    let mut cfg: BuildConfig = serde_json::from_str(
        &std::fs::read_to_string(config).with_context(|| format!("reading {}", config.display()))?,
    )
    .with_context(|| format!("parsing config {}", config.display()))?;
    // Default the output filename from the collection / list the build targets.
    cfg.out = cfg.resolve_out();
    run(&cfg)
}

/// The build **controller**: assemble a bootable image from a [`BuildConfig`].
/// Both the CLI and the GUI call this with the same model.
/// Ensure each id in `ids` carries the "Recommended" category in the staged
/// category DB (`categories.jsonl`) — the collection-scoped recommended list.
/// Adds it to an existing record's `categories`, or appends a minimal record for an
/// id the DB doesn't mention. Idempotent; presence-only.
fn add_recommended_to_cats(cats: &Path, ids: &[String]) -> Result<()> {
    use std::collections::BTreeSet;
    let want: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let text = std::fs::read_to_string(cats).unwrap_or_default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Ok(mut v) = serde_json::from_str::<Value>(t) {
            if let Some(id) = v.get("id").and_then(Value::as_str).map(str::to_string) {
                if want.contains(id.as_str()) {
                    seen.insert(id.clone());
                    match v.get_mut("categories").and_then(Value::as_array_mut) {
                        Some(arr) => {
                            if !arr.iter().any(|c| c.as_str() == Some("Recommended")) {
                                arr.insert(0, Value::from("Recommended"));
                            }
                        }
                        None => {
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert("categories".into(), Value::from(vec!["Recommended"]));
                            }
                        }
                    }
                }
                out.push_str(&v.to_string());
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    for id in ids {
        if !seen.contains(id) {
            out.push_str(&format!("{{\"id\":{id:?},\"categories\":[\"Recommended\"]}}\n"));
        }
    }
    std::fs::write(cats, out)?;
    Ok(())
}

/// Boot the freshly-built image in Snow (7.5.5) so the Finder rebuilds the volume's
/// Desktop DB, then drive MacAtrium's Shut Down for a clean unmount and re-bless 7.1
/// (the ship default). Key/cycle marks are validated for the ≤2 GB HFS volume — the
/// 2 GB cap bounds the Desktop-rebuild time, so they're stable. Fail-soft: any hiccup
/// warns and continues (a finalize step must never abort an otherwise-good build).
fn rebuild_desktop_via_snow(rb: &RbCli, cfg: &BuildConfig, stage: &Path) -> Result<()> {
    if !cfg.rebuild_desktop {
        return Ok(());
    }
    let (Some(harness), Some(rom), Some(mdc)) = (
        cfg.snow_harness.as_ref(),
        cfg.snow_rom.as_ref(),
        cfg.snow_mdc_rom.as_ref(),
    ) else {
        eprintln!("[desktop] rebuild_desktop is on but snow_harness/snow_rom/snow_mdc_rom are not all set — skipping Desktop rebuild");
        return Ok(());
    };
    eprintln!("[desktop]   rebuilding the Desktop DB via Snow (7.5.5 boot, ~90s)…");
    // No Desktop DB ⇒ the Finder rebuilds it on boot (no key-injection needed for it).
    let _ = rb.rm(&cfg.out, "/Desktop DB");
    let _ = rb.rm(&cfg.out, "/Desktop DF");
    if let Err(e) = rb.bless_set(&cfg.out, "/System Folder 7.5.5") {
        eprintln!("[desktop] WARNING: could not bless 7.5.5 ({e:#}); skipping Desktop rebuild — continuing");
        return Ok(());
    }
    let snow_out = stage.join("desktop-snow");
    let _ = std::fs::create_dir_all(&snow_out);
    // Marks: Return past the first-run chooser (10G), ESC menu (10.5G), Up→Shut Down
    // (10.8G), Return to activate (11G); run to 13G. MacAtrium's Shut Down does a
    // clean FlushVol + power-off, so the volume unmounts clean (no repair needed).
    let status = std::process::Command::new(harness)
        .arg(rom)
        .arg(mdc)
        .arg(&cfg.out)
        .arg(&snow_out)
        .arg("13000000000")
        .arg("--snap-every")
        .arg("3000000000")
        .arg("--keys")
        .arg("10000000000:return;10500000000:esc;10800000000:up;11000000000:return")
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("[desktop] WARNING: Snow harness exited {s}; Desktop DB may be stale — continuing"),
        Err(e) => eprintln!("[desktop] WARNING: could not run Snow harness ({e}); skipping Desktop rebuild — continuing"),
    }
    // Fresh first-run: drop the launcher prefs from every System Folder (+ the S6 spot).
    if let Ok(roots) = rb.ls(&cfg.out, "/") {
        for e in roots.iter().filter(|e| e.is_dir) {
            if rb.exists(&cfg.out, &format!("/{}/System", e.name)) {
                let _ = rb.rm(&cfg.out, &format!("/{}/Preferences/MacAtrium Prefs", e.name));
            }
        }
    }
    let _ = rb.rm(&cfg.out, "/MacAtrium/MacAtrium Prefs");
    // Restore the ship default boot.
    if let Err(e) = rb.bless_set(&cfg.out, "/System Folder 7.1") {
        eprintln!("[desktop] WARNING: could not restore the 7.1 bless ({e:#}) — the image may boot 7.5.5!");
    }
    // Read-only clean check (never --repair: a clean Shut Down should leave it clean).
    match rb.fsck(&cfg.out) {
        Ok(out) if out.to_lowercase().contains("error") => {
            eprintln!("[desktop] WARNING: fsck reports the volume is not clean (Shut Down may have mistimed):\n{out}")
        }
        Ok(_) => eprintln!("[desktop]   volume clean; Desktop DB rebuilt; re-blessed 7.1"),
        Err(e) => eprintln!("[desktop] WARNING: fsck did not pass (volume may be dirty): {e:#}"),
    }
    Ok(())
}

pub fn run(cfg: &BuildConfig) -> Result<()> {
    // Resolve base_os -> system + deploy mode via the template registry first, so
    // the rest of the controller works against a fully-populated config.
    let resolved = crate::templates::resolve(cfg)?;
    let cfg = &resolved;

    // Machine-local settings (~/.macatrium.json): the MacPack folder lets the
    // donor resolver find disks referenced by filename (e.g. boot.vhd) that aren't
    // donors.json aliases; `rb_cli` lets the tool path be configured once here
    // rather than baked into every (portable) build config.
    let settings = crate::settings::Settings::load_default();

    // Resolve the rb-cli binary ONCE and thread it through every call site below
    // (harvest/catalog take the path too) — and log which file + version actually
    // runs. A bare "rb-cli" is resolved against $PATH at exec time, so logging it
    // up front is what turns the stale-binary trap (a pre-fix rb-cli shadowing the
    // configured path → silent corrupt catalog) into a one-glance check.
    let rb_bin = crate::rbcli::resolve_bin(&cfg.rb_cli, settings.rb_cli.as_deref());
    crate::rbcli::log_version(&rb_bin);
    let rb = RbCli::new(&rb_bin);
    let stage = cfg
        .stage
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("atrium-image-stage"));
    std::fs::create_dir_all(&stage)?;

    // Materialize the library to a working copy up front (so the build never
    // mutates the source) — and so a build with no `dataset` path uses the library
    // bundled into this tool. The whole pipeline reads/writes `work` from here on.
    let work = stage.join("dataset.jsonl");
    std::fs::write(&work, cfg.dataset_bytes()?)
        .with_context(|| format!("writing working dataset {}", work.display()))?;
    eprintln!(
        "[lib] {}",
        match &cfg.dataset {
            Some(p) => format!("library {}", p.display()),
            None => "library (bundled)".to_string(),
        }
    );

    // A named collection (a saved game list) is resolved to a List selection here:
    // its ids drive the build and its per-title overrides merge over the working
    // dataset. It takes precedence over an inline `selection`.
    let effective_sel: Option<crate::config::Selection> = match &cfg.collection {
        Some(name) => {
            let col = crate::collections::find(name)?;
            eprintln!("[lib] collection '{}' — {} title(s)", col.name, col.ids.len());
            if !col.overrides.is_empty() {
                let ov = stage.join("collection-overrides.jsonl");
                std::fs::write(&ov, col.overrides_jsonl())?;
                merge::run(&work, &ov, &work, false)?;
            }
            Some(crate::config::Selection::List { ids: col.ids })
        }
        None => cfg.selection.clone(),
    };

    // 1. base system -> out
    let system = cfg.system.as_ref().expect("resolve() guarantees system is set");
    eprintln!("[1/7] base system  {} -> {}", system.display(), cfg.out.display());
    // Portable sparse-preserving copy (no shell-out — runs on Windows too). We avoid
    // std::fs::copy: its copy_file_range fast-path de-sparsifies on some filesystems,
    // writing every zero byte and blowing a mostly-empty multi-GB base (e.g. a 10 GB
    // Mac OS 9 image) up to its full nominal size. copy_sparse seeks past zero blocks.
    copy_sparse(system, &cfg.out)
        .with_context(|| format!("copying base image {} -> {}", system.display(), cfg.out.display()))?;

    // 1b. preflight: project disk usage before doing the expensive work, and warn
    // if it won't fit the target (~95% estimate; not a hard gate).
    {
        let n_items: u64 = match &effective_sel {
            Some(sel) => crate::selection::resolve(&work, sel, cfg.base_os.as_deref())
                .map(|(ids, _)| ids.len() as u64)
                .unwrap_or(0),
            None => cfg.harvest.iter().map(|h| h.apps.len() as u64).sum(),
        };
        let est = crate::preflight::estimate(cfg, 0, n_items);
        let mb = |b: u64| b / (1024 * 1024);
        let tgt = if est.target_bytes > 0 {
            format!(", target {} MB", mb(est.target_bytes))
        } else {
            String::new()
        };
        // Apps live in the resource fork, which rb-cli can't size up front, so the
        // app footprint isn't projected here — it's MEASURED live during harvest
        // (the `[footprint]` lines below) as a volume used-space delta. Only base +
        // art (covers/screenshots/icons) are estimable ahead of time.
        eprintln!(
            "[preflight] base {} MB + covers/art ~{} MB est for {} item(s){} (apps measured live during harvest)",
            mb(est.base_bytes),
            mb(est.art_bytes),
            n_items,
            tgt
        );
    }

    // 1c. grow the image to the requested size (disk-size controller).
    crate::preflight::apply_disk_size(&rb, cfg)?;
    // Baseline volume usage after the grow (OS + launcher only) — the zero point
    // the harvest/art footprints are measured against.
    let used_after_grow = rb.fs_used(&cfg.out).unwrap_or(0);

    // 3. harvest apps from donor images into the output + append stubs. Two paths,
    // both may run: a high-level `selection` (dataset ids/categories → donor
    // registry) and the low-level explicit `harvest` list (manual override).
    if let Some(sel) = &effective_sel {
        let donors = crate::donors::Registry::load_default();
        let (plan, reservoir, unresolved, curated) = crate::selection::harvest_plan(
            &work,
            sel,
            cfg.base_os.as_deref(),
            &donors,
            settings.macpack_dir.as_deref(),
        )?;
        if !unresolved.is_empty() {
            eprintln!(
                "[2/7] selection    {} selected app(s) skipped (no source/donor): {}",
                unresolved.len(),
                unresolved.join(", ")
            );
        }
        // Reservoir donors: copy each selected installed folder verbatim (both
        // forks, folder name + curated `app` preserved) — no harvest re-pick or
        // rename. The dataset already carries the right `app`, so no stub rewrite.
        let n_res: usize = reservoir.iter().map(|(_, f)| f.len()).sum();
        if n_res > 0 {
            rb.mkdir_p(&cfg.out, &cfg.apps_root)?;
            let into = format!("{}/", cfg.apps_root.trim_end_matches('/'));
            eprintln!("[2/7] reservoir    copy {n_res} title(s) verbatim");
            for (image, folders) in &reservoir {
                for folder in folders {
                    rb.cp(image, folder, &cfg.out, &into)
                        .with_context(|| format!("reservoir cp {folder}"))?;
                }
            }
        }
        eprintln!("[2/7] selection    harvest {} donor group(s)", plan.len());
        for (image, apps) in &plan {
            harvest::run(
                &rb_bin,
                image,
                apps,
                None,
                &stage.join("apps"),
                Some(&cfg.out),
                &cfg.apps_root,
                Some(&work),
                Some(&curated),       /* keep the selected curated id on each stub */
            )?;
        }
    }
    if !cfg.harvest.is_empty() {
        eprintln!("[2/7] harvest      {} donor source(s)", cfg.harvest.len());
        for h in &cfg.harvest {
            harvest::run(
                &rb_bin,
                &h.image,
                &h.apps,
                h.scan.as_deref(),
                &stage.join("apps"),
                Some(&cfg.out),
                &cfg.apps_root,
                Some(&work),
                None,                 /* explicit harvest: app-name id (no curated map) */
            )?;
        }
    }

    // Footprint #1 — apps. The harvested apps are the build's big half and live in
    // the resource fork, so measure the real cost as the volume used-space delta
    // (fork-accurate; `ls` can't see resource forks). Reported now for immediate
    // feedback on the long harvest step; folded into the summary after art too.
    let used_after_apps = rb.fs_used(&cfg.out).unwrap_or(used_after_grow);
    eprintln!(
        "[footprint] apps {} MB (both forks, on-volume)",
        used_after_apps.saturating_sub(used_after_grow) / (1024 * 1024)
    );

    // 3b. enrich from the Macintosh Garden archive (68K-only) BEFORE LaunchBox, so
    // MG wins for gap-fills; stage its box-front/screenshot art for the art pass.
    let mg_art_dir = stage.join("mg-art");
    if let Some(mga) = &cfg.mg_archive {
        eprintln!("[3b] mg            Macintosh Garden archive {}", mga.display());
        mg::run(&work, mga, &work, false, Some(&mg_art_dir))?;
    }

    // 4. enrich from LaunchBox (fills gaps only; optional color auto-detect)
    if let Some(md) = &cfg.metadata {
        eprintln!("[3/7] enrich       LaunchBox \"{}\"", cfg.platform);
        // When downloading art, also have enrich emit a Box-Front URL manifest.
        let art_manifest = if cfg.download_art {
            Some(stage.join("art-manifest.jsonl"))
        } else {
            None
        };
        enrich::run(
            &work, md, &work, &cfg.platform, false,
            art_manifest.as_deref(), cfg.detect_color, &cfg.curl,
        )?;
    }

    // 5. compatibility/facets overlay (file-or-bundled) — always applied; the
    // overlay wins (color/mouse/maxDepth/minOS/maxOS + corrections).
    {
        let compat = stage.join("compatibility.jsonl");
        std::fs::write(&compat, cfg.compatibility_bytes()?)
            .with_context(|| format!("writing {}", compat.display()))?;
        eprintln!(
            "[4/7] merge        compatibility {}",
            match &cfg.overrides {
                Some(p) => p.display().to_string(),
                None => "(bundled)".to_string(),
            }
        );
        merge::run(&work, &compat, &work, false)?;
    }

    // 6. Filter to titles actually installed on the volume FIRST — a phantom
    // entry would -43 on launch, and (crucially) a small selection from the
    // ~1500-title library shouldn't bake the whole library's art. Art + catalog
    // both work off this filtered set.
    let present = stage.join("dataset.present.jsonl");
    let dropped = filter_present_apps(&rb, &cfg.out, &work, &present)?;
    if !dropped.is_empty() {
        const SHOW: usize = 8;
        let more = dropped.len().saturating_sub(SHOW);
        eprintln!(
            "[5/7] present      dropped {} not-installed title(s): {}{}",
            dropped.len(),
            dropped.iter().take(SHOW).cloned().collect::<Vec<_>>().join(", "),
            if more > 0 { format!(", … (+{more} more)") } else { String::new() }
        );
    }

    // 6b. art: bake box-art + screenshot + app icons for the INSTALLED titles,
    // inject them, and merge the image/shot/icon paths back. (Shared with `add`.)
    bake_art(cfg, &rb, &stage, &present)?;

    // Footprint #2 — covers + art, again a used-space delta. Summary line shows the
    // two halves (apps vs covers/art) and the volume total against the target, so a
    // disk can be right-sized from real numbers instead of an apps-blind estimate.
    let used_after_art = rb.fs_used(&cfg.out).unwrap_or(used_after_apps);
    {
        let mb = |b: u64| b / (1024 * 1024);
        let target = cfg.disk_size_mb.map(|m| m.min(crate::config::MAX_DISK_MB)).unwrap_or(0);
        eprintln!(
            "[footprint] covers + art {} MB · apps {} MB · volume used {} MB{}",
            mb(used_after_art.saturating_sub(used_after_apps)),
            mb(used_after_apps.saturating_sub(used_after_grow)),
            mb(used_after_art),
            if target > 0 { format!(" / {target} MB target") } else { String::new() },
        );
    }

    // 7. catalog: generate + inject from the present (already-filtered) set.
    eprintln!("[6/7] catalog      generate + inject (paged, docs/21)");
    // Paged catalog — handles any library size; the launcher prefers it. Uses the
    // bundled taxonomy + category DB (a build is portable; no extra paths).
    let paged_dir = stage.join("paged");
    let tax = stage.join("taxonomy.json");
    std::fs::write(&tax, crate::config::EMBEDDED_TAXONOMY)?;
    let cats = stage.join("categories.jsonl");
    std::fs::write(&cats, crate::config::EMBEDDED_CATEGORIES)?;
    // Collection-scoped Recommended: tag the collection's `recommended` ids into the
    // staged category DB so they populate the Recommended nav category for this build.
    if let Some(cname) = &cfg.collection {
        if let Ok(coll) = crate::collections::find(cname) {
            if !coll.recommended.is_empty() {
                add_recommended_to_cats(&cats, &coll.recommended)?;
                eprintln!(
                    "[6/7] catalog      {} collection-recommended title(s) -> Recommended",
                    coll.recommended.len()
                );
            }
        }
    }
    let report = catalog::run_paged(&present, &paged_dir, Some(&cats), Some(&tax), false, false)?;
    eprintln!(
        "[6/7] catalog      {} item(s) in {} categor(y/ies) / {} page(s)",
        report.items, report.categories, report.pages
    );
    catalog::inject_paged(&rb_bin, &cfg.out, &paged_dir, &cfg.metadata_dir)?;
    // Legacy single-file catalog too (back-compat for an old launcher; an over-256
    // library can't be a single file, so that case is paged-only).
    let cat = stage.join("catalog.jsonl");
    match catalog::run(&present, &cat, false, false) {
        Ok(_) => catalog::inject(&rb_bin, &cfg.out, &cat, &cfg.metadata_dir, Some(&stage))?,
        Err(e) => eprintln!("[6/7] catalog      legacy single-file skipped: {e}"),
    }

    if cfg.install_all_systems {
        // Multi-System appliance: put MacAtrium in every System Folder so a
        // bless-swap between Systems always boots back into it (docs/36 Phase 2).
        let n = install_into_all_systems(&rb, &cfg, &stage)?;
        eprintln!("[7/7] launcher     installed into {n} System Folder(s) (all-systems)");
        // Per-title runtime dependencies: install each Extension a present title
        // `requires` into the System Folders whose OS needs it (verbatim from a
        // reservoir donor). No-op unless an installed title declares `requires:[…]`.
        install_dependencies(&rb, cfg, &present)?;
        // Rebuild the volume's Desktop DB in Snow (7.5.5 boot), then re-bless 7.1.
        rebuild_desktop_via_snow(&rb, cfg, &stage)?;
    } else if cfg.finder_replace {
        eprintln!("[7/7] launcher     install AS the Finder (System 6 boot shell)");
        install_as_finder(&rb, &cfg, &stage)?;
    } else {
        eprintln!("[7/7] launcher     install into {}", cfg.startup_items);
        install_startup_item(&rb, &cfg, &stage, &cfg.startup_items)?;
        // Disable appliance-inappropriate quick-launch control panels (the Mac OS 8
        // Control Strip + Launcher) in the target System Folder so they don't float
        // over the full-screen launcher (no-op where absent). docs/36.
        let sysfolder = cfg
            .startup_items
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .filter(|p| !p.is_empty())
            .unwrap_or("/System Folder");
        strip_control_panels_in(&rb, &cfg, sysfolder)?;
    }

    // Optional startup / shutdown chimes (WAV -> snd resource on the volume).
    if cfg.startup_sound.is_some() || cfg.shutdown_sound.is_some() {
        rb.mkdir_p(&cfg.out, &cfg.sounds_dir)?;
        if let Some(w) = &cfg.startup_sound {
            bake_sound(&rb, &cfg, &stage, w, "startup")?;
        }
        if let Some(w) = &cfg.shutdown_sound {
            bake_sound(&rb, &cfg, &stage, w, "shutdown")?;
        }
        eprintln!("[snd] startup/shutdown chimes -> {}", cfg.sounds_dir);
    }

    // Right-size: reclaim the working slack so the shipped disk carries ~10% free
    // (30 MB floor on a tiny disk) instead of the full `disk_size_mb`. LAST step —
    // after every content write (systems, deps, Desktop rebuild, chimes).
    crate::preflight::right_size_image(&rb, cfg)?;

    eprintln!(
        "\nimage built: {} ({} items, {} categories, {} page(s))",
        cfg.out.display(),
        report.items,
        report.categories,
        report.pages
    );
    Ok(())
}

/// Write only the library records whose id is in `ids` (dataset order preserved,
/// comments dropped) to `dst` — the delta dataset an [`add_to_disk`] run harvests
/// and catalogs, so the existing on-disk titles are never reprocessed.
fn subset_dataset(full: &Path, ids: &[String], dst: &Path) -> Result<()> {
    use std::collections::HashSet;
    let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let text = std::fs::read_to_string(full)
        .with_context(|| format!("reading library {}", full.display()))?;
    let mut out = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if v.get("id").and_then(Value::as_str).map(|id| want.contains(id)).unwrap_or(false) {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    std::fs::write(dst, out).with_context(|| format!("writing delta {}", dst.display()))?;
    Ok(())
}

/// **Add titles to an already-built MacAtrium disk, in place.** Harvests the
/// selected titles into the existing image, bakes their art, and *merges* their
/// catalog records with the disk's current catalog — so the titles already on the
/// disk keep their baked art (the merge is at the compiled-catalog level, not a
/// regenerate-from-scratch). It does **not** copy a base system or reinstall the
/// launcher: the disk already boots.
///
/// `cfg.out` is the existing disk (mutated in place); `cfg.selection` names the
/// new titles; `cfg.base_os` / `art_depths` should match the disk's original
/// Target so OS-scoping and art depths line up. Library/compatibility default to
/// the bundled data. The union is capped at the device max ([`catalog::MAX_ITEMS`]).
pub fn add_to_disk(cfg: &BuildConfig) -> Result<()> {
    anyhow::ensure!(cfg.out.exists(), "target disk {} not found", cfg.out.display());
    let sel = cfg
        .selection
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no selection: pick the titles to add"))?;

    let settings = crate::settings::Settings::load_default();
    // Resolve + log the rb-cli binary once; reuse it for harvest/catalog below.
    let rb_bin = crate::rbcli::resolve_bin(&cfg.rb_cli, settings.rb_cli.as_deref());
    crate::rbcli::log_version(&rb_bin);
    let rb = RbCli::new(&rb_bin);
    let stage = cfg
        .stage
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("atrium-add-stage"));
    std::fs::create_dir_all(&stage)?;

    // Full library working copy (bundled unless overridden) → resolve the selection.
    let lib = stage.join("library.jsonl");
    std::fs::write(&lib, cfg.dataset_bytes()?)?;
    let (ids, missing) = crate::selection::resolve(&lib, sel, cfg.base_os.as_deref())?;
    if !missing.is_empty() {
        eprintln!("[add] {} requested id(s) not in the library: {}", missing.len(), missing.join(", "));
    }
    anyhow::ensure!(!ids.is_empty(), "no titles selected to add");
    let work = stage.join("delta.jsonl");
    subset_dataset(&lib, &ids, &work)?;
    eprintln!("[add] adding {} title(s) to {}", ids.len(), cfg.out.display());

    // Grow the disk first if asked (only grows), so the new apps have room.
    crate::preflight::apply_disk_size(&rb, cfg)?;

    // Harvest the selected titles' apps into the disk + append harvested stubs to
    // the delta (selection plan + any explicit harvest sources).
    let donors = crate::donors::Registry::load_default();
    let (plan, reservoir, unresolved, curated) = crate::selection::harvest_plan(
        &lib, sel, cfg.base_os.as_deref(), &donors, settings.macpack_dir.as_deref(),
    )?;
    if !unresolved.is_empty() {
        eprintln!("[add] {} selected app(s) skipped (no source/donor): {}", unresolved.len(), unresolved.join(", "));
    }
    let n_res: usize = reservoir.iter().map(|(_, f)| f.len()).sum();
    if n_res > 0 {
        rb.mkdir_p(&cfg.out, &cfg.apps_root)?;
        let into = format!("{}/", cfg.apps_root.trim_end_matches('/'));
        eprintln!("[add] reservoir copy {n_res} title(s) verbatim");
        for (image, folders) in &reservoir {
            for folder in folders {
                rb.cp(image, folder, &cfg.out, &into).with_context(|| format!("reservoir cp {folder}"))?;
            }
        }
    }
    eprintln!("[add] harvest {} donor group(s)", plan.len());
    for (image, apps) in &plan {
        harvest::run(&rb_bin, image, apps, None, &stage.join("apps"), Some(&cfg.out), &cfg.apps_root, Some(&work), Some(&curated))?;
    }
    for h in &cfg.harvest {
        harvest::run(&rb_bin, &h.image, &h.apps, h.scan.as_deref(), &stage.join("apps"), Some(&cfg.out), &cfg.apps_root, Some(&work), None)?;
    }

    // Enrich (MG → LaunchBox) + compatibility overlay + art — on the delta only.
    if let Some(mga) = &cfg.mg_archive {
        mg::run(&work, mga, &work, false, Some(&stage.join("mg-art")))?;
    }
    if let Some(md) = &cfg.metadata {
        let art_manifest = cfg.download_art.then(|| stage.join("art-manifest.jsonl"));
        enrich::run(&work, md, &work, &cfg.platform, false, art_manifest.as_deref(), cfg.detect_color, &cfg.curl)?;
    }
    {
        let compat = stage.join("compatibility.jsonl");
        std::fs::write(&compat, cfg.compatibility_bytes()?)?;
        merge::run(&work, &compat, &work, false)?;
    }
    bake_art(cfg, &rb, &stage, &work)?;

    // Catalog: compile ONLY the new titles that actually landed on the volume,
    // then merge with the disk's existing catalog (existing records keep their art).
    let present = stage.join("delta.present.jsonl");
    let dropped = filter_present_apps(&rb, &cfg.out, &work, &present)?;
    if !dropped.is_empty() {
        const SHOW: usize = 8;
        let more = dropped.len().saturating_sub(SHOW);
        eprintln!(
            "[add] {} selected title(s) not installed (skipped): {}{}",
            dropped.len(),
            dropped.iter().take(SHOW).cloned().collect::<Vec<_>>().join(", "),
            if more > 0 { format!(", … (+{more} more)") } else { String::new() }
        );
    }
    let (new_records, report) = catalog::compile(&std::fs::read_to_string(&present)?)?;
    for w in &report.warnings {
        eprintln!("[add] warning: {w}");
    }

    // Read the disk's current catalog (MacRoman) back into JSON records.
    let cur_path = format!("{}/catalog.jsonl", cfg.metadata_dir.trim_end_matches('/'));
    let cur_tmp = stage.join("catalog-current.jsonl");
    let existing: Vec<Value> = match rb.get(&cfg.out, &cur_path, &cur_tmp, true) {
        Ok(()) => catalog::parse_compiled(&std::fs::read(&cur_tmp)?),
        Err(_) => {
            eprintln!("[add] no existing catalog on disk — creating a fresh one");
            Vec::new()
        }
    };

    // Merge: keep existing order; a re-added id replaces its record, truly-new ids
    // append. Then enforce the device item cap.
    let mut merged: Vec<Value> = existing;
    let mut index: std::collections::HashMap<String, usize> = merged
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.get("id").and_then(Value::as_str).map(|s| (s.to_string(), i)))
        .collect();
    let (mut added, mut updated) = (0usize, 0usize);
    for rec in new_records {
        let Some(id) = rec.get("id").and_then(Value::as_str).map(str::to_string) else { continue };
        match index.get(&id) {
            Some(&i) => { merged[i] = rec; updated += 1; }
            None => { index.insert(id, merged.len()); merged.push(rec); added += 1; }
        }
    }
    anyhow::ensure!(
        merged.len() <= catalog::MAX_ITEMS,
        "merged catalog has {} items, over the device max {} — remove some titles or build a fresh disk",
        merged.len(),
        catalog::MAX_ITEMS
    );

    // Render the union (MacRoman, CR) and inject it (backs up the old catalog).
    let bytes = catalog::render_values(&merged, false, false)?;
    let cat = stage.join("catalog.jsonl");
    std::fs::write(&cat, &bytes)?;
    catalog::inject(&rb_bin, &cfg.out, &cat, &cfg.metadata_dir, Some(&stage))?;

    eprintln!(
        "\nadded to {}: {added} new + {updated} updated title(s); catalog now {} item(s)",
        cfg.out.display(),
        merged.len()
    );
    Ok(())
}

/// CLI convenience: load a [`BuildConfig`] (with `out` = an existing MacAtrium
/// disk and `selection` = the titles to add) from JSON and run [`add_to_disk`].
pub fn add_to_disk_from_path(config: &Path) -> Result<()> {
    let cfg: BuildConfig = serde_json::from_str(
        &std::fs::read_to_string(config).with_context(|| format!("reading {}", config.display()))?,
    )
    .with_context(|| format!("parsing config {}", config.display()))?;
    add_to_disk(&cfg)
}

#[cfg(test)]
mod tests {
    use super::{dep_installs_on, is_system6_folder_name};

    #[test]
    fn dep_install_os_substring_match() {
        let os = vec!["7.1".to_string()];
        // "7.1" matches a 7.1 folder (and its 7.1.x point releases) …
        assert!(dep_installs_on(&os, "System Folder 7.1"));
        assert!(dep_installs_on(&os, "System Folder 7.1.2"));
        // … but not 6.0.8 or 7.5.5 (the version isn't a substring of either name).
        assert!(!dep_installs_on(&os, "System Folder 6.0.8"));
        assert!(!dep_installs_on(&os, "System Folder 7.5.5"));
        // An empty install_os is a deliberate no-op — it never matches any folder.
        assert!(!dep_installs_on(&[], "System Folder 7.1"));
    }

    #[test]
    fn system6_name_gate_accepts_6_0_4_and_up_only() {
        // Installable System 6 (at/above the 6.0.4 Gestalt floor) → finder-replace.
        for n in ["System 6.0.4", "System 6.0.5", "System 6.0.7", "System 6.0.8"] {
            assert!(is_system6_folder_name(n), "{n} should be installable");
        }
        // Below the floor / not System 6 → skipped (they lack Startup Items too).
        for n in ["System 4.1", "System 5.0", "System 5.1", "System 6.0.3",
                  "System 7.5.5", "System Folder", "System Folder 8.1"] {
            assert!(!is_system6_folder_name(n), "{n} should be skipped");
        }
    }
}
