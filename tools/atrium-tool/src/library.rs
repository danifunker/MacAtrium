//! `atrium library scan` — the Library Builder backend.
//!
//! Walks the MacPack donor disks and emits a comprehensive `library.jsonl`: one
//! record per *title* (a game/app/utility), so the curated library can be
//! (re)generated from the pack instead of hand-listed. Pure enumeration — it does
//! NOT copy any forks (that's `harvest` at build time); it only records *where*
//! each title lives so a build can harvest it later.
//!
//! Layout assumptions (verified against `MacPack-20240825-RC1`):
//!   /Games/<year>/<title>/…           -> kind=game,    year from <year>
//!   /Applications/<category>/<title>/… -> kind=app,     genre seeded from <category>
//!   /Utilities/<category>/<title>/…    -> kind=utility, genre seeded from <category>
//! A *title* is the depth-2 folder under a tree root (so helper APPLs/installers
//! nested inside it group under the one title). `genre` is a multi-valued tag list
//! (non-exclusive); only `kind` is the single exclusive top-level bucket.

use crate::harvest::slugify;
use crate::rbcli::RbCli;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// A tree root to scan and the exclusive `kind` it implies.
struct Root {
    name: &'static str,
    kind: &'static str,
    /// true: first path segment is a YEAR (Games); false: it's a category tag.
    year_dirs: bool,
}

const ROOTS: &[Root] = &[
    Root { name: "Games", kind: "game", year_dirs: true },
    Root { name: "Applications", kind: "app", year_dirs: false },
    Root { name: "Utilities", kind: "utility", year_dirs: false },
];

/// One scraped title (a `library.jsonl` record).
struct Title {
    id: String,
    name: String,
    kind: String,
    year: Option<i64>,
    genre: Vec<String>, // multi-valued, non-exclusive
    donor: String,      // donor disk's ORIGINAL filename (e.g. "boot.vhd")
    src_path: String,   // title folder on the donor, for harvest_src.path
    app_path: String,   // launch path in the built image: Apps/<dest>/<appl-rel>
}

#[derive(Default)]
pub struct ScanReport {
    pub titles: usize,
    pub dupes: usize,         // same id seen again (first wins)
    pub roots_missing: usize, // a root not present on a disk
}

/// Leading-year of a Games dir name: "1990" -> 1990, "1996+" -> 1996,
/// "Infocom+" -> None.
fn parse_year(seg: &str) -> Option<i64> {
    let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() == 4 {
        digits.parse().ok()
    } else {
        None
    }
}

/// Pick the launch APPL among a title's APPLs (full paths + creators), preferring
/// one sitting *directly* in the title folder whose name matches the folder, and
/// skipping a bundled Finder/System. Returns the chosen APPL's full path.
fn pick_launch<'a>(appls: &'a [(String, String)], title_path: &str, title_name: &str) -> &'a str {
    let direct_prefix = format!("{title_path}/");
    let base = slugify(title_name);
    // Candidates that aren't a bundled Finder/System app.
    let usable: Vec<&(String, String)> = appls
        .iter()
        .filter(|(path, creator)| {
            creator != "MACS" && path.rsplit('/').next() != Some("Finder")
        })
        .collect();
    let pool = if usable.is_empty() { appls.iter().collect::<Vec<_>>() } else { usable };
    // Score: directly-in-folder beats nested; longer slug-prefix match to the
    // folder name wins; shorter path (shallower) breaks ties.
    pool.iter()
        .max_by_key(|(path, _)| {
            let rel = path.strip_prefix(&direct_prefix).unwrap_or(path);
            let direct = !rel.contains('/');
            let leaf = path.rsplit('/').next().unwrap_or(path);
            let match_len = common_prefix_len(&slugify(leaf), &base);
            (direct, match_len, std::cmp::Reverse(path.len()))
        })
        .map(|(path, _)| path.as_str())
        .unwrap_or("")
}

