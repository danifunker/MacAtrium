//! `atrium mg` — fill the curated dataset from the local **Macintosh Garden**
//! archive (see docs/MacintoshGardenArchive.md), a sibling enrichment source to
//! LaunchBox (`enrich`).
//!
//! Reads `<archive>/metadata/{games,apps}.ndjson`, keeps only **68K-compatible**
//! titles (`architecture ⊇ "68k"`), matches our dataset by normalised name (the
//! same matcher `enrich` uses), and fills `year` / `vendor` / `genre` / `desc`
//! WITHOUT clobbering curated values (unless `--overwrite`). It also:
//!
//! - sets **`source: "Macintosh Garden"`** (visible attribution — MG screenshots
//!   are largely user-contributed),
//! - de-HTMLs the MG description (MG prose is HTML; strip tags + entities + the
//!   internal `/games/…` links so it renders as plain MacRoman text),
//! - detects colour **offline** from a gameplay screenshot already on disk (no
//!   download — the scrape already has it), and
//! - optionally copies the box-front + a gameplay screenshot into an `art_dir`
//!   (`<id>.<ext>` / `<id>.shot.<ext>`) for the existing `image` art→PICT path.

use crate::enrich::{candidate_keys, clamp_desc, is_color_image};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::Path;

struct MgRec {
    nid: i64,
    kind: &'static str, // "games" | "apps"
    title: String,
    year: Option<i64>,
    vendor: Option<String>,
    genres: Vec<String>,
    desc_html: String,
    screenshots: Vec<String>, // filenames, as referenced by the record
}

impl MgRec {
    /// Directory holding this title's scraped images: `<archive>/<kind>/<nid>/`.
    fn dir(&self, archive: &Path) -> std::path::PathBuf {
        archive.join(self.kind).join(self.nid.to_string())
    }
    /// Match-quality score: prefer records that carry a year, a vendor, and
    /// have at least one image actually on disk.
    fn score(&self, archive: &Path) -> u8 {
        let has_img = self
            .screenshots
            .iter()
            .any(|f| self.dir(archive).join(f).is_file());
        self.year.is_some() as u8 + self.vendor.is_some() as u8 + has_img as u8
    }
}

fn arch_has_68k(rec: &Value) -> bool {
    rec.get("architecture")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("68k"))))
        .unwrap_or(false)
}

/// A screenshot filename that is box/cover art rather than a gameplay shot.
fn is_box(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("box") || n.contains("cover") || n.contains("_front") || n.contains("_back")
}

/// Flatten Macintosh Garden's HTML description to plain text: strip comments and
/// tags (which drops the internal `/games/…` links and their hrefs), decode the
/// handful of entities MG uses, and let `clamp_desc` collapse whitespace + cap.
pub fn strip_html(s: &str) -> String {
    // drop HTML comments (e.g. <!--break-->)
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        rest = match rest[i..].find("-->") {
            Some(j) => &rest[i + j + 3..],
            None => "",
        };
    }
    out.push_str(rest);

    // drop tags: copy everything outside <...>
    let mut text = String::with_capacity(out.len());
    let mut depth = 0u32;
    for c in out.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => text.push(c),
            _ => {}
        }
    }

    // decode the common entities (numeric + the named ones MG emits)
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&ndash;", "-")
        .replace("&mdash;", "-")
        .replace("&hellip;", "...");
    text
}

