//! `atrium harvest` — pull apps out of a donor HFS image (the MacPack `.vhd`s)
//! into the MacAtrium tree (docs/06, docs/13 Priority 1).
//!
//! For each source app folder it: finds the launchable `APPL`, then **recursively
//! copies the whole folder tree** (both forks, via `rb-cli get-binhex`),
//! mirroring sub-folders under `/MacAtrium/Apps/<app>/` — so games that keep data
//! in sub-folders (DOOM `.wad`s, level/data directories) come over intact, not
//! just the top-level files. It emits a `data/library.jsonl` stub (id/name/app
//! path + year & kind inferred from the source path). With `--into`, it injects
//! the forks straight into the target image. System clutter bundled in a game
//! folder (System/Finder, Desktop DB/DF, Icon, a bundled System Folder/Trash) is
//! skipped at every level.

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

/// Sub-folders we never recurse into (a bundled System Folder or the volume's
/// housekeeping folders) — they'd drag in a whole System, not game data.
fn is_clutter_dir(e: &Entry) -> bool {
    e.is_dir
        && matches!(
            e.name.as_str(),
            "System Folder"
                | "Desktop Folder"
                | "Trash"
                | "Temporary Items"
                | "TheVolumeSettingsFolder"
                | "Cleanup At Startup"
        )
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

/// Length of the common leading run of two strings (ASCII slug comparison).
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// A colour/B&W hint from an app's name: +1 for a "color"/"8-bit"/"256" build,
/// -1 for a "b/w"/"mono"/"1-bit" build, 0 otherwise. Used to pick the colour
/// build when a game ships both under one folder (e.g. "SimAnt B&W" vs
/// "SimAnt Color"), since this launcher is colour-first.
fn variant_rank(name: &str) -> i32 {
    let n = name.to_ascii_lowercase();
    let bw = n.contains("b/w") || n.contains("b&w") || n.contains("mono")
        || n.contains("1-bit") || n.contains("1 bit")
        || n.contains("black & white") || n.contains("black and white");
    let color = n.contains("color") || n.contains("colour")
        || n.contains("8-bit") || n.contains("8 bit") || n.contains("256")
        || n.contains("24-bit");
    color as i32 - bw as i32
}

/// Choose the app a game folder should launch. Skips the Finder (a bundled
/// mini-Finder some self-booting games ship — creator `MACS` / name "Finder")
/// and, among the remaining `APPL`s, prefers the one whose name best matches the
/// source folder name (so "Crystal Quest" wins over a bundled "CritterEditor",
/// and a "... Level Editor" loses to the game), preferring a colour build over a
/// B&W one on a tie. Falls back to the first APPL.
fn pick_appl(entries: &[Entry], src_folder: &str) -> Option<String> {
    let base_slug = slugify(src_folder.rsplit('/').next().unwrap_or(src_folder));
    let real: Vec<&Entry> = entries
        .iter()
        .filter(|e| !e.is_dir && e.ostype == "APPL" && e.creator != "MACS" && e.name != "Finder")
        .collect();
    if real.is_empty() {
        // No "real" app — last resort, take any APPL so we don't drop the title.
        return entries
            .iter()
            .find(|e| !e.is_dir && e.ostype == "APPL")
            .map(|e| e.name.clone());
    }
    // Best folder-name match; then the colour build; then the shorter name
    // (editors/extras tend to be "<game> <something>"); ls order as final tiebreak.
    let best = real
        .iter()
        .enumerate()
        .max_by_key(|(i, e)| {
            let pfx = common_prefix_len(&slugify(&e.name), &base_slug);
            (pfx, variant_rank(&e.name), std::cmp::Reverse(e.name.len()), std::cmp::Reverse(*i))
        })
        .map(|(_, e)| e)?;
    let best_pfx = common_prefix_len(&slugify(&best.name), &base_slug);
    Some(if best_pfx >= 3 { best.name.clone() } else { real[0].name.clone() })
}

/// Recursively copy a source folder's tree into the target, mirroring sub-folders
/// under `app_dir`. `rel` is the path so far relative to `app_dir` ("" at the
/// top). Every kept file is extracted (both forks) and, with `into`, injected
/// into its mirrored directory. Clutter files/folders are skipped at each level.
#[allow(clippy::too_many_arguments)]
fn harvest_tree(
    rb: &RbCli,
    image: &Path,
    src_folder: &str,
    rel: &str,
    app_dir: &str,
    stage_dir: &Path,
    into: Option<&Path>,
    files: &mut Vec<String>,
    warnings: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth > 12 {
        warnings.push(format!("{src_folder}: folder nesting too deep, stopping"));
        return Ok(());
    }
    let entries = rb
        .ls(image, src_folder)
        .with_context(|| format!("listing {src_folder}"))?;

    for e in &entries {
        if e.name.contains('/') {
            warnings.push(format!("{src_folder}: skipping '{}' (name contains '/')", e.name));
            continue;
        }
        // rb-cli treats source paths as globs, so it can't address a name with a
        // glob metacharacter (see PROMPT-literal-path-flag.md in rusty-backup).
        // Skip such entries rather than abort the whole harvest.
        if e.name.contains(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}')) {
            warnings.push(format!(
                "{src_folder}: skipping '{}' (name has a glob metachar; rb-cli can't address it yet)",
                e.name
            ));
            continue;
        }
        let child_src = format!("{}/{}", src_folder.trim_end_matches('/'), e.name);
        let child_rel = if rel.is_empty() {
            e.name.clone()
        } else {
            format!("{rel}/{}", e.name)
        };

        if e.is_dir {
            if is_clutter_dir(e) {
                continue; // a bundled System Folder / housekeeping dir
            }
            // Mirror the sub-folder on the target, then recurse into it.
            let child_dir = format!("{app_dir}/{child_rel}");
            if let Some(target) = into {
                rb.mkdir_p(target, &child_dir)?;
            }
            harvest_tree(
                rb, image, &child_src, &child_rel, app_dir, stage_dir, into, files, warnings,
                depth + 1,
            )?;
            continue;
        }

        if is_clutter(e) {
            continue;
        }

        // A file: extract both forks and inject into its mirrored directory.
        let dst_dir = match child_rel.rfind('/') {
            Some(i) => format!("{app_dir}/{}", &child_rel[..i]),
            None => app_dir.to_string(),
        };
        let hqx = stage_dir.join(format!("{}.hqx", slugify(&child_rel)));
        rb.get_binhex(image, &child_src, &hqx)
            .with_context(|| format!("extracting {child_src}"))?;
        if let Some(target) = into {
            rb.put_binhex(target, &hqx, &dst_dir)
                .with_context(|| format!("injecting {child_rel} into {dst_dir}"))?;
        }
        files.push(child_rel);
    }
    Ok(())
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

    let app = match pick_appl(&entries, src_folder) {
        Some(name) => name,
        None => return Ok(None),
    };
    let app_dir = format!("{apps_root}/{app}");

    let app_slug = slugify(&app);
    let stage_dir = stage.join(&app_slug);
    std::fs::create_dir_all(&stage_dir)?;
    if let Some(target) = into {
        rb.mkdir_p(target, &app_dir)?;
    }

    // Recursively copy the whole folder tree (preserving sub-folders).
    let mut files = Vec::new();
    harvest_tree(
        rb, image, src_folder, "", &app_dir, &stage_dir, into, &mut files, warnings, 0,
    )?;

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
    fn pick_appl_prefers_game_over_editor_and_finder() {
        let e = |ostype: &str, creator: &str, name: &str| Entry {
            is_dir: false, ostype: ostype.into(), creator: creator.into(),
            name: name.into(), size: 0,
        };
        // Crystal Quest folder bundling an editor + the game: pick the game.
        let entries = vec![
            e("APPL", "CCC1", "CritterEditor 1.0.4M"),
            e("APPL", "CQST", "Crystal Quest"),
        ];
        assert_eq!(pick_appl(&entries, "/Games/1988/Crystal Quest 2.2.5m").as_deref(), Some("Crystal Quest"));
        // A bundled Finder must never be chosen when a real app exists.
        let entries = vec![
            e("APPL", "MACS", "Finder"),
            e("APPL", "WZRD", "Wizardry"),
        ];
        assert_eq!(pick_appl(&entries, "/Games/1990/Wizardry I (3.02)").as_deref(), Some("Wizardry"));
        // Only a Finder present -> last resort still returns it (don't drop the title).
        let entries = vec![e("APPL", "MACS", "Finder")];
        assert_eq!(pick_appl(&entries, "/Games/x/Foo").as_deref(), Some("Finder"));
        // A game shipping both a B&W and a Colour build -> pick Colour.
        let entries = vec![
            e("APPL", "SANT", "SimAnt\u{2122} B&W"),
            e("APPL", "SANT", "SimAnt\u{2122} Color"),
        ];
        assert_eq!(pick_appl(&entries, "/Games/1991/SimAnt 1.0").as_deref(), Some("SimAnt\u{2122} Color"));
    }

    #[test]
    fn clutter_dir_filter() {
        let dir = |name: &str| Entry {
            is_dir: true, ostype: String::new(), creator: String::new(),
            name: name.into(), size: 0,
        };
        let file = |name: &str| Entry {
            is_dir: false, ostype: "WAD ".into(), creator: "x".into(),
            name: name.into(), size: 0,
        };
        assert!(is_clutter_dir(&dir("System Folder")));
        assert!(is_clutter_dir(&dir("Trash")));
        // a real game data sub-folder is kept, and files are never clutter-dirs
        assert!(!is_clutter_dir(&dir("wads")));
        assert!(!is_clutter_dir(&dir("Levels")));
        assert!(!is_clutter_dir(&file("System Folder")));
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