/// Length of the shared leading run of two ASCII slugs.
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// Scan the donor `disks` (each `(original_filename, host_path)`) into a deduped
/// set of titles, written as `library.jsonl` to `out`.
pub fn scan(rb: &RbCli, disks: &[(String, std::path::PathBuf)], out: &Path, release: Option<&str>) -> Result<ScanReport> {
    let mut titles: BTreeMap<String, Title> = BTreeMap::new(); // by id; first wins
    let mut report = ScanReport::default();

    for (donor, disk) in disks {
        for root in ROOTS {
            let entries = match rb.ls(disk, &format!("/{}/**", root.name)) {
                Ok(e) => e,
                Err(_) => {
                    report.roots_missing += 1;
                    continue; // root absent on this disk
                }
            };
            // Group APPL files by their depth-2 title folder.
            let mut groups: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
            for e in &entries {
                if e.is_dir || e.ostype != "APPL" {
                    continue;
                }
                let rel: Vec<&str> = e
                    .name
                    .trim_start_matches('/')
                    .strip_prefix(root.name)
                    .unwrap_or("")
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .collect();
                if rel.len() < 2 {
                    continue; // a bare APPL under root/<a>; skip in v1
                }
                let title_path = format!("/{}/{}/{}", root.name, rel[0], rel[1]);
                groups
                    .entry(title_path)
                    .or_default()
                    .push((e.name.clone(), e.creator.clone()));
            }
            for (title_path, appls) in groups {
                // Owned fields first — the segments borrow `title_path`, which we move below.
                let (a, id, name) = {
                    let segs: Vec<&str> = title_path.trim_start_matches('/').split('/').collect();
                    let a = segs[1].to_string(); // <year|category>
                    let b = segs[2].to_string(); // <title folder>
                    (a, slugify(&b), b)
                };
                if id.is_empty() {
                    continue;
                }
                if titles.contains_key(&id) {
                    report.dupes += 1;
                    continue;
                }
                // kind is the single exclusive bucket; the first path segment is a
                // multi-valued (non-exclusive) tag UNLESS it's a Games <year>, which
                // becomes the `year`. Non-year Games groups ("01 Sys 6", "WIP" on the
                // Supplement disk — an OS/era grouping) are kept as metadata tags
                // rather than dropped.
                let (year, genre) = match (root.year_dirs, parse_year(&a)) {
                    (true, Some(y)) => (Some(y), Vec::new()),
                    _ => (None, vec![a]),
                };
                let appl = pick_launch(&appls, &title_path, &name);
                let appl_rel = appl
                    .strip_prefix(&format!("{title_path}/"))
                    .unwrap_or_else(|| appl.rsplit('/').next().unwrap_or(appl));
                let app_path = format!("Apps/{name}/{appl_rel}");
                titles.insert(
                    id.clone(),
                    Title {
                        id,
                        name,
                        kind: root.kind.to_string(),
                        year,
                        genre,
                        donor: donor.clone(),
                        src_path: title_path,
                        app_path,
                    },
                );
            }
        }
    }

    report.titles = titles.len();
    let mut body = String::new();
    body.push_str("# data/library.jsonl — curated MacAtrium source dataset (generated by `atrium library scan`).\n");
    if let Some(r) = release {
        body.push_str(&format!("# MacPack release: {r}\n"));
    }
    for t in titles.values() {
        body.push_str(&record_line(t));
        body.push('\n');
    }
    std::fs::write(out, body).with_context(|| format!("writing {}", out.display()))?;
    Ok(report)
}

/// Per-title requirement/facet fields — these live in `compatibility.jsonl`, not
/// `library.jsonl` (the latter is identity + descriptive metadata only).
const REQ_FIELDS: &[&str] = &[
    "color", "mouse", "maxDepth", "minOS", "maxOS", "minMem", "minCPU", "arch",
];

