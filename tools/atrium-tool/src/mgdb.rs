//! `atrium::mgdb` — explore the Macintosh Garden archive, cross-referenced
//! against MacPack (the library), to find **what we're missing**.
//!
//! Loads every MG record (`metadata/{games,apps}.ndjson`, ~21k titles) into a
//! queryable [`Entry`] table — OS support (`system`), `architecture`, `year`,
//! `category`, `perspective`, type (game/app) — and flags each as **in MacPack**
//! or not by matching its title against the library the same way `mg`/`enrich`
//! do (`candidate_keys`). Colour/B&W and mouse aren't in MG's structured data:
//! colour is filled offline from a scraped screenshot ([`detect_color`], cached
//! in `metadata/color-cache.jsonl`); mouse is only known for curated titles
//! (`compatibility.jsonl`). [`Filter`] narrows the table for a view (the CLI
//! `mg-list` verb and the GUI Database tab).

use crate::enrich::{candidate_keys, is_color_image};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Game,
    App,
}

impl Kind {
    /// The archive image subdir for this kind (`<archive>/games/<nid>/…`).
    fn dir(self) -> &'static str {
        match self {
            Kind::Game => "games",
            Kind::App => "apps",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Game => "game",
            Kind::App => "app",
        }
    }
}

/// One Macintosh Garden title, with the fields a view filters on.
#[derive(Clone, Debug)]
pub struct Entry {
    pub nid: i64,
    pub kind: Kind,
    pub title: String,
    pub year: Option<i64>,
    /// Supported OS labels, e.g. `["Mac OS 6","Mac OS 7",…]`.
    pub systems: Vec<String>,
    pub arch: Vec<String>,
    pub categories: Vec<String>,
    pub perspective: Vec<String>,
    /// Colour (true) / B&W (false): curated (`compatibility.jsonl`) wins, else the
    /// offline screenshot-detect cache, else `None` (unknown).
    pub color: Option<bool>,
    /// Mouse required — only known for curated titles; `None` otherwise.
    pub mouse: Option<bool>,
    /// Whether a same-named title is already in MacPack (the library).
    pub in_macpack: bool,
    pub screenshots: Vec<String>,
}

impl Entry {
    /// Numeric rank of an OS label for min/max ordering (`Mac OS 1 - 5`..`Mac OS X`).
    fn os_rank(label: &str) -> Option<f32> {
        Some(match label.trim() {
            "Mac OS 1 - 5" | "Mac OS 1-5" => 5.0,
            "Mac OS 6" => 6.0,
            "Mac OS 7" => 7.0,
            "Mac OS 8" => 8.0,
            "Mac OS 8.5" => 8.5,
            "Mac OS 9" => 9.0,
            "Mac OS X" | "Mac OS X (Classic)" => 10.0,
            _ => return None,
        })
    }
    /// Earliest supported OS label (lowest rank), or `None` if unknown.
    pub fn min_os(&self) -> Option<&str> {
        self.systems
            .iter()
            .filter(|s| Self::os_rank(s).is_some())
            .min_by(|a, b| Self::os_rank(a).unwrap().partial_cmp(&Self::os_rank(b).unwrap()).unwrap())
            .map(String::as_str)
    }
    /// Latest supported OS label (highest rank), or `None` if unknown.
    pub fn max_os(&self) -> Option<&str> {
        self.systems
            .iter()
            .filter(|s| Self::os_rank(s).is_some())
            .max_by(|a, b| Self::os_rank(a).unwrap().partial_cmp(&Self::os_rank(b).unwrap()).unwrap())
            .map(String::as_str)
    }
    pub fn is_68k(&self) -> bool {
        self.arch.iter().any(|a| a.eq_ignore_ascii_case("68k"))
    }
    /// The first on-disk screenshot for this title (preferring a gameplay shot
    /// over box/cover art), for colour detection.
    fn screenshot_on_disk(&self, archive: &Path) -> Option<PathBuf> {
        let dir = archive.join(self.kind.dir()).join(self.nid.to_string());
        let on_disk = |f: &String| {
            let p = dir.join(f);
            p.is_file().then_some(p)
        };
        let is_box = |n: &str| {
            let n = n.to_ascii_lowercase();
            n.contains("box") || n.contains("cover") || n.contains("_front") || n.contains("_back")
        };
        self.screenshots.iter().filter(|f| !is_box(f)).find_map(on_disk)
            .or_else(|| self.screenshots.iter().find_map(on_disk))
    }
}

