//! `atrium catalog` — compile a curated source dataset into the on-Mac
//! `catalog.jsonl` (schema v2, docs/06).
//!
//! The source dataset (`data/library.jsonl`) is PR-friendly UTF-8 JSONL keyed by
//! `id`, carrying *facet* fields (year, vendor, color, mouse, genre). This tool
//! derives the many-to-many `categories` array the launcher navigates — the
//! "facets + decade buckets" model — and emits CR-delimited MacRoman bytes that
//! rb-cli writes to the volume as type `TEXT`.

use crate::macroman;
use crate::rbcli::RbCli;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

// On-device parser limits (src/catalog.h, src/model.h). We validate against
// these so a generated catalog never silently overflows the 68k reader.
/// Hard cap on catalog items the 68k reader accepts (legacy single-file catalog).
/// A merge (`atrium add`) must keep the union within this.
pub const MAX_ITEMS: usize = 256;

/// Paged catalog (docs/21): the most items in a single category **page**. The
/// generator splits any larger category into numbered sub-pages, so the launcher
/// never holds more than this many slim records at once — the bound that keeps a
/// 4 MB Mac Plus within its partition. Sized for the smallest (B&W) target.
pub const MAX_CAT_ITEMS: usize = 128;
const MAX_ITEM_CATS: usize = 8;
const ITEM_ID_LEN: usize = 47;
const ITEM_NAME_LEN: usize = 63;
const ITEM_PATH_LEN: usize = 191;
const ITEM_CAT_LEN: usize = 31;
const ITEM_DESC_LEN: usize = 255;
const ITEM_VENDOR_LEN: usize = 39;
const ITEM_GENRE_LEN: usize = 63;
const MAX_CATS: usize = 64; // distinct named categories (excludes synthesized "All")
const ITEM_SOURCE_LEN: usize = 39;
// CD-title fields (docs/45), mirroring the device buffers (src/catalog.h) minus NUL.
const ITEM_CDIMG_LEN: usize = 63;
const ITEM_CDVOL_LEN: usize = 27;
const ITEM_CDAPP_LEN: usize = 127;

/// One curated record from the source dataset. Only `id`, `name`, `app` are
/// required; the facet fields are optional and drive category derivation.
#[derive(Debug, Deserialize)]
struct SourceItem {
    id: String,
    name: String,
    app: String,
    /// "game" (default) | "app" | "utility" — the top-level category.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    vendor: Option<String>,
    /// true → "Color", false → "B&W", absent → no colour facet.
    #[serde(default)]
    color: Option<bool>,
    /// Max colour depth (bpp: 1/4/8/16/32) this title tolerates. Some titles
    /// refuse or crash above a given depth (Dark Castle needs 1-bit), so the
    /// launcher drops the screen to the closest available depth ≤ this before
    /// launching, then restores. Absent → launch at the current screen depth.
    /// This facet has no metadata source — it lives in the curated overrides DB.
    #[serde(rename = "maxDepth", default)]
    max_depth: Option<i64>,
    /// true → "Mouse Required", false → "No Mouse", absent → no mouse facet.
    #[serde(default)]
    mouse: Option<bool>,
    /// Extra genre categories, e.g. ["Action", "Platformer"].
    #[serde(default)]
    genre: Vec<String>,
    /// Manual extra categories, e.g. ["Recommended"] (preserve dataset order).
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    image: Option<String>,
    /// Optional second artwork (gameplay screenshot) base path.
    #[serde(default)]
    shot: Option<String>,
    /// Optional small per-item icon base path (the app's Finder icon), drawn in
    /// the launcher's list-row gutter. Resolved as a depth variant like art.
    #[serde(default)]
    icon: Option<String>,
    #[serde(rename = "type", default)]
    type_: Option<String>,
    #[serde(default)]
    creator: Option<String>,
    /// Optional attribution for the metadata/art source (e.g. "Macintosh
    /// Garden"), shown in the More Info card. Passed through to the catalog.
    #[serde(default)]
    source: Option<String>,
    /// Optional per-item launch hotkey (a single character): the launcher
    /// launches this title when the key is pressed. Doubles as a per-item
    /// gamepad-button map (MiSTer maps joystick buttons → keystrokes).
    #[serde(default)]
    hotkey: Option<String>,
    /// CD-based title (docs/45): the host SD-card CD image filename (e.g.
    /// "MYST.iso"), matched case-insensitively against the BlueSCSI Toolbox
    /// LIST CDS enumeration and auto-inserted before launch. Curated override.
    #[serde(rename = "cdImage", default)]
    cd_image: Option<String>,
    /// true → the CD must mount before this title launches; false → optional.
    /// Absent → the device defaults it to required for any CD title.
    #[serde(rename = "cdRequired", default)]
    cd_required: Option<bool>,
    /// Expected Mac HFS volume name of the mounted CD (fast-path + verification).
    #[serde(rename = "cdVolume", default)]
    cd_volume: Option<String>,
    /// Run-from-CD app path, relative to the CD volume ROOT. Present → launch this
    /// app off the mounted CD; absent → app-on-HD (launch `app` under /MacAtrium
    /// with the CD mounted only as a data volume).
    #[serde(rename = "cdApp", default)]
    cd_app: Option<String>,
}