const COMPAT_HEADER: &str = "\
# data/compatibility.jsonl — per-title requirements/facets, keyed by id, applied
# over the library by `atrium merge` (this overlay wins). Pre-seeded by
# `atrium library split` (moves these fields out of the enriched library) and
# hand-editable; hand-verified entries WIN over the seeded ones.
#   color   true=Color / false=B&W (a facet + colour-detect result)
#   mouse   true=Mouse Required
#   maxDepth deepest screen bpp a title tolerates (1/4/8/16/32); launcher caps to it
#   minOS / maxOS   dotted OS range, e.g. \"6.0.8\"/\"7.5\" (build drops out-of-range)
#   minMem  minimum RAM in KB (optional; for hardware targeting / preflight)
#   minCPU  minimum CPU, e.g. \"68000\"/\"68020\"/\"68030\" (optional)
#   arch    \"68K\" / \"PPC\" / \"BOTH\"
";

#[derive(Default)]
pub struct SplitReport {
    pub moved: usize,
    pub compat_entries: usize,
}

/// Parse a JSONL overlay file into id -> object (comments/blank lines skipped).
fn parse_jsonl_by_id(path: &Path) -> BTreeMap<String, Map<String, Value>> {
    let mut map = BTreeMap::new();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(t) {
            if let Some(id) = o.get("id").and_then(|v| v.as_str()) {
                map.insert(id.to_string(), o);
            }
        }
    }
    map
}

/// Move the [`REQ_FIELDS`] out of `library` into `compat` (merging with existing
/// compat entries, which WIN — hand-verified data is authoritative), stripping
/// them from the library. Rewrites both files. Repeatable: after `mg` enrich
/// re-adds color/mouse to the library, re-running re-extracts them.
pub fn split(library: &Path, compat: &Path) -> Result<SplitReport> {
    let mut compat_map = parse_jsonl_by_id(compat);
    let lib_text =
        std::fs::read_to_string(library).with_context(|| format!("reading {}", library.display()))?;
    let mut out = String::new();
    let mut moved = 0;
    for line in lib_text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut rec: Map<String, Value> = match serde_json::from_str(t) {
            Ok(Value::Object(o)) => o,
            _ => {
                out.push_str(line);
                out.push('\n');
                continue;
            }
        };
        let id = rec.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let extracted: Vec<(String, Value)> = REQ_FIELDS
            .iter()
            .filter_map(|f| rec.remove(*f).map(|v| ((*f).to_string(), v)))
            .collect();
        if !id.is_empty() && !extracted.is_empty() {
            let entry = compat_map.entry(id.clone()).or_default();
            if !entry.contains_key("id") {
                entry.insert("id".into(), Value::from(id));
            }
            for (k, v) in extracted {
                entry.entry(k).or_insert(v); // existing (hand-verified) wins
            }
            moved += 1;
        }
        out.push_str(&Value::Object(rec).to_string());
        out.push('\n');
    }
    std::fs::write(library, out).with_context(|| format!("writing {}", library.display()))?;

    let mut cbody = String::from(COMPAT_HEADER);
    for m in compat_map.values() {
        cbody.push_str(&Value::Object(m.clone()).to_string());
        cbody.push('\n');
    }
    std::fs::write(compat, cbody).with_context(|| format!("writing {}", compat.display()))?;
    Ok(SplitReport { moved, compat_entries: compat_map.len() })
}

/// Seed report for [`categorize`].
pub struct CategorizeReport {
    pub titles: usize,
    pub assigned: usize,
    pub preserved: usize,
    pub uncategorized: usize,
    pub per_category: BTreeMap<String, usize>,
}

