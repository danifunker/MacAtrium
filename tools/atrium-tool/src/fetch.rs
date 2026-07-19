//! `atrium fetch` — Phase 2 of the Macintosh Garden integration:
//! download a 68K title's software from the
//! Macintosh Garden static mirror, extract it with **rb-cli** (StuffIt / Compact
//! Pro / MAR / BinHex / MacBinary), and optionally inject the forks into an image
//! under `Apps/` as a harvestable app.
//!
//! On-demand and per-title (the binaries are tens of MB–GB each), into a local
//! download cache that is NOT committed. Formats `rb-cli` can't open yet (`.zip`,
//! disk images, `.sitx`) are skipped with a message — `.sitx` is a PPC/OS9-era
//! format handled in the later OS 9.2.2 phase.

use crate::enrich::candidate_keys;
use crate::harvest::{self, Harvested};
use crate::rbcli::RbCli;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const MIRROR: &str = "https://gardenmirror.oldapplestuff.com";
// The mirror 403s requests without a User-Agent.
const USER_AGENT: &str = "MacAtrium-archive/0.1 (+atrium fetch)";

/// How a download maps to extraction.
enum Kind {
    /// StuffIt/CompactPro/MAR/SEA/BinHex-wrapped archive → `rb-cli archive extract`.
    Archive,
    /// A single BinHex file (both forks) → `put-binhex` directly.
    BinHex,
    /// A single MacBinary file (both forks) → `put-macbinary` directly.
    MacBinary,
}

fn classify(filename: &str) -> Option<Kind> {
    let lc = filename.to_ascii_lowercase();
    let ext = lc.rsplit('.').next().unwrap_or("");
    match ext {
        "sit" | "cpt" | "sea" | "mar" => Some(Kind::Archive),
        "hqx" => {
            // a StuffIt/CPT wrapped as .hqx is still an archive; a plain .hqx is a
            // single file. ".sit.hqx" etc. → archive; otherwise treat as single.
            if lc.contains(".sit") || lc.contains(".cpt") || lc.contains(".sea") {
                Some(Kind::Archive)
            } else {
                Some(Kind::BinHex)
            }
        }
        "bin" => Some(Kind::MacBinary),
        _ => None, // .zip / .iso / .dmg / .img / .sitx / splits — not in this pass
    }
}

/// Load a title record from its on-disk `info.json`, returning (kind_dir, record).
/// `kind_dir` is "games" or "apps" (the mirror path segment).
fn load_record(archive: &Path, nid: i64) -> Result<(&'static str, Value)> {
    for kind in ["games", "apps"] {
        let p = archive.join(kind).join(nid.to_string()).join("info.json");
        if p.is_file() {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&p)?)
                .with_context(|| format!("parsing {}", p.display()))?;
            return Ok((kind, v));
        }
    }
    bail!("no info.json for nid {nid} under {}", archive.display())
}

/// List a title's Macintosh Garden download filenames (`files[].filename` from its
/// `info.json`) — for a UI to offer as an explicit `mg.files` pick. Empty when the
/// title has no cached `info.json` under `archive`.
pub fn list_downloads(archive: &Path, nid: i64) -> Vec<String> {
    let Ok((_, rec)) = load_record(archive, nid) else {
        return Vec::new();
    };
    rec.get("files")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|f| f.get("filename").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Filenames that are *not* the title's main software — patches/updaters, demos,
/// docs/readmes, manuals, samples. The auto-picker deprioritises these so an
/// updater doesn't win over the full game (the SimCity 2000 v1.2 case, where the
/// `.sea.hqx` updater is an *archive* the old "first archive wins" rule grabbed
/// over the plain-`.hqx` full game). Filename heuristic only; the durable fix is
/// an explicit `mg.files` pick.
fn is_aux_name(name: &str) -> bool {
    let lc = name.to_ascii_lowercase();
    const AUX: &[&str] = &[
        "updat", "upgrade", "patch", "demo", "readme", "read me", "read_me",
        "manual", "instructions", "sample", "trial",
    ];
    AUX.iter().any(|k| lc.contains(k))
}

/// A comparable version key parsed from a filename — the *largest* version-looking
/// numeric token, so newer sorts higher. A token qualifies as a version if it is
/// dotted (`1.2`) or a small bare integer (`< 100`); 4-digit years and large title
/// numbers (`SimCity 2000`, `688 Attack Sub`) are ignored. Absent → `(0,0,0)`.
fn version_key(name: &str) -> (u32, u32, u32) {
    let bytes = name.as_bytes();
    let mut best = (0u32, 0u32, 0u32);
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut dotted = false;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            dotted |= bytes[i] == b'.';
            i += 1;
        }
        let mut parts = name[start..i]
            .split('.')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<u32>().unwrap_or(0));
        let v = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if (dotted || v.0 < 100) && v > best {
            best = v;
        }
    }
    best
}

