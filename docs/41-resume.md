# 41 — Resume: multi-disk libraries (docs/37) — Phases 1–2 + Phase-3 core landed

Paste into a fresh session **on the WSL box** to continue. **State: multi-disk
aggregation is implemented, builds clean (Retro68), host tests 89/89, and is VERIFIED
end-to-end on a 2-SCSI Snow harness** (Mac II, System 7.5.5, 8-bit colour): two disks
aggregate, categories show `[0]` / `[1]`, each page loads from its own volume, and
MacAtrium Status lists both. Evidence: `docs/evidence/37-multidisk-*.png`.

## What multi-disk does (docs/37)
At startup the launcher enumerates every mounted HFS volume with a
`/MacAtrium/metadata` library, **concatenates** their category lists (each category
tagged with its source volume), and browses / loads-art / launches each title from
the disk it lives on. With ≥2 library disks, categories carry a **`[N]`** disk token
(N = volume-table index, boot = 0); the **MacAtrium Status** screen (Quick-Launch
menu) is the legend. Per-disk *namespacing*, not a by-name merge → the RAM-hard merge
never happens (a resident page is always one disk's one ≤128-item sub-page, so the
128-item cap that protects a 4 MB Mac holds automatically).

## Implemented this session
- **`macfs.c/.h`** — `macfs_make_spec_on(vref, rel, spec)` (`macfs_make_spec` now
  wraps it); `macfs_volumes(VolTable*)` walks `PBHGetVInfo` `ioVolIndex 1..N`, keeps
  volumes with `/MacAtrium/metadata`, boot at `v[0]`, captures `{vref, HFS name, crDate}`.
- **`model.h/.c`** — `CatRef.vol` + `ModelCat.vol`; `model_index_init` copies the tag.
  `MODEL_MAX_CATS` 65 → **128** (N disks' categories concatenate; VOL_MAX=6 caps it).
- **`main.c`** — `gVols`; `vol_vref(vol)`; `load_index` aggregates every disk's
  `index.jsonl` (tagging each category's `vol`); `load_page` reads `cats/<slug>.jsonl`
  from the category's volume; `do_launch` launches on the category's volume;
  `run_status_dialog()` (the MacAtrium Status modal).
- **`art.c/.h`, `launch.c/.h`** — `art_load` / `art_load_rsrc` / `launch_app` take a
  `vref` and resolve on the item's source disk.
- **`ui.c/.h`** — `Ui.vols`; `ui_cur_vref` (current category's volume, boot fallback);
  `ui_append_disk_tag` composes `[N]` at draw time (the carousel/icon headers + the
  list-view category panel); the MacAtrium Status menu row (`MROW_STATUS` →
  `UI_SHOW_STATUS`).
- **`tests/host_test.c`** — asserts `model_index_init` carries the volume tag (89/89).
- **docs/37** rewritten as the locked design.

## Build + test (this box)
```sh
export RETRO68=~/repos/Retro68-build && cd ~/repos/MacAtrium && cmake --build build -j
cd tests && make clean && make && ./host_test    # 91/91 (make doesn't track header deps)
```
Both green as of this session. → `build/MacAtrium.bin`.

## Verified (2026-07-08) — how to reproduce
2-SCSI boot on Snow (Mac II, 7.5.5, 8-bit). Assets on **this** box:
- ROMs: `/mnt/c/temp/mistercore/lbmactwo_MiSTer/releases/{MacIIFDHD.rom, 341-0868.BIN}`
  (the MDC 8•24 ROM; the README's `3410868.bin`).
- Boot disks: `/mnt/c/Temp/ClassicMacHDDs/MacLC_7-5-5.hda`, `…/MacLC_7-1.hda`.
- The harness now takes **`--disk2 <hdd2>`** (attaches SCSI id 1); rebuilt in
  `~/repos/snow` (`cargo build -r -p testrunner --bin macatrium_harness`; note the repo
  pins Rust **1.95.0** — `rustup toolchain install 1.95.0` if its manifest is missing).

Recipe (scripted in `~/mac-mdverify/`):
1. Two **paged** libraries (`libA`, `libB`) — hand-written `index.jsonl` + `cats/*.jsonl`
   (records need `id`/`name`/`app`; a shared "Action" category across both disks proves
   `[N]`). `build.sh` copies `MacLC_7-5-5.hda`→`diskA.hda`, `MacLC_7-1.hda`→`diskB.hda`,
   injects each library + the launcher into `/System Folder/Startup Items` (both disks,
   so boot order is irrelevant), and `setvolname`s them "MacAtrium One" / "MacAtrium Two".
2. `macatrium_harness macii.rom mdc.bin diskA.hda out N --disk2 diskB.hda --snap-every … --keys "…"`.
3. Result (evidence in `docs/evidence/`): `Action [0]` (Space Blaster, disk 0),
   `Arcade [1]` (Coin Muncher, disk 1), and Status listing both disks + correct counts.
   The first frame is the normal first-run chooser → single-disk path is unaffected.

## Selection restore — DONE (2026-07-08)
No persisted volume identity. `model_select` (`src/model.c`) resolves an ambiguous
saved category to the **boot disk** (volumes are boot-first, so boot is `v[0]` by
construction) and falls back to **Recommended** when the saved category's disk was
removed. Host test `test_model_recommended_fallback` (91/91).

## Dropped
- **Host `volume.jsonl` (was Phase 4)** — not needed: restore assumes boot-first + a
  Recommended fallback, and Status uses the live HFS volume name. No stable id persisted.

## Gotchas / invariants
- `[N]` is a LIVE display index (mount order), **never persisted** — and nothing
  volume-specific is persisted at all. Don't bake `[N]` into `ModelCat.name` (breaks
  restore + overflows the 32 B name).
- `prefs.c` / `sound.c` / `bless.c` stay **boot-only** by design; only catalog / art /
  launch went volume-aware. `catalog_parse_into` stays pure (untouched).
- One volume tag **per page**, not per item — every item on a resident page shares one
  source disk, because a page is one disk's one sub-page.
