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

mod catalog;
mod macroman;

use anyhow::Result;
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
    },

    /// (planned) Convert PNG/JPG artwork to PICT (1-bit + 8-bit depth variants).
    Pict {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// (planned) Harvest apps out of a donor HFS image into the /MacAtrium tree.
    Harvest {
        #[arg(long)]
        image: Option<PathBuf>,
    },

    /// (planned) Assemble a full bootable appliance image end-to-end.
    Image {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Catalog { src, out, lf, crlf } => {
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
        }
        Cmd::Pict { .. } => {
            return not_yet(
                "pict",
                "PNG/JPG → PICT with 1-bit + 8-bit depth variants, sized to the \
                 target resolution; written into /MacAtrium/images and referenced \
                 by the catalog's `image` field (docs/06 Images).",
            );
        }
        Cmd::Harvest { .. } => {
            return not_yet(
                "harvest",
                "enumerate apps in a donor HFS image (via rb-cli ls), extract the \
                 chosen ones with both forks (get-binhex), and lay them into \
                 /MacAtrium/Apps — emitting matching dataset stubs to enrich.",
            );
        }
        Cmd::Image { .. } => {
            return not_yet(
                "image",
                "orchestrate a full build: generate the catalog, harvest apps, \
                 convert art, install the launcher, and emit a bootable .hda — \
                 retiring the bash assemble.sh (docs/13 Priority 1).",
            );
        }
    }
    Ok(())
}

fn not_yet(name: &str, plan: &str) -> Result<()> {
    anyhow::bail!("`atrium {name}` is not implemented yet. Planned: {plan}");
}
