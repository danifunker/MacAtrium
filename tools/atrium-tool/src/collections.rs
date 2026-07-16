//! `atrium::collections` — named, saved game lists (build "flavors").
//!
//! A **collection** is one JSON file: `{ name, label, ids[], overrides{} }`. It
//! names a subset of the library so a build can be driven by a friendly name
//! (e.g. `Mac68KColorGames_v1`) rather than an inline id list. Collections layer
//! like [Targets](crate::targets): **bundled** examples in the repo
//! (`data/collections/*.json`) are overlaid by **user** collections
//! (`~/.macatrium/collections/*.json`), a user file winning a name clash. A build
//! selects one by name — the `ids` become a `Selection::List`, and per-title
//! `overrides` merge over the dataset at build time (the source-override channel).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One saved game list.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Collection {
    /// Machine name (matches the filename stem, e.g. "Mac68KColorGames_v1").
    #[serde(default)]
    pub name: String,
    /// Human description shown in the picker.
    #[serde(default)]
    pub label: String,
    /// The library ids this collection includes (kept in build order).
    pub ids: Vec<String>,
    /// Per-title field overrides merged over the matching dataset record at build
    /// time — the source-override channel (e.g. a corrected `app`/`harvest_src`).
    /// Keyed by id; each value is a partial record whose fields win.
    #[serde(default)]
    pub overrides: BTreeMap<String, Value>,
    /// Library ids this build should surface in the **Recommended** nav category,
    /// in addition to any taxonomy seeds. Scoped to this loadable game list, so it
    /// uses the collection's own ids (build order preserved).
    #[serde(default)]
    pub recommended: Vec<String>,
}

impl Collection {
    pub fn load(path: &Path) -> Result<Collection> {
        let txt = std::fs::read_to_string(path)
            .with_context(|| format!("reading collection {}", path.display()))?;
        serde_json::from_str(&txt)
            .with_context(|| format!("parsing collection {}", path.display()))
    }

    /// The `overrides` as an id-keyed JSONL overlay string, ready for
    /// [`merge::run`](crate::merge::run). Empty when there are no overrides.
    pub fn overrides_jsonl(&self) -> String {
        let mut s = String::new();
        for (id, ov) in &self.overrides {
            let mut m: Map<String, Value> = match ov {
                Value::Object(o) => o.clone(),
                _ => Map::new(),
            };
            m.insert("id".into(), Value::from(id.clone()));
            s.push_str(&Value::Object(m).to_string());
            s.push('\n');
        }
        s
    }
}

/// Bundled collections dir: `$MACATRIUM_COLLECTIONS`, else `data/collections`.
pub fn bundled_dir() -> PathBuf {
    std::env::var_os("MACATRIUM_COLLECTIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/collections"))
}

/// User collections dir: `<home>/.macatrium/collections` (a sibling of the
/// `~/.macatrium.json` settings file). `None` if no home is set.
pub fn user_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".macatrium").join("collections"))
}

/// The dirs searched for a collection, **user first** (so a user file wins).
fn search_dirs() -> Vec<PathBuf> {
    user_dir().into_iter().chain(std::iter::once(bundled_dir())).collect()
}

/// Load a collection by name (`<dir>/<name>.json`), the user dir winning over the
/// bundled dir. Fills `name` from the filename stem when the file omits it.
pub fn find(name: &str) -> Result<Collection> {
    for dir in search_dirs() {
        let p = dir.join(format!("{name}.json"));
        if p.exists() {
            let mut c = Collection::load(&p)?;
            if c.name.is_empty() {
                c.name = name.to_string();
            }
            return Ok(c);
        }
    }
    anyhow::bail!(
        "collection '{name}' not found (looked in {})",
        search_dirs().iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")
    )
}

/// One listed collection + where it came from.
pub struct Listed {
    pub collection: Collection,
    pub origin: &'static str, // "user" | "bundled"
    pub path: PathBuf,
}

/// List every available collection (a user entry shadows a bundled one of the
/// same name), sorted by name — for the `atrium collections` verb / GUI picker.
pub fn list() -> Vec<Listed> {
    let mut by_name: BTreeMap<String, Listed> = BTreeMap::new();
    // Bundled first, then user overwrites by name (so a user collection wins).
    for (dir, origin) in [
        (bundled_dir(), "bundled"),
        (user_dir().unwrap_or_default(), "user"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(mut c) = Collection::load(&p) {
                    let name = if c.name.is_empty() {
                        p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                    } else {
                        c.name.clone()
                    };
                    c.name = name.clone();
                    by_name.insert(name, Listed { collection: c, origin, path: p });
                }
            }
        }
    }
    by_name.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_emits_overrides_overlay() {
        let json = r#"{"name":"test","label":"a test","ids":["a","b"],
            "overrides":{"b":{"app":"Apps/B ƒ/B","source":"Macintosh Garden"}}}"#;
        let c: Collection = serde_json::from_str(json).unwrap();
        assert_eq!(c.ids, vec!["a", "b"]);
        assert_eq!(c.name, "test");
        // overrides -> a one-line id-keyed overlay carrying the override fields.
        let ov = c.overrides_jsonl();
        let v: Value = serde_json::from_str(ov.trim()).unwrap();
        assert_eq!(v["id"], "b");
        assert_eq!(v["app"], "Apps/B ƒ/B");
    }

    #[test]
    fn minimal_collection_has_no_overlay() {
        let c: Collection = serde_json::from_str(r#"{"ids":["x"]}"#).unwrap();
        assert_eq!(c.ids, vec!["x"]);
        assert!(c.overrides_jsonl().is_empty());
    }
}
