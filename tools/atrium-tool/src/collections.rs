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

    /// Write the collection back as pretty JSON (the `data/collections/*.json`
    /// format). Used by the in-place disk verbs to keep the saved selection in step
    /// with what they just added to / removed from a built disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .with_context(|| format!("serialising collection {}", self.name))?;
        std::fs::write(path, json + "\n")
            .with_context(|| format!("writing collection {}", path.display()))
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

/// The primary bundled dir — kept for callers that need a single path. Prefer
/// [`bundled_dirs`], which also finds the copy shipped next to the executable.
pub fn bundled_dir() -> PathBuf {
    bundled_dirs().into_iter().next().unwrap_or_else(|| PathBuf::from("data/collections"))
}

/// Every directory searched for **bundled** collections — the curated lists that
/// ship with the app — in priority order:
///
/// 1. `$MACATRIUM_COLLECTIONS`, if set (explicit override, wins outright);
/// 2. `collections/` **next to the running executable** — how an installed app
///    finds the lists packaged alongside it;
/// 3. `../Resources/collections` — the same thing inside a macOS `.app`;
/// 4. `data/collections` relative to the working dir — the repo-checkout case.
///
/// Only existing dirs are returned. All of 2-4 are listed because a developer
/// runs from a checkout while a user runs from an install; neither layout should
/// need configuration.
pub fn bundled_dirs() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("MACATRIUM_COLLECTIONS") {
        return vec![PathBuf::from(p)];
    }
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
    let mut dirs = bundled_candidates(exe_dir.as_deref());
    dirs.retain(|d| d.is_dir());
    dirs
}

/// The search list [`bundled_dirs`] filters — split out so the packaging layout
/// can be tested without a real installed app to run from.
fn bundled_candidates(exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe_dir {
        dirs.push(dir.join("collections"));
        // A macOS .app keeps non-code in Contents/Resources: codesign treats
        // everything beside the executable in Contents/MacOS as a code object, so
        // a plain .json there fails the whole bundle with "code object is not
        // signed at all". The executable sits in Contents/MacOS, which makes
        // Resources its sibling one level up.
        dirs.push(dir.join("..").join("Resources").join("collections"));
    }
    dirs.push(PathBuf::from("data/collections"));
    dirs
}

/// Where the user's **own** collections are saved:
/// [`Settings::collections_dir`](crate::settings::Settings) if set, else
/// `<Documents>/MacAtrium/Collections` (see
/// [`settings::user_root`](crate::settings::user_root)).
///
/// Documents rather than a dotfolder because these are the user's own work —
/// discoverable, backed up, and easy to hand to someone else. They're small JSON
/// id lists, so cloud sync is a feature here, unlike for built disk images.
pub fn user_dir() -> Option<PathBuf> {
    if let Some(d) = crate::settings::Settings::load_default().collections_dir {
        return Some(d);
    }
    Some(crate::settings::user_root().join("Collections"))
}

/// The pre-Documents user dir (`<home>/.macatrium/collections`). Still searched
/// for reads so collections saved by an earlier build don't vanish; nothing is
/// ever written here any more.
fn legacy_user_dir() -> Option<PathBuf> {
    crate::settings::home().map(|h| h.join(".macatrium").join("collections"))
}

/// The user collections dir, created if missing — where a newly saved collection
/// goes. Errors only if there's no home at all or the dir can't be created.
pub fn ensure_user_dir() -> Result<PathBuf> {
    let dir = user_dir().context("no user collections dir (no HOME / USERPROFILE set)")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating collections dir {}", dir.display()))?;
    Ok(dir)
}

/// Save `c` into the user collections dir as `<name>.json`, creating the dir if
/// needed, and return the path written. This is the write side the GUI uses: a
/// saved collection always lands somewhere the app can find again, regardless of
/// the working directory.
pub fn save_user(c: &Collection) -> Result<PathBuf> {
    anyhow::ensure!(!c.name.trim().is_empty(), "a collection needs a name");
    let path = ensure_user_dir()?.join(format!("{}.json", c.name.trim()));
    c.save(&path)?;
    Ok(path)
}

/// Delete a saved collection by name, returning the path removed. Only the user
/// dir is touched — a bundled (repo) collection is never deleted out from under a
/// checkout.
pub fn delete_user(name: &str) -> Result<PathBuf> {
    // Both user locations, so a collection saved by an earlier build (which used
    // ~/.macatrium/collections) is still removable rather than undeletable.
    let cands: Vec<PathBuf> = user_dir()
        .into_iter()
        .chain(legacy_user_dir())
        .map(|d| d.join(format!("{name}.json")))
        .collect();
    let path = cands.iter().find(|p| p.exists()).ok_or_else(|| {
        anyhow::anyhow!(
            "no user collection {name:?} (looked in {})",
            cands.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        )
    })?;
    std::fs::remove_file(path)
        .with_context(|| format!("removing collection {}", path.display()))?;
    Ok(path.clone())
}