/// The on-Mac record. Field order here is the emitted JSON field order.
#[derive(Debug, Serialize)]
struct OutItem {
    id: String,
    name: String,
    categories: Vec<String>,
    app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<i64>,
    /// Developer/publisher, shown in the launcher detail + More Info card.
    #[serde(skip_serializing_if = "Option::is_none")]
    vendor: Option<String>,
    /// Genres joined for display (e.g. "Action, Platformer"); navigation still
    /// uses the `categories` array.
    #[serde(skip_serializing_if = "Option::is_none")]
    genre: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    /// Second artwork (gameplay screenshot) base path; the launcher can show it
    /// instead of the box art per the user's Artwork setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    shot: Option<String>,
    /// Single-character launch hotkey (gamepad button map); omitted if none.
    #[serde(skip_serializing_if = "Option::is_none")]
    hotkey: Option<String>,
    /// Small per-item list-row icon base path; omitted if none.
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    /// Metadata/art attribution (e.g. "Macintosh Garden"); omitted if none.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// Max colour depth (bpp) the launcher caps the screen to before launching
    /// this title; omitted → no cap (launch at the current depth).
    #[serde(rename = "maxDepth", skip_serializing_if = "Option::is_none")]
    max_depth: Option<i64>,
    /// CD-based title (docs/45): host CD image filename, auto-inserted via the
    /// BlueSCSI Toolbox before launch; omitted → not a CD title.
    #[serde(rename = "cdImage", skip_serializing_if = "Option::is_none")]
    cd_image: Option<String>,
    /// Whether the disc must mount to launch; omitted → device default (required).
    #[serde(rename = "cdRequired", skip_serializing_if = "Option::is_none")]
    cd_required: Option<bool>,
    /// Expected mounted CD volume name (fast-path + verification); omitted if none.
    #[serde(rename = "cdVolume", skip_serializing_if = "Option::is_none")]
    cd_volume: Option<String>,
    /// Run-from-CD app path relative to the CD volume root; omitted → app-on-HD.
    #[serde(rename = "cdApp", skip_serializing_if = "Option::is_none")]
    cd_app: Option<String>,
}

/// Top-level category for a `kind` value.
fn kind_category(kind: Option<&str>) -> &'static str {
    match kind.unwrap_or("game") {
        "app" | "application" => "Applications",
        "utility" | "util" => "Utilities",
        _ => "Games",
    }
}

/// Decade bucket for a year, e.g. 1986 → "1980s", 1996 → "1990s".
fn decade_bucket(year: i64) -> String {
    format!("{}s", (year / 10) * 10)
}

/// Derive the many-to-many `categories` array for one item, in display priority
/// order, de-duplicated (case-insensitively, first spelling wins).
fn derive_categories(it: &SourceItem) -> Vec<String> {
    let mut cats: Vec<String> = Vec::new();
    let push = |cats: &mut Vec<String>, c: String| {
        if !c.is_empty() && !cats.iter().any(|e| e.eq_ignore_ascii_case(&c)) {
            cats.push(c);
        }
    };

    push(&mut cats, kind_category(it.kind.as_deref()).to_string());
    for g in &it.genre {
        push(&mut cats, g.trim().to_string());
    }
    // Colour facet: an explicit `color` (a manual override or screenshot
    // detection) always wins; otherwise a pre-1987 title falls back to B&W, since
    // the Mac was 1-bit-only until the Mac II (1987).
    let color = it.color.or_else(|| it.year.filter(|&y| y < 1987).map(|_| false));
    if let Some(color) = color {
        push(&mut cats, if color { "Color" } else { "B&W" }.to_string());
    }
    if let Some(year) = it.year {
        push(&mut cats, decade_bucket(year));
    }
    if let Some(v) = &it.vendor {
        push(&mut cats, v.trim().to_string());
    }
    if let Some(mouse) = it.mouse {
        push(
            &mut cats,
            if mouse { "Mouse Required" } else { "No Mouse" }.to_string(),
        );
    }
    for c in &it.categories {
        push(&mut cats, c.trim().to_string());
    }
    cats
}

/// Outcome of a generation run, returned for reporting/testing.
pub struct Report {
    pub items: usize,
    pub categories: BTreeMap<String, usize>, // name -> item count
    pub warnings: Vec<String>,
    pub lossy_chars: usize,
    pub bytes: usize,
}

