//! `atrium fetch` — Phase 2 of the Macintosh Garden integration
//! (docs/MacintoshGardenArchive.md): download a 68K title's software from the
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

/// Pick the best downloadable file for a record: first one whose format rb-cli
/// can extract. Returns (filename, Kind).
fn pick_file(rec: &Value) -> Option<(String, Kind)> {
    let files = rec.get("files").and_then(Value::as_array)?;
    // Prefer real archives over single-file forms, but any supported one will do.
    let mut fallback: Option<(String, Kind)> = None;
    for f in files {
        let Some(name) = f.get("filename").and_then(Value::as_str) else { continue };
        match classify(name) {
            Some(k @ Kind::Archive) => return Some((name.to_string(), k)),
            Some(k) if fallback.is_none() => fallback = Some((name.to_string(), k)),
            _ => {}
        }
    }
    fallback
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

/// Download `url` → `dest` via curl (resumable, generous timeout for big files).
fn download(url: &str, dest: &Path, curl: &str) -> Result<()> {
    if dest.is_file() && std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) > 0 {
        return Ok(()); // already cached
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dst = dest.to_string_lossy();
    let status = std::process::Command::new(curl)
        .args(["-sL", "--fail", "-A", USER_AGENT, "--max-time", "600", "-o", &dst, url])
        .status()
        .with_context(|| format!("running {curl}"))?;
    anyhow::ensure!(status.success(), "curl failed for {url}");
    anyhow::ensure!(
        std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) > 0,
        "empty download for {url}"
    );
    Ok(())
}

/// Resolve dataset record names → MG nids by the shared matcher.
fn match_dataset(src: &Path, archive: &Path) -> Result<Vec<i64>> {
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
    let mut out = Vec::new();
    let text = std::fs::read_to_string(src)?;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let v: Value = match serde_json::from_str(t) { Ok(v) => v, Err(_) => continue };
        let Some(name) = v.get("name").and_then(Value::as_str) else { continue };
        if let Some(nid) = candidate_keys(name).into_iter().find_map(|k| idx.get(&k).copied()) {
            if !out.contains(&nid) {
                out.push(nid);
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    archive: &Path,
    nids: &[i64],
    src: Option<&Path>,
    downloads: Option<&Path>,
    into: Option<&Path>,
    apps_root: &str,
    append_to: Option<&Path>,
    rb_cli: &str,
    curl: &str,
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

    // Resolve the target nids.
    let mut targets: Vec<i64> = nids.to_vec();
    if let Some(s) = src {
        for n in match_dataset(s, archive)? {
            if !targets.contains(&n) {
                targets.push(n);
            }
        }
    }
    if targets.is_empty() {
        bail!("no targets — pass --nid <N> and/or --src <dataset>");
    }
    eprintln!("fetch: {} target title(s)", targets.len());

    let (mut ok, mut skipped, mut injected) = (0usize, 0usize, 0usize);
    let mut stubs: Vec<Harvested> = Vec::new();
    for nid in targets {
        let (kind_dir, rec) = match load_record(archive, nid) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  skip nid {nid}: {e}");
                skipped += 1;
                continue;
            }
        };
        let title = rec.get("title").and_then(Value::as_str).unwrap_or("?").to_string();
        let Some((filename, kclass)) = pick_file(&rec) else {
            eprintln!("  skip [{nid}] {title}: no rb-cli-extractable download (zip/iso/sitx only)");
            skipped += 1;
            continue;
        };

        // Download from the static mirror into the (uncommitted) cache.
        let url = format!("{MIRROR}/{kind_dir}/{}", urlencode(&filename));
        let dest = dl_root.join(kind_dir).join(nid.to_string()).join(&filename);
        if let Err(e) = download(&url, &dest, curl) {
            eprintln!("  skip [{nid}] {title}: download failed: {e}");
            skipped += 1;
            continue;
        }
        ok += 1;

        // Extract into a per-title staging dir → a set of .hqx (or the file itself).
        let tdir = stage.join(nid.to_string());
        let _ = std::fs::remove_dir_all(&tdir);
        std::fs::create_dir_all(&tdir)?;
        // forks: (host file, relative dir under apps_root, is_macbinary). An
        // archive expands to a whole tree (the app folder + docs + maybe an inner
        // disk image); we inject it structure-preserving, sanitising each path
        // component for HFS. A bare .bin/.hqx is a single file under <title>/.
        let forks: Vec<(PathBuf, String, bool)> = match kclass {
            Kind::Archive => {
                if let Err(e) = rb.archive_extract(&dest, &tdir) {
                    eprintln!("  [{nid}] {title}: extract failed: {e}");
                    continue;
                }
                collect_forks(&tdir)
            }
            Kind::BinHex => vec![(dest.clone(), folder_name(&title), false)],
            Kind::MacBinary => vec![(dest.clone(), folder_name(&title), true)],
        };
        eprintln!("  [{nid}] {title}: {} -> {} file(s)", filename, forks.len());

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
                    rb.put_binhex(img, f, &dst_dir)
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
    let Ok(entries) = rb.ls(image, dir) else { return };
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
}