/// The dirs searched for a collection, **user first** (so a user file wins over
/// a shipped one of the same name), then the legacy user dir, then every bundled
/// location.
fn search_dirs() -> Vec<PathBuf> {
    user_dir()
        .into_iter()
        .chain(legacy_user_dir())
        .chain(bundled_dirs())
        .collect()
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

/// The file backing a named collection (`<dir>/<name>.json`), user dir winning —
/// the path the in-place disk verbs write back to when syncing the selection.
pub fn find_path(name: &str) -> Option<PathBuf> {
    search_dirs().into_iter().map(|d| d.join(format!("{name}.json"))).find(|p| p.exists())
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
    // Legacy sits between the two: it outranks a shipped list but yields to a
    // collection saved in the current user dir.
    // `.rev()`: bundled_dirs is priority-ordered (highest first), but this loop
    // lets later inserts win — so walk it backwards to keep that priority.
    let dirs: Vec<(PathBuf, &'static str)> = bundled_dirs()
        .into_iter()
        .rev()
        .map(|d| (d, "bundled"))
        .chain(legacy_user_dir().map(|d| (d, "user")))
        .chain(user_dir().map(|d| (d, "user")))
        .collect();
    for (dir, origin) in dirs {
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

    /// A macOS `.app` cannot carry data files beside its executable: codesign
    /// treats everything in `Contents/MacOS` as a code object and fails the whole
    /// bundle with "code object is not signed at all" over a stray `.json`. So the
    /// packaged lists live in `Contents/Resources/collections`, and the search has
    /// to look there — keep this in step with the bundle layout in release.yml.
    #[test]
    fn a_mac_app_bundle_finds_its_lists_in_resources() {
        let exe_dir = PathBuf::from("/Apps/MacAtrium Manager.app/Contents/MacOS");
        let cands = bundled_candidates(Some(&exe_dir));

        let has = |suffix: &str| {
            cands.iter().any(|p| {
                p.components().count() > 0
                    && p.to_string_lossy().replace('\\', "/").ends_with(suffix)
            })
        };
        assert!(has("Contents/MacOS/collections"), "beside the exe: {cands:?}");
        assert!(
            has("Contents/MacOS/../Resources/collections"),
            "the bundle's Resources dir is missing from the search: {cands:?}"
        );
        assert_eq!(
            cands.last().map(|p| p.to_string_lossy().replace('\\', "/")),
            Some("data/collections".to_string()),
            "the repo-checkout path stays last, so an install never loses to it"
        );
    }

    /// With no executable path at all, the checkout layout is still searched —
    /// the lookup must never come back empty-handed for a developer.
    #[test]
    fn without_an_exe_dir_the_checkout_path_remains() {
        assert_eq!(bundled_candidates(None), vec![PathBuf::from("data/collections")]);
    }

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

    /// The separation the whole layout rests on: what the user saves must never
    /// land in a directory the app ships lists from. If these ever coincided,
    /// "Save as…" would overwrite a curated list and an app update would silently
    /// revert the user's work.
    #[test]
    fn the_user_dir_is_never_a_shipped_dir() {
        let Some(user) = user_dir() else { return };
        for b in bundled_dirs() {
            assert_ne!(
                b.canonicalize().unwrap_or(b.clone()),
                user.canonicalize().unwrap_or(user.clone()),
                "user collections would be written into a shipped dir: {}",
                b.display()
            );
        }
    }

    /// `bundled_dirs` only reports directories that exist, so callers can iterate
    /// without probing — and an app with no shipped lists yields an empty list
    /// rather than a phantom path.
    #[test]
    fn bundled_dirs_are_real_directories() {
        for d in bundled_dirs() {
            assert!(d.is_dir(), "bundled_dirs returned a non-directory: {}", d.display());
        }
    }

    /// A user collection shadows a shipped one of the same name — the fork rule
    /// that lets someone tweak a curated list without losing the original.
    #[test]
    fn search_order_puts_user_before_shipped() {
        let dirs = search_dirs();
        if let (Some(user), Some(first_bundled)) = (user_dir(), bundled_dirs().into_iter().next()) {
            let ui = dirs.iter().position(|d| *d == user);
            let bi = dirs.iter().position(|d| *d == first_bundled);
            if let (Some(ui), Some(bi)) = (ui, bi) {
                assert!(ui < bi, "a shipped dir outranks the user dir in the search order");
            }
        }
    }
}