/// Parse + validate + facet a whole source dataset into `OutItem`s.
/// Lines that are blank or begin with `#` / `//` are skipped (comments).
fn build(src_text: &str) -> Result<(Vec<OutItem>, Report)> {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut cat_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_ids: BTreeMap<String, usize> = BTreeMap::new();
    // Launch hotkey (lowercased) -> line it was first claimed on, for dup warnings.
    let mut seen_hotkeys: BTreeMap<char, usize> = BTreeMap::new();

    for (lineno, raw) in src_text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let ln = lineno + 1;
        let it: SourceItem = serde_json::from_str(line)
            .with_context(|| format!("line {ln}: invalid source record"))?;

        // Required-field length guards (match on-device buffer sizes).
        let mut warn = |w: String| warnings.push(format!("line {ln} ({}): {w}", it.id));
        if it.id.chars().count() > ITEM_ID_LEN {
            warn(format!("id longer than {ITEM_ID_LEN} chars"));
        }
        if it.name.chars().count() > ITEM_NAME_LEN {
            warn(format!("name longer than {ITEM_NAME_LEN} chars"));
        }
        if it.app.chars().count() > ITEM_PATH_LEN {
            warn(format!("app path longer than {ITEM_PATH_LEN} chars"));
        }
        if let Some(v) = &it.cd_image {
            if v.chars().count() > ITEM_CDIMG_LEN {
                warn(format!("cdImage longer than {ITEM_CDIMG_LEN} chars (will truncate on device)"));
            }
        }
        if let Some(v) = &it.cd_volume {
            if v.chars().count() > ITEM_CDVOL_LEN {
                warn(format!("cdVolume longer than {ITEM_CDVOL_LEN} chars (will truncate on device)"));
            }
        }
        if let Some(v) = &it.cd_app {
            if v.chars().count() > ITEM_CDAPP_LEN {
                warn(format!("cdApp longer than {ITEM_CDAPP_LEN} chars (will truncate on device)"));
            }
        }
        if let Some(d) = &it.desc {
            if d.chars().count() > ITEM_DESC_LEN {
                warn(format!("desc longer than {ITEM_DESC_LEN} chars (will truncate on device)"));
            }
        }
        if let Some(v) = &it.vendor {
            if v.chars().count() > ITEM_VENDOR_LEN {
                warn(format!("vendor longer than {ITEM_VENDOR_LEN} chars (will truncate on device)"));
            }
        }
        if let Some(prev) = seen_ids.insert(it.id.clone(), ln) {
            warn(format!("duplicate id (also on line {prev})"));
        }

        // Facet → categories, with the on-device cat limits enforced.
        let mut cats = derive_categories(&it);
        for c in &cats {
            if c.chars().count() > ITEM_CAT_LEN {
                warnings.push(format!(
                    "line {ln} ({}): category \"{c}\" longer than {ITEM_CAT_LEN} chars",
                    it.id
                ));
            }
        }
        if cats.len() > MAX_ITEM_CATS {
            warnings.push(format!(
                "line {ln} ({}): {} categories exceeds device max {MAX_ITEM_CATS}; dropping {:?}",
                it.id,
                cats.len(),
                &cats[MAX_ITEM_CATS..]
            ));
            cats.truncate(MAX_ITEM_CATS);
        }
        for c in &cats {
            *cat_counts.entry(c.clone()).or_insert(0) += 1;
        }

        // Display-only fields: developer (vendor) and the genres joined for the
        // detail line + More Info card. Navigation still uses `categories`.
        let vendor = it
            .vendor
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let genre = {
            let g: Vec<&str> = it.genre.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if g.is_empty() { None } else { Some(g.join(", ")) }
        };
        if let Some(g) = &genre {
            if g.chars().count() > ITEM_GENRE_LEN {
                warnings.push(format!(
                    "line {ln} ({}): genre longer than {ITEM_GENRE_LEN} chars (will truncate on device)",
                    it.id
                ));
            }
        }

        // Launch hotkey: normalize to a single character (the device stores one
        // byte). Warn on a multi-char value (we keep the first) and on a key
        // already claimed by an earlier item (first match wins on device).
        let hotkey = it.hotkey.as_deref().map(str::trim).filter(|h| !h.is_empty()).and_then(|h| {
            let mut chars = h.chars();
            let first = chars.next().unwrap();
            if chars.next().is_some() {
                warnings.push(format!(
                    "line {ln} ({}): hotkey \"{h}\" longer than 1 char; using '{first}'",
                    it.id
                ));
            }
            if !first.is_ascii_graphic() {
                warnings.push(format!(
                    "line {ln} ({}): hotkey '{first}' is not a printable ASCII key; ignored",
                    it.id
                ));
                return None;
            }
            let lower = first.to_ascii_lowercase();
            if let Some(prev) = seen_hotkeys.insert(lower, ln) {
                warnings.push(format!(
                    "line {ln} ({}): hotkey '{first}' already used on line {prev}",
                    it.id
                ));
            }
            Some(first.to_string())
        });

        out.push(OutItem {
            id: it.id,
            name: it.name,
            categories: cats,
            app: it.app,
            year: it.year,
            vendor,
            genre,
            type_: it.type_,
            creator: it.creator,
            desc: it.desc,
            image: it.image,
            shot: it.shot,
            hotkey,
            icon: it.icon,
            source: it.source.map(|s| s.chars().take(ITEM_SOURCE_LEN).collect()),
            max_depth: it.max_depth,
            cd_image: it.cd_image,
            cd_required: it.cd_required,
            cd_volume: it.cd_volume,
            cd_app: it.cd_app,
        });
    }

    // Note: the legacy MAX_ITEMS (256) ceiling is enforced by `run`, not here —
    // the paged generator (`run_paged`) deliberately exceeds it across pages and
    // bounds each page by MAX_CAT_ITEMS instead (docs/21).
    if cat_counts.len() > MAX_CATS {
        warnings.push(format!(
            "{} distinct categories exceeds device max {MAX_CATS}",
            cat_counts.len()
        ));
    }

    let report = Report {
        items: out.len(),
        categories: cat_counts,
        warnings,
        lossy_chars: 0,
        bytes: 0,
    };
    Ok((out, report))
}

/// Serialize items to a single byte buffer: one compact JSON object per line,
/// `sep`-delimited (CR for the device; LF for host debugging), MacRoman-encoded.
fn render(items: &[OutItem], crlf: bool, lf: bool) -> Result<(Vec<u8>, usize)> {
    let sep = if lf {
        "\n"
    } else if crlf {
        "\r\n"
    } else {
        "\r"
    };
    let mut utf8 = String::new();
    for it in items {
        utf8.push_str(&serde_json::to_string(it)?);
        utf8.push_str(sep);
    }
    let (bytes, lossy) = macroman::encode(&utf8);
    Ok((bytes, lossy))
}

/// Compile a UTF-8 source dataset into the on-Mac catalog records as JSON
/// `Value`s (the same field set [`render`] emits), without encoding to MacRoman.
/// Used by `atrium add` to facet just the *new* titles, then merge them with the
/// existing on-volume catalog records before a single re-render.
pub fn compile(src_text: &str) -> Result<(Vec<Value>, Report)> {
    let (items, report) = build(src_text)?;
    let values = items
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<Value>, _>>()?;
    Ok((values, report))
}

