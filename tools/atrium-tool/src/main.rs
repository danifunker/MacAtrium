//! `atrium` — MacAtrium host build tooling (docs/06, docs/13 Priority 1).
//!
//! Pure Rust, no native dependencies, so it builds and runs identically on
//! macOS, Windows, and Linux — the cross-platform, CI-able home for everything
//! the 68k launcher can't do itself: compiling the curated dataset into the
//! on-Mac catalog, converting artwork to PICT, harvesting apps out of donor HFS
//! images, and assembling a bootable appliance image.
//!
//! Today `catalog` is implemented; `pict`, `harvest`, and `image` are scaffolded
//! stubs that describe their planned behaviour.

use anyhow::Result;
use atrium::{catalog, enrich, harvest, image, merge, pict};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "atrium", version, about = "MacAtrium host build tooling")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Compile a curated dataset into an on-Mac catalog.jsonl (faceted
    /// categories, CR line endings, MacRoman encoding).
    Catalog {
        /// Curated source dataset (UTF-8 JSONL, e.g. data/library.jsonl).
        #[arg(long)]
        src: PathBuf,
        /// Output catalog path (write to the volume with rb-cli put --type TEXT).
        #[arg(long)]
        out: PathBuf,
        /// Emit LF line endings (host debugging) instead of CR (the device default).
        #[arg(long, conflicts_with = "crlf")]
        lf: bool,
        /// Emit CRLF line endings instead of bare CR.
        #[arg(long)]
        crlf: bool,
        /// Also inject the catalog into this target image's metadata dir,
        /// backing up any existing catalog first (rb-cli get).
        #[arg(long)]
        into: Option<PathBuf>,
        /// Where to save the backed-up existing catalog (default: next to --out).
        #[arg(long)]
        backup_dir: Option<PathBuf>,
        /// Metadata dir inside the image.
        #[arg(long, default_value = "/MacAtrium/metadata")]
        metadata_dir: String,
        /// Path to the rb-cli binary (for --into).
        #[arg(long, default_value = "rb-cli")]
        rb_cli: String,
    },

    /// Fill the curated dataset from the LaunchBox Games Database (Metadata.xml):
    /// year, vendor (publisher), and genre[], matched by name, without clobbering
    /// curated values. Optionally emit a Box-Front art manifest.
    Enrich {
        /// Curated source dataset to enrich (e.g. data/library.jsonl).
        #[arg(long)]
        src: PathBuf,
        /// LaunchBox Metadata.xml (unzip of gamesdb.launchbox-app.com/Metadata.zip).
        #[arg(long)]
        metadata: PathBuf,
        /// Output dataset (may equal --src to enrich in place).
        #[arg(long)]
        out: PathBuf,
        /// LaunchBox platform to match.
        #[arg(long, default_value = "Apple Mac OS")]
        platform: String,
        /// Overwrite existing values instead of only filling missing ones.
        #[arg(long)]
        overwrite: bool,
        /// Write a JSONL manifest of Box-Front art URLs (id, databaseID, art).
        #[arg(long)]
        art_manifest: Option<PathBuf>,
        /// Auto-detect color/B&W from a gameplay screenshot (downloads images).
        #[arg(long)]
        detect_color: bool,
        /// curl binary used to download screenshots for --detect-color.
        #[arg(long, default_value = "curl")]
        curl: String,
    },

    /// Capture manual data into the overrides overlay (the CLI "checkbox" for the
    /// color/mouse facets LaunchBox lacks, plus corrections). Upserts by id.
    Set {
        /// Overrides overlay to write (e.g. data/overrides.jsonl).
        #[arg(long, default_value = "data/overrides.jsonl")]
        overlay: PathBuf,
        /// The item id to set.
        #[arg(long)]
        id: String,
        /// Mark as Color (--color) or B&W (--bw).
        #[arg(long, conflicts_with = "bw")]
        color: bool,
        #[arg(long)]
        bw: bool,
        /// Mark Mouse Required (--mouse) or No Mouse (--no-mouse).
        #[arg(long, conflicts_with = "no_mouse")]
        mouse: bool,
        #[arg(long)]
        no_mouse: bool,
        #[arg(long)]
        year: Option<i64>,
        #[arg(long)]
        vendor: Option<String>,
        /// Comma-separated genres, e.g. "Action,Puzzle".
        #[arg(long)]
        genre: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        image: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        app: Option<String>,
    },

    /// Apply a manual overrides overlay (partial records by id) onto the dataset:
    /// overlay fields win (manual corrections + color/mouse LaunchBox lacks);
    /// overlay ids not in the base are appended as new records.
    Merge {
        /// Base dataset (e.g. data/library.jsonl).
        #[arg(long)]
        base: PathBuf,
        /// Overrides overlay (partial records by id, e.g. data/overrides.jsonl).
        #[arg(long)]
        overlay: PathBuf,
        /// Output dataset (may equal --base to merge in place).
        #[arg(long)]
        out: PathBuf,
        /// Only fill missing fields instead of letting the overlay win.
        #[arg(long)]
        fill_missing: bool,
    },

    /// Convert PNG/JPEG artwork to a classic-Mac PICT file at a given depth.
    Pict {
        /// Source image (PNG or JPEG).
        #[arg(long)]
        input: PathBuf,
        /// Output .pict file.
        #[arg(long)]
        out: PathBuf,
        /// Pixel depth: 1, 4, 8 (indexed) or 16 (direct). Default 8.
        #[arg(long, default_value = "8")]
        depth: String,
        /// Store rows uncompressed (skip PackBits) for indexed depths.
        #[arg(long)]
        no_pack: bool,
        /// Downscale so the longest side is at most this many pixels (aspect kept).
        #[arg(long)]
        max: Option<u32>,
    },

    /// Harvest apps out of a donor HFS image (a MacPack .vhd) into the
    /// /MacAtrium tree: extract both forks, stage them, emit dataset stubs.
    Harvest {
        /// Source HFS image to harvest from (e.g. ~/macpack-work/boot.vhd).
        #[arg(long)]
        image: PathBuf,
        /// A source app folder to harvest (repeatable), e.g. "/Games/1986/Dark Castle 1.2".
        #[arg(long = "app")]
        apps: Vec<String>,
        /// Harvest every subfolder of this source dir as an app, e.g. "/Games/1986".
        #[arg(long)]
        scan: Option<String>,
        /// Staging dir for extracted .hqx forks + harvested.jsonl stubs.
        #[arg(long)]
        stage: PathBuf,
        /// Optionally inject the forks straight into this target image.
        #[arg(long)]
        into: Option<PathBuf>,
        /// Target Apps root inside the image.
        #[arg(long, default_value = "/MacAtrium/Apps")]
        apps_root: String,
        /// Append harvested stubs to this curated dataset (de-duped by id), so
        /// populating is incremental — run repeatedly without losing curation.
        #[arg(long)]
        append_to: Option<PathBuf>,
        /// Path to the rb-cli binary (defaults to `rb-cli` on PATH).
        #[arg(long, default_value = "rb-cli")]
        rb_cli: String,
    },

    /// Assemble a full bootable appliance image end-to-end from a JSON config:
    /// base system → harvest → enrich → merge → art → catalog → launcher.
    Image {
        /// Build config (JSON). See README for the schema.
        #[arg(long)]
        config: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Catalog {
            src,
            out,
            lf,
            crlf,
            into,
            backup_dir,
            metadata_dir,
            rb_cli,
        } => {
            let report = catalog::run(&src, &out, lf, crlf)?;
            eprintln!(
                "catalog: {} items, {} categories, {} bytes -> {}",
                report.items,
                report.categories.len(),
                report.bytes,
                out.display()
            );
            for (name, n) in &report.categories {
                eprintln!("  {:<24} {}", name, n);
            }
            if report.lossy_chars > 0 {
                eprintln!(
                    "  warning: {} character(s) had no MacRoman equivalent (emitted '?')",
                    report.lossy_chars
                );
            }
            for w in &report.warnings {
                eprintln!("  warning: {w}");
            }
            if let Some(image) = into {
                catalog::inject(&rb_cli, &image, &out, &metadata_dir, backup_dir.as_deref())?;
            }
        }
        Cmd::Enrich { src, metadata, out, platform, overwrite, art_manifest, detect_color, curl } => {
            enrich::run(&src, &metadata, &out, &platform, overwrite, art_manifest.as_deref(), detect_color, &curl)?;
        }

        Cmd::Set {
            overlay, id, color, bw, mouse, no_mouse,
            year, vendor, genre, desc, image, name, app,
        } => {
            use serde_json::{Map, Value};
            let mut f = Map::new();
            if color { f.insert("color".into(), Value::Bool(true)); }
            if bw { f.insert("color".into(), Value::Bool(false)); }
            if mouse { f.insert("mouse".into(), Value::Bool(true)); }
            if no_mouse { f.insert("mouse".into(), Value::Bool(false)); }
            if let Some(y) = year { f.insert("year".into(), Value::from(y)); }
            if let Some(v) = vendor { f.insert("vendor".into(), Value::from(v)); }
            if let Some(g) = genre {
                let arr: Vec<Value> = g.split(',').map(|s| Value::from(s.trim())).collect();
                f.insert("genre".into(), Value::Array(arr));
            }
            if let Some(d) = desc { f.insert("desc".into(), Value::from(d)); }
            if let Some(i) = image { f.insert("image".into(), Value::from(i)); }
            if let Some(n) = name { f.insert("name".into(), Value::from(n)); }
            if let Some(a) = app { f.insert("app".into(), Value::from(a)); }
            if f.is_empty() {
                anyhow::bail!("nothing to set — pass --color/--bw, --mouse/--no-mouse, or a field");
            }
            merge::set(&overlay, &id, &f)?;
        }

        Cmd::Merge { base, overlay, out, fill_missing } => {
            merge::run(&base, &overlay, &out, fill_missing)?;
        }

        Cmd::Pict { input, out, depth, no_pack, max } => {
            let d = pict::Depth::parse(&depth)?;
            let s = pict::run(&input, &out, d, !no_pack, max)?;
            eprintln!(
                "pict: {}x{} {}-bit ({} colors) -> {} ({} bytes)",
                s.width,
                s.height,
                s.depth,
                if s.colors > 0 { s.colors.to_string() } else { "direct".into() },
                out.display(),
                s.bytes
            );
        }
        Cmd::Harvest {
            image,
            apps,
            scan,
            stage,
            into,
            apps_root,
            append_to,
            rb_cli,
        } => {
            harvest::run(
                &rb_cli,
                &image,
                &apps,
                scan.as_deref(),
                &stage,
                into.as_deref(),
                &apps_root,
                append_to.as_deref(),
            )?;
        }
        Cmd::Image { config } => {
            image::run(&config)?;
        }
    }
    Ok(())
}