/// Resolve which download(s) to fetch for a record, best/most-wanted first.
///
/// `allowed` is the explicit `mg.files` pick list (empty = auto). With an explicit
/// list we take each named file the record actually lists and rb-cli can extract,
/// **in the given order** (a title may ship several disks). With no list we
/// auto-pick the single best: real software over an updater/demo/readme, then
/// newest version, then a real archive over a bare single-file form.
fn pick_file(rec: &Value, allowed: &[String]) -> Vec<(String, Kind)> {
    let Some(files) = rec.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    let listed = |name: &str| {
        files
            .iter()
            .any(|f| f.get("filename").and_then(Value::as_str) == Some(name))
    };

    if !allowed.is_empty() {
        // Explicit pick list: honor order, keep only listed + extractable names.
        return allowed
            .iter()
            .filter(|n| listed(n))
            .filter_map(|n| classify(n).map(|k| (n.clone(), k)))
            .collect();
    }

    // Auto: rank every extractable candidate, take the single best.
    let mut best: Option<(String, Kind)> = None;
    let mut best_key = (0u8, (0u32, 0u32, 0u32), 0u8);
    for f in files {
        let Some(name) = f.get("filename").and_then(Value::as_str) else { continue };
        let Some(kind) = classify(name) else { continue };
        let key = (
            u8::from(!is_aux_name(name)),
            version_key(name),
            u8::from(matches!(kind, Kind::Archive)),
        );
        if best.is_none() || key > best_key {
            best_key = key;
            best = Some((name.to_string(), kind));
        }
    }
    best.into_iter().collect()
}

/// HFS-safe app folder name from a title: keep alphanumerics/space/dash, collapse
/// whitespace, drop glob metacharacters (rb-cli can't address them), cap at 31.
fn folder_name(title: &str) -> String {
    let mut s = String::new();
    let mut sp = false;
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' {
            if sp && !s.is_empty() {
                s.push(' ');
            }
            s.push(c);
            sp = false;
        } else {
            sp = true;
        }
    }
    s.chars().take(31).collect::<String>().trim().to_string()
}

/// Download `url` → `dest` over HTTP (native Rust `ureq`/rustls — no external curl
/// dependency). Streams to disk; skips if already cached.
fn download(url: &str, dest: &Path) -> Result<()> {
    if dest.is_file() && std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) > 0 {
        return Ok(()); // already cached
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| anyhow::anyhow!("GET {url}: {e}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    std::io::copy(&mut reader, &mut file).with_context(|| format!("downloading {url}"))?;
    drop(file);
    anyhow::ensure!(
        std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) > 0,
        "empty download for {url}"
    );
    Ok(())
}

/// Resolve one dataset record to its MG target `(nid, files)`.
///
/// An explicit `mg` overlay wins — `{"nid":N,"files":[...]}` is durable and exact,
/// so a title fetches the right edition even when its name doesn't match the MG
/// index (or matches the wrong node). Without it we fall back to name-matching via
/// the shared matcher. `files` is the explicit download pick list (empty →
/// auto-pick at fetch time). Pure over the pre-built `idx` so it unit-tests.
fn record_target(rec: &Value, idx: &HashMap<String, i64>) -> Option<(i64, Vec<String>)> {
    if let Some(mg) = rec.get("mg") {
        if let Some(nid) = mg.get("nid").and_then(Value::as_i64) {
            let files = mg
                .get("files")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            return Some((nid, files));
        }
    }
    let name = rec.get("name").and_then(Value::as_str)?;
    let nid = candidate_keys(name).into_iter().find_map(|k| idx.get(&k).copied())?;
    Some((nid, Vec::new()))
}

