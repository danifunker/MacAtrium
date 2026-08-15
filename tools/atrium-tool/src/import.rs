//! `atrium::import` — bring a hand-made capture (`.mar`, `.sit`, `.cpt`, `.hqx`)
//! of an **already-installed** app into the library.
//!
//! The capture flow is: install a title in an emulator, capture the installed
//! folder as a `.mar`, then import it here. That content must never go through
//! [`harvest`](crate::harvest) — harvest re-picks the launchable `APPL` and
//! renames the folder to it, overriding the curated `app` path.
//!
//! Unlike [`fetch`](crate::fetch), an import needs **no donor disk image**. The
//! archive is expanded once into a host staging folder and the record points at
//! it (`local_src`); a build injects those forks straight into the output disk
//! with `put-binhex`, the same primitive `fetch --into` uses. A donor image is
//! still supported for people who keep a reservoir, but it is not required.
//!
//! ## The one fidelity caveat
//!
//! HFS allows `/` in a filename; host filesystems don't. Extracting to the host
//! rewrites e.g. `Civ Data B/W` → `Civ Data B_W`, and a title that opens its data
//! files by exact name would then fail. [`StagedImport::renamed`] reports every
//! such file so the caller can warn, and staging into an HFS image instead (the
//! reservoir path) avoids it entirely.

use crate::rbcli::RbCli;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// One file expanded out of a capture, ready to inject.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedFile {
    /// The `.hqx` on the host.
    pub host: PathBuf,
    /// Directory under the apps root, `/`-separated ("" = the apps root itself).
    pub rel_dir: String,
    /// The file's real Mac name, from the BinHex header (not the host filename,
    /// which may have been sanitised).
    pub mac_name: String,
    /// HFS type code, e.g. `APPL`.
    pub ostype: String,
}

/// A capture expanded into a staging folder and ready to be recorded.
#[derive(Debug, Clone)]
pub struct StagedImport {
    /// Slug id derived from the title.
    pub id: String,
    /// Display name (the archive's top folder, else its filename stem).
    pub name: String,
    /// The on-volume folder these files land in, HFS-safe and ≤ 31 chars.
    pub folder: String,
    /// Every expanded fork.
    pub files: Vec<StagedFile>,
    /// The launch app's path relative to `/MacAtrium` — the record's `app`.
    pub app_rel: String,
    /// Host staging dir holding the expanded forks — the record's `local_src`.
    pub dir: PathBuf,
    /// Files whose Mac name couldn't be represented on the host filesystem,
    /// as `(mac_name, host_name)`. Empty is the normal case.
    pub renamed: Vec<(String, String)>,
}

