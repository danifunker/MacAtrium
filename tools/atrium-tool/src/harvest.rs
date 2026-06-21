//! `atrium harvest` — pull apps out of a donor HFS image (the MacPack `.vhd`s)
//! into the MacAtrium tree (docs/06, docs/13 Priority 1).
//!
//! For each source app folder it: lists the folder, finds the launchable `APPL`,
//! extracts that plus its data files (both forks, via `rb-cli get-binhex`) to a
//! staging dir, and emits a `data/library.jsonl` stub (id/name/app path + year &
//! kind inferred from the source path). With `--into`, it also injects the forks
//! straight into a target image's `/MacAtrium/Apps`. System clutter bundled in a
//! game folder (System/Finder, Desktop DB/DF, Icon) is skipped.

use crate::rbcli::{Entry, RbCli};
use anyhow::{bail, Context, Result};
use std::path::Path;

/// What we learned about one harvested app — surfaced for the manifest + stubs.
#[derive(Debug)]
pub struct Harvested {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub app_path: String, // relative to /MacAtrium, e.g. "Apps/Dark Castle/Dark Castle"
    pub files: Vec<String>,
}

/// Files we never copy out of a game folder (a bundled mini-System, the Finder,
/// the desktop database, or the custom-icon marker).
fn is_clutter(e: &Entry) -> bool {
    matches!(e.ostype.as_str(), "ZSYS" | "FNDR")
        || e.name == "Desktop DB"
        || e.name == "Desktop DF"
        || e.name == "Desktop"
        || e.name == "Icon\r"
        || e.name == "Icon"
}

/// Lowercase ASCII slug; common Latin accents folded so "Déjà Vu" → "deja-vu".
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        let folded = fold_accent(c);
        if folded.is_ascii_alphanumeric() {
            out.push(folded.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' | 'À' | 'Á' | 'Â' | 'Ä' | 'Ã' | 'Å' => 'a',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ñ' | 'Ñ' => 'n',
        'ç' | 'Ç' => 'c',
        other => other,
    }
}

/// Infer (kind, genre) from the source folder path: `/Games/...` → game,
/// `/Applications/<genre>/...` → app + genre, `/Utilities/...` → utility.
fn infer_kind(src_folder: &str) -> (String, Option<String>) {
    let comps: Vec<&str> = src_folder.split('/').filter(|c| !c.is_empty()).collect();
    for (i, c) in comps.iter().enumerate() {
        match *c {
            "Games" => return ("game".into(), None),
            "Applications" => {
                let genre = comps.get(i + 1).filter(|g| **g != "?").map(|g| g.to_string());
                return ("app".into(), genre);
            }
            "Utilities" => return ("utility".into(), None),
            _ => {}
        }
    }
    ("game".into(), None)
}

/// A path component that is exactly a 4-digit plausible year (e.g. `/Games/1986/`).
fn infer_year(src_folder: &str) -> Option<i64> {
    src_folder
        .split('/')
        .filter_map(|c| c.parse::<i64>().ok())
        .find(|y| (1970..=2010).contains(y))
}