/// Render already-compiled catalog records (one JSON object per element, e.g.
/// from [`compile`] or [`parse_compiled`]) to the on-device byte format
/// (MacRoman, `sep`-delimited). Lets a merged catalog be re-emitted without
/// re-deriving any facets, so existing records keep their baked art paths.
pub fn render_values(items: &[Value], crlf: bool, lf: bool) -> Result<Vec<u8>> {
    let sep = if lf { "\n" } else if crlf { "\r\n" } else { "\r" };
    let mut utf8 = String::new();
    for it in items {
        utf8.push_str(&serde_json::to_string(it)?);
        utf8.push_str(sep);
    }
    Ok(macroman::encode(&utf8).0)
}

/// Parse a compiled on-volume catalog's bytes (MacRoman, CR/LF/CRLF separated)
/// back into JSON records, in order. Blank/comment lines and any record that
/// won't parse are skipped (an odd line shouldn't sink a whole merge).
pub fn parse_compiled(bytes: &[u8]) -> Vec<Value> {
    let text = macroman::decode(bytes);
    let mut out = Vec::new();
    for line in text.split(['\r', '\n']) {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            out.push(v);
        }
    }
    out
}

/// Run the `catalog` subcommand.
pub fn run(src: &Path, out: &Path, lf: bool, crlf: bool) -> Result<Report> {
    let src_text = std::fs::read_to_string(src)
        .with_context(|| format!("reading source dataset {}", src.display()))?;
    let (items, mut report) = build(&src_text)?;
    if items.len() > MAX_ITEMS {
        bail!(
            "{} items exceeds the {MAX_ITEMS}-item device max for a single-file catalog \
             — use the paged catalog (`--paged-out`, docs/21) for larger libraries",
            items.len()
        );
    }
    let (bytes, lossy) = render(&items, crlf, lf)?;
    report.lossy_chars = lossy;
    report.bytes = bytes.len();
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(report)
}

// ---- paged catalog (docs/21) -------------------------------------------------

/// The navigation **taxonomy** (`data/taxonomy.json`): the canonical category
/// list + display order, plus the seed maps `library categorize` uses to bootstrap
/// the editable category DB (`data/categories.jsonl`). Unknown JSON fields (the
/// `_comment`) are ignored.
#[derive(Deserialize, Clone, Default)]
pub struct Taxonomy {
    /// Categories in display order — the launcher pages them in this order;
    /// `order[0]` (Recommended) is the default landing view.
    pub order: Vec<String>,
    #[serde(default)]
    pub default: String,
    /// Bucket for a game that lands in no genre category (so none are unreachable).
    #[serde(default)]
    pub catch_all_game: String,
    /// `kind` → category (app → Applications, utility → Utilities).
    #[serde(default)]
    pub kind_map: BTreeMap<String, String>,
    /// raw genre → category bucket (the seed; the DB then overrides).
    #[serde(default)]
    pub genre_map: BTreeMap<String, String>,
    /// Curated Recommended seed (ids).
    #[serde(default)]
    pub recommended: Vec<String>,
    /// Per-id extra category seeds beyond what genre/facets give.
    #[serde(default)]
    pub adds: BTreeMap<String, Vec<String>>,
}

impl Taxonomy {
    pub fn load(path: &Path) -> Result<Taxonomy> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("reading taxonomy {}", path.display()))?;
        serde_json::from_str(&txt).with_context(|| format!("parsing taxonomy {}", path.display()))
    }
    pub fn parse(bytes: &[u8]) -> Result<Taxonomy> {
        serde_json::from_slice(bytes).context("parsing taxonomy")
    }
    /// Sort a title's categories into display (`order`) order; categories not in
    /// `order` follow, alphabetically — so nothing is dropped.
    pub fn order_cats(&self, cats: &mut Vec<String>) {
        let rank = |c: &str| self.order.iter().position(|o| o == c).unwrap_or(usize::MAX);
        cats.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
        cats.dedup();
    }
}


/// Recommendation-style categories keep dataset order (mirror of the device
/// `model_is_list_ordered`); every other category sorts alphabetically.
fn is_list_ordered(name: &str) -> bool {
    name.eq_ignore_ascii_case("Recommended") || name.eq_ignore_ascii_case("Staff Picks")
}

/// A filesystem-safe, MacRoman-safe slug for a category name → `cats/<slug>.jsonl`.
/// Lowercased ASCII alphanumerics, runs of anything else collapse to one `-`,
/// capped so `<slug>.jsonl` fits HFS's 31-char limit.
fn slugify(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let mut s = s.trim_matches('-').to_string();
    if s.len() > 24 {
        s.truncate(24);
        s = s.trim_end_matches('-').to_string();
    }
    if s.is_empty() { "cat".to_string() } else { s }
}

fn unique_slug(name: &str, used: &mut HashSet<String>) -> String {
    let base = slugify(name);
    if used.insert(base.clone()) {
        return base;
    }
    for n in 2.. {
        let cand = format!("{base}-{n}");
        if used.insert(cand.clone()) {
            return cand;
        }
    }
    unreachable!()
}

/// Outcome of a paged generation, for reporting/testing.
pub struct PagedReport {
    pub categories: usize,
    pub pages: usize,
    pub items: usize,
    pub hotkeys: usize,
    /// The largest single page produced — a sanity check that the split kept
    /// every page within [`MAX_CAT_ITEMS`].
    pub biggest_page: (String, usize),
    pub warnings: Vec<String>,
}

