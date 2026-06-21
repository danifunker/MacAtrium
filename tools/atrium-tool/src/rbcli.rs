//! Thin wrapper over the `rb-cli` binary (rusty-backup) — the volume-I/O layer
//! `atrium` shells out to for reading/writing HFS images. We parse the handful
//! of verbs we need; rb-cli stays the source of truth for the bytes.

use anyhow::{bail, Context, Result};
use std::path::Path;
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
    pub fn put_binhex(&self, image: &Path, hqx: &Path, dst_dir: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let h = hqx.to_string_lossy();
        self.run(&["put-binhex", &img, &h, "--dst-dir", dst_dir, "-q"])?;
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

    /// Install a MacBinary archive (both forks + Finder info) into a directory.
    pub fn put_macbinary(&self, image: &Path, host: &Path, dst_dir: &str) -> Result<()> {
        let img = image.to_string_lossy();
        let h = host.to_string_lossy();
        self.run(&["put-macbinary", &img, &h, "--dst-dir", dst_dir, "-q"])?;
        Ok(())
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
    fn ignores_headers() {
        assert!(parse_ls_line("Partition 2 (APM): Apple_HFS ...").is_none());
        assert!(parse_ls_line("Blessed System Folder: /System 6.0.8").is_none());
        assert!(parse_ls_line("").is_none());
    }
}