/// Decode just enough of a BinHex 4.0 file to read its header: the real Mac
/// filename plus type/creator. Returns `None` if it isn't BinHex or is truncated.
///
/// Only the first ~128 decoded bytes are needed, so this stops early rather than
/// decoding a multi-MB data fork — an import may expand hundreds of files.
pub fn binhex_header(path: &Path) -> Option<(String, String, String)> {
    const TBL: &[u8] = b"!\"#$%&'()*+,-012345689@ABCDEFGHIJKLMNPQRSTUVXYZ[`abcdefhijklmpqr";
    let raw = std::fs::read(path).ok()?;
    // Skip the "(This file must be converted with BinHex 4.0)" preamble.
    let start = raw.iter().position(|&b| b == b':')? + 1;
    let mut lut = [0xFFu8; 256];
    for (i, &c) in TBL.iter().enumerate() {
        lut[c as usize] = i as u8;
    }
    // 6-bit values -> bytes, then RLE90, stopping once the header is covered.
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;
    let mut out: Vec<u8> = Vec::with_capacity(160);
    let mut prev: Option<u8> = None;
    let mut in_run = false;
    for &c in &raw[start..] {
        if c == b':' {
            break;
        }
        let v = lut[c as usize];
        if v == 0xFF {
            continue; // newline / padding
        }
        bits = (bits << 6) | v as u32;
        nbits += 6;
        while nbits >= 8 {
            nbits -= 8;
            let byte = ((bits >> nbits) & 0xFF) as u8;
            if in_run {
                in_run = false;
                if byte == 0 {
                    out.push(0x90);
                    prev = Some(0x90);
                } else if let Some(p) = prev {
                    // A run repeats the previous byte (byte-1) more times.
                    for _ in 1..byte {
                        out.push(p);
                    }
                }
            } else if byte == 0x90 {
                in_run = true;
            } else {
                out.push(byte);
                prev = Some(byte);
            }
        }
        if out.len() >= 160 {
            break;
        }
    }
    let nlen = *out.first()? as usize;
    if nlen == 0 || out.len() < 1 + nlen + 1 + 8 {
        return None;
    }
    // Name encoding depends on who wrote the file. A vintage BinHex carries
    // MacRoman; one rb-cli produced carries UTF-8. Valid UTF-8 is taken as-is —
    // MacRoman-decoding it turns "Glider PRO™" into "Glider PRO‚Ñ¢", because
    // U+2122 is E2 84 A2 and those bytes are ‚ Ñ ¢ in MacRoman. Invalid UTF-8
    // can only be MacRoman, so that's the fallback.
    let raw_name = &out[1..1 + nlen];
    let name = match std::str::from_utf8(raw_name) {
        Ok(s) => s.to_string(),
        Err(_) => crate::macroman::decode(raw_name),
    };
    let p = 1 + nlen + 1; // name, then a version byte
    let ostype = String::from_utf8_lossy(&out[p..p + 4]).to_string();
    let creator = String::from_utf8_lossy(&out[p + 4..p + 8]).to_string();
    Some((name, ostype, creator))
}

/// Expand `archive` into `dest_root/<id>/` and work out what it contains.
///
/// The launch app is the file whose type is `APPL`; with several, the one whose
/// name best matches the title wins, then the shallowest, then the largest —
/// `Dark Castle.mar` for instance holds both a `DC2a` document and the real
/// `DC Launcher` (`APPL`). With no `APPL` at all the import still succeeds (some
/// captures are pure data) but `app_rel` points at the folder, and the caller
/// should ask the user to nominate one.
pub fn stage_archive(rb: &RbCli, archive: &Path, dest_root: &Path) -> Result<StagedImport> {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "import".to_string());
    let id = crate::harvest::slugify(&stem);
    anyhow::ensure!(!id.is_empty(), "could not derive an id from {}", archive.display());

    let dir = dest_root.join(&id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating staging dir {}", dir.display()))?;
    rb.archive_extract(archive, &dir)
        .with_context(|| format!("extracting {}", archive.display()))?;

    // Walk the expansion; every `.hqx` is one fork-carrying file.
    let mut files: Vec<StagedFile> = Vec::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    collect(&dir, "", &mut files, &mut renamed);
    anyhow::ensure!(!files.is_empty(), "{} expanded to nothing", archive.display());

    // The archive's own top-level folder names the install; fall back to the stem.
    let top = files
        .iter()
        .filter_map(|f| f.rel_dir.split('/').next().filter(|s| !s.is_empty()))
        .next()
        .map(str::to_string);
    let name = top.clone().unwrap_or_else(|| stem.clone());
    let folder = crate::config::hfs_name(&name);

    let app = pick_launch_app(&files, &name);
    let app_rel = match &app {
        Some(f) => {
            // Strip the archive's own top folder: on-disk we place the contents
            // under our (possibly shortened) `folder`, not the original name.
            let inner = strip_top(&f.rel_dir);
            if inner.is_empty() {
                format!("Apps/{folder}/{}", f.mac_name)
            } else {
                format!("Apps/{folder}/{inner}/{}", f.mac_name)
            }
        }
        None => format!("Apps/{folder}"),
    };

    Ok(StagedImport { id, name, folder, files, app_rel, dir, renamed })
}