/// Seed/refresh the editable category DB (`data/categories.jsonl`, docs/21) from
/// the library + compatibility facets + the taxonomy seed maps. A title already
/// present in `out` is **preserved** (hand/GUI edits win); a new title is
/// auto-assigned: genre→bucket(s) (`genre_map`), kind→Applications/Utilities
/// (`kind_map`), `color=false`→Black & White, `mouse=false`→No Mouse Required,
/// the curated `recommended`/`adds` seeds, and a `catch_all_game` so no game is
/// unreachable. Re-runnable as the library grows.
pub fn categorize(
    library: &Path,
    compat: &Path,
    taxonomy: &Path,
    out: &Path,
) -> Result<CategorizeReport> {
    use std::collections::HashSet;
    let tax = crate::catalog::Taxonomy::load(taxonomy)?;
    let facets = parse_jsonl_by_id(compat); // id -> {color, mouse, …}
    let existing = parse_jsonl_by_id(out); // id -> {categories} (preserve)
    let rec: HashSet<&str> = tax.recommended.iter().map(String::as_str).collect();

    let lib_text =
        std::fs::read_to_string(library).with_context(|| format!("reading {}", library.display()))?;
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    let mut per_category: BTreeMap<String, usize> = BTreeMap::new();
    let (mut assigned, mut preserved, mut uncategorized) = (0, 0, 0);

    for line in lib_text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Ok(Value::Object(rec_obj)) = serde_json::from_str::<Value>(t) else { continue };
        let Some(id) = rec_obj.get("id").and_then(Value::as_str) else { continue };

        // Preserve a hand/GUI-curated entry verbatim.
        let cats: Vec<String> = if let Some(ex) = existing.get(id) {
            preserved += 1;
            ex.get("categories")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default()
        } else {
            let kind = rec_obj.get("kind").and_then(Value::as_str).unwrap_or("game");
            let mut cats: Vec<String> = Vec::new();
            let mut push = |c: &str| {
                if !c.is_empty() && !cats.iter().any(|x| x == c) {
                    cats.push(c.to_string());
                }
            };
            if rec.contains(id) {
                push("Recommended");
            }
            if let Some(adds) = tax.adds.get(id) {
                for c in adds {
                    push(c);
                }
            }
            if let Some(kc) = tax.kind_map.get(kind) {
                push(kc);
            }
            let mut had_game_bucket = false;
            if let Some(gs) = rec_obj.get("genre").and_then(Value::as_array) {
                for g in gs.iter().filter_map(Value::as_str) {
                    if let Some(b) = tax.genre_map.get(g) {
                        push(b);
                        had_game_bucket = true;
                    }
                }
            }
            let facet = |k: &str| facets.get(id).and_then(|f| f.get(k)).and_then(Value::as_bool);
            if facet("color") == Some(false) {
                push("Black & White");
            }
            if facet("mouse") == Some(false) {
                push("No Mouse Required");
            }
            // A game with no genre bucket (and not an app/utility) lands in the
            // catch-all so it's still reachable until hand-sorted.
            let is_game = !tax.kind_map.contains_key(kind);
            if is_game && !had_game_bucket && !tax.catch_all_game.is_empty() {
                push(&tax.catch_all_game);
            }
            cats
        };

        let mut cats = cats;
        tax.order_cats(&mut cats);
        if cats.is_empty() {
            uncategorized += 1;
        } else {
            assigned += 1;
            for c in &cats {
                *per_category.entry(c.clone()).or_default() += 1;
            }
        }
        rows.push((id.to_string(), cats));
    }

    let mut body = String::from(
        "# data/categories.jsonl — the editable category DB (docs/21), keyed by id.\n\
         # Seeded by `atrium library categorize`; hand/GUI edits are preserved on re-run.\n",
    );
    for (id, cats) in &rows {
        if cats.is_empty() {
            continue;
        }
        let m: Map<String, Value> = [
            ("id".to_string(), Value::from(id.clone())),
            ("categories".to_string(), Value::from(cats.clone())),
        ]
        .into_iter()
        .collect();
        body.push_str(&Value::Object(m).to_string());
        body.push('\n');
    }
    std::fs::write(out, body).with_context(|| format!("writing {}", out.display()))?;

    Ok(CategorizeReport { titles: rows.len(), assigned, preserved, uncategorized, per_category })
}