fn arr_str(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// First 4-digit year in a string field (MG years are strings, sometimes ranges).
fn year_of(v: &Value) -> Option<i64> {
    let s = v.get("year").and_then(Value::as_str)?;
    let digits: String = s.chars().skip_while(|c| !c.is_ascii_digit()).take(4).collect();
    digits.parse().ok().filter(|y: &i64| *y > 1900 && *y < 2100)
}

/// Build the candidate-key → library-id index used both to flag MacPack presence
/// and to pull curated colour/mouse for a matched title.
fn library_index(library_jsonl: &[u8]) -> HashMap<String, String> {
    let mut idx = HashMap::new();
    for line in String::from_utf8_lossy(library_jsonl).lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(t) else { continue };
        let (Some(id), Some(name)) = (
            v.get("id").and_then(Value::as_str),
            v.get("name").and_then(Value::as_str),
        ) else {
            continue;
        };
        for k in candidate_keys(name) {
            idx.entry(k).or_insert_with(|| id.to_string());
        }
    }
    idx
}

/// Curated colour/mouse facets keyed by library id (from `compatibility.jsonl`).
fn compat_facets(compat_jsonl: &[u8]) -> HashMap<String, (Option<bool>, Option<bool>)> {
    let mut m = HashMap::new();
    for line in String::from_utf8_lossy(compat_jsonl).lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(t) else { continue };
        let Some(id) = v.get("id").and_then(Value::as_str) else { continue };
        let color = v.get("color").and_then(Value::as_bool);
        let mouse = v.get("mouse").and_then(Value::as_bool);
        if color.is_some() || mouse.is_some() {
            m.insert(id.to_string(), (color, mouse));
        }
    }
    m
}

/// The offline colour cache: MG nid → colour (true)/B&W (false).
pub type ColorCache = HashMap<i64, bool>;

/// Path of the colour cache within an archive.
pub fn color_cache_path(archive: &Path) -> PathBuf {
    archive.join("metadata/color-cache.jsonl")
}

pub fn load_color_cache(archive: &Path) -> ColorCache {
    let mut c = ColorCache::new();
    if let Ok(text) = std::fs::read_to_string(color_cache_path(archive)) {
        for line in text.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                if let (Some(nid), Some(col)) =
                    (v.get("nid").and_then(Value::as_i64), v.get("color").and_then(Value::as_bool))
                {
                    c.insert(nid, col);
                }
            }
        }
    }
    c
}

pub fn save_color_cache(archive: &Path, cache: &ColorCache) -> Result<()> {
    let mut keys: Vec<&i64> = cache.keys().collect();
    keys.sort();
    let mut out = String::new();
    for nid in keys {
        out.push_str(&format!("{{\"nid\":{nid},\"color\":{}}}\n", cache[nid]));
    }
    let p = color_cache_path(archive);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&p, out).with_context(|| format!("writing colour cache {}", p.display()))
}

fn parse_ndjson(path: &Path, key: &str, kind: Kind, out: &mut Vec<Entry>) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Ok(()), // a missing file just contributes no records
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        let Some(g) = rec.get("data").and_then(|d| d.get(key)) else { continue };
        let Some(title) = g.get("title").and_then(Value::as_str) else { continue };
        let categories = {
            let c = arr_str(g, "category");
            if c.is_empty() { arr_str(g, "category_app") } else { c }
        };
        let screenshots = g
            .get("screenshots")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|s| s.get("filename").and_then(Value::as_str)).map(str::to_string).collect())
            .unwrap_or_default();
        out.push(Entry {
            nid: rec.get("nid").and_then(Value::as_i64).unwrap_or(-1),
            kind,
            title: title.to_string(),
            year: year_of(g),
            systems: arr_str(g, "system"),
            arch: arr_str(g, "architecture"),
            categories,
            perspective: arr_str(g, "perspective"),
            color: None,
            mouse: None,
            in_macpack: false,
            screenshots,
        });
    }
    Ok(())
}