fn str_field(rec: &Value, key: &str) -> Option<String> {
    rec.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn first_str_of_array(rec: &Value, key: &str) -> Option<String> {
    rec.get(key)
        .and_then(Value::as_array)?
        .iter()
        .find_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn array_of_strings(rec: &Value, key: &str) -> Vec<String> {
    rec.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Load the 68K-compatible records from one ndjson file.
fn load_ndjson(path: &Path, key: &str, kind: &'static str) -> Result<Vec<MgRec>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()), // missing file → no records of that kind
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obj: Value = serde_json::from_str(line)
            .with_context(|| format!("parsing {} line", path.display()))?;
        let Some(rec) = obj.get("data").and_then(|d| d.get(key)) else {
            continue;
        };
        if !arch_has_68k(rec) {
            continue;
        }
        let Some(title) = str_field(rec, "title") else {
            continue;
        };
        let nid = obj.get("nid").and_then(Value::as_i64).unwrap_or(-1);
        let screenshots = rec
            .get("screenshots")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.get("filename").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        out.push(MgRec {
            nid,
            kind,
            title,
            year: str_field(rec, "year").and_then(|y| y.parse().ok()),
            vendor: first_str_of_array(rec, "publisher").or_else(|| first_str_of_array(rec, "author")),
            // games use `category`, apps use `category_app`
            genres: {
                let g = array_of_strings(rec, "category");
                if g.is_empty() { array_of_strings(rec, "category_app") } else { g }
            },
            desc_html: str_field(rec, "description").unwrap_or_default(),
            screenshots,
        });
    }
    Ok(out)
}

fn missing(obj: &Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

fn is_blank_or_comment(t: &str) -> bool {
    t.is_empty() || t.starts_with('#') || t.starts_with("//")
}

fn ext_of(name: &str) -> &str {
    name.rsplit('.').next().filter(|e| e.len() <= 5 && !e.is_empty()).unwrap_or("img")
}

/// Run `atrium mg`: enrich `src` → `out` from the MG `archive`. When `art_dir`
/// is given, also copy each matched title's box-front + gameplay screenshot there
/// for the `image` art pass.
pub fn run(
    src: &Path,
    archive: &Path,
    out: &Path,
    overwrite: bool,
    art_dir: Option<&Path>,
) -> Result<()> {
    let mut recs = load_ndjson(&archive.join("metadata/games.ndjson"), "game", "games")?;
    recs.extend(load_ndjson(&archive.join("metadata/apps.ndjson"), "app", "apps")?);
    eprintln!("MacGarden: {} 68K-compatible record(s)", recs.len());

    // candidate key -> best record (prefer the one with year+vendor+on-disk art)
    let mut idx: HashMap<String, usize> = HashMap::new();
    for (i, r) in recs.iter().enumerate() {
        let s = r.score(archive);
        for k in candidate_keys(&r.title) {
            match idx.get(&k) {
                Some(&j) if recs[j].score(archive) >= s => {}
                _ => {
                    idx.insert(k, i);
                }
            }
        }
    }

    if let Some(d) = art_dir {
        std::fs::create_dir_all(d)?;
    }

    let text = std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
    let mut out_text = String::new();
    let (mut total, mut matched, mut col_n, mut bw_n, mut art_n, mut shot_n) = (0, 0, 0, 0, 0, 0);
    let mut unmatched = Vec::new();

    for line in text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            out_text.push_str(line);
            out_text.push('\n');
            continue;
        }
        total += 1;
        let mut obj: Map<String, Value> =
            serde_json::from_str(t).with_context(|| format!("parsing dataset line: {t}"))?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("").to_string();

        let hit = candidate_keys(&name).into_iter().find_map(|k| idx.get(&k).copied());
        let Some(ri) = hit else {
            unmatched.push(format!("{name} ({id})"));
            out_text.push_str(&serde_json::to_string(&Value::Object(obj))?);
            out_text.push('\n');
            continue;
        };
        matched += 1;
        let r = &recs[ri];

        if let Some(y) = r.year {
            if overwrite || missing(&obj, "year") {
                obj.insert("year".into(), Value::from(y));
            }
        }
        if let Some(v) = &r.vendor {
            if overwrite || missing(&obj, "vendor") {
                obj.insert("vendor".into(), Value::from(v.clone()));
            }
        }
        if !r.genres.is_empty() && (overwrite || missing(&obj, "genre")) {
            obj.insert("genre".into(), Value::from(r.genres.clone()));
        }
        if !r.desc_html.is_empty() && (overwrite || missing(&obj, "desc")) {
            let plain = clamp_desc(&strip_html(&r.desc_html));
            if !plain.is_empty() {
                obj.insert("desc".into(), Value::from(plain));
            }
        }
        // visible attribution
        if overwrite || missing(&obj, "source") {
            obj.insert("source".into(), Value::from("Macintosh Garden"));
        }

        // offline colour detect from a gameplay screenshot already on disk.
        // Pre-1987 Macs were 1-bit, so don't let a colourful later shot mislabel
        // them — leave `color` for the catalog/curation to decide (matches enrich).
        let pre_color = obj.get("year").and_then(Value::as_i64).is_some_and(|y| y < 1987);
        if !pre_color && (overwrite || missing(&obj, "color")) {
            if let Some(shot) = r
                .screenshots
                .iter()
                .filter(|f| !is_box(f))
                .map(|f| r.dir(archive).join(f))
                .find(|p| p.is_file())
            {
                if let Ok(is_col) = is_color_image(&shot) {
                    obj.insert("color".into(), Value::Bool(is_col));
                    if is_col { col_n += 1 } else { bw_n += 1 }
                }
            }
        }

        // copy art for the image pass (box-front → <id>.<ext>, shot → <id>.shot.<ext>)
        if let Some(adir) = art_dir {
            if !id.is_empty() {
                if let Some(b) = r.screenshots.iter().find(|f| is_box(f)) {
                    let srcf = r.dir(archive).join(b);
                    if srcf.is_file() {
                        let dst = adir.join(format!("{id}.{}", ext_of(b)));
                        if std::fs::copy(&srcf, &dst).is_ok() {
                            art_n += 1;
                        }
                    }
                }
                if let Some(s) = r.screenshots.iter().find(|f| !is_box(f)) {
                    let srcf = r.dir(archive).join(s);
                    if srcf.is_file() {
                        let dst = adir.join(format!("{id}.shot.{}", ext_of(s)));
                        if std::fs::copy(&srcf, &dst).is_ok() {
                            shot_n += 1;
                        }
                    }
                }
            }
        }

        out_text.push_str(&serde_json::to_string(&Value::Object(obj))?);
        out_text.push('\n');
    }

    std::fs::write(out, &out_text).with_context(|| format!("writing {}", out.display()))?;
    eprintln!(
        "MacGarden: matched {matched}/{total} ({} unmatched); colour-detect {col_n} colour / {bw_n} B&W",
        unmatched.len()
    );
    if art_dir.is_some() {
        eprintln!("MacGarden: copied {art_n} box-front + {shot_n} screenshot image(s) to art dir");
    }
    for u in &unmatched {
        eprintln!("  unmatched: {u}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html_tags_links_entities() {
        let h = "<blockquote>Hi <i>there</i></blockquote> see <a href=\"/games/x\">X</a> &amp; Y &ndash; done<!--break-->";
        let got = strip_html(h);
        assert!(!got.contains('<'));
        assert!(!got.contains("/games/"));
        assert!(got.contains("& Y - done"));
        assert!(got.contains("X")); // link text kept, href dropped
    }

    #[test]
    fn box_vs_screenshot() {
        assert!(is_box("Lemmings_box_front.jpg"));
        assert!(is_box("caesar_ii_coverart.png"));
        assert!(is_box("uninvited_reference_back.jpg"));
        assert!(!is_box("monkey_island_2_starting.jpg"));
    }

    #[test]
    fn detects_68k() {
        let v: Value = serde_json::from_str(r#"{"architecture":["68k","PPC"]}"#).unwrap();
        assert!(arch_has_68k(&v));
        let v: Value = serde_json::from_str(r#"{"architecture":["PPC"]}"#).unwrap();
        assert!(!arch_has_68k(&v));
        let v: Value = serde_json::from_str(r#"{"architecture":[]}"#).unwrap();
        assert!(!arch_has_68k(&v));
    }
}
