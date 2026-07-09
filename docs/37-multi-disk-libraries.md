# 37 — Multi-disk libraries (aggregate independent `/MacAtrium` volumes at startup)

**Status: DESIGN LOCKED — implementing.** Requested 2026-07-06; scoped, decisions
resolved, and reduced to a concat (not a merge) on 2026-07-08. This supersedes the
original SCOPING draft.

> **Implementation status (2026-07-08):** Phases 1–2 and the Phase-3 category-count
> bump (`MODEL_MAX_CATS` → 128) are implemented in C, build clean with Retro68, pass
> host tests (89/89), and are **VERIFIED end-to-end on a 2-SCSI Snow harness** (Mac II,
> System 7.5.5, 8-bit colour): two independently-built disks aggregate, categories
> carry `[0]` / `[1]`, each page loads from its own volume, and the MacAtrium Status
> screen lists both disks with correct counts (Disk 0 = 2 cats/4 titles, Disk 1 =
> 2 cats/3 titles). Evidence: `docs/evidence/37-multidisk-{action-disk0,arcade-disk1,status}.png`.
> The harness gained a `--disk2` flag. Selection restore is boot-disk-first with a
> Recommended fallback when a saved category's disk is removed (2026-07-08, host test
> `test_model_recommended_fallback`); host `volume.jsonl` was judged unnecessary and
> dropped. Handoff: docs/41-resume.md.

## What it is

At launcher startup, enumerate every mounted HFS volume that carries its own
self-contained `/MacAtrium` library (its own `metadata/` + `Apps/` + `images/`),
and present them together — each title launched, and its art loaded, from **the
disk it lives on**. "Plug in several curated collection disks and see them all at
once," each disk independently built by `atrium image`.

Read-only aggregation, **startup-time only** (no hot-plug mid-session), **fixed
SCSI hard disks only** (v1 — no removable/CD, no eject-swap handling).

## Decisions (resolved 2026-07-08)

1. **Scope:** full N-disk aggregation (all phases), not a boot+data stopgap.
2. **Media:** fixed SCSI HDs only.
3. **Merge model — per-disk *namespacing*, not a silent by-name merge.** Each disk
   keeps its own categories. When **≥2** library disks are mounted, the launcher
   disambiguates every category with a short disk token **`[N]`** — `N` is the
   volume's index in the launcher's volume table (boot = `[0]`). A single-disk
   boot is byte-for-byte unchanged (no token). This is the decision that keeps the
   whole feature simple (see "Why this is low-risk").
4. **Legend:** a new **MacAtrium Status** screen (Quick-Launch menu) maps
   `Disk N → volume name` plus environment info, so the compact `[N]` stays legible.
5. **Selection restore — no persisted volume identity.** `[N]` is live-only, never
   persisted. A restored selection resolves an ambiguous saved category name to the
   **boot disk** (volumes are ordered boot-first, so the boot disk is `v[0]` by
   construction); if the saved category is gone (its disk was removed) it falls back
   to **Recommended**. So no `volume.jsonl` / stable id is needed (host Phase 4 dropped
   2026-07-08). A vRefNum is never persisted.
6. **Dedup:** none at runtime — a title present on two disks legitimately appears
   under each disk's namespace. (Deduping would force loading every page at startup,
   defeating the paged design; cross-disk overlap is a curation concern, not a
   launcher one.)

### `[N]` = enumeration index, not SCSI ID
The token is the volume's position in the startup walk (trivial, always available,
and identical across real hardware / Snow / QEMU). The true SCSI target is *not*
cheaply derivable from a vRefNum (walk the drive queue → `DrvQEl` → driver refNum →
SCSI Manager), and differs across emulators — so it is at most an optional *column
in the Status screen*, never a dependency of the label. `[N]` is a **live display
token**, not an identity: a changed mount order across boots may renumber it, which
is fine because persisted references key off the stable id (decision 5).

## Why this is far lower-risk than the original scoping feared

