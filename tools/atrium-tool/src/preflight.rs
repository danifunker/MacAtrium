//! `atrium::preflight` — disk-size + space-estimate controller.
//!
//! Two related concerns:
//!  * [`apply_disk_size`] grows the output image to the configured target size
//!    (capped at the HFS 2 GB ceiling) during assembly.
//!  * [`estimate`] projects how much space a build will use *before* running it,
//!    so a view can warn when a selection won't fit. The estimate aims for ~95%,
//!    not exactness.

use crate::config::{BuildConfig, MAX_DISK_MB};
use crate::rbcli::RbCli;
use anyhow::{Context, Result};
use std::path::Path;

/// A space projection for a build, in bytes. `fits` compares against the target.
#[derive(Clone, Debug, Default)]
pub struct Estimate {
    pub base_bytes: u64,
    pub apps_bytes: u64,
    pub art_bytes: u64,
    pub overhead_bytes: u64,
    pub target_bytes: u64,
}

impl Estimate {
    pub fn total(&self) -> u64 {
        self.base_bytes + self.apps_bytes + self.art_bytes + self.overhead_bytes
    }
    pub fn fits(&self) -> bool {
        self.target_bytes == 0 || self.total() <= self.target_bytes
    }
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Project disk usage for `cfg` (resolved). `app_fork_bytes` is the summed size of
/// the selected apps' forks (the selection controller computes this); pass 0 if
/// unknown. Art is estimated from the per-item baked-variant count × a typical
/// size. Within ~95% is the goal — enough to catch "won't fit".
pub fn estimate(cfg: &BuildConfig, app_fork_bytes: u64, n_items: u64) -> Estimate {
    let base_bytes = cfg.system.as_ref().map(|p| file_len(p)).unwrap_or(0);
    // Each baked art item: ~1 raw (1-bit, ~10 KB) + ~1 colour PICT (~40 KB) per
    // depth variant, plus an app icon (~4 KB). Use a flat ~60 KB/item × depths.
    let depth_variants = cfg.art_depths.len().max(1) as u64;
    let art_bytes = n_items.saturating_mul(60_000).saturating_mul(depth_variants);
    let overhead_bytes = 512 * 1024; // catalog + dir entries + slack
    let target_bytes = cfg
        .disk_size_mb
        .map(|mb| mb.min(MAX_DISK_MB) * 1024 * 1024)
        .unwrap_or(0);
    Estimate {
        base_bytes,
        apps_bytes: app_fork_bytes,
        art_bytes,
        overhead_bytes,
        target_bytes,
    }
}

/// Grow the output image to `disk_size_mb` (capped at the HFS 2 GB ceiling) by
/// cloning it into a fresh, larger APM disk via `rb-cli expand`. No-op when unset
/// or when the base already meets/exceeds the target (HFS can't shrink here).
/// Run right after the base copy, before harvest, so apps land in the bigger
/// volume.
pub fn apply_disk_size(rb: &RbCli, cfg: &BuildConfig) -> Result<()> {
    let Some(mb) = cfg.disk_size_mb else { return Ok(()) };
    let mb = mb.min(MAX_DISK_MB);
    let want = mb * 1024 * 1024;
    let have = file_len(&cfg.out);
    if have >= want {
        eprintln!("[disk] base is {} MB ≥ target {} MB — left as-is (no shrink)", have / (1024 * 1024), mb);
        return Ok(());
    }
    eprintln!("[disk] growing {} MB → {} MB", have / (1024 * 1024), mb);
    let tmp = cfg.out.with_extension("expand.tmp");
    rb.expand(&cfg.out, mb, &tmp)
        .with_context(|| format!("expanding {} to {} MB", cfg.out.display(), mb))?;
    std::fs::rename(&tmp, &cfg.out)
        .with_context(|| format!("replacing {} with expanded image", cfg.out.display()))?;
    Ok(())
}

/// Right-size the finished output: shrink the volume (via `rb-cli expand` into a
/// fresh, smaller APM disk) so free space is `free_space_pct`% of the final disk,
/// or at least `free_space_min_mb` MB on a small disk — reclaiming the slack left by
/// growing to the working `disk_size_mb`. Run LAST, after every content write (incl.
/// the Desktop rebuild). `expand` re-wraps the clone bootable, preserving the blessed
/// System Folder + boot blocks; the target never exceeds the current size or the HFS
/// 2 GB cap. A smaller volume re-packs at a smaller allocation block (freeing a little
/// more), so iterate until within ~16 MB of the target (1–2 passes in practice).
/// No-op when `right_size` is off or there's nothing worth reclaiming.
pub fn right_size_image(rb: &RbCli, cfg: &BuildConfig) -> Result<()> {
    if !cfg.right_size {
        return Ok(());
    }
    let mb = 1024 * 1024;
    let pct = cfg.free_space_pct.min(90);
    let min_free = cfg.free_space_min_mb.saturating_mul(mb);
    let ceil_mb = cfg.disk_size_mb.unwrap_or(MAX_DISK_MB).min(MAX_DISK_MB);
    for pass in 0..3u32 {
        let used = rb
            .fs_used(&cfg.out)
            .context("measuring used space to right-size the image")?;
        // free = pct% of the FINAL disk: with total = used + free, that is
        // free = used * pct / (100 - pct); floored at `free_space_min_mb`.
        let free = (used.saturating_mul(pct) / (100 - pct).max(1)).max(min_free);
        let used_mb = (used + mb - 1) / mb;
        let target_mb = ((used + free + mb - 1) / mb)
            .min(ceil_mb)
            .max(used_mb + cfg.free_space_min_mb);
        let have_mb = file_len(&cfg.out) / mb;
        if have_mb <= target_mb + 16 {
            if pass == 0 {
                eprintln!(
                    "[disk] right-size: {} MB used on a {} MB disk — already tight, left as-is",
                    used_mb, have_mb
                );
            }
            break;
        }
        eprintln!(
            "[disk] right-sizing {} MB → {} MB ({} MB used + ~{} MB free)",
            have_mb,
            target_mb,
            used_mb,
            target_mb.saturating_sub(used_mb)
        );
        let tmp = cfg.out.with_extension("rightsize.tmp");
        rb.expand(&cfg.out, target_mb, &tmp)
            .with_context(|| format!("right-sizing {} to {} MB", cfg.out.display(), target_mb))?;
        std::fs::rename(&tmp, &cfg.out)
            .with_context(|| format!("replacing {} with the right-sized image", cfg.out.display()))?;
    }
    Ok(())
}
