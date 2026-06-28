//! Thin wrapper over the `rb-cli` binary (rusty-backup) — the volume-I/O layer
//! `atrium` shells out to for reading/writing HFS images. We parse the handful
//! of verbs we need; rb-cli stays the source of truth for the bytes.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One entry from `rb-cli ls -q`. The listing is fixed-column:
/// `FILE`/`DIR` (0..4), size right-aligned (4..16), OSType (18..22),
/// creator (23..27), then the name at byte 29 (the prefix is always ASCII, so
/// byte 29 is a safe char boundary even when the name is multi-byte UTF-8).
#[derive(Debug, Clone)]
pub struct Entry {
    pub is_dir: bool,
    pub ostype: String,
    pub name: String,
    /// Parsed for completeness / future use (manifests, dedup); not all callers
    /// read them yet.
    #[allow(dead_code)]
    pub creator: String,
    #[allow(dead_code)]
    pub size: u64,
}

pub struct RbCli {
    pub bin: String,
}

impl RbCli {
    pub fn new(bin: &str) -> Self {
        RbCli { bin: bin.to_string() }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        // DEBUG: if RBCLI_ARGV_LOG is set, append every invocation's argv (one
        // line per call) so the rb-cli maintainer can audit rm/--force/collisions.
        if let Ok(p) = std::env::var("RBCLI_ARGV_LOG") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                let _ = writeln!(f, "{} {}", self.bin, args.join(" "));
            }
        }
        let out = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("running `{}` (is rb-cli on PATH? pass --rb-cli)", self.bin))?;
        if !out.status.success() {
            bail!(
                "rb-cli {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// List a directory inside an image.
    pub fn ls(&self, image: &Path, path: &str) -> Result<Vec<Entry>> {
        let img = image.to_string_lossy();
        let out = self.run(&["ls", "-q", &img, path])?;
        Ok(out.lines().filter_map(parse_ls_line).collect())
    }

    /// Extract a file (both forks + Finder info) as a BinHex .hqx on the host.
    pub fn get_binhex(&self, image: &Path, src: &str, out_hqx: &Path) -> Result<()> {
        let img = image.to_string_lossy();
        let dst = out_hqx.to_string_lossy();
        self.run(&["get-binhex", "-q", &img, src, &dst])?;
        Ok(())
    }

    /// Create a directory inside an image. rb-cli's mkdir is not recursive
    /// (parent must exist), so create each prefix; errors on already-existing
    /// levels are swallowed (a genuinely uncreatable leaf surfaces at put time).
    pub fn mkdir_p(&self, image: &Path, path: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let mut prefix = String::new();
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            prefix.push('/');
            prefix.push_str(comp);
            let _ = self.run(&["mkdir", &img, &prefix, "-q"]);
        }
        Ok(())
    }

    /// Decode a .hqx and write it (both forks) into a directory inside an image.
    /// Clears `hasBeenInited` so the Finder re-reads each injected app's `BNDL` on
    /// the fresh disk and shows real icons (a copied-in app with `hasBeenInited`
    /// still set is treated as already-catalogued → generic icon). Matches the
    /// flag policy the launcher install applies to itself.
    pub fn put_binhex(&self, image: &Path, hqx: &Path, dst_dir: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let h = hqx.to_string_lossy();
        self.run(&["put-binhex", &img, &h, "--dst-dir", dst_dir, "--clear-inited", "-q"])?;
        Ok(())
    }

    /// Extract a plain file (data fork) from an image to the host. Returns Err if
    /// the source doesn't exist — callers that treat absence as fine ignore that.
    /// `force` overwrites an existing host destination.
    pub fn get(&self, image: &Path, src: &str, out: &Path, force: bool) -> Result<()> {
        let img = image.to_string_lossy();
        let dst = out.to_string_lossy();
        let mut args = vec!["get", "-q", &img, src, &dst];
        if force {
            args.push("--force");
        }
        self.run(&args)?;
        Ok(())
    }

    /// Write a host file into an image as a TEXT file with the given type/creator,
    /// overwriting any existing file (--force).
    pub fn put_text(&self, image: &Path, host: &Path, dst: &str, type_: &str, creator: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let h = host.to_string_lossy();
        self.run(&["put", &img, &h, dst, "--type", type_, "--creator", creator, "--force", "-q"])?;
        Ok(())
    }

    /// Put a host file with an explicit type/creator (both forks not involved),
    /// overwriting any existing file.
    pub fn put_typed(&self, image: &Path, host: &Path, dst: &str, type_: &str, creator: &str) -> Result<()> {
        self.put_text(image, host, dst, type_, creator)
    }

    /// Replace the resource fork of an existing file from a host file (e.g. a
    /// `snd ` sound resource baked by `atrium snd`).
    pub fn set_rsrc(&self, image: &Path, dst: &str, host_rsrc: &Path) -> Result<()> {
        let img = image.to_string_lossy();
        let h = host_rsrc.to_string_lossy();
        self.run(&["setrsrc", &img, dst, "--from-file", &h])?;
        Ok(())
    }

    /// Extract a classic-Mac archive (StuffIt `.sit`/`.sea`, Compact Pro `.cpt`,
    /// `.mar`, or BinHex-wrapped `.hqx`) to a host directory as one `.hqx` per
    /// file (both forks + Finder info) — exactly what `put_binhex` ingests. The
    /// decoding lives in rb-cli's `macarchive` module (pure Rust).
    pub fn archive_extract(&self, archive: &Path, dest: &Path) -> Result<()> {
        let a = archive.to_string_lossy();
        let d = dest.to_string_lossy();
        self.run(&["archive", "extract", &a, &d, "--format", "binhex"])?;
        Ok(())
    }

    /// Change an existing file's HFS type/creator codes.
    pub fn chmeta(&self, image: &Path, path: &str, type_: &str, creator: &str) -> Result<()> {
        let img = image.to_string_lossy();
        self.run(&["chmeta", &img, path, "--type", type_, "--creator", creator])?;
        Ok(())
    }

    /// Install a MacBinary archive (both forks + Finder info) into a directory,
    /// overwriting any existing file so a rebuild onto a non-clean image (e.g.
    /// one that already has the launcher) is idempotent.
    pub fn put_macbinary(&self, image: &Path, host: &Path, dst_dir: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let h = host.to_string_lossy();
        self.run(&["put-macbinary", &img, &h, "--dst-dir", dst_dir, "--force", "-q"])?;
        Ok(())
    }

    /// Grow (or re-block) the classic-HFS volume in `image` to `size_mb` MB by
    /// cloning it into a fresh APM disk at `output`. Used to size a built image to
    /// the requested target. The clone preserves the volume (boot blocks, blessed
    /// System Folder, files) and re-wraps it bootable.
    pub fn expand(&self, image: &Path, size_mb: u64, output: &Path) -> Result<()> {
        let img = image.to_string_lossy();
        let out = output.to_string_lossy();
        let size = format!("{size_mb}M");
        self.run(&["expand", &img, "--size", &size, "--output", &out, "-q"])?;
        Ok(())
    }

    /// True if `path` (a file or directory) exists inside the image. Implemented
    /// by listing the parent and checking for the leaf, so it works for the
    /// space/`ƒ`-containing names our app paths carry. Any rb-cli error (missing
    /// parent, etc.) reads as "does not exist".
    pub fn exists(&self, image: &Path, path: &str) -> bool {
        let trimmed = path.trim_end_matches('/');
        let (parent, leaf) = match trimmed.rsplit_once('/') {
            Some((p, l)) if !l.is_empty() => (if p.is_empty() { "/" } else { p }, l),
            _ => return false,
        };
        match self.ls(image, parent) {
            Ok(entries) => entries.iter().any(|e| e.name == leaf),
            Err(_) => false,
        }
    }
}