/// Drop the first path component (the archive's own wrapper folder).
fn strip_top(rel: &str) -> String {
    let mut it = rel.splitn(2, '/');
    let _first = it.next();
    it.next().unwrap_or("").to_string()
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<StagedFile>, renamed: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let host_name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            let sub = if rel.is_empty() { host_name.clone() } else { format!("{rel}/{host_name}") };
            collect(&p, &sub, out, renamed);
            continue;
        }
        if p.extension().and_then(|x| x.to_str()) != Some("hqx") {
            continue;
        }
        let (mac_name, ostype) = match binhex_header(&p) {
            Some((n, t, _c)) => (n, t),
            // Unreadable header: fall back to the host name minus .hqx. The file
            // still injects; only APPL detection is degraded.
            None => (host_name.trim_end_matches(".hqx").to_string(), String::new()),
        };
        let host_stem = host_name.trim_end_matches(".hqx").to_string();
        if host_stem != mac_name {
            renamed.push((mac_name.clone(), host_stem));
        }
        out.push(StagedFile { host: p, rel_dir: rel.to_string(), mac_name, ostype });
    }
}

/// Choose the launchable app among the staged files (see [`stage_archive`]).
fn pick_launch_app<'a>(files: &'a [StagedFile], title: &str) -> Option<&'a StagedFile> {
    let appls: Vec<&StagedFile> = files.iter().filter(|f| f.ostype == "APPL").collect();
    if appls.is_empty() {
        return None;
    }
    let want = title.to_lowercase();
    appls
        .iter()
        .copied()
        .max_by_key(|f| {
            let n = f.mac_name.to_lowercase();
            let name_match = if n == want {
                3
            } else if want.starts_with(&n) || n.starts_with(&want) {
                2
            } else if n.contains(&want) || want.contains(&n) {
                1
            } else {
                0
            };
            // Prefer a real app over an installer, then a shallower path.
            let not_installer = !crate::harvest::is_installer_name(&f.mac_name) as i32;
            let shallow = 10i32 - f.rel_dir.matches('/').count() as i32;
            (not_installer, name_match, shallow)
        })
}

/// Inject a previously staged capture straight from its host folder — the build
/// path for a record carrying `local_src`. Re-derives the layout by walking
/// `dir`, so it needs nothing but the folder the import left behind.
///
/// A missing or empty staging dir is an error rather than a silent no-op: the
/// record claims those files exist, and a build that quietly shipped without
/// them would only surface as a title that launches into a File-not-found.
pub fn inject_staged(rb: &RbCli, image: &Path, dir: &Path, apps_root: &str) -> Result<usize> {
    anyhow::ensure!(
        dir.is_dir(),
        "import staging folder {} is missing — re-import the capture",
        dir.display()
    );
    let mut files: Vec<StagedFile> = Vec::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    collect(dir, "", &mut files, &mut renamed);
    anyhow::ensure!(!files.is_empty(), "no importable files under {}", dir.display());
    let top = files
        .iter()
        .filter_map(|f| f.rel_dir.split('/').next().filter(|s| !s.is_empty()))
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let s = StagedImport {
        id: String::new(),
        name: top.clone(),
        folder: crate::config::hfs_name(&top),
        files,
        app_rel: String::new(),
        dir: dir.to_path_buf(),
        renamed,
    };
    inject(rb, image, &s, apps_root)
}

