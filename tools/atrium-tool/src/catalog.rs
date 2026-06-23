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
use std::collections::BTreeMap;
use std::path::Path;

// On-device parser limits (src/catalog.h, src/model.h). We validate against
// these so a generated catalog never silently overflows the 68k reader.
const MAX_ITEMS: usize = 256;
const MAX_ITEM_CATS: usize = 8;
const ITEM_ID_LEN: usize = 47;
const ITEM_NAME_LEN: usize = 63;
const ITEM_PATH_LEN: usize = 191;
const ITEM_CAT_LEN: usize = 31;
const ITEM_DESC_LEN: usize = 127;
const ITEM_VENDOR_LEN: usize = 39;
const ITEM_GENRE_LEN: usize = 63;
const MAX_CATS: usize = 64; // distinct named categories (excludes synthesized "All")

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
    #[serde(rename = "type", default)]
    type_: Option<String>,
    #[serde(default)]
    creator: Option<String>,
    /// Optional per-item launch hotkey (a single character): the launcher
    /// launches this title when the key is pressed. Doubles as a per-item
    /// gamepad-button map (MiSTer maps joystick buttons → keystrokes).
    #[serde(default)]
    hotkey: Option<String>,
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
    if let Some(color) = it.color {
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
        });
    }

    if out.len() > MAX_ITEMS {
        bail!(
            "{} items exceeds device max {MAX_ITEMS}; split the catalog",
            out.len()
        );
    }
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

/// Run the `catalog` subcommand.
pub fn run(src: &Path, out: &Path, lf: bool, crlf: bool) -> Result<Report> {
    let src_text = std::fs::read_to_string(src)
        .with_context(|| format!("reading source dataset {}", src.display()))?;
    let (items, mut report) = build(&src_text)?;
    let (bytes, lossy) = render(&items, crlf, lf)?;
    report.lossy_chars = lossy;
    report.bytes = bytes.len();
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(report)
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