/// Resolve dataset records → MG `(nid, files)` targets by explicit `mg` overlay or
/// the shared name-matcher.
fn match_dataset(src: &Path, archive: &Path) -> Result<Vec<(i64, Vec<String>)>> {
    // Build candidate-key → nid index from the MG metadata (68K records carry the
    // images; we just need title→nid here, so read both ndjson files lightly).
    let mut idx: HashMap<String, i64> = HashMap::new();
    for (kind, key) in [("games", "game"), ("apps", "app")] {
        let p = archive.join("metadata").join(format!("{kind}.ndjson"));
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
            let Some(rec) = v.get("data").and_then(|d| d.get(key)) else { continue };
            let Some(title) = rec.get("title").and_then(Value::as_str) else { continue };
            let Some(nid) = v.get("nid").and_then(Value::as_i64) else { continue };
            for k in candidate_keys(title) {
                idx.entry(k).or_insert(nid);
            }
        }
    }
    let mut out: Vec<(i64, Vec<String>)> = Vec::new();
    let text = std::fs::read_to_string(src)?;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let v: Value = match serde_json::from_str(t) { Ok(v) => v, Err(_) => continue };
        let Some((nid, files)) = record_target(&v, &idx) else { continue };
        match out.iter_mut().find(|(n, _)| *n == nid) {
            // Same nid seen again: keep the first, but adopt files if we had none.
            Some(slot) if slot.1.is_empty() && !files.is_empty() => slot.1 = files,
            Some(_) => {}
            None => out.push((nid, files)),
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    archive: &Path,
    nids: &[i64],
    file: Option<&str>,
    src: Option<&Path>,
    downloads: Option<&Path>,
    into: Option<&Path>,
    apps_root: &str,
    append_to: Option<&Path>,
    rb_cli: &str,
    _curl: &str, // vestigial: downloads now use ureq (removed in the UI pass)
    stage: Option<&Path>,
) -> Result<()> {
    let dl_root = downloads
        .map(Path::to_path_buf)
        .unwrap_or_else(|| archive.join("downloads"));
    let stage = stage
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("atrium-fetch-stage"));
    std::fs::create_dir_all(&stage)?;
    let rb = RbCli::new(rb_cli);

    // Resolve the targets: (nid, explicit mg.files pick list). Bare `--nid` args
    // carry no explicit pick (they auto-pick, unless the global `--file` steers).
    let mut targets: Vec<(i64, Vec<String>)> = nids.iter().map(|&n| (n, Vec::new())).collect();
    if let Some(s) = src {
        for (n, files) in match_dataset(s, archive)? {
            match targets.iter_mut().find(|(x, _)| *x == n) {
                Some(slot) if slot.1.is_empty() && !files.is_empty() => slot.1 = files,
                Some(_) => {}
                None => targets.push((n, files)),
            }
        }
    }
    if targets.is_empty() {
        bail!("no targets — pass --nid <N> and/or --src <dataset>");
    }
    eprintln!("fetch: {} target title(s)", targets.len());

    let (mut ok, mut skipped, mut injected) = (0usize, 0usize, 0usize);
    let mut stubs: Vec<Harvested> = Vec::new();
    for (nid, want_files) in targets {
        let (kind_dir, rec) = match load_record(archive, nid) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  skip nid {nid}: {e}");
                skipped += 1;
                continue;
            }
        };
        let title = rec.get("title").and_then(Value::as_str).unwrap_or("?").to_string();
        // Resolve which download(s) to fetch. Precedence: the global `--file`
        // override (exact, one file, applies to every target) > the title's
        // explicit `mg.files` pick list > the smart auto-pick. `mg.files` may name
        // several disks; `--file`/auto yield exactly one. (The auto-pick now
        // deprioritises updaters/demos, so e.g. the SimCity 2000 v1.2 `.sea.hqx`
        // updater no longer beats the plain-`.hqx` full game.)
        let picks: Vec<(String, Kind)> = match file {
            Some(want) => {
                let listed = rec
                    .get("files")
                    .and_then(Value::as_array)
                    .is_some_and(|fs| {
                        fs.iter().any(|f| f.get("filename").and_then(Value::as_str) == Some(want))
                    });
                if !listed {
                    eprintln!("  skip [{nid}] {title}: --file {want} not listed for this title");
                    skipped += 1;
                    continue;
                }
                classify(want).map(|k| (want.to_string(), k)).into_iter().collect()
            }
            None => pick_file(&rec, &want_files),
        };
        if picks.is_empty() {
            eprintln!(
                "  skip [{nid}] {title}: {}",
                if file.is_some() {
                    "requested --file is a format rb-cli can't extract (zip/iso/sitx)"
                } else if !want_files.is_empty() {
                    "none of the mg.files picks are listed + extractable"
                } else {
                    "no rb-cli-extractable download (zip/iso/sitx only)"
                }
            );
            skipped += 1;
            continue;
        }

        // Fetch + extract each pick, accumulating forks. Each archive expands into
        // its own staging subdir so a multi-disk title's archives don't re-collect
        // each other's forks; a bare .bin/.hqx is a single file under <title>/.
        // forks: (host file, relative dir under apps_root, is_macbinary).
        let tdir = stage.join(nid.to_string());
        let _ = std::fs::remove_dir_all(&tdir);
        std::fs::create_dir_all(&tdir)?;
        let mut forks: Vec<(PathBuf, String, bool)> = Vec::new();
        let mut primary_file: Option<String> = None;
        for (idx, (filename, kclass)) in picks.iter().enumerate() {
            // Download from the static mirror into the (uncommitted) cache.
            let url = format!("{MIRROR}/{kind_dir}/{}", urlencode(filename));
            let dest = dl_root.join(kind_dir).join(nid.to_string()).join(filename);
            if let Err(e) = download(&url, &dest) {
                eprintln!("  [{nid}] {title}: download failed for {filename}: {e}");
                continue;
            }
            if primary_file.is_none() {
                primary_file = Some(filename.clone());
            }
            match kclass {
                Kind::Archive => {
                    let sub = tdir.join(idx.to_string());
                    if let Err(e) = std::fs::create_dir_all(&sub) {
                        eprintln!("  [{nid}] {title}: staging dir failed for {filename}: {e}");
                        continue;
                    }
                    if let Err(e) = rb.archive_extract(&dest, &sub) {
                        eprintln!("  [{nid}] {title}: extract failed for {filename}: {e}");
                        continue;
                    }
                    forks.extend(collect_forks(&sub));
                }
                Kind::BinHex => forks.push((dest.clone(), folder_name(&title), false)),
                Kind::MacBinary => forks.push((dest.clone(), folder_name(&title), true)),
            }
        }
        let Some(primary_file) = primary_file else {
            eprintln!("  skip [{nid}] {title}: all download(s) failed");
            skipped += 1;
            continue;
        };
        ok += 1;
        eprintln!("  [{nid}] {title}: {} pick(s) -> {} fork(s)", picks.len(), forks.len());

        // Optionally inject into the image under Apps/, preserving structure.
        if let Some(img) = into {
            let mut n = 0;
            let mut roots: HashSet<String> = HashSet::new();
            for (f, rel, is_macbin) in &forks {
                let dst_dir = if rel.is_empty() {
                    apps_root.trim_end_matches('/').to_string()
                } else {
                    roots.insert(rel.split('/').next().unwrap_or(rel).to_string());
                    format!("{}/{}", apps_root.trim_end_matches('/'), rel)
                };
                rb.mkdir_p(img, &dst_dir)?;
                let r = if *is_macbin {
                    rb.put_macbinary(img, f, &dst_dir)
                } else {
                    rb.put_binhex(img, f, &dst_dir, None)
                };
                match r {
                    Ok(()) => n += 1,
                    Err(e) => eprintln!("    inject {} failed: {e}", f.display()),
                }
            }
            if n > 0 {
                injected += 1;
                eprintln!("    injected {n} fork(s) -> {apps_root}");
            }
            // Emit a minimal dataset stub: find the injected APPL (the launch
            // target) and record id/name/kind/year/app — `atrium mg`/`enrich`
            // fill the rest later (same pattern as harvest).
            if append_to.is_some() && n > 0 {
                let search_roots: Vec<String> = if roots.is_empty() {
                    vec![apps_root.trim_end_matches('/').to_string()]
                } else {
                    roots.iter().map(|r| format!("{}/{}", apps_root.trim_end_matches('/'), r)).collect()
                };
                let mut appls: Vec<(String, String)> = Vec::new();
                for r in &search_roots {
                    collect_appls(&rb, img, r, &mut appls, 0);
                }
                match pick_appl_path(&appls, &title) {
                    Some(full) => {
                        let leaf = full.rsplit('/').next().unwrap_or(full.as_str());
                        let was_installer = harvest::is_installer_name(leaf);
                        let app_rel = full
                            .strip_prefix("/MacAtrium/")
                            .unwrap_or(full.trim_start_matches('/'))
                            .to_string();
                        let kind = if kind_dir == "apps" { "app" } else { "game" };
                        let genre = rec
                            .get("category")
                            .or_else(|| rec.get("category_app"))
                            .and_then(Value::as_array)
                            .and_then(|a| a.first())
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        stubs.push(Harvested {
                            id: harvest::slugify(&title),
                            name: title.clone(),
                            kind: kind.into(),
                            year: rec.get("year").and_then(Value::as_str).and_then(|y| y.parse().ok()),
                            genre,
                            app_path: app_rel,
                            files: Vec::new(),
                            // Provenance (Q1): record the MacGarden download + nid,
                            // and flag when the picked launch app is an installer.
                            // With multiple picks (multi-disk) this is the first.
                            was_installer,
                            download: Some(primary_file.clone()),
                            mg_nid: Some(nid),
                        });
                    }
                    None => eprintln!("    note: no APPL found under {apps_root} — no stub emitted"),
                }
            }
        }
    }

    if let Some(dataset) = append_to {
        if !stubs.is_empty() {
            let (appended, skipped_dups) = harvest::append_to_dataset(dataset, &stubs)?;
            eprintln!(
                "fetch: appended {appended} stub(s) to {} ({skipped_dups} already present)",
                dataset.display()
            );
        }
    }

    eprintln!("fetch: {ok} downloaded, {injected} injected, {skipped} skipped -> cache {}", dl_root.display());
    Ok(())
}

