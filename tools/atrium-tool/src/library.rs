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