/// Harvest one source app folder. Returns None if the folder has no launchable
/// `APPL` (logged by the caller).
fn harvest_one(
    rb: &RbCli,
    image: &Path,
    src_folder: &str,
    stage: &Path,
    apps_root: &str,
    into: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<Option<Harvested>> {
    let entries = rb
        .ls(image, src_folder)
        .with_context(|| format!("listing {src_folder}"))?;

    let app = match entries.iter().find(|e| !e.is_dir && e.ostype == "APPL") {
        Some(e) => e.name.clone(),
        None => return Ok(None),
    };
    let app_dir = format!("{apps_root}/{app}");

    let keep: Vec<&Entry> = entries
        .iter()
        .filter(|e| !e.is_dir && !is_clutter(e))
        .collect();

    // Stage + (optionally) inject each kept file, both forks.
    let app_slug = slugify(&app);
    let stage_dir = stage.join(&app_slug);
    std::fs::create_dir_all(&stage_dir)?;
    if let Some(target) = into {
        rb.mkdir_p(target, &app_dir)?;
    }

    let mut files = Vec::new();
    for e in &keep {
        if e.name.contains('/') {
            warnings.push(format!("{src_folder}: skipping '{}' (name contains '/')", e.name));
            continue;
        }
        let src = format!("{src_folder}/{}", e.name);
        let hqx = stage_dir.join(format!("{}.hqx", slugify(&e.name)));
        rb.get_binhex(image, &src, &hqx)
            .with_context(|| format!("extracting {src}"))?;
        if let Some(target) = into {
            rb.put_binhex(target, &hqx, &app_dir)
                .with_context(|| format!("injecting {} into {app_dir}", e.name))?;
        }
        files.push(e.name.clone());
    }

    if files.is_empty() {
        warnings.push(format!("{src_folder}: APPL '{app}' but no files extracted"));
    }

    let (kind, genre) = infer_kind(src_folder);
    Ok(Some(Harvested {
        id: app_slug,
        name: app.clone(),
        kind,
        year: infer_year(src_folder),
        genre,
        app_path: format!("Apps/{app}/{app}"),
        files,
    }))
}

/// Append harvested stubs to a curated dataset, de-duplicated by `id` so existing
/// (hand-enriched) entries are never clobbered and re-runs are idempotent.
/// Returns (appended, skipped). The dataset file is created if absent.
fn append_to_dataset(dataset: &Path, harvested: &[Harvested]) -> Result<(usize, usize)> {
    let existing = std::fs::read_to_string(dataset).unwrap_or_default();
    let (out, appended, skipped) = merge_stubs(&existing, harvested);
    if appended > 0 {
        std::fs::write(dataset, out)
            .with_context(|| format!("appending to {}", dataset.display()))?;
    }
    Ok((appended, skipped))
}

/// Pure merge: append stub lines for harvested apps whose `id` isn't already in
/// `existing` (comments/blank lines ignored when collecting ids). Returns the new
/// file text and (appended, skipped) counts.
fn merge_stubs(existing: &str, harvested: &[Harvested]) -> (String, usize, usize) {
    let have: std::collections::HashSet<String> = existing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect();

    let mut appended = 0usize;
    let mut skipped = 0usize;
    let mut add = String::new();
    for h in harvested {
        if have.contains(&h.id) {
            skipped += 1;
        } else {
            add.push_str(&stub_line(h));
            add.push('\n');
            appended += 1;
        }
    }
    let mut out = existing.to_string();
    if appended > 0 {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&add);
    }
    (out, appended, skipped)
}