/// Load the category DB (`data/categories.jsonl`, docs/21): id → its categories.
fn load_category_db(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading category DB {}", path.display()))?;
    let mut db = HashMap::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(t) else { continue };
        if let Some(id) = v.get("id").and_then(Value::as_str) {
            let cats = v
                .get("categories")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            db.insert(id.to_string(), cats);
        }
    }
    Ok(db)
}

/// Generate the **paged** catalog tree (docs/21) under `out_dir`:
/// `index.jsonl` (one line per category page) + `cats/<slug>.jsonl` (slim records)
/// + `hotkeys.jsonl` (the few items with a launch hotkey).
///
/// Category **membership** comes from the editable DB at `categories` when given
/// (else each item's derived `categories`); category **order** follows `taxonomy`
/// when given (else first-encountered). Items sort alphabetically within a
/// category except recommendation-style ones (which keep DB order); any category
/// over [`MAX_CAT_ITEMS`] splits into numbered sub-pages. All files are
/// MacRoman-encoded like the legacy catalog.
pub fn run_paged(
    src: &Path,
    out_dir: &Path,
    categories: Option<&Path>,
    taxonomy: Option<&Path>,
    lf: bool,
    crlf: bool,
) -> Result<PagedReport> {
    let src_text = std::fs::read_to_string(src)
        .with_context(|| format!("reading source dataset {}", src.display()))?;
    let (items, base) = build(&src_text)?;
    let db = match categories {
        Some(p) => Some(load_category_db(p)?),
        None => None,
    };
    let tax = match taxonomy {
        Some(p) => Some(Taxonomy::load(p)?),
        None => None,
    };
    // an item's categories: the DB (if given), else its derived set.
    let cats_of = |i: usize| -> Vec<String> {
        match &db {
            Some(db) => db.get(&items[i].id).cloned().unwrap_or_default(),
            None => items[i].categories.clone(),
        }
    };

    // category name → item indices, in first-encountered order...
    let mut pos: HashMap<String, usize> = HashMap::new();
    let mut cats: Vec<(String, Vec<usize>)> = Vec::new();
    for i in 0..items.len() {
        for c in cats_of(i) {
            let at = *pos.entry(c.clone()).or_insert_with(|| {
                cats.push((c.clone(), Vec::new()));
                cats.len() - 1
            });
            cats[at].1.push(i);
        }
    }
    // ...then re-order the categories themselves by the taxonomy (present ones in
    // `order`, any extras alphabetically after).
    if let Some(tax) = &tax {
        let rank = |c: &str| tax.order.iter().position(|o| o == c).unwrap_or(usize::MAX);
        cats.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    }
    // order within each category (alpha unless list-ordered, e.g. Recommended).
    for (name, idxs) in &mut cats {
        if !is_list_ordered(name) {
            idxs.sort_by(|&a, &b| items[a].name.to_lowercase().cmp(&items[b].name.to_lowercase()));
        }
    }

    let cats_dir = out_dir.join("cats");
    std::fs::create_dir_all(&cats_dir)
        .with_context(|| format!("creating {}", cats_dir.display()))?;

    let mut index: Vec<Value> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut pages = 0usize;
    let mut biggest = (String::new(), 0usize);
    for (name, idxs) in &cats {
        let ordered = is_list_ordered(name);
        for (page_no, chunk) in idxs.chunks(MAX_CAT_ITEMS).enumerate() {
            let page_name = if page_no == 0 {
                name.clone()
            } else {
                format!("{name} ({})", page_no + 1)
            };
            let slug = unique_slug(&page_name, &mut used);
            // v1: full on-device records (CatItem unchanged), but with the
            // item's `categories` set to its DB membership (so the launcher's
            // tag display matches navigation), capped at the device max. The
            // per-item struct-slim (deriving art paths from an `art` base) is a
            // separate v2 — it would ripple through the Toolbox UI (docs/21 §6).
            let recs: Vec<Value> = chunk
                .iter()
                .map(|&i| -> Result<Value> {
                    let mut v = serde_json::to_value(&items[i])?;
                    if let Value::Object(m) = &mut v {
                        let mut c = cats_of(i);
                        c.truncate(MAX_ITEM_CATS);
                        m.insert("categories".into(), serde_json::to_value(c)?);
                    }
                    Ok(v)
                })
                .collect::<Result<_>>()?;
            let bytes = render_values(&recs, crlf, lf)?;
            std::fs::write(cats_dir.join(format!("{slug}.jsonl")), &bytes)
                .with_context(|| format!("writing page {slug}"))?;
            let entry: Map<String, Value> = [
                ("name".to_string(), Value::from(page_name.clone())),
                ("slug".to_string(), Value::from(slug)),
                ("count".to_string(), Value::from(chunk.len())),
                ("ordered".to_string(), Value::from(ordered)),
            ]
            .into_iter()
            .collect();
            index.push(Value::Object(entry));
            if chunk.len() > biggest.1 {
                biggest = (page_name, chunk.len());
            }
            pages += 1;
        }
    }
    std::fs::write(out_dir.join("index.jsonl"), render_values(&index, crlf, lf)?)
        .with_context(|| format!("writing {}", out_dir.join("index.jsonl").display()))?;

    // hotkeys: only items that carry one, so a hotkey launches without a page load.
    let hotkeys: Vec<Value> = items
        .iter()
        .filter_map(|it| {
            it.hotkey.as_ref().map(|k| {
                let m: Map<String, Value> = [
                    ("key".to_string(), Value::from(k.clone())),
                    ("id".to_string(), Value::from(it.id.clone())),
                    ("name".to_string(), Value::from(it.name.clone())),
                    ("app".to_string(), Value::from(it.app.clone())),
                ]
                .into_iter()
                .collect();
                Value::Object(m)
            })
        })
        .collect();
    std::fs::write(out_dir.join("hotkeys.jsonl"), render_values(&hotkeys, crlf, lf)?)
        .with_context(|| format!("writing {}", out_dir.join("hotkeys.jsonl").display()))?;

    Ok(PagedReport {
        categories: cats.len(),
        pages,
        items: items.len(),
        hotkeys: hotkeys.len(),
        biggest_page: biggest,
        warnings: base.warnings,
    })
}

