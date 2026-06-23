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
use crate::{catalog, enrich, harvest, icons, merge, pict, snd};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn d_startup() -> String { "/System Folder/Startup Items".into() }
fn d_platform() -> String { "Apple Mac OS".into() }
fn d_rbcli() -> String { "rb-cli".into() }
fn d_apps_root() -> String { "/MacAtrium/Apps".into() }
fn d_metadir() -> String { "/MacAtrium/metadata".into() }
fn d_imagesdir() -> String { "/MacAtrium/images".into() }
fn d_artdepth() -> String { "8".into() }
fn d_curl() -> String { "curl".into() }
fn d_sounds_dir() -> String { "/MacAtrium/sounds".into() }

#[derive(Deserialize)]
struct HarvestSrc {
    image: PathBuf,
    #[serde(default)]
    apps: Vec<String>,
    #[serde(default)]
    scan: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    /// Base bootable System image to build on top of.
    system: PathBuf,
    /// Output image to produce (overwritten).
    out: PathBuf,
    /// The launcher MacBinary (build/MacAtrium.bin) to install.
    launcher: PathBuf,
    /// Curated dataset (copied; never mutated by the build).
    dataset: PathBuf,
    #[serde(default = "d_startup")]
    startup_items: String,
    /// Manual overrides overlay (applied after enrich).
    #[serde(default)]
    overrides: Option<PathBuf>,
    /// LaunchBox Metadata.xml — if set, enrich the dataset.
    #[serde(default)]
    metadata: Option<PathBuf>,
    #[serde(default = "d_platform")]
    platform: String,
    /// Auto-detect color/B&W from LaunchBox screenshots during enrich.
    #[serde(default)]
    detect_color: bool,
    #[serde(default = "d_curl")]
    curl: String,
    /// Apps to harvest from donor images into the output.
    #[serde(default)]
    harvest: Vec<HarvestSrc>,
    /// Directory of source artwork named `<id>.png` / `.jpg` — converted to PICT.
    #[serde(default)]
    art_dir: Option<PathBuf>,
    #[serde(default = "d_artdepth")]
    art_depth: String,
    /// Generate multiple depth variants (e.g. ["1","8"]) named `<id>.<depth>.pict`
    /// with the catalog `image` set to the base path, so the launcher picks the
    /// variant matching the screen depth. Empty → a single `<id>.pict` at art_depth.
    #[serde(default)]
    art_depths: Vec<String>,
    /// Downscale art so its longest side is at most this many pixels.
    #[serde(default)]
    art_max: Option<u32>,
    /// Download Box-Front art from LaunchBox (needs `metadata`) when no local
    /// art_dir file exists for an item.
    #[serde(default)]
    download_art: bool,
    #[serde(default = "d_rbcli")]
    rb_cli: String,
    #[serde(default = "d_apps_root")]
    apps_root: String,
    #[serde(default = "d_metadir")]
    metadata_dir: String,
    #[serde(default = "d_imagesdir")]
    images_dir: String,
    /// Staging dir for intermediates (default: a temp dir).
    #[serde(default)]
    stage: Option<PathBuf>,
    /// Optional startup chime (PCM WAV) baked into the image; the launcher plays
    /// it on launch when the user turns Startup Sound on. Capped at 7 seconds.
    #[serde(default)]
    startup_sound: Option<PathBuf>,
    /// Optional shutdown chime (PCM WAV) — played on Shut Down when enabled.
    #[serde(default)]
    shutdown_sound: Option<PathBuf>,
    /// Where the chimes live on the volume (the launcher reads sounds/startup,
    /// sounds/shutdown under /MacAtrium).
    #[serde(default = "d_sounds_dir")]
    sounds_dir: String,
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
    cfg: &Config,
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
    // which `icons::app_icl8_png` resolves into a PNG for `pict`.
    let pngfile = stage.join(format!("{id}.icon.png"));
    if matches!(icons::app_icl8_png(&hqx_bytes, &pngfile), Ok(true)) {
        let pictfile = stage.join(format!("{id}.icon.8.pict"));
        if pict::run(&pngfile, &pictfile, pict::Depth::Eight, true, None).is_ok() {
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
    cfg: &Config,
    stage: &Path,
    images_rel: &str,
    name: &str,
    src: &Path,
    depths: &[pict::Depth],
    multi: bool,
) -> Result<Option<String>> {
    let mut any = false;
    for d in depths {
        let sfx = if multi { format!(".{}", d.bits()) } else { String::new() };
        // 1-bit ships as a raw CopyBits bitmap (.raw); colour depths as PICT.
        let raw = *d == pict::Depth::One;
        let ext = if raw { "raw" } else { "pict" };
        let stagefile = stage.join(format!("{name}{sfx}.{ext}"));
        let ok = if raw {
            pict::run_raw1(src, &stagefile, cfg.art_max).is_ok()
        } else {
            pict::run(src, &stagefile, *d, true, cfg.art_max).is_ok()
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
fn bake_sound(rb: &RbCli, cfg: &Config, stage: &Path, wav: &Path, name: &str) -> Result<()> {
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

pub fn run(config: &Path) -> Result<()> {
    let cfg: Config = serde_json::from_str(
        &std::fs::read_to_string(config).with_context(|| format!("reading {}", config.display()))?,
    )
    .with_context(|| format!("parsing config {}", config.display()))?;

    let rb = RbCli::new(&cfg.rb_cli);
    let stage = cfg
        .stage
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("atrium-image-stage"));
    std::fs::create_dir_all(&stage)?;

    // 1. base system -> out
    eprintln!("[1/7] base system  {} -> {}", cfg.system.display(), cfg.out.display());
    std::fs::copy(&cfg.system, &cfg.out)
        .with_context(|| format!("copying {} -> {}", cfg.system.display(), cfg.out.display()))?;

    // 2. working copy of the dataset (the build never mutates the source)
    let work = stage.join("dataset.jsonl");
    std::fs::copy(&cfg.dataset, &work)
        .with_context(|| format!("copying dataset {}", cfg.dataset.display()))?;

    // 3. harvest apps from donor images into the output + append stubs
    if !cfg.harvest.is_empty() {
        eprintln!("[2/7] harvest      {} donor source(s)", cfg.harvest.len());
        for h in &cfg.harvest {
            harvest::run(
                &cfg.rb_cli,
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

    // 5. manual overrides (win)
    if let Some(ov) = &cfg.overrides {
        eprintln!("[4/7] merge        overrides {}", ov.display());
        merge::run(&work, ov, &work, false)?;
    }

    // 6. art: gather sources (local art_dir wins; else download Box-Front from
    // LaunchBox), convert -> PICT, inject, set the catalog image field.
    if cfg.art_dir.is_some() || cfg.download_art {
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
                    if enrich::download(url, &dst, &cfg.curl).is_ok() {
                        downloaded.insert(id.to_string(), dst);
                    }
                }
                if let Some(url) = v.get("shot").and_then(Value::as_str) {
                    let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img");
                    let dst = dl_dir.join(format!("{id}.shot.{ext}"));
                    if enrich::download(url, &dst, &cfg.curl).is_ok() {
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
        for (id, app) in dataset_records(&work)? {
            // Box-Front (catalog `image`) and Screenshot (catalog `shot`); a local
            // art_dir wins, else the downloaded file. art_dir screenshots are named
            // `<id>.shot.<ext>`.
            let box_src = cfg.art_dir.as_ref().and_then(|adir| find_art(adir, &id))
                .or_else(|| downloaded.get(&id).cloned());
            let shot_name = format!("{id}.shot");
            let shot_src = cfg.art_dir.as_ref().and_then(|adir| find_art(adir, &shot_name))
                .or_else(|| downloaded_shot.get(&id).cloned());

            let mut fields = String::new();
            let mut has_box = false;
            if let Some(src) = &box_src {
                if let Some(rel) = bake_variants(&rb, &cfg, &stage, images_rel, &id, src, &depths, multi)? {
                    fields.push_str(&format!(",\"image\":{rel:?}"));
                    n += 1;
                    has_box = true;
                }
            }
            if let Some(src) = &shot_src {
                if let Some(rel) = bake_variants(&rb, &cfg, &stage, images_rel, &shot_name, src, &depths, multi)? {
                    fields.push_str(&format!(",\"shot\":{rel:?}"));
                    n_shot += 1;
                }
            }
            // App's own Finder icon (ICN#/icl8) for the list-row gutter — baked
            // for every item with an app. When the item has no box art, reuse it
            // as the big art-pane fallback (the old behaviour, now for all rows).
            if let Some(app) = app.as_deref().filter(|a| !a.is_empty()) {
                if let Some(icon_rel) = bake_icon(&rb, &cfg, &stage, images_rel, &id, app)? {
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
            "[5/7] art          {n} box-art + {n_shot} screenshot + {n_icon} app-icon item(s) at {depth_label}-bit"
        );
        if n + n_shot + n_icon > 0 {
            let ovf = stage.join("art-overlay.jsonl");
            std::fs::write(&ovf, overlay)?;
            merge::run(&work, &ovf, &work, false)?;
        }
    }

    // 7. catalog (generate + inject) and launcher install
    eprintln!("[6/7] catalog      generate + inject");
    let cat = stage.join("catalog.jsonl");
    let report = catalog::run(&work, &cat, false, false)?;
    catalog::inject(&cfg.rb_cli, &cfg.out, &cat, &cfg.metadata_dir, Some(&stage))?;

    eprintln!("[7/7] launcher     install into {}", cfg.startup_items);
    rb.mkdir_p(&cfg.out, &cfg.startup_items)?;
    rb.put_macbinary(&cfg.out, &cfg.launcher, &cfg.startup_items)?;

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
        "\nimage built: {} ({} items, {} categories)",
        cfg.out.display(),
        report.items,
        report.categories.len()
    );
    Ok(())
}