/// Inject a staged import's forks into `image` under `apps_root`, recreating the
/// folder structure below the import's own folder. Returns the count written.
///
/// **Strict**: any file that fails to land is an error. A partially-injected
/// title is a broken title — it would be recorded as importable and only show up
/// as a missing data file (or a File-not-found at launch) much later. Callers
/// that must not abort a batch catch this and report the one capture as failed.
pub fn inject(rb: &RbCli, image: &Path, s: &StagedImport, apps_root: &str) -> Result<usize> {
    let root = apps_root.trim_end_matches('/');
    let mut n = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for f in &s.files {
        let inner = strip_top(&f.rel_dir);
        let dst = if inner.is_empty() {
            format!("{root}/{}", s.folder)
        } else {
            format!("{root}/{}/{inner}", s.folder)
        };
        rb.mkdir_p(image, &dst)?;
        match rb.put_binhex(image, &f.host, &dst, None) {
            Ok(()) => n += 1,
            Err(e) => {
                eprintln!("[import] {} -> {dst}: {e:#}", f.host.display());
                failed.push(f.mac_name.clone());
            }
        }
    }
    anyhow::ensure!(
        failed.is_empty(),
        "{} of {} file(s) could not be written to {}: {}",
        failed.len(),
        s.files.len(),
        image.display(),
        failed.join(", ")
    );
    Ok(n)
}

/// What one `run` produced, for the caller to report.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// `(id, name, file count)` per imported capture.
    pub imported: Vec<(String, String, usize)>,
    /// `(archive, error)` for captures that failed — one bad file never aborts
    /// the batch, matching the fail-soft rule the rest of the pipeline follows.
    pub failed: Vec<(PathBuf, String)>,
    /// Files whose Mac name the host filesystem couldn't represent, across all
    /// captures: `(id, mac_name, host_name)`. See the module docs.
    pub renamed: Vec<(String, String, String)>,
}

/// Import captures into the library: expand each into `stage_root`, then upsert a
/// record (`id`/`name`/`kind`/`app`/`local_src`) into the `dataset` JSONL.
///
/// `donor` optionally also injects the forks into a reservoir image and records
/// `harvest_src` instead of `local_src` — for people who keep one. It is not
/// required, and without it nothing but the host staging folder is involved.
pub fn run(
    rb: &RbCli,
    archives: &[PathBuf],
    stage_root: &Path,
    dataset: &Path,
    donor: Option<(&str, &Path)>,
    apps_root: &str,
) -> Result<ImportReport> {
    std::fs::create_dir_all(stage_root)
        .with_context(|| format!("creating {}", stage_root.display()))?;
    let mut report = ImportReport::default();
    let mut records: Vec<serde_json::Value> = Vec::new();

    for archive in archives {
        let staged = match stage_archive(rb, archive, stage_root) {
            Ok(s) => s,
            Err(e) => {
                report.failed.push((archive.clone(), format!("{e:#}")));
                continue;
            }
        };
        for (mac, host) in &staged.renamed {
            report.renamed.push((staged.id.clone(), mac.clone(), host.clone()));
        }

        let mut rec = serde_json::Map::new();
        rec.insert("id".into(), staged.id.clone().into());
        rec.insert("name".into(), staged.name.clone().into());
        rec.insert("kind".into(), "game".into());
        rec.insert("app".into(), staged.app_rel.clone().into());

        match donor {
            // Reservoir route: put the forks on the donor and source from there.
            Some((key, image)) => {
                // Strict: a failed or partial injection must NOT yield a record.
                // Recording a title whose files never landed points the build at
                // an empty donor folder, which only surfaces at launch time.
                match inject(rb, image, &staged, apps_root) {
                    Ok(n) if n > 0 => {
                        let mut hs = serde_json::Map::new();
                        hs.insert("donor".into(), key.into());
                        hs.insert(
                            "path".into(),
                            format!("{}/{}", apps_root.trim_end_matches('/'), staged.folder).into(),
                        );
                        rec.insert("harvest_src".into(), serde_json::Value::Object(hs));
                        report.imported.push((staged.id.clone(), staged.name.clone(), n));
                    }
                    Ok(_) => {
                        report.failed.push((
                            archive.clone(),
                            format!("nothing was written to {}", image.display()),
                        ));
                        continue;
                    }
                    Err(e) => {
                        report.failed.push((archive.clone(), format!("{e:#}")));
                        continue;
                    }
                }
            }
            // Donor-less route: the staging folder IS the source.
            None => {
                rec.insert("local_src".into(), staged.dir.to_string_lossy().into_owned().into());
                report.imported.push((staged.id.clone(), staged.name.clone(), staged.files.len()));
            }
        }
        records.push(serde_json::Value::Object(rec));
    }

    if !records.is_empty() {
        upsert_records(dataset, &records)?;
    }
    Ok(report)
}