/// Inject a generated catalog into an image's metadata dir, backing up any
/// existing catalog first (so we never silently overwrite the on-volume index).
pub fn inject(
    rb_bin: &str,
    image: &Path,
    catalog_file: &Path,
    metadata_dir: &str,
    backup_dir: Option<&Path>,
) -> Result<()> {
    let rb = RbCli::new(rb_bin);
    let dst = format!("{}/catalog.jsonl", metadata_dir.trim_end_matches('/'));

    // Back up the existing on-volume catalog (best-effort; absent is fine).
    let backup = backup_dir
        .map(|d| d.join("catalog-prev.jsonl"))
        .unwrap_or_else(|| {
            catalog_file
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("catalog-prev.jsonl")
        });
    match rb.get(image, &dst, &backup, true) {
        Ok(()) => eprintln!("backed up existing catalog -> {}", backup.display()),
        Err(_) => eprintln!("no existing catalog to back up (first install)"),
    }

    rb.mkdir_p(image, metadata_dir)?;
    rb.put_text(image, catalog_file, &dst, "TEXT", "ttxt")
        .with_context(|| format!("writing {dst}"))?;
    eprintln!("injected catalog -> {} : {dst}", image.display());
    Ok(())
}

/// Inject a paged catalog tree (docs/21) — `index.jsonl` + `cats/<slug>.jsonl` +
/// `hotkeys.jsonl` — into an image's metadata dir (and `metadata/cats`), as TEXT.
/// The launcher reads `metadata/index.jsonl` first; if present it pages, else it
/// falls back to the legacy single `catalog.jsonl`.
pub fn inject_paged(rb_bin: &str, image: &Path, paged_dir: &Path, metadata_dir: &str) -> Result<()> {
    let rb = RbCli::new(rb_bin);
    let md = metadata_dir.trim_end_matches('/');
    rb.mkdir_p(image, md)?;
    let cats_vol = format!("{md}/cats");
    rb.mkdir_p(image, &cats_vol)?;

    for f in ["index.jsonl", "hotkeys.jsonl"] {
        let host = paged_dir.join(f);
        if host.exists() {
            rb.put_text(image, &host, &format!("{md}/{f}"), "TEXT", "ttxt")
                .with_context(|| format!("injecting {f}"))?;
        }
    }
    let cats_host = paged_dir.join("cats");
    let mut pages: Vec<std::path::PathBuf> = std::fs::read_dir(&cats_host)
        .with_context(|| format!("reading {}", cats_host.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect();
    pages.sort();
    for p in &pages {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        rb.put_text(image, p, &format!("{cats_vol}/{name}"), "TEXT", "ttxt")
            .with_context(|| format!("injecting page {name}"))?;
    }
    eprintln!(
        "injected paged catalog -> {} : {md}/ (index + {} page(s) + hotkeys)",
        image.display(),
        pages.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decade_buckets() {
        assert_eq!(decade_bucket(1986), "1980s");
        assert_eq!(decade_bucket(1990), "1990s");
        assert_eq!(decade_bucket(1996), "1990s");
        assert_eq!(decade_bucket(1984), "1980s");
    }

    fn item(json: &str) -> SourceItem {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn facets_in_priority_order() {
        let it = item(
            r#"{"id":"dark-castle","name":"Dark Castle","app":"Apps/Dark Castle/Dark Castle",
                "year":1986,"vendor":"Silicon Beach Software","color":false,"mouse":true,
                "genre":["Action"]}"#,
        );
        let cats = derive_categories(&it);
        assert_eq!(
            cats,
            vec![
                "Games",
                "Action",
                "B&W",
                "1980s",
                "Silicon Beach Software",
                "Mouse Required",
            ]
        );
    }

    #[test]
    fn pre_1987_defaults_to_bw_but_explicit_color_wins() {
        // No color facet + year < 1987 -> B&W (Mac was 1-bit only).
        let it = item(r#"{"id":"x","name":"X","app":"a","year":1985}"#);
        assert!(derive_categories(&it).iter().any(|c| c == "B&W"));
        // An explicit colour facet overrides the year prior.
        let it = item(r#"{"id":"x","name":"X","app":"a","year":1985,"color":true}"#);
        let cats = derive_categories(&it);
        assert!(cats.iter().any(|c| c == "Color"));
        assert!(!cats.iter().any(|c| c == "B&W"));
        // 1987+ with no facet stays unclassified (neither Color nor B&W).
        let it = item(r#"{"id":"x","name":"X","app":"a","year":1990}"#);
        let cats = derive_categories(&it);
        assert!(!cats.iter().any(|c| c == "Color" || c == "B&W"));
    }

    #[test]
    fn color_and_kind_mapping() {
        let it = item(
            r#"{"id":"x","name":"X","app":"Apps/X/X","kind":"utility","color":true,"mouse":false}"#,
        );
        let cats = derive_categories(&it);
        assert!(cats.contains(&"Utilities".to_string()));
        assert!(cats.contains(&"Color".to_string()));
        assert!(cats.contains(&"No Mouse".to_string()));
    }

    #[test]
    fn dedup_is_case_insensitive() {
        let it = item(
            r#"{"id":"x","name":"X","app":"Apps/X/X","genre":["Games"],"categories":["GAMES"]}"#,
        );
        let cats = derive_categories(&it);
        // "Games" (kind) collides with genre "Games" and manual "GAMES".
        assert_eq!(cats.iter().filter(|c| c.eq_ignore_ascii_case("games")).count(), 1);
    }

    #[test]
    fn emits_cr_and_macroman() {
        let (items, _) = build(
            r#"{"id":"x","name":"Café","app":"Apps/X/X","year":1990,"color":true}"#,
        )
        .unwrap();
        let (bytes, lossy) = render(&items, false, false).unwrap();
        assert_eq!(lossy, 0);
        assert!(bytes.ends_with(b"\r"));
        assert!(!bytes.contains(&b'\n'));
        // é encoded as MacRoman 0x8E, not UTF-8 0xC3 0xA9.
        assert!(bytes.windows(1).any(|w| w == [0x8E]));
        assert!(!bytes.windows(2).any(|w| w == [0xC3, 0xA9]));
    }

    #[test]
    fn emits_vendor_and_genre_strings() {
        let (items, _) = build(
            r#"{"id":"dc","name":"Dark Castle","app":"a","vendor":"Silicon Beach Software","genre":["Action","Arcade"],"year":1986}"#,
        )
        .unwrap();
        assert_eq!(items[0].vendor.as_deref(), Some("Silicon Beach Software"));
        assert_eq!(items[0].genre.as_deref(), Some("Action, Arcade"));
        // No vendor/genre -> omitted (None).
        let (bare, _) = build(r#"{"id":"x","name":"X","app":"a"}"#).unwrap();
        assert!(bare[0].vendor.is_none() && bare[0].genre.is_none());
    }

    #[test]
    fn emits_cd_fields() {
        // A run-from-CD title carries cdImage/cdVolume/cdApp; cdRequired passes through.
        let (items, _) = build(
            r#"{"id":"myst","name":"Myst","app":"x","cdImage":"MYST.iso","cdVolume":"Myst","cdApp":"Myst/Myst","cdRequired":true}"#,
        )
        .unwrap();
        assert_eq!(items[0].cd_image.as_deref(), Some("MYST.iso"));
        assert_eq!(items[0].cd_volume.as_deref(), Some("Myst"));
        assert_eq!(items[0].cd_app.as_deref(), Some("Myst/Myst"));
        assert_eq!(items[0].cd_required, Some(true));
        // A non-CD title omits every CD field (None -> not serialized).
        let (bare, _) = build(r#"{"id":"x","name":"X","app":"a"}"#).unwrap();
        assert!(
            bare[0].cd_image.is_none()
                && bare[0].cd_volume.is_none()
                && bare[0].cd_app.is_none()
                && bare[0].cd_required.is_none()
        );
    }

    #[test]
    fn hotkey_passthrough_and_normalization() {
        // Single char passes through; a multi-char value keeps the first char.
        let (items, report) = build(
            "{\"id\":\"a\",\"name\":\"A\",\"app\":\"a\",\"hotkey\":\"1\"}\n\
             {\"id\":\"b\",\"name\":\"B\",\"app\":\"b\",\"hotkey\":\"xy\"}\n\
             {\"id\":\"c\",\"name\":\"C\",\"app\":\"c\"}",
        )
        .unwrap();
        assert_eq!(items[0].hotkey.as_deref(), Some("1"));
        assert_eq!(items[1].hotkey.as_deref(), Some("x")); // first char of "xy"
        assert!(items[2].hotkey.is_none());
        assert!(report.warnings.iter().any(|w| w.contains("longer than 1 char")));
    }

    #[test]
    fn duplicate_hotkey_warns() {
        let (_items, report) = build(
            "{\"id\":\"a\",\"name\":\"A\",\"app\":\"a\",\"hotkey\":\"g\"}\n\
             {\"id\":\"b\",\"name\":\"B\",\"app\":\"b\",\"hotkey\":\"G\"}",
        )
        .unwrap();
        // 'g' and 'G' collide case-insensitively.
        assert!(report.warnings.iter().any(|w| w.contains("already used")));
    }

    #[test]
    fn comments_and_blanks_skipped() {
        let (items, _) = build(
            "# a comment\n\n{\"id\":\"x\",\"name\":\"X\",\"app\":\"a\"}\n// trailing\n",
        )
        .unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn slugify_is_hfs_and_macroman_safe() {
        assert_eq!(slugify("Action"), "action");
        assert_eq!(slugify("B&W"), "b-w");
        assert_eq!(slugify("Card & Casino"), "card-casino");
        assert_eq!(slugify("Games (2)"), "games-2");
        assert_eq!(slugify("Silicon Beach Software"), "silicon-beach-software");
        // long names truncate so "<slug>.jsonl" fits HFS 31
        assert!(slugify("A Really Very Long Category Name Indeed").len() <= 24);
    }

    #[test]
    fn paged_splits_categories_and_writes_tree() {
        // 200 games in "Action" -> two pages (128 + 72); the index + files reflect it.
        let mut src = String::new();
        for i in 0..200 {
            src.push_str(&format!(
                "{{\"id\":\"g{i}\",\"name\":\"Game {i:03}\",\"app\":\"Apps/G{i}/G{i}\",\"kind\":\"game\",\"genre\":[\"Action\"]}}\n"
            ));
        }
        let dir = std::env::temp_dir().join(format!("atrium-paged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = dir.join("src.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&s, &src).unwrap();

        let r = run_paged(&s, &dir, None, None, false, false).unwrap();
        assert_eq!(r.items, 200);
        // every page is within the cap
        assert!(r.biggest_page.1 <= MAX_CAT_ITEMS, "page over cap: {:?}", r.biggest_page);

        // the index lists the split "Action" + "Action (2)" pages (MacRoman/CR).
        let index = macroman::decode(&std::fs::read(dir.join("index.jsonl")).unwrap());
        let names: Vec<String> = index
            .split(['\r', '\n'])
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        assert!(names.contains(&"Action".to_string()));
        assert!(names.contains(&"Action (2)".to_string()));

        // the first Action page file exists with full on-device records (v1).
        let page = macroman::decode(&std::fs::read(dir.join("cats/action.jsonl")).unwrap());
        let first = page.split(['\r', '\n']).find(|l| !l.trim().is_empty()).unwrap();
        let rec: Value = serde_json::from_str(first).unwrap();
        assert_eq!(rec.get("id").and_then(Value::as_str).map(|s| s.starts_with('g')), Some(true));
        // categories are the item's membership (the file's own category present).
        let rc: Vec<&str> = rec.get("categories").and_then(Value::as_array).unwrap()
            .iter().filter_map(Value::as_str).collect();
        assert!(rc.contains(&"Action & Arcade") || rc.contains(&"Action"),
            "record carries its category membership: {rc:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paged_uses_category_db_in_taxonomy_order() {
        let dir = std::env::temp_dir().join(format!("atrium-paged-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // two titles; the DB (not their genre) decides membership.
        let src = "{\"id\":\"a\",\"name\":\"Aaa\",\"app\":\"Apps/A/A\",\"genre\":[\"Arcade\"]}\n\
                   {\"id\":\"b\",\"name\":\"Bbb\",\"app\":\"Apps/B/B\"}\n";
        let s = dir.join("src.jsonl");
        std::fs::write(&s, src).unwrap();
        std::fs::write(dir.join("categories.jsonl"),
            "{\"id\":\"a\",\"categories\":[\"Action & Arcade\",\"Recommended\"]}\n\
             {\"id\":\"b\",\"categories\":[\"Recommended\"]}\n").unwrap();
        std::fs::write(dir.join("taxonomy.json"),
            "{\"order\":[\"Recommended\",\"Action & Arcade\"]}").unwrap();

        let r = run_paged(&s, &dir, Some(&dir.join("categories.jsonl")), Some(&dir.join("taxonomy.json")), false, false).unwrap();
        assert_eq!(r.categories, 2);
        // index follows taxonomy order: Recommended first, then Action & Arcade.
        let idx = macroman::decode(&std::fs::read(dir.join("index.jsonl")).unwrap());
        let names: Vec<String> = idx.split(['\r', '\n']).filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v.get("name").and_then(Value::as_str).map(str::to_string)).collect();
        assert_eq!(names, vec!["Recommended", "Action & Arcade"]);
        // Recommended holds both; Action & Arcade only "a".
        let aa = macroman::decode(&std::fs::read(dir.join("cats/action-arcade.jsonl")).unwrap());
        assert!(aa.contains("\"id\":\"a\"") && !aa.contains("\"id\":\"b\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paged_hotkeys_only_for_hotkeyed_items() {
        let src = "{\"id\":\"a\",\"name\":\"A\",\"app\":\"Apps/A/A\",\"hotkey\":\"a\"}\n\
                   {\"id\":\"b\",\"name\":\"B\",\"app\":\"Apps/B/B\"}\n";
        let dir = std::env::temp_dir().join(format!("atrium-paged-hk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = dir.join("src.jsonl");
        std::fs::write(&s, src).unwrap();
        let r = run_paged(&s, &dir, None, None, false, false).unwrap();
        assert_eq!(r.hotkeys, 1);
        let hk = macroman::decode(&std::fs::read(dir.join("hotkeys.jsonl")).unwrap());
        let v: Value = serde_json::from_str(hk.split(['\r', '\n']).find(|l| !l.trim().is_empty()).unwrap()).unwrap();
        assert_eq!(v.get("key").and_then(Value::as_str), Some("a"));
        assert_eq!(v.get("app").and_then(Value::as_str), Some("Apps/A/A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_render_parse_round_trip() {
        // compile a source record -> Values, render to MacRoman bytes, parse back.
        let (vals, report) = compile(
            r#"{"id":"dc","name":"Café Dark Castle","app":"Apps/DC/DC","year":1986,"color":false,"genre":["Action"]}"#,
        )
        .unwrap();
        assert_eq!(report.items, 1);
        assert_eq!(vals[0]["id"], "dc");
        assert_eq!(vals[0]["genre"], "Action"); // joined display string
        assert!(vals[0]["categories"].as_array().unwrap().iter().any(|c| c == "B&W"));

        let bytes = render_values(&vals, false, false).unwrap();
        assert!(bytes.ends_with(b"\r") && !bytes.contains(&b'\n'));
        let back = parse_compiled(&bytes);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0]["id"], "dc");
        assert_eq!(back[0]["name"], "Café Dark Castle"); // MacRoman round-trips
    }

    #[test]
    fn clamps_to_eight_categories() {
        let it = item(
            r#"{"id":"x","name":"X","app":"a","kind":"game","color":true,"year":1990,
                "vendor":"V","mouse":true,"genre":["A","B","C","D","E"]}"#,
        );
        let cats = derive_categories(&it);
        // Games + 5 genres + Color + 1990s + V + Mouse Required = 10 before clamp.
        assert!(cats.len() > MAX_ITEM_CATS);
    }
}
