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
    /// Apps to harvest from donor images into the output.
    #[serde(default)]
    harvest: Vec<HarvestSrc>,
    /// Directory of source artwork named `<id>.png` / `.jpg` — converted to PICT.
    #[serde(default)]
    art_dir: Option<PathBuf>,
    #[serde(default = "d_artdepth")]
    art_depth: String,
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

    // 4. enrich from LaunchBox (fills gaps only)
    if let Some(md) = &cfg.metadata {
        eprintln!("[3/7] enrich       LaunchBox \"{}\"", cfg.platform);
        enrich::run(&work, md, &work, &cfg.platform, false, None)?;
    }

    // 5. manual overrides (win)
    if let Some(ov) = &cfg.overrides {
        eprintln!("[4/7] merge        overrides {}", ov.display());
        merge::run(&work, ov, &work, false)?;
    }

    // 6. art: convert <id>.png/jpg -> PICT, inject, set the catalog image field
    if let Some(adir) = &cfg.art_dir {
        let depth = pict::Depth::parse(&cfg.art_depth)?;
        rb.mkdir_p(&cfg.out, &cfg.images_dir)?;
        let mut overlay = String::new();
        let mut n = 0;
        for id in dataset_ids(&work)? {
            if let Some(src) = find_art(adir, &id) {
                let pictfile = stage.join(format!("{id}.pict"));
                pict::run(&src, &pictfile, depth, true)?;
                let dst = format!("{}/{}.pict", cfg.images_dir.trim_end_matches('/'), id);
                rb.put_typed(&cfg.out, &pictfile, &dst, "PICT", "ttxt")?;
                let rel = dst.strip_prefix("/MacAtrium/").unwrap_or(&dst);
                overlay.push_str(&format!("{{\"id\":{id:?},\"image\":{rel:?}}}\n"));
                n += 1;
            }
        }
        eprintln!("[5/7] art          {n} PICT(s) at {}-bit", cfg.art_depth);
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