/// Recursively collect extracted `.hqx` forks under `root`, each paired with its
/// relative directory (every path component sanitised HFS-safe) under the apps
/// root. The `.hqx` suffix is dropped when computing on-volume names by put-binhex.
fn collect_forks(root: &Path) -> Vec<(PathBuf, String, bool)> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(PathBuf, String, bool)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let raw_name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                let comp = folder_name(&raw_name);
                let child_rel = if rel.is_empty() { comp } else { format!("{rel}/{comp}") };
                walk(&p, &child_rel, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("hqx") {
                out.push((p, rel.to_string(), false));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

/// Recursively collect `(full_path, leaf_name)` for every `APPL` under `dir` on
/// the image (depth-guarded).
fn collect_appls(rb: &RbCli, image: &Path, dir: &str, out: &mut Vec<(String, String)>, depth: usize) {
    if depth > 12 {
        return;
    }
    // Exact listing so a bracket-named folder lists instead of globbing. The
    // `/`-skip stays: this walker joins raw names into `p` without escaping, so
    // a slash-named source isn't addressable here (the `harvest` path handles
    // those via `esc`).
    let Ok(entries) = rb.ls_exact(image, dir) else { return };
    for e in entries {
        if e.name.contains('/') {
            continue;
        }
        let p = format!("{}/{}", dir.trim_end_matches('/'), e.name);
        if e.is_dir {
            collect_appls(rb, image, &p, out, depth + 1);
        } else if e.ostype == "APPL" {
            out.push((p, e.name));
        }
    }
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// Pick the launch-target APPL: skip a bundled Finder, then prefer the one whose
/// leaf name best matches the title (longest shared slug prefix), then the
/// shorter name (editors/extras tend to be "<game> <something>").
fn pick_appl_path(appls: &[(String, String)], title: &str) -> Option<String> {
    if appls.is_empty() {
        return None;
    }
    let tslug = harvest::slugify(title);
    let real: Vec<&(String, String)> = appls.iter().filter(|(_, n)| n != "Finder").collect();
    let pool: Vec<&(String, String)> = if real.is_empty() { appls.iter().collect() } else { real };
    pool.into_iter()
        .max_by_key(|(_, n)| {
            (common_prefix_len(&harvest::slugify(n), &tslug), std::cmp::Reverse(n.len()))
        })
        .map(|(p, _)| p.clone())
}

/// Minimal percent-encoding for a path segment (spaces, etc.); keeps the
/// unreserved set + a few filename-safe chars.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_formats() {
        assert!(matches!(classify("Foo.sit"), Some(Kind::Archive)));
        assert!(matches!(classify("Foo.sit.hqx"), Some(Kind::Archive)));
        assert!(matches!(classify("Foo.hqx"), Some(Kind::BinHex)));
        assert!(matches!(classify("Foo.bin"), Some(Kind::MacBinary)));
        assert!(classify("Foo.zip").is_none());
        assert!(classify("Foo.sitx").is_none());
        assert!(classify("Foo.iso").is_none());
    }

    #[test]
    fn folder_name_is_hfs_safe() {
        assert_eq!(folder_name("Prince of Persia"), "Prince of Persia");
        assert_eq!(folder_name("Glider 4.0!!"), "Glider 4 0");
        assert!(folder_name(&"x".repeat(50)).len() <= 31);
    }

    #[test]
    fn urlencodes_spaces() {
        assert_eq!(urlencode("a b.sit"), "a%20b.sit");
        assert_eq!(urlencode("HyperCard-Player-241.sit"), "HyperCard-Player-241.sit");
    }

    /// A record whose `files` are the given filenames.
    fn rec_with(files: &[&str]) -> Value {
        let arr: Vec<Value> = files.iter().map(|f| serde_json::json!({ "filename": f })).collect();
        serde_json::json!({ "files": arr })
    }

    fn pick_names(rec: &Value, allowed: &[&str]) -> Vec<String> {
        let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
        pick_file(rec, &allowed).into_iter().map(|(n, _)| n).collect()
    }

    #[test]
    fn version_key_ignores_title_numbers() {
        assert_eq!(version_key("SimCity 2000 1.2.hqx"), (1, 2, 0)); // 2000 is title, 1.2 wins
        assert_eq!(version_key("688 Attack Sub.sit"), (0, 0, 0)); // big bare int, not a version
        assert_eq!(version_key("Glider 4.0.sit"), (4, 0, 0));
        assert_eq!(version_key("3D Checkers 5.1.sit"), (5, 1, 0));
        assert_eq!(version_key("Prince of Persia.sit"), (0, 0, 0));
    }

    #[test]
    fn auto_pick_prefers_full_over_updater() {
        // The SimCity case: a `.sea.hqx` updater is an *archive* the old "first
        // archive wins" rule grabbed over the plain-`.hqx` full game.
        let rec = rec_with(&["SimCity 2000 1.2 Updater.sea.hqx", "SimCity 2000 1.2.hqx"]);
        assert_eq!(pick_names(&rec, &[]), vec!["SimCity 2000 1.2.hqx"]);
    }

    #[test]
    fn auto_pick_prefers_newest_version() {
        let rec = rec_with(&["Glider 3.1.2.sit", "Glider 4.0.sit"]);
        assert_eq!(pick_names(&rec, &[]), vec!["Glider 4.0.sit"]);
    }

    #[test]
    fn auto_pick_prefers_archive_as_tiebreak() {
        // Same (non-aux, version) → a real archive beats a bare single-file form.
        let rec = rec_with(&["Game.hqx", "Game.sit"]);
        assert_eq!(pick_names(&rec, &[]), vec!["Game.sit"]);
    }

    #[test]
    fn auto_pick_skips_unsupported_formats() {
        let rec = rec_with(&["Game.zip", "Game.iso", "Game.sit"]);
        assert_eq!(pick_names(&rec, &[]), vec!["Game.sit"]);
        assert!(pick_names(&rec_with(&["Game.zip", "Game.sitx"]), &[]).is_empty());
    }

    #[test]
    fn explicit_allowlist_keeps_order_and_filters() {
        let rec = rec_with(&["Disk 1.img", "Game A.sit", "Game B.sit"]);
        // Honor the given order; drop the unlisted ("Nope") and unsupported (.img).
        assert_eq!(
            pick_names(&rec, &["Game B.sit", "Nope.sit", "Disk 1.img", "Game A.sit"]),
            vec!["Game B.sit", "Game A.sit"],
        );
    }

    #[test]
    fn record_target_prefers_explicit_mg() {
        let idx: HashMap<String, i64> = HashMap::new(); // empty → name-match would fail
        let rec = serde_json::json!({
            "id": "sc2k", "name": "Unmatchable Name",
            "mg": { "nid": 15475, "files": ["SimCity 2000 1.2.hqx"] }
        });
        assert_eq!(
            record_target(&rec, &idx),
            Some((15475, vec!["SimCity 2000 1.2.hqx".to_string()]))
        );
    }

    #[test]
    fn record_target_mg_without_files_is_empty_list() {
        let idx: HashMap<String, i64> = HashMap::new();
        let rec = serde_json::json!({ "id": "x", "name": "X", "mg": { "nid": 7 } });
        assert_eq!(record_target(&rec, &idx), Some((7, Vec::new())));
    }

    #[test]
    fn record_target_falls_back_to_name_match() {
        let mut idx = HashMap::new();
        for k in candidate_keys("Dark Castle") {
            idx.insert(k, 42);
        }
        let rec = serde_json::json!({ "id": "dc", "name": "Dark Castle" });
        assert_eq!(record_target(&rec, &idx), Some((42, Vec::new())));
        // No mg + no name match → None.
        let miss = serde_json::json!({ "id": "z", "name": "Totally Unknown Xyzzy" });
        assert_eq!(record_target(&miss, &idx), None);
    }

    #[test]
    fn list_downloads_reads_info_json_filenames() {
        let dir = std::env::temp_dir().join(format!("atrium-fetch-ld-{}", std::process::id()));
        let g = dir.join("games").join("999");
        std::fs::create_dir_all(&g).unwrap();
        std::fs::write(
            g.join("info.json"),
            r#"{"title":"X","files":[{"filename":"A.sit"},{"filename":"B.hqx"},{"no":"filename"}]}"#,
        )
        .unwrap();
        assert_eq!(list_downloads(&dir, 999), vec!["A.sit".to_string(), "B.hqx".to_string()]);
        assert!(list_downloads(&dir, 12345).is_empty()); // no info.json → empty
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_mg_overlay_round_trips_to_record_target() {
        // The GUI pin (pin_mg_download) writes mg.{nid,files} to the curated overlay
        // via merge::set; fetch's record_target must read that exact pin back. This
        // exercises the real write path (same fn the GUI calls) end to end.
        let dir = std::env::temp_dir().join(format!("atrium-fetch-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let overlay = dir.join("curated.jsonl");
        std::fs::write(&overlay, "").unwrap();

        let mut mg = serde_json::Map::new();
        mg.insert("nid".into(), Value::from(15475));
        mg.insert("files".into(), Value::from(vec!["SimCity 2000 1.2.hqx".to_string()]));
        let mut fields = serde_json::Map::new();
        fields.insert("mg".into(), Value::Object(mg));
        crate::merge::set(&overlay, "simcity-2000", &fields).unwrap();

        let text = std::fs::read_to_string(&overlay).unwrap();
        let line = text.lines().find(|l| !l.trim().is_empty()).unwrap();
        let rec: Value = serde_json::from_str(line).unwrap();
        assert_eq!(rec.get("id").and_then(Value::as_str), Some("simcity-2000"));
        assert_eq!(
            record_target(&rec, &HashMap::new()),
            Some((15475, vec!["SimCity 2000 1.2.hqx".to_string()]))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