The generator already splits any category over `MAX_CAT_ITEMS` (128) into
**numbered sub-pages, each its own index entry** (`tools/atrium-tool/src/catalog.rs`
`run_paged`: `idxs.chunks(MAX_CAT_ITEMS)` → `name`, `name (2)`, …). The launcher
already navigates **each sub-page as its own category slot** (`model_index_init`
makes one `ModelCat` per index line — no by-name merge; that only happens in the
legacy `model_build`). Therefore:

- **A resident page is always exactly one disk's one sub-page (≤128 items).**
  Aggregation is a **concatenation of index entries**, not a cross-disk page
  assembly. The 128-item RAM bound that protects a 4 MB Mac is preserved
  automatically — the original **Risk 1 (OOM from merging N disks) dissolves.**
- **One volume tag per page, not per item** — every item on a resident page shares
  one source disk. No `CatItem` widening and no parallel `vol[]` array; the pure,
  host-tested `catalog_parse_into` stays untouched.
- Per-disk namespacing (decision 3) removes the cross-disk **merge, dedup,
  ordering, and host set-builder** work entirely.

## The three single-volume assumptions (grounded) and how each breaks

1. **`macfs.c` is boot-only.** `gBootVRef` is one cached vRefNum; `macfs_make_spec`
   walks `/MacAtrium/<rel>` on it. → add `macfs_make_spec_on(vref, rel, spec)`;
   `macfs_make_spec` becomes a wrapper passing the boot vref.
2. **No volume table.** → new `macfs_volumes(VolTable *)`: walk `PBHGetVInfo` with
   `ioVolIndex = 1,2,3,…` to `nsvErr`, keep each volume with `/MacAtrium/metadata`
   (`dir_id_of`), capture `{vRefNum, hfsName, crDate}` (+ stableId from
   `volume.jsonl` when present). Boot volume is entry 0 (real vRefNum via the
   existing `macfs_boot_vref` WDRefNum normalization). Data volumes from
   `ioVolIndex` already return a **real** vRefNum → launch-ready, no fix-up.
3. **`CatItem`/`ModelCat` carry no volume.** → tag each `ModelCat` slot (and the
   resident page) with its **volume-table index**. Downstream art/launch resolve on
   that vref. `CatItem` and the parser are unchanged.

## Design

### Volume table + volume-aware `macfs` (the primitive docs/23 reuses)
```c
typedef struct { short vref; Str27 name; unsigned long crDate; long stableId; } VolEntry;
typedef struct { VolEntry v[VOL_MAX]; int n; } VolTable;   /* VOL_MAX ~ 6 */
```
`macfs_make_spec_on(short vref, const char *rel, FSSpec *spec)` is today's
`macfs_make_spec` with the vref as a parameter; the old signature wraps it with
`macfs_boot_vref`. Every art/catalog/launch read takes the item's source vref.

### Per-disk categories + `[N]` label (composed at display time)
`load_index` reads each volume's `metadata/index.jsonl` and **appends** its
`CatRef`s to the model, tagging each resulting `ModelCat` with its volume-table
index. Categories are ordered **grouped by disk** (boot first; taxonomy order
within each), so the browse list reads as contiguous per-disk sections and the boot
disk's Recommended stays the landing view. The `[N]` token is **composed at render
time** from the slot's volume index (only when `VolTable.n > 1`) — never stored in
`ModelCat.name` (32 B), so it can't overflow and doesn't perturb slug/name matching
or a persisted selection. It coexists with the sub-page suffix: `Action & Arcade (2) [1]`.

### MacAtrium Status (the legend)
New Quick-Launch entry: `MROW_STATUS` (`src/ui.c:36` enum, assembled at `src/ui.c:175`)
→ `UI_SHOW_STATUS` (`src/ui.h` `UiCommand`) → `run_status_dialog()` in `main.c`
(movable-modal + focus-ring, like `run_os_chooser`). Shows MacAtrium version, OS
version, CPU tier, screen depth; then one row per mounted library disk —
`Disk N — <volume name> — <c> categories, <t> titles`. Later: an
absent-but-remembered disk line; optional SCSI ID / free space.

