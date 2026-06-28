# 23 — Multi-volume library (backlog: break the 2 GB boot-disk cap)

**Status: BACKLOG / major feature.** Not started. Captured 2026-06-28 after the
q800 harness verified the wall below.

## Why

Classic Mac OS **boot volumes are capped at 2 GB**. Verified empirically on the
QEMU Quadra 800 harness (`tools/qemu-harness`):

- A 1.9 GB 7.5.5 disk **boots** (launcher comes up).
- The *same* disk `expand`ed to 3.0 GB **Sad Macs** (boot-failure icon, codes
  `0000000F` / `00000004`). The expand itself is fine — it's the >2 GB volume the
  ROM won't boot from. (HFS itself goes to ~4 GiB; the limit is *booting*.)

So a single bootable disk can't hold the full library at full-resolution art:
the "Millions" full build is ~537 titles × ~2.5 MB of 720px 1/8/24-bit art ≈
**~2.5 GB**, which exceeds the 2 GB boot cap. Today's stopgap is to shrink the art
(`max_art_size: 448x448`) so everything fits one bootable ≤2 GB disk — at the cost
of art resolution.

## The idea

Span the library across **multiple SCSI volumes**:

- **One boot volume (≤ 2 GB):** System 7.5.5 + the MacAtrium launcher + the paged
  metadata (`/MacAtrium/metadata/*`). Small and bootable.
- **N non-boot data volumes (≤ 4 GiB each):** the apps (`/MacAtrium/Apps/...`) and
  the art (`/MacAtrium/images/...`). **Non-boot volumes are NOT capped at 2 GB** —
  they go to the HFS ~4 GiB ceiling — so each data disk can be up to ~4 GiB, and we
  can attach several. Mac OS mounts all SCSI volumes at boot.

This breaks the per-disk 2 GB *boot* limit (only the small boot disk must be ≤2 GB)
and the single-disk 4 GiB HFS limit (add more data disks), so the full library can
keep full-resolution 24-bit art.

## What it touches

- **Build (`atrium image`):** emit multiple disk images — one bootable system disk
  + one or more data disks — and distribute Apps/art across the data disks (bin-pack
  to ≤4 GiB each). Bless only the boot disk.
- **Catalog/metadata:** each catalog record needs to know **which volume** its app
  and art live on (a volume tag / volume-relative path). The paged metadata stays on
  the boot disk; it points outward to the data volumes.
- **Launcher (C):** resolve app/art paths across **mounted volumes**, not just the
  boot volume — enumerate mounted HFS volumes, find the one holding each item (by
  volume name or a manifest), and read/launch from there. Handle a data volume being
  absent gracefully (grey out / "insert disk").
- **Harness + real hardware:** attach multiple SCSI disks
  (`-device scsi-hd,...,scsi-id=N`). q800 supports several SCSI IDs.

## Open questions

- Volume identity: match by HFS volume name, a UUID-ish tag in a manifest file, or
  SCSI ID? Names are user-renamable; a manifest on the boot disk is more robust.
- Path scheme: keep `/MacAtrium/Apps/...` but prefix with a volume? e.g. resolve
  `<VolName>:MacAtrium:Apps:...`. The launcher already builds Mac paths for launch.
- Do apps launch correctly from a non-boot volume under 7.5.5? (Expected yes —
  classic apps run from any mounted volume — but verify on the q800.)
- Build-time bin-packing + keeping a title's app+art together on one data volume.

## Interim

Until this lands, full builds use `max_art_size` to fit one bootable ≤2 GB disk
(see the Quadra/Millions target). Related: memory `color-art-memory-budget`,
`qemu-q800-harness`; docs/21 (paged catalog).
