//! `atrium::selection` — choose which dataset apps go into an image (controller).
//!
//! The dataset (`library.jsonl`) is the master library. A build selects a subset
//! via [`Selection`](crate::config::Selection):
//!   * `List { ids }`       — an explicit manual list (handy for testing)
//!   * `Categories { .. }`  — every app whose `genre`/`categories` intersects
//!   * `All`                — everything (optionally OS-scoped)
//!
//! Optional per-app `minOS`/`maxOS` (dotted strings like "6.0.8"/"7.5") scope a
//! title to OS versions; when `base_os` is given, out-of-range apps are dropped.
//!
//! Each harvestable app carries a `source` ({donor, path}) — a donor *key* (into
//! [`donors`](crate::donors)) plus the app's path on that donor. [`harvest_plan`]
//! turns a selection into a per-donor harvest list the build runs.

use crate::config::Selection;
use crate::donors::Registry as Donors;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One dataset row, just the fields selection cares about.
struct Row {
    id: String,
    cats: Vec<String>,
    min_os: Option<String>,
    max_os: Option<String>,
    /// (donor key, app path on that donor), when harvestable.
    source: Option<(String, String)>,
}

fn rows(dataset: &Path) -> Result<Vec<Row>> {
    let text = std::fs::read_to_string(dataset)
        .with_context(|| format!("reading dataset {}", dataset.display()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let v: Value = match serde_json::from_str(t) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(id) = v.get("id").and_then(Value::as_str) else { continue };
        let mut cats: Vec<String> = Vec::new();
        for key in ["categories", "genre"] {
            if let Some(arr) = v.get(key).and_then(Value::as_array) {
                cats.extend(arr.iter().filter_map(Value::as_str).map(|s| s.to_lowercase()));
            }
        }
        let source = v.get("harvest_src").and_then(|s| {
            let donor = s.get("donor").and_then(Value::as_str)?;
            let path = s.get("path").and_then(Value::as_str)?;
            Some((donor.to_string(), path.to_string()))
        });
        out.push(Row {
            id: id.to_string(),
            cats,
            min_os: v.get("minOS").and_then(Value::as_str).map(str::to_string),
            max_os: v.get("maxOS").and_then(Value::as_str).map(str::to_string),
            source,
        });
    }
    Ok(out)
}

/// Compare dotted version strings ("6.0.8" vs "7.1") numerically, component-wise.
fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut pa = a.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    let pb: Vec<u32> = b.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect();
    let mut i = 0;
    loop {
        let x = pa.next();
        let y = pb.get(i).copied();
        match (x, y) {
            (None, None) => return std::cmp::Ordering::Equal,
            _ => {
                let xv = x.unwrap_or(0);
                let yv = y.unwrap_or(0);
                match xv.cmp(&yv) {
                    std::cmp::Ordering::Equal => {}
                    o => return o,
                }
            }
        }
        i += 1;
    }
}

fn os_ok(row: &Row, base_os: Option<&str>) -> bool {
    let Some(os) = base_os else { return true };
    if let Some(min) = &row.min_os {
        if ver_cmp(os, min) == std::cmp::Ordering::Less {
            return false;
        }
    }
    if let Some(max) = &row.max_os {
        if ver_cmp(os, max) == std::cmp::Ordering::Greater {
            return false;
        }
    }
    true
}

/// Apply a selection (+ optional OS scope) to the rows. Returns the chosen rows
/// (in selection order for `List`, dataset order otherwise) and any `List` ids
/// that weren't found, for the view to surface.
fn select_rows<'a>(
    rows: &'a [Row],
    sel: &Selection,
    base_os: Option<&str>,
) -> (Vec<&'a Row>, Vec<String>) {
    let mut missing = Vec::new();
    let chosen: Vec<&Row> = match sel {
        Selection::List { ids } => ids
            .iter()
            .filter_map(|id| match rows.iter().find(|r| &r.id == id) {
                Some(r) if os_ok(r, base_os) => Some(r),
                Some(_) => None, // exists but OS-incompatible
                None => {
                    missing.push(id.clone());
                    None
                }
            })
            .collect(),
        Selection::Categories { categories } => {
            let want: Vec<String> = categories.iter().map(|c| c.to_lowercase()).collect();
            rows.iter()
                .filter(|r| os_ok(r, base_os) && r.cats.iter().any(|c| want.contains(c)))
                .collect()
        }
        Selection::All => rows.iter().filter(|r| os_ok(r, base_os)).collect(),
    };
    (chosen, missing)
}

/// Resolve a selection into the concrete set of dataset ids to include, plus any
/// missing `List` ids.
pub fn resolve(
    dataset: &Path,
    sel: &Selection,
    base_os: Option<&str>,
) -> Result<(Vec<String>, Vec<String>)> {
    let rows = rows(dataset)?;
    let (chosen, missing) = select_rows(&rows, sel, base_os);
    Ok((chosen.iter().map(|r| r.id.clone()).collect(), missing))
}

/// Resolve a `harvest_src.donor` reference to a disk-image path: a `donors.json`
/// alias first (e.g. "pop"/"supplement" — the curated overlay), else a disk
/// *filename* (e.g. "boot.vhd" — what `library scan` records) found under the
/// configured MacPack folder. `None` if neither resolves.
fn resolve_donor(donor: &str, donors: &Donors, macpack_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = donors.get(donor) {
        return Some(p.clone());
    }
    if let Some(dir) = macpack_dir {
        let p = dir.join(donor);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Turn a selection into a per-donor harvest list: each entry is a donor disk
/// image and the app paths to pull from it. Selected apps with no `source`, or a
/// `source` whose donor neither matches a registry alias nor a disk in the
/// MacPack folder, are returned in `unresolved` (by id) so the caller can warn
/// rather than silently skip.
pub fn harvest_plan(
    dataset: &Path,
    sel: &Selection,
    base_os: Option<&str>,
    donors: &Donors,
    macpack_dir: Option<&Path>,
) -> Result<(Vec<(PathBuf, Vec<String>)>, Vec<String>)> {
    let rows = rows(dataset)?;
    let (chosen, _missing) = select_rows(&rows, sel, base_os);
    let mut groups: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let mut unresolved = Vec::new();
    for r in chosen {
        match &r.source {
            Some((donor, path)) => match resolve_donor(donor, donors, macpack_dir) {
                Some(img) => groups.entry(img).or_default().push(path.clone()),
                None => unresolved.push(r.id.clone()),
            },
            None => unresolved.push(r.id.clone()),
        }
    }
    Ok((groups.into_iter().collect(), unresolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::donors::Registry;

    #[test]
    fn donor_resolves_alias_then_filename() {
        let mut reg = Registry::default();
        reg.0.insert("pop".into(), PathBuf::from("/disks/pop.hda"));
        // registry alias wins
        assert_eq!(resolve_donor("pop", &reg, None), Some(PathBuf::from("/disks/pop.hda")));
        // filename donor resolves under the MacPack folder (must exist)
        let dir = std::env::temp_dir();
        let f = dir.join("atrium_donor_boot.vhd");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(
            resolve_donor("atrium_donor_boot.vhd", &reg, Some(&dir)),
            Some(f.clone())
        );
        // unknown alias + missing file -> None
        assert_eq!(resolve_donor("nope.vhd", &reg, Some(&dir)), None);
        let _ = std::fs::remove_file(&f);
    }
}