/// Serialize one title as a `library.jsonl` line (stable key order via the model).
fn record_line(t: &Title) -> String {
    let mut m = Map::new();
    m.insert("id".into(), Value::from(t.id.clone()));
    m.insert("name".into(), Value::from(t.name.clone()));
    m.insert("kind".into(), Value::from(t.kind.clone()));
    if let Some(y) = t.year {
        m.insert("year".into(), Value::from(y));
    }
    if !t.genre.is_empty() {
        m.insert("genre".into(), Value::from(t.genre.clone()));
    }
    m.insert("app".into(), Value::from(t.app_path.clone()));
    let mut hs = Map::new();
    hs.insert("donor".into(), Value::from(t.donor.clone()));
    hs.insert("path".into(), Value::from(t.src_path.clone()));
    m.insert("harvest_src".into(), Value::Object(hs));
    Value::Object(m).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_parsing() {
        assert_eq!(parse_year("1990"), Some(1990));
        assert_eq!(parse_year("1996+"), Some(1996));
        assert_eq!(parse_year("Infocom+"), None);
        assert_eq!(parse_year("Coding"), None);
    }

    #[test]
    fn launch_pick_prefers_direct_name_match_over_nested_and_finder() {
        let appls = vec![
            ("/Games/1990/Lemmings/Extras/Installer".into(), "xxxx".into()),
            ("/Games/1990/Lemmings/Lemmings".into(), "LEMM".into()),
            ("/Games/1990/Lemmings/Finder".into(), "MACS".into()),
        ];
        assert_eq!(
            pick_launch(&appls, "/Games/1990/Lemmings", "Lemmings"),
            "/Games/1990/Lemmings/Lemmings"
        );
    }

    #[test]
    fn split_moves_facets_and_existing_wins() {
        let dir = std::env::temp_dir();
        let lib = dir.join("atrium_split_lib.jsonl");
        let compat = dir.join("atrium_split_compat.jsonl");
        std::fs::write(
            &lib,
            "{\"id\":\"a\",\"name\":\"A\",\"color\":true,\"mouse\":false,\"vendor\":\"X\"}\n\
             {\"id\":\"b\",\"name\":\"B\"}\n",
        )
        .unwrap();
        // hand-verified: maxDepth 8 + color FALSE must win over the extracted color:true
        std::fs::write(&compat, "{\"id\":\"a\",\"maxDepth\":8,\"color\":false}\n").unwrap();

        let r = split(&lib, &compat).unwrap();
        assert_eq!(r.moved, 1); // only "a" had facets

        let lib_out = std::fs::read_to_string(&lib).unwrap();
        assert!(!lib_out.contains("\"color\""), "color stripped from library");
        assert!(!lib_out.contains("\"mouse\""), "mouse stripped from library");
        assert!(lib_out.contains("\"vendor\":\"X\""), "descriptive fields kept");

        let cmap = parse_jsonl_by_id(&compat);
        let a = &cmap["a"];
        assert_eq!(a["maxDepth"], 8);
        assert_eq!(a["color"], false, "hand-verified value wins over extracted");
        assert_eq!(a["mouse"], false, "new facet added");
        let _ = std::fs::remove_file(&lib);
        let _ = std::fs::remove_file(&compat);
    }

    #[test]
    fn record_has_kind_genre_list_and_harvest_src() {
        let t = Title {
            id: "archon".into(),
            name: "Archon".into(),
            kind: "game".into(),
            year: Some(1986),
            genre: vec![],
            donor: "boot.vhd".into(),
            src_path: "/Games/1986/Archon".into(),
            app_path: "Apps/Archon/Archon".into(),
        };
        let v: Value = serde_json::from_str(&record_line(&t)).unwrap();
        assert_eq!(v["kind"], "game");
        assert_eq!(v["year"], 1986);
        assert_eq!(v["harvest_src"]["donor"], "boot.vhd");
        assert_eq!(v["app"], "Apps/Archon/Archon");
    }
}