/// Load + cross-reference the whole archive. `library`/`compat` are the JSONL
/// bytes to cross-reference against (the bundled MacPack scan + compatibility
/// overlay by default). Colour comes from the on-disk cache (curated wins); mouse
/// from the curated overlay only.
pub fn load(archive: &Path, library: &[u8], compat: &[u8]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    parse_ndjson(&archive.join("metadata/games.ndjson"), "game", Kind::Game, &mut entries)?;
    parse_ndjson(&archive.join("metadata/apps.ndjson"), "app", Kind::App, &mut entries)?;

    let lib = library_index(library);
    let facets = compat_facets(compat);
    let cache = load_color_cache(archive);

    for e in &mut entries {
        // MacPack presence + curated facets via the first matching candidate key.
        let lib_id = candidate_keys(&e.title).into_iter().find_map(|k| lib.get(&k).cloned());
        e.in_macpack = lib_id.is_some();
        let curated = lib_id.as_deref().and_then(|id| facets.get(id));
        e.color = curated.and_then(|(c, _)| *c).or_else(|| cache.get(&e.nid).copied());
        e.mouse = curated.and_then(|(_, m)| *m);
    }
    Ok(entries)
}

/// Detect colour for the given entries that have no colour yet but do have a
/// screenshot on disk; update `cache` in place and return how many were detected.
/// `progress(done, total)` is called as it goes.
pub fn detect_color(
    archive: &Path,
    entries: &[Entry],
    cache: &mut ColorCache,
    mut progress: impl FnMut(usize, usize),
) -> usize {
    let todo: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.color.is_none() && !cache.contains_key(&e.nid))
        .collect();
    let total = todo.len();
    let mut done = 0;
    for e in todo {
        if let Some(shot) = e.screenshot_on_disk(archive) {
            if let Ok(col) = is_color_image(&shot) {
                cache.insert(e.nid, col);
                done += 1;
            }
        }
        progress(done, total);
    }
    done
}

/// A query over the table — every `Some` field must match (AND).
#[derive(Clone, Default, Debug)]
pub struct Filter {
    pub kind: Option<Kind>,
    pub arch: Option<String>,    // case-insensitive substring of any architecture
    pub system: Option<String>,  // a supported-OS label that must be present
    pub min_year: Option<i64>,
    pub max_year: Option<i64>,
    pub category: Option<String>, // case-insensitive substring of any category
    pub color: Option<bool>,
    pub mouse: Option<bool>,
    /// `Some(true)` = only titles already in MacPack; `Some(false)` = only missing.
    pub in_macpack: Option<bool>,
    pub search: Option<String>, // case-insensitive substring of the title
}