/// Decide which `rb-cli` binary a build runs. The per-build [`cfg.rb_cli`] wins
/// over the machine-local `settings.rb_cli`, with one override: an **absolute**
/// path always beats a **bare** name. A bare name (e.g. `"rb-cli"`) is resolved
/// against `$PATH` at exec time, so it silently runs whatever binary is first
/// there — which once let a build pick up a *stale, pre-fix* rb-cli on `$PATH`
/// and write a corrupt HFS catalog while the configured absolute path went
/// ignored. Preferring an explicit absolute path makes the binary a build uses a
/// deterministic choice instead of a function of `$PATH`.
///
/// [`cfg.rb_cli`]: crate::config::BuildConfig::rb_cli
pub fn resolve_bin(cfg_rb_cli: &str, settings_rb_cli: Option<&str>) -> String {
    let cfg_abs = Path::new(cfg_rb_cli).is_absolute();
    match settings_rb_cli {
        // settings has an absolute path and cfg is only a bare name → use it.
        // (When cfg is itself absolute, cfg wins: it's the per-build choice.)
        Some(s) if !cfg_abs && Path::new(s).is_absolute() => s.to_string(),
        _ => cfg_rb_cli.to_string(),
    }
}

/// Resolve a (possibly bare) binary name to the actual file that would exec,
/// walking `$PATH` for a bare name — so a log can show which file really ran.
fn resolve_abs(bin: &str) -> Option<PathBuf> {
    let p = Path::new(bin);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    if bin.contains('/') {
        return std::fs::canonicalize(p).ok();
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(bin))
        .find(|c| c.is_file())
}

