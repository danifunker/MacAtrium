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
/// System-6 boot launches it as the shell. Patches the MacBinary internal name to
/// "Finder", injects it (overwriting the real Finder), and retypes it.
fn install_as_finder(rb: &RbCli, cfg: &BuildConfig, stage: &Path) -> Result<()> {
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
    rb.put_macbinary(&cfg.out, &patched, "/System Folder")?;   // lands as "Finder" (--force)
    rb.chmeta(&cfg.out, "/System Folder/Finder", "FNDR", "MACS")?;
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
    for line in text.lines() {
        let t = line.trim();
        let keep = if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            true
        } else if let Ok(v) = serde_json::from_str::<Value>(t) {
            match v.get("app").and_then(Value::as_str) {
                Some(a) if !a.is_empty() => {
                    let full = format!("/MacAtrium/{}", a.trim_start_matches('/'));
                    if rb.exists(image, &full) {
                        true
                    } else {
                        dropped.push(
                            v.get("name").and_then(Value::as_str).unwrap_or(a).to_string(),
                        );
                        false
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
    let mut any = false;
    let (aw, ah) = cfg.art_bounds();
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
        // LaunchBox download. (MG art is era-accurate Mac art; docs/MacintoshGardenArchive.md.)
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

/// CLI convenience (a *view* helper): load a [`BuildConfig`] from a JSON file and
/// run it. The GUI builds the `BuildConfig` directly and calls [`run`].
pub fn run_from_path(config: &Path) -> Result<()> {
    let cfg: BuildConfig = serde_json::from_str(
        &std::fs::read_to_string(config).with_context(|| format!("reading {}", config.display()))?,
    )
    .with_context(|| format!("parsing config {}", config.display()))?;
    run(&cfg)
}

/// The build **controller**: assemble a bootable image from a [`BuildConfig`].
/// Both the CLI and the GUI call this with the same model.
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

    // 1. base system -> out
    let system = cfg.system.as_ref().expect("resolve() guarantees system is set");
    eprintln!("[1/7] base system  {} -> {}", system.display(), cfg.out.display());
    // Use `cp --sparse=always`, not std::fs::copy: the latter's copy_file_range
    // path de-sparsifies on some filesystems, writing every zero byte — which blows
    // up on large (e.g. 10 GB Mac OS 9) base images. cp keeps the holes, so the
    // output stays as small as the data it actually holds.
    let cp = std::process::Command::new("cp")
        .arg("--sparse=always")
        .arg(system)
        .arg(&cfg.out)
        .status()
        .with_context(|| format!("running cp {} -> {}", system.display(), cfg.out.display()))?;
    if !cp.success() {
        anyhow::bail!("cp {} -> {} failed", system.display(), cfg.out.display());
    }

    // 1b. preflight: project disk usage before doing the expensive work, and warn
    // if it won't fit the target (~95% estimate; not a hard gate).
    {
        let n_items: u64 = match &cfg.selection {
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
    if let Some(sel) = &cfg.selection {
        let donors = crate::donors::Registry::load_default();
        let (plan, unresolved) = crate::selection::harvest_plan(
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

    if cfg.finder_replace {
        eprintln!("[7/7] launcher     install AS the Finder (System 6 boot shell)");
        install_as_finder(&rb, &cfg, &stage)?;
    } else {
        eprintln!("[7/7] launcher     install into {}", cfg.startup_items);
        rb.mkdir_p(&cfg.out, &cfg.startup_items)?;
        // Set the bundle bit so the Finder shows the app's real icon (it's a
        // browsable app here, unlike the finder_replace appliance).
        let mut bytes = cfg.launcher_bytes()?;
        anyhow::ensure!(bytes.len() > 128, "launcher .bin too small to be MacBinary");
        set_bundle_bit(&mut bytes);
        apply_app_mem(&cfg, &mut bytes);
        let patched = stage.join("MacAtrium.bundle.bin");
        std::fs::write(&patched, &bytes)?;
        rb.put_macbinary(&cfg.out, &patched, &cfg.startup_items)?;
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
    let (plan, unresolved) = crate::selection::harvest_plan(
        &lib, sel, cfg.base_os.as_deref(), &donors, settings.macpack_dir.as_deref(),
    )?;
    if !unresolved.is_empty() {
        eprintln!("[add] {} selected app(s) skipped (no source/donor): {}", unresolved.len(), unresolved.join(", "));
    }
    eprintln!("[add] harvest {} donor group(s)", plan.len());
    for (image, apps) in &plan {
        harvest::run(&rb_bin, image, apps, None, &stage.join("apps"), Some(&cfg.out), &cfg.apps_root, Some(&work))?;
    }
    for h in &cfg.harvest {
        harvest::run(&rb_bin, &h.image, &h.apps, h.scan.as_deref(), &stage.join("apps"), Some(&cfg.out), &cfg.apps_root, Some(&work))?;
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