impl Filter {
    pub fn matches(&self, e: &Entry) -> bool {
        if let Some(k) = self.kind {
            if e.kind != k {
                return false;
            }
        }
        if let Some(a) = &self.arch {
            let a = a.to_lowercase();
            if !e.arch.iter().any(|x| x.to_lowercase().contains(&a)) {
                return false;
            }
        }
        if let Some(s) = &self.system {
            let s = s.to_lowercase();
            if !e.systems.iter().any(|x| x.to_lowercase().contains(&s)) {
                return false;
            }
        }
        if let Some(c) = &self.category {
            let c = c.to_lowercase();
            if !e.categories.iter().any(|x| x.to_lowercase().contains(&c)) {
                return false;
            }
        }
        if let Some(min) = self.min_year {
            if e.year.map(|y| y < min).unwrap_or(true) {
                return false;
            }
        }
        if let Some(max) = self.max_year {
            if e.year.map(|y| y > max).unwrap_or(true) {
                return false;
            }
        }
        if let Some(col) = self.color {
            if e.color != Some(col) {
                return false;
            }
        }
        if let Some(m) = self.mouse {
            if e.mouse != Some(m) {
                return false;
            }
        }
        if let Some(p) = self.in_macpack {
            if e.in_macpack != p {
                return false;
            }
        }
        if let Some(q) = &self.search {
            if !e.title.to_lowercase().contains(&q.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

fn distinct<'a>(it: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut set: HashSet<&str> = HashSet::new();
    for s in it {
        set.insert(s);
    }
    let mut v: Vec<String> = set.into_iter().map(str::to_string).collect();
    v.sort();
    v
}

/// Distinct category names across the table (sorted) — for a filter combo.
pub fn categories(entries: &[Entry]) -> Vec<String> {
    distinct(entries.iter().flat_map(|e| e.categories.iter()))
}

/// Distinct architecture labels across the table (sorted) — for a filter combo.
pub fn architectures(entries: &[Entry]) -> Vec<String> {
    distinct(entries.iter().flat_map(|e| e.arch.iter()))
}

/// Distinct supported-OS labels across the table (sorted) — for a filter combo.
pub fn systems(entries: &[Entry]) -> Vec<String> {
    distinct(entries.iter().flat_map(|e| e.systems.iter()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, kind: Kind, year: i64, systems: &[&str], arch: &[&str], cats: &[&str]) -> Entry {
        Entry {
            nid: 1,
            kind,
            title: title.into(),
            year: Some(year),
            systems: systems.iter().map(|s| s.to_string()).collect(),
            arch: arch.iter().map(|s| s.to_string()).collect(),
            categories: cats.iter().map(|s| s.to_string()).collect(),
            perspective: vec![],
            color: None,
            mouse: None,
            in_macpack: false,
            screenshots: vec![],
        }
    }

    #[test]
    fn min_max_os_from_system_set() {
        let e = entry("X", Kind::Game, 1992, &["Mac OS 7", "Mac OS 6", "Mac OS 9", "Mac OS 8.5"], &["68k"], &["Adventure"]);
        assert_eq!(e.min_os(), Some("Mac OS 6"));
        assert_eq!(e.max_os(), Some("Mac OS 9"));
        assert!(e.is_68k());
    }

    #[test]
    fn filter_ands_fields() {
        let mut e = entry("Dark Castle", Kind::Game, 1986, &["Mac OS 6"], &["68k"], &["Action"]);
        e.in_macpack = false;
        let f = Filter {
            kind: Some(Kind::Game),
            arch: Some("68k".into()),
            in_macpack: Some(false), // missing from MacPack
            max_year: Some(1990),
            category: Some("action".into()),
            ..Default::default()
        };
        assert!(f.matches(&e));
        // a PPC-only title fails the arch filter
        let ppc = entry("Other", Kind::Game, 1997, &["Mac OS 8"], &["PPC"], &["RPG"]);
        assert!(!f.matches(&ppc));
        // an in-MacPack title fails the "missing" filter
        let mut have = e.clone();
        have.in_macpack = true;
        assert!(!f.matches(&have));
    }

    #[test]
    fn cross_reference_flags_macpack_presence() {
        // a tiny library with one title; an MG entry of the same name matches.
        let library = br#"{"id":"dark-castle","name":"Dark Castle"}"#;
        let lib = library_index(library);
        let hit = candidate_keys("Dark Castle 1.2").into_iter().any(|k| lib.contains_key(&k));
        assert!(hit, "versioned MG title still matches the library by candidate key");
        let miss = candidate_keys("Totally Unknown Game").into_iter().any(|k| lib.contains_key(&k));
        assert!(!miss);
    }

    #[test]
    fn year_parses_leading_digits() {
        let v: Value = serde_json::from_str(r#"{"year":"1992"}"#).unwrap();
        assert_eq!(year_of(&v), Some(1992));
        let v: Value = serde_json::from_str(r#"{"year":"c. 1995"}"#).unwrap();
        assert_eq!(year_of(&v), Some(1995));
        let v: Value = serde_json::from_str(r#"{"year":"unknown"}"#).unwrap();
        assert_eq!(year_of(&v), None);
    }
}
