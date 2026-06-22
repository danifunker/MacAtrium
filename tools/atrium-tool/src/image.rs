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
use crate::{catalog, enrich, harvest, merge, pict};
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
}

fn dataset_ids(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut ids = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            if let Some(id) = v.get("id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
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

        // Downloaded Box-Front art, id -> local file (from the enrich manifest).
        let mut downloaded: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
        if cfg.download_art {
            let manifest = stage.join("art-manifest.jsonl");
            let dl_dir = stage.join("art-dl");
            std::fs::create_dir_all(&dl_dir)?;
            for line in std::fs::read_to_string(&manifest).unwrap_or_default().lines() {
                let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
                let (Some(id), Some(url)) = (
                    v.get("id").and_then(Value::as_str),
                    v.get("art").and_then(Value::as_str),
                ) else { continue };
                let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img");
                let dst = dl_dir.join(format!("{id}.{ext}"));
                if enrich::download(url, &dst, &cfg.curl).is_ok() {
                    downloaded.insert(id.to_string(), dst);
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
        for id in dataset_ids(&work)? {
            let src = cfg
                .art_dir
                .as_ref()
                .and_then(|adir| find_art(adir, &id))
                .or_else(|| downloaded.get(&id).cloned());
            if let Some(src) = src {
                let mut any = false;
                for d in &depths {
                    let sfx = if multi { format!(".{}", d.bits()) } else { String::new() };
                    // 1-bit art ships as a raw CopyBits-ready bitmap (.raw), not a
                    // PICT: the launcher blits it directly, dodging the Snow
                    // DrawPicture fault on some valid 1-bit art (docs/14). Colour
                    // depths stay PICT (DrawPicture is fine there).
                    let raw = *d == pict::Depth::One;
                    let ext = if raw { "raw" } else { "pict" };
                    let stagefile = stage.join(format!("{id}{sfx}.{ext}"));
                    let ok = if raw {
                        pict::run_raw1(&src, &stagefile, cfg.art_max).is_ok()
                    } else {
                        pict::run(&src, &stagefile, *d, true, cfg.art_max).is_ok()
                    };
                    if !ok {
                        continue; // skip art that won't decode rather than fail
                    }
                    let dst = format!("{}/{}{}.{}", cfg.images_dir.trim_end_matches('/'), id, sfx, ext);
                    let (ftype, creator) = if raw { ("ABMP", "ttxt") } else { ("PICT", "ttxt") };
                    rb.put_typed(&cfg.out, &stagefile, &dst, ftype, creator)?;
                    any = true;
                }
                if any {
                    // base path for variants; explicit ext for the single case
                    let rel = if multi {
                        format!("{images_rel}/{id}")
                    } else {
                        let ext = if depths[0] == pict::Depth::One { "raw" } else { "pict" };
                        format!("{images_rel}/{id}.{ext}")
                    };
                    overlay.push_str(&format!("{{\"id\":{id:?},\"image\":{rel:?}}}\n"));
                    n += 1;
                }
            }
        }
        let depth_label = if multi { cfg.art_depths.join("/") } else { cfg.art_depth.clone() };
        eprintln!("[5/7] art          {n} item(s) at {depth_label}-bit ({} downloaded)", downloaded.len());
        if n > 0 {
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

    eprintln!(
        "\nimage built: {} ({} items, {} categories)",
        cfg.out.display(),
        report.items,
        report.categories.len()
    );
    Ok(())
}