### Selection restore (no persisted volume identity)
Prefs store just `{category name, item id}` — no disk id. `model_select` resolves an
ambiguous name to the **boot disk's** copy (volumes are boot-first), and falls back to
**Recommended** when the saved category is absent (its disk removed). The `[N]` label
and the Status name come from the live volume table (the HFS volume name); nothing
volume-specific is persisted, so no `volume.jsonl` is required.

## What it touches

- **New:** `macfs_make_spec_on`, `macfs_volumes` + `VolTable` (`macfs.c/.h`);
  `run_status_dialog` (`main.c`); `UI_SHOW_STATUS` / `MROW_STATUS` (`ui.h`/`ui.c`);
  a display-label helper.
- **Changed:** `load_index` / `load_page` (`main.c`) — per-volume read + slot
  tagging; `do_launch`, `art_load` — resolve on the page's volume;
  `MODEL_MAX_CATS` bump (~128) + a disk-count cap; `save_prefs` / `model_select` —
  stableId.
- **Unchanged (correctly boot-only):** `prefs.c` (`MacAtrium Prefs`), `sound.c`
  (`sounds/*`), `bless.c` (System Folders); `catalog_parse_into` (pure).
- **Host (Phase 4, optional):** `run_paged` / `image.rs` emit `metadata/volume.jsonl`.

## Runtime flow (68k, at startup)

1. `macfs_volumes` → volume table (volumes with `/MacAtrium/metadata`), boot at 0.
2. For each, read `index.jsonl`; append its categories to the model, tagged and
   grouped by disk.
3. Browse: a category page loads `cats/<slug>.jsonl` from the slot's volume; the
   resident page records that volume.
4. Art + launch resolve on the page's volume. `[N]` shown when >1 disk; the Status
   screen decodes it.

## Risks (updated)

1. ~~RAM / aggregation OOM~~ — **dissolved** by sub-page-per-slot (above).
2. **Volume identity ≠ vRefNum** — sidestepped: nothing volume-specific is persisted.
   `[N]` is live-only; selection restore assumes boot-first + a Recommended fallback.
3. **Category-count growth** — namespacing multiplies index entries by disk count
   against `MODEL_MAX_CATS = 65`. Bump to ~128 (~140 KB BSS, fine on the Mac II /
   Quadra editions that would carry several SCSI disks); cap aggregated disks (~6);
   the 4 MB B&W compact aggregates fewer. Optional slim: a paged `ModelCat.idx[]`
   needs only 128, not `MAX_ITEMS` (256).
4. **Absent remembered disk** — fixed-SCSI makes mid-session loss a non-issue; a
   disk simply not mounted this boot is skipped; a saved selection falls back (first
   row) unless its stableId matches a live volume.
5. **Boot-scan latency** — small (a few `PBHGetVInfo` + N tiny `index.jsonl` reads);
   log/measure on a multi-SCSI machine.
6. **Prefs/hotkeys cross-disk** — saved selection carries stableId (Phase 3);
   `hotkeys.jsonl` is per-disk, resolved to `(volume, app)`.

## Phases

1. **Volume-aware plumbing** — `macfs_make_spec_on` + `macfs_volumes` / `VolTable`;
   route art/launch/loaders through `_on`. Boot volume still the only library. Pure,
   Snow-testable; no behaviour change.
2. **Two-disk aggregate + Status v1** — concat disk 2's index, tag slots, per-volume
   `load_page`, page-volume for art/launch, the `[N]` label, and the MacAtrium
   Status screen. Verify boot+1 on a 2-SCSI Snow harness.
3. **N-disk + polish** — generalize the concat, bump/cap `MODEL_MAX_CATS`,
   absent-disk grace, stableId in prefs for precise selection restore.
4. ~~**Host `volume.jsonl`**~~ — **dropped (2026-07-08).** Selection restore assumes
   the boot disk + a Recommended fallback, so no persisted volume identity is needed;
   Status uses the live HFS volume name.

## Verify

Snow boots a Mac II with SCSI disks (harness recipe: docs/36). Attach a second SCSI
image carrying its own `/MacAtrium`, boot, and confirm both disks' categories appear
(`[0]` / `[1]`), each title launches and loads art from its own volume, and the
Status screen lists both disks. QMP screendump for headless capture (memory
`q800-qemu-windows-verify`).