/// Log the resolved rb-cli binary + its `--version` at the start of a build. The
/// stale-binary trap (a bare `"rb-cli"` on `$PATH` shadowing the configured
/// path) is invisible until you can see exactly which file ran and what version
/// it is — so surface both up front, where one glance settles it.
pub fn log_version(bin: &str) {
    let resolved = resolve_abs(bin);
    let exec = resolved
        .as_deref()
        .map(Path::as_os_str)
        .unwrap_or_else(|| std::ffi::OsStr::new(bin));
    let ver = Command::new(exec)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "version unknown".to_string());
    match resolved {
        None => eprintln!("[rb-cli] {bin} (NOT FOUND on PATH)"),
        Some(p) if p.as_path() == Path::new(bin) => eprintln!("[rb-cli] {} ({ver})", p.display()),
        Some(p) => eprintln!("[rb-cli] {bin} -> {} ({ver})", p.display()),
    }
}

fn parse_ls_line(line: &str) -> Option<Entry> {
    let is_dir = if line.starts_with("FILE") {
        false
    } else if line.starts_with("DIR") {
        true
    } else {
        return None; // header / blank / partition lines
    };
    let name = line.get(29..)?.trim_end_matches(['\r', '\n']).to_string();
    if name.is_empty() {
        return None;
    }
    let ostype = line.get(18..22).map(|s| s.trim().to_string()).unwrap_or_default();
    let creator = line.get(23..27).map(|s| s.trim().to_string()).unwrap_or_default();
    let size = line
        .get(4..16)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Some(Entry { is_dir, ostype, creator, name, size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_and_dir_lines() {
        let e = parse_ls_line("FILE           0  APPL DCAS  Dark Castle").unwrap();
        assert!(!e.is_dir);
        assert_eq!(e.ostype, "APPL");
        assert_eq!(e.creator, "DCAS");
        assert_eq!(e.name, "Dark Castle");

        let e = parse_ls_line("FILE        1024  ZSYS MACS  System").unwrap();
        assert_eq!(e.ostype, "ZSYS");
        assert_eq!(e.size, 1024);
        assert_eq!(e.name, "System");

        let e = parse_ls_line("DIR            0             Trash").unwrap();
        assert!(e.is_dir);
        assert_eq!(e.name, "Trash");
        assert_eq!(e.ostype, "");
    }

    #[test]
    fn multibyte_name_after_ascii_prefix() {
        // "Déjà Vu" — the prefix is ASCII so byte 29 stays a char boundary.
        let e = parse_ls_line("FILE           0  APPL MIND  Déjà Vu").unwrap();
        assert_eq!(e.name, "Déjà Vu");
    }

    #[test]
    fn resolve_bin_prefers_absolute_and_per_build() {
        // cfg's absolute path beats a bare settings name — the stale-$PATH trap
        // (a bare "rb-cli" silently shadowing the configured binary).
        assert_eq!(resolve_bin("/opt/rb-cli", Some("rb-cli")), "/opt/rb-cli");
        // cfg's absolute path also wins over an absolute settings path (per-build).
        assert_eq!(resolve_bin("/opt/rb-cli", Some("/usr/bin/rb-cli")), "/opt/rb-cli");
        // cfg is the bare default → an absolute settings path is preferred.
        assert_eq!(resolve_bin("rb-cli", Some("/usr/bin/rb-cli")), "/usr/bin/rb-cli");
        // both bare (or no settings) → the per-build cfg value.
        assert_eq!(resolve_bin("rb-cli", Some("rb-cli")), "rb-cli");
        assert_eq!(resolve_bin("/opt/rb-cli", None), "/opt/rb-cli");
        assert_eq!(resolve_bin("rb-cli", None), "rb-cli");
    }

    #[test]
    fn ignores_headers() {
        assert!(parse_ls_line("Partition 2 (APM): Apple_HFS ...").is_none());
        assert!(parse_ls_line("Blessed System Folder: /System 6.0.8").is_none());
        assert!(parse_ls_line("").is_none());
    }
}
