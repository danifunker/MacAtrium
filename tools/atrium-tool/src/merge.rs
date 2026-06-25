//! `atrium merge` — apply a manual overrides overlay onto the dataset.
//!
//! The overlay (`data/overrides.jsonl`) holds **manually-captured** data keyed by
//! `id`: corrections, the `color`/`mouse` facets LaunchBox doesn't carry, and
//! whole records for titles `enrich` couldn't match. Overlay fields **win** over
//! the base (the point of a manual override); `--fill-missing` flips that to
//! only-fill-gaps. Overlay records whose `id` isn't in the base are appended as
//! new dataset entries. Comments/blank lines in the base are preserved.
//!
//! Pipeline precedence: `enrich` (fill-missing from LaunchBox) → `merge` (manual
//! overrides win) → `catalog`.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn is_blank_or_comment(t: &str) -> bool {
    t.is_empty() || t.starts_with('#') || t.starts_with("//")
}

fn missing(obj: &Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

/// Apply overlay fields (except `id`) onto base. Returns count of fields changed.
fn apply(base: &mut Map<String, Value>, overlay: &Map<String, Value>, fill_missing: bool) -> usize {
    let mut n = 0;
    for (k, v) in overlay {
        if k == "id" {
            continue;
        }
        if fill_missing && !missing(base, k) {
            continue;
        }
        if base.get(k) != Some(v) {
            base.insert(k.clone(), v.clone());
            n += 1;
        }
    }
    n
}

fn id_of(obj: &Map<String, Value>) -> Option<String> {
    obj.get("id").and_then(Value::as_str).map(String::from)
}

pub fn run(base: &Path, overlay: &Path, out: &Path, fill_missing: bool) -> Result<()> {
    // Parse the overlay into id -> partial record.
    let overlay_text = std::fs::read_to_string(overlay)
        .with_context(|| format!("reading {}", overlay.display()))?;
    let mut overrides: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in overlay_text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            continue;
        }
        let obj: Map<String, Value> =
            serde_json::from_str(t).with_context(|| format!("overlay line: {t}"))?;
        let id = id_of(&obj).with_context(|| format!("overlay record missing id: {t}"))?;
        if overrides.insert(id.clone(), obj).is_none() {
            order.push(id);
        }
    }

    // Walk the base, applying overrides to matching records (comments preserved).
    let base_text =
        std::fs::read_to_string(base).with_context(|| format!("reading {}", base.display()))?;
    let mut out_text = String::new();
    let mut applied: Vec<String> = Vec::new();
    let mut changed = 0usize;

    for line in base_text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            out_text.push_str(line);
            out_text.push('\n');
            continue;
        }
        let mut obj: Map<String, Value> =
            serde_json::from_str(t).with_context(|| format!("base line: {t}"))?;
        if let Some(id) = id_of(&obj) {
            if let Some(ov) = overrides.get(&id) {
                changed += apply(&mut obj, ov, fill_missing);
                applied.push(id);
            }
        }
        out_text.push_str(&serde_json::to_string(&Value::Object(obj))?);
        out_text.push('\n');
    }

    // Append overlay records that didn't match any base id — but ONLY complete
    // ones (with name + app). A partial override (e.g. just `maxDepth`/`color`)
    // for a title not in *this* build is an augment, not a new entry; appending
    // it as an incomplete record would break the catalog compile. The overrides
    // DB is shared across builds, so silently skip such partials here.
    let mut added = 0usize;
    let mut skipped = 0usize;
    for id in &order {
        if applied.contains(id) {
            continue;
        }
        let rec = &overrides[id];
        if missing(rec, "name") || missing(rec, "app") {
            skipped += 1; // partial override for a title not in this build
            continue;
        }
        out_text.push_str(&serde_json::to_string(&Value::Object(rec.clone()))?);
        out_text.push('\n');
        added += 1;
    }

    std::fs::write(out, &out_text).with_context(|| format!("writing {}", out.display()))?;
    eprintln!(
        "merge: {} override(s) applied ({changed} field change(s)), {added} new record(s) appended, \
         {skipped} partial(s) skipped (not in this build) -> {}",
        applied.len(),
        out.display()
    );
    Ok(())
}

/// `atrium set` — upsert a single override record (the CLI way to capture the
/// data that isn't in LaunchBox: the color/mouse facets, corrections, custom
/// art/desc). Updates the matching `id` record in the overlay or appends a new
/// one; comments/order preserved.
pub fn set(overlay: &Path, id: &str, fields: &Map<String, Value>) -> Result<()> {
    let text = std::fs::read_to_string(overlay).unwrap_or_default();
    let mut out = String::new();
    let mut found = false;
    let mut changed = 0usize;

    for line in text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut obj: Map<String, Value> =
            serde_json::from_str(t).with_context(|| format!("overlay line: {t}"))?;
        if id_of(&obj).as_deref() == Some(id) {
            found = true;
            for (k, v) in fields {
                if obj.get(k) != Some(v) {
                    obj.insert(k.clone(), v.clone());
                    changed += 1;
                }
            }
        }
        out.push_str(&serde_json::to_string(&Value::Object(obj))?);
        out.push('\n');
    }

    if !found {
        let mut obj = Map::new();
        obj.insert("id".into(), Value::from(id));
        for (k, v) in fields {
            obj.insert(k.clone(), v.clone());
        }
        out.push_str(&serde_json::to_string(&Value::Object(obj))?);
        out.push('\n');
        changed = fields.len();
    }

    std::fs::write(overlay, &out).with_context(|| format!("writing {}", overlay.display()))?;
    eprintln!(
        "set: {} \"{id}\" ({changed} field(s)) -> {}",
        if found { "updated" } else { "added" },
        overlay.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(s: &str) -> Map<String, Value> {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn overlay_wins_by_default() {
        let mut base = obj(r#"{"id":"x","year":1990,"vendor":"Old"}"#);
        let ov = obj(r#"{"id":"x","vendor":"New","color":true}"#);
        let n = apply(&mut base, &ov, false);
        assert_eq!(n, 2); // vendor changed + color added
        assert_eq!(base.get("vendor").unwrap(), "New");
        assert_eq!(base.get("color").unwrap(), &Value::Bool(true));
        assert_eq!(base.get("year").unwrap(), 1990); // untouched
    }

    #[test]
    fn fill_missing_keeps_existing() {
        let mut base = obj(r#"{"id":"x","vendor":"Curated"}"#);
        let ov = obj(r#"{"id":"x","vendor":"LB","mouse":true}"#);
        let n = apply(&mut base, &ov, true);
        assert_eq!(n, 1); // only mouse added; vendor kept
        assert_eq!(base.get("vendor").unwrap(), "Curated");
        assert_eq!(base.get("mouse").unwrap(), &Value::Bool(true));
    }
}
