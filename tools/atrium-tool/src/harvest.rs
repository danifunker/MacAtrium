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
    /// Provenance (Q1): the picked launch app looks like an installer/patcher
    /// rather than the game itself — so a redistributor knows this record's `app`
    /// is (or came from) an installer.
    pub was_installer: bool,
    /// The original download's filename (Macintosh Garden), when known.
    pub download: Option<String>,
    /// The Macintosh Garden node id the software came from, when known.
    pub mg_nid: Option<i64>,
}

/// Whether a file/app name looks like an installer/patcher rather than the
/// runnable game (a provenance signal, Q1): matches "install(er)", "setup",
/// "updater", or "patch", case-insensitively.
pub fn is_installer_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["install", "setup", "updater", "patch"].iter().any(|k| n.contains(k))
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

/// Escape one path component so rb-cli addresses it as an exact literal in the
/// slash grammar: a real `\` becomes `\\` and a real `/` becomes `\/`. Mirrors
/// rb-cli's `parse::escape_path_component`. Classic-Mac HFS volumes allow `/`
/// in a name (it's the Finder's display of the `:` HFS separator), so a donor
/// file like `Oxyd b/w` must be escaped before it can be addressed by name.
fn esc(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c == '\\' || c == '/' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Sanitize a donor name into an on-disk destination component: map `/` — the
/// only byte that can't appear in a `/`-joined target path — to `-`. Never `:`
/// (the HFS path separator). The on-disk name deliberately differs from the
/// donor original; the catalog records this sanitized name so the launcher can
/// find and launch the file.
fn sanitize(name: &str) -> String {
    name.replace('/', "-")
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
        .ls_exact(image, src_folder)
        .with_context(|| format!("listing {src_folder}"))?;

    for e in &entries {
        // Source addressing uses the slash grammar with `\/` escaping, so a name
        // containing a literal `/` (e.g. `Oxyd b/w`) is addressed verbatim
        // instead of being skipped. The destination component is sanitized
        // (`/` -> `-`) since `/` can't live inside a `/`-joined target path.
        let child_src = format!("{}/{}", src_folder.trim_end_matches('/'), esc(&e.name));
        let dst_name = sanitize(&e.name);
        let child_rel = if rel.is_empty() {
            dst_name.clone()
        } else {
            format!("{rel}/{dst_name}")
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
        // put-binhex takes the filename from the BinHex header (the donor's real
        // name, possibly with a `/`), so force the sanitized on-disk name via
        // --rename when they differ.
        let dst_dir = match child_rel.rfind('/') {
            Some(i) => format!("{app_dir}/{}", &child_rel[..i]),
            None => app_dir.to_string(),
        };
        let rename = (dst_name != e.name).then_some(dst_name.as_str());
        let hqx = stage_dir.join(format!("{}.hqx", slugify(&child_rel)));
        rb.get_binhex(image, &child_src, &hqx)
            .with_context(|| format!("extracting {child_src}"))?;
        if let Some(target) = into {
            rb.put_binhex(target, &hqx, &dst_dir, rename)
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
    curated_id: Option<&str>,
) -> Result<Option<Harvested>> {
    let entries = rb
        .ls_exact(image, src_folder)
        .with_context(|| format!("listing {src_folder}"))?;

    let app = match pick_appl(&entries, src_folder) {
        Some(name) => name,
        None => return Ok(None),
    };
    // The on-disk folder + app-file names are sanitized (a donor APPL like
    // `Oxyd b/w` can't be a `/`-joined path component); the display `name`
    // keeps the donor original.
    let app_dst = sanitize(&app);
    let app_dir = format!("{apps_root}/{app_dst}");

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
        // Prefer the SELECTED curated id (so the install reconnects to its curated
        // record's metadata + categories) over the app-name slug. They differ when
        // the launchable app is named differently from its folder/curated title —
        // e.g. the folder `Oxyd 3.6` whose APPL is `Oxyd b/w` (id oxyd-3-6 vs the
        // slug oxyd-b-w). append_to_dataset then just corrects that record's `app`.
        id: curated_id.map(str::to_string).unwrap_or(app_slug),
        name: app.clone(),
        kind,
        year: infer_year(src_folder),
        genre,
        app_path: format!("Apps/{app_dst}/{app_dst}"),
        files,
        was_installer: is_installer_name(&app),
        download: None,
        mg_nid: None,
    }))
}

/// Append harvested stubs to a curated dataset, de-duplicated by `id` so existing
/// (hand-enriched) entries are never clobbered and re-runs are idempotent.
/// Returns (appended, skipped). The dataset file is created if absent.
/// Shared with `atrium fetch` (MG-injected apps emit the same minimal stubs).
pub fn append_to_dataset(dataset: &Path, harvested: &[Harvested]) -> Result<(usize, usize)> {
    let existing = std::fs::read_to_string(dataset).unwrap_or_default();
    let (out, appended, skipped) = merge_stubs(&existing, harvested);
    if out != existing {
        std::fs::write(dataset, out)
            .with_context(|| format!("appending to {}", dataset.display()))?;
    }
    Ok((appended, skipped))
}

/// Pure merge: append stub lines for harvested apps whose `id` isn't already in
/// `existing`; for a harvested app whose `id` IS already present (a curated record),
/// correct that record's `app` to the actual on-disk install path — leaving its
/// metadata + categories untouched. This reconnects a sanitized install (e.g. the
/// `/`-renamed `Oxyd b-w`) to its curated record. Comments/blank lines are kept
/// verbatim. Returns the new file text and (appended, matched) counts.
fn merge_stubs(existing: &str, harvested: &[Harvested]) -> (String, usize, usize) {
    use std::collections::{HashMap, HashSet};
    let by_id: HashMap<&str, &Harvested> = harvested.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut have: HashSet<String> = HashSet::new();
    let mut lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            lines.push(line.to_string());
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(t) {
            Ok(v) => v,
            Err(_) => { lines.push(line.to_string()); continue; }
        };
        if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
            have.insert(id.to_string());
            if let Some(h) = by_id.get(id) {
                let cur = v.get("app").and_then(|a| a.as_str()).unwrap_or("");
                if cur != h.app_path {            /* fix the install path in place */
                    let mut v2 = v.clone();
                    v2["app"] = serde_json::Value::String(h.app_path.clone());
                    lines.push(serde_json::to_string(&v2).unwrap_or_else(|_| line.to_string()));
                    continue;
                }
            }
        }
        lines.push(line.to_string());
    }
    let mut appended = 0usize;
    for h in harvested {
        if !have.contains(&h.id) {
            lines.push(stub_line(h));
            appended += 1;
        }
    }
    let skipped = harvested.len() - appended;
    (lines.join("\n") + "\n", appended, skipped)
}

