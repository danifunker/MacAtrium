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

use anyhow::{Context, Result};
use atrium::{catalog, enrich, fetch, harvest, icons, image, merge, mg, pict, snd};
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
        /// Also emit the PAGED catalog tree here (docs/21): index.jsonl +
        /// cats/<slug>.jsonl + hotkeys.jsonl, slim records, categories split at
        /// MAX_CAT_ITEMS. The legacy --out file is still written.
        #[arg(long)]
        paged_out: Option<PathBuf>,
        /// Category DB for `--paged-out` membership (docs/21). Default
        /// data/categories.jsonl.
        #[arg(long, default_value = "data/categories.jsonl")]
        categories: PathBuf,
        /// Taxonomy for `--paged-out` category order. Default data/taxonomy.json.
        #[arg(long, default_value = "data/taxonomy.json")]
        taxonomy: PathBuf,
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

    /// Fill the curated dataset from the local Macintosh Garden archive
    /// (68K-only): year/vendor/genre/desc (de-HTML'd) + source attribution +
    /// offline colour detect. Optionally copy box-front/screenshot art into a dir.
    Mg {
        /// Curated source dataset to enrich (e.g. data/library.jsonl).
        #[arg(long)]
        src: PathBuf,
        /// Macintosh Garden data store (holds metadata/*.ndjson + <kind>/<nid>/).
        /// Defaults to $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive.
        #[arg(long = "mg-archive", env = "MACATRIUM_MG_ARCHIVE")]
        archive: Option<PathBuf>,
        /// Output dataset (may equal --src to enrich in place).
        #[arg(long)]
        out: PathBuf,
        /// Overwrite existing values instead of only filling missing ones.
        #[arg(long)]
        overwrite: bool,
        /// Copy each matched title's box-front + gameplay screenshot here as
        /// `<id>.<ext>` / `<id>.shot.<ext>` for the `image` art pass.
        #[arg(long)]
        art_dir: Option<PathBuf>,
    },

    /// Fetch a 68K title's software from the Macintosh Garden mirror, extract it
    /// with rb-cli (StuffIt/CompactPro/MAR/BinHex/MacBinary), and optionally inject
    /// the forks into an image under Apps/. On-demand, per-title (Phase 2).
    Fetch {
        /// Macintosh Garden data store (for metadata + per-title info.json).
        /// Defaults to $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive.
        #[arg(long = "mg-archive", env = "MACATRIUM_MG_ARCHIVE")]
        archive: Option<PathBuf>,
        /// MG node id(s) to fetch (repeatable).
        #[arg(long = "nid")]
        nids: Vec<i64>,
        /// Or: fetch for every dataset record that matches an MG title.
        #[arg(long)]
        src: Option<PathBuf>,
        /// Download cache dir (default: <archive>/downloads; never committed).
        #[arg(long)]
        downloads: Option<PathBuf>,
        /// Inject the extracted forks into this image under Apps/.
        #[arg(long)]
        into: Option<PathBuf>,
        #[arg(long, default_value = "/MacAtrium/Apps")]
        apps_root: String,
        /// Append a minimal dataset stub (id/name/kind/year/app pointing at the
        /// injected APPL) to this dataset, de-duped by id — so the fetched title
        /// shows in the catalog. `atrium mg`/`enrich` fill the rest later.
        #[arg(long)]
        append_to: Option<PathBuf>,
        #[arg(long, default_value = "rb-cli")]
        rb_cli: String,
        #[arg(long, default_value = "curl")]
        curl: String,
        /// Extraction staging dir (default: a temp dir).
        #[arg(long)]
        stage: Option<PathBuf>,
    },

    /// Capture manual data into the overrides overlay (the CLI "checkbox" for the
    /// color/mouse facets LaunchBox lacks, plus corrections). Upserts by id.
    Set {
        /// Overrides overlay to write (e.g. data/compatibility.jsonl).
        #[arg(long, default_value = "data/compatibility.jsonl")]
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
        /// Overrides overlay (partial records by id, e.g. data/compatibility.jsonl).
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
        /// Pixel depth: 1, 4, 8 (indexed) or 16/24 (direct). Default 8.
        #[arg(long, default_value = "8")]
        depth: String,
        /// Store rows uncompressed (skip PackBits) for indexed depths.
        #[arg(long)]
        no_pack: bool,
        /// Emit a raw 1-bit bitmap sidecar (CopyBits-ready) instead of a PICT.
        /// Implies 1-bit; the launcher blits it directly, dodging the Snow
        /// DrawPicture fault on some 1-bit art (docs/14).
        #[arg(long)]
        raw: bool,
        /// Downscale so the longest side is at most this many pixels (aspect kept).
        #[arg(long)]
        max: Option<u32>,
    },

    /// Extract an app's Finder icon (ICN#) from a BinHex (.hqx) export
    /// (`rb-cli get-binhex`) to a raw 1-bit bitmap (.raw) the launcher can blit.
    Icon {
        /// Source BinHex 4.0 (.hqx) of the app (both forks).
        #[arg(long)]
        hqx: PathBuf,
        /// Output .raw file (32x32 1-bit).
        #[arg(long)]
        out: PathBuf,
    },

    /// Bake a PCM WAV chime into a Mac sound file's resource fork (a `snd `
    /// resource id 128), for the launcher's startup/shutdown sound. Capped at
    /// 7 seconds. Write it to the volume with `rb-cli setrsrc` (`atrium image`
    /// does this for `startup_sound` / `shutdown_sound`).
    Snd {
        /// Source PCM WAV (8/16-bit, mono or stereo).
        #[arg(long)]
        wav: PathBuf,
        /// Output resource-fork file.
        #[arg(long)]
        out: PathBuf,
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

    /// Add titles to an already-built MacAtrium disk, in place: harvest the
    /// selected titles into the existing image, bake their art, and merge their
    /// catalog records with the disk's current catalog (existing titles keep
    /// their art). No base copy or launcher reinstall. Uses the same JSON config
    /// as `image`, where `out` is the existing disk and `selection` the new titles.
    Add {
        /// Add config (JSON): `out` = the existing MacAtrium .hda, `selection` =
        /// the titles to add (+ `base_os`/`art_depths` matching the disk's Target).
        #[arg(long)]
        config: PathBuf,
    },

    /// Inspect or patch the launcher's `'SIZE'` (-1) memory partition — the
    /// per-config `app_mem_kb` that `atrium image` bakes in. With no `--pref`,
    /// prints the current preferred/minimum; with `--pref` (and optional `--min`),
    /// writes a patched launcher (in place, or to `--out`). Handy for measuring a
    /// build's real peak by trying partition sizes without a full image rebuild.
    Size {
        /// Launcher MacBinary to read/patch (e.g. build/MacAtrium.bin).
        #[arg(long)]
        launcher: PathBuf,
        /// Preferred partition size in KB. Omit to just report the current values.
        #[arg(long)]
        pref: Option<u32>,
        /// Minimum partition size in KB (defaults to `--pref`; clamped <= pref).
        #[arg(long)]
        min: Option<u32>,
        /// Write the patched launcher here (default: patch `--launcher` in place).
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Library Builder: (re)generate the curated library from source disks.
    Library {
        #[command(subcommand)]
        action: LibraryCmd,
    },

    /// List the build Targets — named profiles (base OS + art depths + launcher
    /// RAM + disk size) the GUI's Target picker and a build select. Shows the
    /// bundled defaults overlaid with any user targets from ~/.macatrium.json.
    Targets,

    /// Explore the Macintosh Garden archive, cross-referenced against MacPack —
    /// to find what we're missing. Filters by type/architecture/OS/year/category
    /// (+ colour via the offline cache); `--missing` shows only titles NOT in
    /// the bundled library. With no filters, prints a summary breakdown.
    MgList {
        /// MG data store. Defaults to $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive.
        #[arg(long = "mg-archive", env = "MACATRIUM_MG_ARCHIVE")]
        archive: Option<PathBuf>,
        /// Restrict to games or apps.
        #[arg(long, value_parser = ["game", "app"])]
        kind: Option<String>,
        /// Architecture substring, e.g. "68k" or "PPC".
        #[arg(long)]
        arch: Option<String>,
        /// A supported-OS label that must be present, e.g. "Mac OS 7".
        #[arg(long)]
        system: Option<String>,
        #[arg(long)]
        min_year: Option<i64>,
        #[arg(long)]
        max_year: Option<i64>,
        /// Category/genre substring, e.g. "Adventure" or "Utilities".
        #[arg(long)]
        category: Option<String>,
        /// Only titles NOT in MacPack (the library) — the "what are we missing" view.
        #[arg(long, conflicts_with = "have")]
        missing: bool,
        /// Only titles already in MacPack.
        #[arg(long)]
        have: bool,
        /// Colour only (`--color`) or B&W only (`--bw`); uses the colour cache.
        #[arg(long, conflicts_with = "bw")]
        color: bool,
        #[arg(long)]
        bw: bool,
        /// Title substring search.
        #[arg(long)]
        search: Option<String>,
        /// Detect colour (offline, from screenshots) for the filtered set first,
        /// caching the results — so `--color`/`--bw` have data to filter on.
        #[arg(long)]
        detect_color: bool,
        /// Max rows to print (default 40; the summary count is always shown).
        #[arg(long, default_value = "40")]
        limit: usize,
        /// Print only the count + breakdown, no rows.
        #[arg(long)]
        count: bool,
    },

    /// View or update the user settings (~/.macatrium.json): machine-local source
    /// locations + tool paths. With no flags, prints the current settings.
    Config {
        /// Folder holding the MacPack donor disks (boot.vhd, Supplement.vhd, …).
        #[arg(long)]
        macpack_dir: Option<PathBuf>,
        /// Macintosh Garden archive (MG-Archive) root.
        #[arg(long)]
        mg_archive: Option<PathBuf>,
        /// rb-cli binary path.
        #[arg(long)]
        rb_cli: Option<String>,
        /// Download/work cache dir.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum LibraryCmd {
    /// Scan donor disks (the MacPack) and emit a comprehensive `library.jsonl` —
    /// one record per title (id/name/kind/year/genre/app/harvest_src), no copying.
    Scan {
        /// Folder holding the donor disks (e.g. the unzipped MacPack).
        #[arg(long)]
        macpack: PathBuf,
        /// Donor disk filename(s) within --macpack to scan; absolute paths allowed.
        /// Default: boot.vhd + Supplement.vhd.
        #[arg(long = "disk")]
        disks: Vec<String>,
        /// Output library.jsonl.
        #[arg(long)]
        out: PathBuf,
        /// MacPack release tag recorded in the header (e.g. 20240825-RC1).
        #[arg(long)]
        release: Option<String>,
        #[arg(long, default_value = "rb-cli")]
        rb_cli: String,
    },

    /// Move requirement/facet fields (color/mouse/maxDepth/minOS/maxOS/minMem/
    /// minCPU/arch) out of the library into the compatibility companion; existing
    /// (hand-verified) compatibility entries win. Run after `mg`/`enrich`.
    Split {
        /// The library.jsonl to strip the fields from (rewritten in place).
        #[arg(long)]
        library: PathBuf,
        /// The compatibility.jsonl to merge them into (created if absent).
        #[arg(long)]
        compat: PathBuf,
    },

    /// Seed/refresh the editable category DB (data/categories.jsonl, docs/21):
    /// assign each title to the navigation categories (multi-membership) from the
    /// taxonomy seed maps + facets. Hand/GUI edits already in the DB are preserved.
    Categorize {
        #[arg(long, default_value = "data/library.jsonl")]
        library: PathBuf,
        #[arg(long, default_value = "data/compatibility.jsonl")]
        compat: PathBuf,
        #[arg(long, default_value = "data/taxonomy.json")]
        taxonomy: PathBuf,
        /// The category DB to write/refresh.
        #[arg(long, default_value = "data/categories.jsonl")]
        out: PathBuf,
    },
}

/// Truncate a display string to `n` chars (… if cut), for table columns.
fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Catalog {
            src,
            out,
            lf,
            crlf,
            paged_out,
            categories,
            taxonomy,
            into,
            backup_dir,
            metadata_dir,
            rb_cli,
        } => {
            // Paged tree first (handles any library size, docs/21).
            if let Some(dir) = &paged_out {
                let pr = catalog::run_paged(&src, dir, Some(&categories), Some(&taxonomy), lf, crlf)?;
                eprintln!(
                    "paged: {} item(s) → {} categor(y/ies) in {} page(s), {} hotkey(s) -> {}",
                    pr.items, pr.categories, pr.pages, pr.hotkeys, dir.display()
                );
                eprintln!(
                    "  largest page: \"{}\" ({} items; cap {})",
                    pr.biggest_page.0, pr.biggest_page.1, atrium::catalog::MAX_CAT_ITEMS
                );
            }
            // Legacy single-file catalog (≤256). With --paged-out on a large
            // library this can't be produced — warn rather than fail the run.
            match catalog::run(&src, &out, lf, crlf) {
                Ok(report) => {
                    eprintln!(
                        "catalog: {} items, {} categories, {} bytes -> {}",
                        report.items, report.categories.len(), report.bytes, out.display()
                    );
                    for (name, n) in &report.categories {
                        eprintln!("  {:<24} {}", name, n);
                    }
                    if report.lossy_chars > 0 {
                        eprintln!("  warning: {} character(s) had no MacRoman equivalent (emitted '?')", report.lossy_chars);
                    }
                    for w in &report.warnings {
                        eprintln!("  warning: {w}");
                    }
                    if let Some(image) = into {
                        catalog::inject(&rb_cli, &image, &out, &metadata_dir, backup_dir.as_deref())?;
                    }
                }
                Err(e) if paged_out.is_some() => {
                    eprintln!("legacy single-file catalog skipped: {e}");
                }
                Err(e) => return Err(e),
            }
        }
        Cmd::Enrich { src, metadata, out, platform, overwrite, art_manifest, detect_color, curl } => {
            enrich::run(&src, &metadata, &out, &platform, overwrite, art_manifest.as_deref(), detect_color, &curl)?;
        }
        Cmd::Mg { src, archive, out, overwrite, art_dir } => {
            let archive = mg::resolve_archive(archive);
            eprintln!("mg: data store {}", archive.display());
            mg::run(&src, &archive, &out, overwrite, art_dir.as_deref())?;
        }
        Cmd::Fetch { archive, nids, src, downloads, into, apps_root, append_to, rb_cli, curl, stage } => {
            let archive = mg::resolve_archive(archive);
            eprintln!("fetch: data store {}", archive.display());
            fetch::run(
                &archive, &nids, src.as_deref(), downloads.as_deref(), into.as_deref(),
                &apps_root, append_to.as_deref(), &rb_cli, &curl, stage.as_deref(),
            )?;
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

        Cmd::Pict { input, out, depth, no_pack, raw, max } => {
            // CLI `--max` is a longest-side cap (square box); absent => no bound.
            let (mw, mh) = max.map(|m| (m, m)).unwrap_or((u32::MAX, u32::MAX));
            let s = if raw {
                pict::run_raw1(&input, &out, mw, mh)?
            } else {
                let d = pict::Depth::parse(&depth)?;
                pict::run(&input, &out, d, !no_pack, mw, mh)?
            };
            eprintln!(
                "pict: {}x{} {}-bit ({}) -> {} ({} bytes)",
                s.width,
                s.height,
                s.depth,
                if raw { "raw bitmap".into() } else if s.colors > 0 { format!("{} colors", s.colors) } else { "direct".into() },
                out.display(),
                s.bytes
            );
        }
        Cmd::Icon { hqx, out } => {
            let bytes = std::fs::read(&hqx)?;
            match icons::app_icon_raw1(&bytes)? {
                Some(raw) => {
                    std::fs::write(&out, &raw)?;
                    eprintln!("icon: 32x32 1-bit -> {} ({} bytes)", out.display(), raw.len());
                }
                None => anyhow::bail!("no usable ICN# in {}", hqx.display()),
            }
        }
        Cmd::Snd { wav, out } => {
            snd::run(&wav, &out)?;
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
                None,
            )?;
        }
        Cmd::Image { config } => {
            image::run_from_path(&config)?;
        }
        Cmd::Add { config } => {
            image::add_to_disk_from_path(&config)?;
        }
        Cmd::Size { launcher, pref, min, out } => {
            let mut bytes = std::fs::read(&launcher)
                .with_context(|| format!("reading launcher {}", launcher.display()))?;
            let (cur_p, cur_m) = atrium::size_rsrc::read_app_mem(&bytes)?;
            eprintln!(
                "{}: SIZE (-1) = {} KB preferred / {} KB minimum",
                launcher.display(),
                cur_p / 1024,
                cur_m / 1024
            );
            if let Some(p) = pref {
                let m = min.unwrap_or(p).min(p);
                atrium::size_rsrc::patch_app_mem(&mut bytes, p * 1024, m * 1024)?;
                let dst = out.unwrap_or_else(|| launcher.clone());
                std::fs::write(&dst, &bytes)
                    .with_context(|| format!("writing {}", dst.display()))?;
                eprintln!("patched -> {} KB / {} KB  ({})", p, m, dst.display());
            }
        }
        Cmd::Library { action } => match action {
            LibraryCmd::Scan { macpack, disks, out, release, rb_cli } => {
                let rb = atrium::rbcli::RbCli::new(&rb_cli);
                let names = if disks.is_empty() {
                    vec!["boot.vhd".to_string(), "Supplement.vhd".to_string()]
                } else {
                    disks
                };
                let resolved: Vec<(String, PathBuf)> = names
                    .into_iter()
                    .map(|n| {
                        let pb = PathBuf::from(&n);
                        let p = if pb.is_absolute() { pb } else { macpack.join(&n) };
                        (n, p)
                    })
                    .filter(|(n, p)| {
                        let ok = p.exists();
                        if !ok {
                            eprintln!("  skip donor (not found): {n}");
                        }
                        ok
                    })
                    .collect();
                anyhow::ensure!(
                    !resolved.is_empty(),
                    "no donor disks found in {}",
                    macpack.display()
                );
                for (n, _) in &resolved {
                    eprintln!("  scanning donor: {n}");
                }
                let report = atrium::library::scan(&rb, &resolved, &out, release.as_deref())?;
                eprintln!(
                    "library scan: {} title(s){} -> {}",
                    report.titles,
                    if report.dupes > 0 {
                        format!(" ({} duplicate id(s) skipped)", report.dupes)
                    } else {
                        String::new()
                    },
                    out.display()
                );
            }
            LibraryCmd::Split { library, compat } => {
                let r = atrium::library::split(&library, &compat)?;
                eprintln!(
                    "library split: moved facets for {} title(s); compatibility now {} entries -> {}",
                    r.moved,
                    r.compat_entries,
                    compat.display()
                );
            }
            LibraryCmd::Categorize { library, compat, taxonomy, out } => {
                let r = atrium::library::categorize(&library, &compat, &taxonomy, &out)?;
                eprintln!(
                    "categorize: {} title(s) — {} assigned, {} preserved, {} uncategorized -> {}",
                    r.titles, r.assigned, r.preserved, r.uncategorized, out.display()
                );
                eprintln!("by category:");
                for cat in atrium::catalog::Taxonomy::load(&taxonomy).map(|t| t.order).unwrap_or_default() {
                    if let Some(n) = r.per_category.get(&cat) {
                        eprintln!("  {n:5}  {cat}");
                    }
                }
            }
        },
        Cmd::Targets => {
            let reg = atrium::targets::Registry::load_default();
            let bundled = atrium::targets::Registry::bundled();
            eprintln!("targets ({} total):", reg.0.len());
            for (name, t) in &reg.0 {
                let origin = if bundled.get(name) == Some(t) { "bundled" } else { "user" };
                let depths = if t.art_depths.is_empty() { "-".into() } else { t.art_depths.join("/") };
                let mem = t.app_mem_kb.map(|[p, m]| format!("{p}/{m} KB")).unwrap_or_else(|| "default".into());
                eprintln!("  {name}  [{origin}]");
                eprintln!("      base OS {}  ·  art {depths}  ·  RAM {mem}", t.base_os);
                if !t.label.is_empty() {
                    eprintln!("      {}", t.label);
                }
            }
        }
        Cmd::MgList {
            archive, kind, arch, system, min_year, max_year, category,
            missing, have, color, bw, search, detect_color, limit, count,
        } => {
            use atrium::mgdb::{self, Filter, Kind};
            let archive = mg::resolve_archive(archive);
            eprintln!("mg-list: data store {}", archive.display());
            let mut entries = mgdb::load(
                &archive,
                atrium::config::EMBEDDED_LIBRARY,
                atrium::config::EMBEDDED_COMPAT,
            )?;
            eprintln!("loaded {} MG record(s)", entries.len());

            let filter = Filter {
                kind: kind.as_deref().map(|k| if k == "app" { Kind::App } else { Kind::Game }),
                arch,
                system,
                min_year,
                max_year,
                category,
                color: if color { Some(true) } else if bw { Some(false) } else { None },
                mouse: None,
                in_macpack: if missing { Some(false) } else if have { Some(true) } else { None },
                search,
            };

            // Offline colour detect, scoped to the set matching every OTHER filter,
            // so `--color`/`--bw` have data without scanning all ~21k screenshots.
            if detect_color {
                let mut base = filter.clone();
                base.color = None;
                let subset: Vec<mgdb::Entry> = entries.iter().filter(|e| base.matches(e)).cloned().collect();
                let mut cache = mgdb::load_color_cache(&archive);
                eprintln!("detecting colour for up to {} matched title(s) with screenshots…", subset.len());
                let n = mgdb::detect_color(&archive, &subset, &mut cache, |d, t| {
                    if d > 0 && d % 50 == 0 { eprintln!("  …{d}/{t}"); }
                });
                mgdb::save_color_cache(&archive, &cache)?;
                eprintln!("detected colour for {n} new title(s)");
                for e in &mut entries {
                    if e.color.is_none() {
                        e.color = cache.get(&e.nid).copied();
                    }
                }
            }

            let matches: Vec<&mgdb::Entry> = entries.iter().filter(|e| filter.matches(e)).collect();
            let total = matches.len();
            let games = matches.iter().filter(|e| e.kind == Kind::Game).count();
            let k68 = matches.iter().filter(|e| e.is_68k()).count();
            let in_mp = matches.iter().filter(|e| e.in_macpack).count();
            eprintln!(
                "\n{total} match(es): {games} game(s) + {} app(s); {k68} run on 68k; {in_mp} already in MacPack, {} missing",
                total - games,
                total - in_mp
            );
            let mut catc: std::collections::BTreeMap<String, usize> = Default::default();
            for e in &matches {
                for c in &e.categories {
                    *catc.entry(c.clone()).or_default() += 1;
                }
            }
            let mut cats: Vec<(String, usize)> = catc.into_iter().collect();
            cats.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            if !cats.is_empty() {
                eprintln!("top categories:");
                for (c, n) in cats.iter().take(15) {
                    eprintln!("  {n:5}  {c}");
                }
            }

            if !count {
                eprintln!("\nshowing {} of {total}:", total.min(limit));
                for e in matches.iter().take(limit) {
                    let os = match (e.min_os(), e.max_os()) {
                        (Some(a), Some(b)) if a != b => format!("{a} – {b}"),
                        (Some(a), _) => a.to_string(),
                        _ => "?".into(),
                    };
                    let col = match e.color { Some(true) => "colour", Some(false) => "B&W", None => "?" };
                    let mark = if e.in_macpack { " " } else { "*" }; // * = missing from MacPack
                    eprintln!(
                        "{mark} {:<38} {:>4}  {:<8} {:<20} {:<6}  {}",
                        trunc(&e.title, 38),
                        e.year.map(|y| y.to_string()).unwrap_or_default(),
                        trunc(&e.arch.join("/"), 8),
                        trunc(&os, 20),
                        col,
                        trunc(&e.categories.join(", "), 30),
                    );
                }
                eprintln!("(* = missing from MacPack)");
            }
        }
        Cmd::Config { macpack_dir, mg_archive, rb_cli, cache_dir } => {
            use atrium::settings::{default_path, Settings};
            let path = default_path();
            let mut s = Settings::load(&path);
            let mut changed = false;
            if macpack_dir.is_some() { s.macpack_dir = macpack_dir; changed = true; }
            if mg_archive.is_some() { s.mg_archive = mg_archive; changed = true; }
            if rb_cli.is_some() { s.rb_cli = rb_cli; changed = true; }
            if cache_dir.is_some() { s.cache_dir = cache_dir; changed = true; }
            if changed {
                s.save(&path)?;
                eprintln!("updated {}", path.display());
            }
            let show = |label: &str, v: Option<String>| {
                eprintln!("  {label:<12} {}", v.unwrap_or_else(|| "(unset)".into()));
            };
            eprintln!("settings ({}):", path.display());
            show("macpack_dir", s.macpack_dir.map(|p| p.display().to_string()));
            show("mg_archive", s.mg_archive.map(|p| p.display().to_string()));
            show("rb_cli", s.rb_cli);
            show("cache_dir", s.cache_dir.map(|p| p.display().to_string()));
        }
    }
    Ok(())
}