/// Serialize a harvested app as a `data/library.jsonl` stub line.
fn stub_line(h: &Harvested) -> String {
    let mut s = format!(
        "{{\"id\":{:?},\"name\":{:?},\"kind\":{:?}",
        h.id, h.name, h.kind
    );
    if let Some(y) = h.year {
        s.push_str(&format!(",\"year\":{y}"));
    }
    if let Some(g) = &h.genre {
        s.push_str(&format!(",\"genre\":[{g:?}]"));
    }
    s.push_str(&format!(",\"app\":{:?}}}", h.app_path));
    s
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    rb_bin: &str,
    image: &Path,
    apps: &[String],
    scan: Option<&str>,
    stage: &Path,
    into: Option<&Path>,
    apps_root: &str,
    append_to: Option<&Path>,
) -> Result<()> {
    let rb = RbCli::new(rb_bin);
    std::fs::create_dir_all(stage)?;

    // Build the work list: explicit --app folders + (if --scan) each subfolder.
    let mut folders: Vec<String> = apps.to_vec();
    if let Some(dir) = scan {
        let subs = rb.ls(image, dir).with_context(|| format!("scanning {dir}"))?;
        for e in subs.iter().filter(|e| e.is_dir) {
            folders.push(format!("{}/{}", dir.trim_end_matches('/'), e.name));
        }
    }
    if folders.is_empty() {
        bail!("nothing to harvest: pass --app <folder> (repeatable) or --scan <dir>");
    }

    let mut warnings = Vec::new();
    let mut harvested = Vec::new();
    for folder in &folders {
        match harvest_one(&rb, image, folder, stage, apps_root, into, &mut warnings)? {
            Some(h) => {
                eprintln!(
                    "harvested {:<28} <- {}  ({} file{})",
                    h.name,
                    folder,
                    h.files.len(),
                    if h.files.len() == 1 { "" } else { "s" }
                );
                harvested.push(h);
            }
            None => warnings.push(format!("{folder}: no APPL found, skipped")),
        }
    }

    // Emit the dataset stubs (merge into data/library.jsonl) + a manifest.
    let stubs: String = harvested
        .iter()
        .map(|h| stub_line(h) + "\n")
        .collect::<String>();
    let stubs_path = stage.join("harvested.jsonl");
    std::fs::write(&stubs_path, &stubs)?;

    eprintln!(
        "\n{} app(s) harvested -> {}\nstubs -> {}",
        harvested.len(),
        stage.display(),
        stubs_path.display()
    );
    if into.is_some() {
        eprintln!("injected into target image's {apps_root}");
    }
    if let Some(dataset) = append_to {
        let (appended, skipped) = append_to_dataset(dataset, &harvested)?;
        eprintln!(
            "appended {appended} new stub(s) to {} ({skipped} already present)",
            dataset.display()
        );
    }
    for w in &warnings {
        eprintln!("  warning: {w}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Dark Castle"), "dark-castle");
        assert_eq!(slugify("Maze Wars+"), "maze-wars");
        assert_eq!(slugify("Déjà Vu"), "deja-vu");
        assert_eq!(slugify("Shufflepuck Café"), "shufflepuck-cafe");
        assert_eq!(slugify("4th & Inches"), "4th-inches");
    }

    #[test]
    fn kind_and_genre() {
        assert_eq!(infer_kind("/Games/1986/Dark Castle 1.2"), ("game".into(), None));
        assert_eq!(
            infer_kind("/Applications/Music/Foo"),
            ("app".into(), Some("Music".into()))
        );
        assert_eq!(infer_kind("/Utilities/Bar"), ("utility".into(), None));
    }

    #[test]
    fn years() {
        assert_eq!(infer_year("/Games/1986/Dark Castle 1.2"), Some(1986));
        assert_eq!(infer_year("/Games/01 Sys 6/Dark Castle 1.2"), None);
        // a year-prefixed app name must not be mistaken for the folder year
        assert_eq!(infer_year("/Games/1989/1990 Ford Simulator II"), Some(1989));
    }

    #[test]
    fn clutter_filter() {
        let mk = |ostype: &str, name: &str| Entry {
            is_dir: false,
            ostype: ostype.into(),
            creator: "x".into(),
            name: name.into(),
            size: 0,
        };
        assert!(is_clutter(&mk("ZSYS", "System")));
        assert!(is_clutter(&mk("FNDR", "Finder")));
        assert!(is_clutter(&mk("BTFL", "Desktop DB")));
        assert!(!is_clutter(&mk("APPL", "Dark Castle")));
        assert!(!is_clutter(&mk("DCFL", "Data A")));
    }

    #[test]
    fn merge_dedups_by_id() {
        let existing = "# header\n{\"id\":\"dark-castle\",\"name\":\"Dark Castle\",\"vendor\":\"Silicon Beach Software\"}\n";
        let stubs = vec![
            Harvested { id: "dark-castle".into(), name: "Dark Castle".into(), kind: "game".into(), year: None, genre: None, app_path: "Apps/Dark Castle/Dark Castle".into(), files: vec![] },
            Harvested { id: "lemmings".into(), name: "Lemmings".into(), kind: "game".into(), year: Some(1991), genre: None, app_path: "Apps/Lemmings/Lemmings".into(), files: vec![] },
        ];
        let (out, appended, skipped) = merge_stubs(existing, &stubs);
        assert_eq!((appended, skipped), (1, 1));
        // existing enriched dark-castle entry preserved, lemmings appended
        assert!(out.contains("Silicon Beach Software"));
        assert!(out.contains("\"id\":\"lemmings\""));
        assert_eq!(out.matches("dark-castle").count(), 1);
    }

    #[test]
    fn stub_format() {
        let h = Harvested {
            id: "dark-castle".into(),
            name: "Dark Castle".into(),
            kind: "game".into(),
            year: Some(1986),
            genre: None,
            app_path: "Apps/Dark Castle/Dark Castle".into(),
            files: vec![],
        };
        assert_eq!(
            stub_line(&h),
            r#"{"id":"dark-castle","name":"Dark Castle","kind":"game","year":1986,"app":"Apps/Dark Castle/Dark Castle"}"#
        );
    }
}