/// JSON-encode a string. Rust's `{:?}` is almost-JSON but escapes control
/// chars as `\u{7f}` (braces), which is invalid JSON — serde does it right.
fn jstr(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Serialize a harvested app as a `data/library.jsonl` stub line.
fn stub_line(h: &Harvested) -> String {
    let mut s = format!(
        "{{\"id\":{},\"name\":{},\"kind\":{}",
        jstr(&h.id),
        jstr(&h.name),
        jstr(&h.kind)
    );
    if let Some(y) = h.year {
        s.push_str(&format!(",\"year\":{y}"));
    }
    if let Some(g) = &h.genre {
        s.push_str(&format!(",\"genre\":[{}]", jstr(g)));
    }
    s.push_str(&format!(",\"app\":{}", jstr(&h.app_path)));
    if h.was_installer {
        s.push_str(",\"was_installer\":true");
    }
    if let Some(d) = &h.download {
        s.push_str(&format!(",\"download\":{}", jstr(d)));
    }
    if let Some(nid) = h.mg_nid {
        s.push_str(&format!(",\"mg_nid\":{nid}"));
    }
    s.push('}');
    s
}

#[allow(clippy::too_many_arguments)]
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
    curated: Option<&std::collections::HashMap<String, String>>,
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
        // A folder that can't be listed (a mistyped/odd name in a curated list) or
        // otherwise fails is skipped with a warning, not fatal — one bad path
        // shouldn't abort a 30-title build.
        let cid = curated.and_then(|m| m.get(folder)).map(String::as_str);
        let res = match harvest_one(&rb, image, folder, stage, apps_root, into, &mut warnings, cid) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("{folder}: skipped ({e})"));
                continue;
            }
        };
        match res {
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
    fn esc_escapes_slash_and_backslash_for_rb_cli() {
        // A donor file like "Oxyd b/w" must address verbatim via the \/ escape.
        assert_eq!(esc("Oxyd b/w"), r"Oxyd b\/w");
        assert_eq!(esc(r"a\b"), r"a\\b");
        assert_eq!(esc("plain"), "plain");
        // Composed into a source path, only the in-name slash is escaped.
        let child_src = format!("{}/{}", "/Games/Oxyd 3.6", esc("Oxyd b/w"));
        assert_eq!(child_src, r"/Games/Oxyd 3.6/Oxyd b\/w");
    }

    #[test]
    fn sanitize_maps_slash_not_colon() {
        assert_eq!(sanitize("Oxyd b/w"), "Oxyd b-w");
        assert_eq!(sanitize("TCP/IP Tool"), "TCP-IP Tool");
        // No `/` -> unchanged (a colon never appears in an HFS name).
        assert_eq!(sanitize("Dark Castle"), "Dark Castle");
    }

    #[test]
    fn slash_name_round_trips_through_app_path() {
        // Mirrors harvest_one's catalog derivation for an APPL named "Oxyd b/w":
        // the on-disk path is sanitized; the catalog app field carries no '/'.
        let app = "Oxyd b/w";
        let app_dst = sanitize(app);
        let app_path = format!("Apps/{app_dst}/{app_dst}");
        assert_eq!(app_path, "Apps/Oxyd b-w/Oxyd b-w");
        // No path component carries the donor's literal slash.
        assert!(app_path.split('/').all(|c| !c.contains("b/w")));
        // The display name keeps the donor original.
        assert_eq!(app, "Oxyd b/w");
    }

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
            Harvested { id: "dark-castle".into(), name: "Dark Castle".into(), kind: "game".into(), year: None, genre: None, app_path: "Apps/Dark Castle/Dark Castle".into(), files: vec![], was_installer: false, download: None, mg_nid: None },
            Harvested { id: "lemmings".into(), name: "Lemmings".into(), kind: "game".into(), year: Some(1991), genre: None, app_path: "Apps/Lemmings/Lemmings".into(), files: vec![], was_installer: false, download: None, mg_nid: None },
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
            was_installer: false,
            download: None,
            mg_nid: None,
        };
        assert_eq!(
            stub_line(&h),
            r#"{"id":"dark-castle","name":"Dark Castle","kind":"game","year":1986,"app":"Apps/Dark Castle/Dark Castle"}"#
        );
    }

    #[test]
    fn installer_name_detection() {
        assert!(is_installer_name("Apeiron Installer"));
        assert!(is_installer_name("Install Foo"));
        assert!(is_installer_name("BDC Data A patch"));
        assert!(!is_installer_name("Apeiron"));
        assert!(!is_installer_name("Dark Castle"));
    }

    #[test]
    fn stub_emits_provenance_only_when_set() {
        let h = Harvested {
            id: "apeiron".into(), name: "Apeiron".into(), kind: "game".into(),
            year: None, genre: None, app_path: "Apps/Apeiron/Apeiron".into(),
            files: vec![], was_installer: true,
            download: Some("Apeiron 1.0.2.sit".into()), mg_nid: Some(123),
        };
        let line = stub_line(&h);
        assert!(line.contains(r#""was_installer":true"#));
        assert!(line.contains(r#""download":"Apeiron 1.0.2.sit""#));
        assert!(line.contains(r#""mg_nid":123"#));
    }
}