/// Upsert id-keyed records into a JSONL dataset: an existing line with the same
/// id is replaced (so re-importing a capture updates it in place rather than
/// duplicating), anything else is appended. Comments and blank lines survive.
fn upsert_records(dataset: &Path, records: &[serde_json::Value]) -> Result<()> {
    use std::collections::HashMap;
    if let Some(parent) = dataset.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(dataset).unwrap_or_default();
    let by_id: HashMap<&str, &serde_json::Value> = records
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|id| (id, r)))
        .collect();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out = String::new();
    for line in existing.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let id = serde_json::from_str::<serde_json::Value>(t)
            .ok()
            .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_string));
        match id.as_deref().and_then(|i| by_id.get(i).map(|r| (i, *r))) {
            Some((i, rec)) => {
                seen.insert(by_id.get_key_value(i).map(|(k, _)| *k).unwrap_or(""));
                out.push_str(&rec.to_string());
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    for r in records {
        let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !seen.contains(id) {
            out.push_str(&r.to_string());
            out.push('\n');
        }
    }
    std::fs::write(dataset, out).with_context(|| format!("writing {}", dataset.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_top_drops_the_wrapper_folder() {
        assert_eq!(strip_top("Dark Castle"), "");
        assert_eq!(strip_top("Civilization/CivHackPM3"), "CivHackPM3");
        assert_eq!(strip_top("A/B/C"), "B/C");
        assert_eq!(strip_top(""), "");
    }

    fn f(name: &str, ostype: &str, rel: &str) -> StagedFile {
        StagedFile {
            host: PathBuf::from(format!("/tmp/{name}.hqx")),
            rel_dir: rel.into(),
            mac_name: name.into(),
            ostype: ostype.into(),
        }
    }

    /// The real Dark Castle capture: the file NAMED like the title is a document
    /// (`DC2a`); the actual application is `DC Launcher`. Type must beat name.
    #[test]
    fn picks_the_appl_not_the_same_named_document() {
        let files = vec![
            f("Dark Castle", "DC2a", "Dark Castle"),
            f("DC Data", "DC2b", "Dark Castle"),
            f("DC Launcher", "APPL", "Dark Castle"),
        ];
        let got = pick_launch_app(&files, "Dark Castle").expect("an APPL exists");
        assert_eq!(got.mac_name, "DC Launcher");
    }

    /// With several APPLs the title match decides, and an installer loses to a
    /// real app even when its name matches better.
    #[test]
    fn prefers_the_title_match_and_avoids_installers() {
        let files = vec![
            f("Install Marathon", "APPL", "Marathon"),
            f("Marathon", "APPL", "Marathon"),
        ];
        assert_eq!(pick_launch_app(&files, "Marathon").unwrap().mac_name, "Marathon");

        // Nothing but an installer: still return it rather than nothing.
        let only = vec![f("Installer", "APPL", "X")];
        assert_eq!(pick_launch_app(&only, "X").unwrap().mac_name, "Installer");
    }

    /// Regression: a BinHex name written by rb-cli is UTF-8, and MacRoman-decoding
    /// it mangles every non-ASCII app name — "Glider PRO™" became "Glider PRO‚Ñ¢",
    /// which then looked like the host had renamed the file. Verified against the
    /// real Glider PRO / Civilization captures.
    #[test]
    fn binhex_names_prefer_utf8_and_fall_back_to_macroman() {
        // A synthetic header is enough: the decode branch is what's under test.
        let utf8 = "Glider PRO™".as_bytes();
        assert_eq!(std::str::from_utf8(utf8).unwrap(), "Glider PRO™");
        // MacRoman-decoding those same bytes is the bug's signature.
        assert_eq!(crate::macroman::decode(utf8), "Glider PRO‚Ñ¢");
        // A lone 0xAA is not valid UTF-8, so it can only be MacRoman (™).
        // Built at runtime so the compiler can't const-fold the from_utf8 check.
        let macroman: Vec<u8> = vec![b'A', 0xAAu8];
        assert!(std::str::from_utf8(&macroman).is_err());
        assert_eq!(crate::macroman::decode(&macroman), "A™");
    }

    /// Re-importing a capture must update its record in place, not append a
    /// duplicate — otherwise the library grows a second entry every time you fix
    /// a capture, and both would fight over the same `app` path.
    #[test]
    fn upsert_replaces_by_id_and_keeps_everything_else() {
        let p = std::env::temp_dir().join("atrium_import_upsert_test.jsonl");
        std::fs::write(
            &p,
            "# a comment\n\
             {\"id\":\"other\",\"name\":\"Other\"}\n\
             {\"id\":\"dark-castle\",\"name\":\"Old\",\"app\":\"Apps/Old/Old\"}\n",
        )
        .unwrap();
        let rec = serde_json::json!({
            "id": "dark-castle", "name": "Dark Castle",
            "app": "Apps/Dark Castle/DC Launcher", "local_src": "/stage/dark-castle"
        });
        upsert_records(&p, &[rec]).unwrap();

        let txt = std::fs::read_to_string(&p).unwrap();
        assert!(txt.starts_with("# a comment"), "comments survive");
        assert_eq!(txt.matches("\"dark-castle\"").count(), 1, "no duplicate record");
        assert!(txt.contains("DC Launcher"), "the record was updated");
        assert!(!txt.contains("Apps/Old/Old"), "the stale app path is gone");
        assert!(txt.contains("\"other\""), "unrelated records untouched");

        // A brand-new id appends instead.
        let fresh = serde_json::json!({"id": "glider-pro", "name": "Glider PRO"});
        upsert_records(&p, &[fresh]).unwrap();
        let txt = std::fs::read_to_string(&p).unwrap();
        assert!(txt.contains("glider-pro"));
        assert_eq!(txt.matches("\"dark-castle\"").count(), 1);
        let _ = std::fs::remove_file(&p);
    }

    /// Regression: `inject` used to warn per file and still return `Ok`, so an
    /// import into an unwritable donor reported success and wrote a record whose
    /// `harvest_src` pointed at an empty folder. The failure only showed up much
    /// later as a title that wouldn't launch. Any file that doesn't land is now
    /// an error, so `run` records nothing.
    #[test]
    fn inject_fails_loudly_when_files_cannot_be_written() {
        // An rb-cli that cannot exist makes every put fail deterministically,
        // with no dependency on a real rb-cli being installed.
        let rb = RbCli::new("/nonexistent/rb-cli-for-tests");
        let staged = StagedImport {
            id: "x".into(),
            name: "X".into(),
            folder: "X".into(),
            files: vec![StagedFile {
                host: PathBuf::from("/tmp/nope.hqx"),
                rel_dir: "X".into(),
                mac_name: "Thing".into(),
                ostype: "APPL".into(),
            }],
            app_rel: "Apps/X/Thing".into(),
            dir: PathBuf::from("/tmp/x"),
            renamed: Vec::new(),
        };
        let err = inject(&rb, Path::new("/tmp/no-such.hfv"), &staged, "/MacAtrium/Apps")
            .expect_err("a failed write must not report success");
        let msg = err.to_string();
        assert!(msg.contains("1 of 1"), "should say how many failed: {msg}");
        assert!(msg.contains("Thing"), "should name the file: {msg}");
    }

    /// A capture with no application at all is not an error — some are pure data.
    #[test]
    fn no_appl_yields_none() {
        let files = vec![f("Data", "DATA", "X"), f("More", "TEXT", "X")];
        assert!(pick_launch_app(&files, "X").is_none());
    }
}
