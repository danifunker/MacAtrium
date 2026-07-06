# 37 — Multi-disk libraries (aggregate independent `/MacAtrium` volumes at startup)

**Status: SCOPING.** Requested 2026-07-06. Scope + risk analysis; not started.

## What it is

Today MacAtrium reads **one** `/MacAtrium` tree, on the boot volume. This epic: at
launcher startup, **enumerate every mounted HFS volume that carries its own
self-contained `/MacAtrium` library** (its own `metadata/` + `Apps/` + `images/`),
and present them as **one merged library** — each item launched, and its art loaded,
from **the disk it lives on**.

Think "plug in several curated collection disks and see them all at once," each disk
independently built by `atrium image`. Read-only aggregation; **startup-time only**
(the request: "connected when MacAtrium starts up" — no hot-plug mid-session).

### Relationship to [23-multi-volume-library.md](23-multi-volume-library.md)
docs/23 is the **complementary** model: **one** library whose metadata stays central
(boot disk) and points *outward* to data volumes, to beat the 2 GB boot cap. This
epic is **N independent** libraries merged at runtime. They can share one mechanism —
**a per-record volume tag + volume-aware path resolution** — so build both on the
same primitive rather than twice. (Decision below.)

## The three single-volume assumptions to break (grounded in the code)

1. **`macfs.c` is boot-volume-only.** `gBootVRef` is a single cached vRefNum;
   `macfs_make_spec(rel)` always walks `/MacAtrium/<rel>` on it. → Need
   `macfs_make_spec_on(vref, rel, spec)`; the boot-only version becomes a wrapper.
2. **`CatItem` has no volume.** `app`/`image`/`shot`/`icon` are `/MacAtrium`-relative
   with no owning volume. → Add a volume tag (small index into a volume table), set
   at page-load from *which disk's file the record came from*.
3. **The model is RAM-paged on purpose.** `MAX_CAT_ITEMS=128`, one resident page,
   `MODEL_MAX_CATS=65`. Aggregation must **stay within these bounds** — it cannot just
   concatenate N disks' catalogs. This is the crux (Risk 1).

## What it touches

- **Volume discovery (new, `macfs`/`env`):** walk mounted volumes via `PBHGetVInfo`
  with `ioVolIndex = 1,2,3,…` until `nsvErr`; for each, test for `/MacAtrium/metadata`
  (`dir_id_of`). Build a resident **volume table**: `{stableId, name, vRefNum}` per
  library disk. Boot volume included.
- **Volume-aware `macfs`:** `macfs_make_spec_on(vref, rel, spec)`; art/catalog/launch
  all take the item's source vRefNum.
- **Catalog + model aggregation:** merge each disk's `index.jsonl` category list
  (by name, summing counts); when a category page loads, read that category's
  `cats/<slug>.jsonl` from **every** disk that has it and concatenate into the one
  resident page, **tagging each `CatItem` with its source volume**. Respect
  `MAX_CAT_ITEMS` (Risk 1).
- **Launch (`launch.c`):** `launch_app_on(vref, appRel, …)` — resolve + `ResolveAliasFile`
  + `LaunchApplication` on the item's volume (classic apps run from any mounted volume
  under 7.x). Handle a source volume that's gone (grey out / notice).
- **Host (`atrium`):** minimal — each disk is already a normal MacAtrium build. Add:
  **stamp a stable volume id + display name** into each disk's metadata (a
  `metadata/volume.jsonl`), and a way to build a coherent *set* (avoid id/category
  collisions). No cross-disk pathing needed in this model.

## Runtime flow (68k, at startup)

1. Enumerate mounted volumes → volume table (those with `/MacAtrium/metadata`).
2. For each, read its `index.jsonl` → per-disk category lists.
3. **Merge** into the resident aggregated index (category name → set of
   `(volume, slug, count)`); display summed counts.
4. On category-page load: read `cats/<slug>.jsonl` from each contributing disk,
   concatenate (capped), tag each item's volume. Everything downstream
   (`art_load`, `launch`) uses the tag.

## Highest-risk items (ranked)

1. **RAM vs. aggregation — the crux.** The paged design exists so a 4 MB Mac never
   holds more than one 128-item page. Merging a category across N disks can blow past
   `MAX_CAT_ITEMS` and OOM-crash the target. *Mitigation:* treat each disk's
   `(category, sub-page)` as its own page — never hold more than one disk's slice of a
   category at once — and page *within* the merged category. The resident volume table
   + merged index must also fit (watch `MODEL_MAX_CATS=65` across N disks).
2. **Volume identity is not the vRefNum.** vRefNums are assigned at mount and change
   across boots / mount order; two disks can share a name ("Untitled"). Tagging records
   with a raw vRefNum → items resolve to the *wrong* disk after a reorder. *Mitigation:*
   a **host-stamped stable id** in `metadata/volume.jsonl`, matched to live volumes at
   startup, with the **HFS volume creation date** (`ioVCrDate` from `PBHGetVInfo`, set at
   format, rename-proof) as the fallback identity. Never persist a vRefNum.
3. **`CatItem`/parser changes + memory growth.** Adding volume attribution touches the
   record struct (128 KB/page today) and the loader (attribution happens at load, not
   in the pure-C `catalog_parse_into` — keep the parser volume-agnostic; tag in the
   model/loader). A per-page parallel `u8 vol[MAX_CAT_ITEMS]` is cheaper than widening
   `CatItem`.
4. **Merge semantics / dedup.** Same `id` on two disks; same category on several disks;
   Recommended ordering *across* disks; per-disk count display. Needs explicit rules
   (dedup by id? keep both? disk-priority order?).
5. **Cross-volume aliases → "Where is X?" dialog.** `launch.c` already resolves aliases
   (7.x). A `/MacAtrium/Apps/foo` alias whose target is on an absent/ejected disk fires
   the **blocking Alias Manager search dialog** — bad in a kiosk launcher. *Mitigation:*
   prefer direct per-volume paths; if aliases are used, pre-check the target with a
   no-UI resolve and grey out on miss.
6. **Removable / ejectable media.** A `/MacAtrium` CD-ROM or a disk ejected mid-session:
   launching an app whose disk is gone → error or eject-swap prompts. *Decision:* fixed
   SCSI HDs only (v1), or include removables?
7. **Boot-scan latency.** Scanning every mounted volume + reading N `index.jsonl` at
   startup adds boot time on a slow Mac with several SCSI disks (bounded — indexes are
   small; still, log/measure).
8. **Prefs/hotkeys cross-disk.** Hotkeys + "recently played" (boot-volume prefs) must
   store the volume tag too, or they resolve to the wrong disk. 6.0.8: volume
   enumeration works, but the Alias Manager doesn't (direct paths only there).

## Decisions needed
- **Unify with docs/23** (one volume-tag primitive for both) or keep separate?
- **Identity:** host-stamped id + creation-date fallback (recommended) vs. volume name?
- **Dedup:** same `id` on two disks — dedup (which wins?) or show both?
- **Media:** fixed SCSI only, or removables too?
- **Merge display:** categories merged silently, or shown per-disk ("Action · Disk 2")?

## Suggested phasing
1. **Volume-aware `macfs`** (`macfs_make_spec_on`) + volume table enumeration — no UI
   change; boot volume still the only library. Pure plumbing, testable.
2. **Two-disk aggregation** (boot + one data disk) with the volume tag through
   catalog→model→art→launch; verify in Snow with a 2-SCSI harness config.
3. **N-disk merge semantics** (dedup, ordering, counts) + absent-volume UX.
4. **Host** `volume.jsonl` stamping + multi-disk set builder.

## Verify
Snow harness already boots a Mac II with SCSI disks; attach a second SCSI image
(HD20SC-style) each carrying its own `/MacAtrium`, boot, confirm both disks' items
appear and each launches/loads art from its own volume. (Harness recipe: docs/36.)
