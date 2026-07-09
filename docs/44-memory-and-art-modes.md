# 44 — Memory budget & the art-capability set — implementation plan

**Status: PLAN — ready to implement.** Opened 2026-07-09. A single **multi-OS binary**
(docs/36 image forks; docs/37 multi-disk) must size its own memory and pick art depth **at
runtime**, from a 4 MB 1-bit compact to a 32 MB 24-bit Quadra. This is the trackable plan —
tick the boxes as phases land; fill the numbers table as P1 produces measurements.

## Progress

- [x] **P1 — ArtCaps probe + Status readout** — measurement only, no behaviour change; yields the real numbers for P3
- [ ] **P2 — Budget-aware art loader** — the one behavioural change (art degrades a tier instead of OOM)
- [ ] **P3 — Multi-OS `'SIZE'` numbers** — low-minimum / high-preferred partition
- [ ] **P4 — (optional) Art Quality setting** — user-facing cap, gated by ArtCaps

## Why (condensed)

The launcher declares one `'SIZE'` (-1) partition. Today memory is a **per-build** knob:
`atrium image` patches it from each config's `app_mem_kb: [pref, min]`, so B&W bakes 512/384 KB
and full-colour 3584/3072 KB. That only works because those are **separate** images. A multi-OS
image is one image → one `'SIZE'` → one `app_mem_kb`, but the same binary runs everywhere. Bake
it high → won't launch on the compact; bake it low → the colour case can't hold an 8/24-bit
PICT. So the launcher must **adapt at runtime** to the partition it was granted and the depths
the card can show.

## What already exists (reuse — don't reinvent)

- **Per-build `'SIZE'` patch:** `tools/atrium-tool/src/size_rsrc.rs` + `image.rs` bake
  `app_mem_kb`; `atrium size <bin> --pref <KB> --min <KB>` patches/reads it **without a rebuild**;
  `cmake -DMEM_DEBUG=ON` paints a live peak-partition overlay ([mem.c](src/mem.c)).
- **The big buffer is already off-partition.** The off-screen GWorld composites at screen depth
  but is allocated `useTempMem | noNewDevice` **first**, with a step-down ladder (screen → 8-bit
  → direct) under memory pressure ([render.c:70-113](src/render.c:70)). *This is the exact
  pattern the art loader should copy.*
- **The fork art loader + the OOM hole.** `art_load_rsrc(vref, rel, depth)` pulls a depth-matched
  variant from a per-item `.rsrc` — `PICT` id `128+bits` (132/136/144/152 = 4/8/16/24-bit) or the
  1-bit `ABMP` (129) ([art.h:31](src/art.h:31)). `art_rsrc_order` builds the try-order **exact →
  *deeper* → shallower** — i.e. it prefers the **largest** variant and has **no memory guard**.
  All variants are already on disk in the fork, so the loader can pick any tier at runtime.
- **VRAM gate already exists:** `display_depths()` ([display.c](src/display.c)) returns only the
  depths `HasDepth(mainDevice, d)` can display at the current resolution — the VRAM verdict, no
  raw-VRAM math. The Settings "Colour Depth" stepper already walks that set.
- **Art is size-bounded at build** by `max_art_size` (`config.rs`; e.g. `384x384`).

## The design (the model)

One **`ArtCaps`** set, computed once at startup. Per art mode `M ∈ {1-bit, 8-bit, 24-bit}`:

```
displayable = HasDepth(mainDevice, depth(M))    // VRAM   — can the card show it?
affordable  = artBudget >= peakArtBytes(M)      // memory — can the partition hold it?
enabled[M]  = displayable && affordable
maxAffordableDepth = deepest affordable depth
defaultMode        = highest enabled mode
```

- **Screen depth and art depth are separate axes.** The GWorld renders at *screen* depth in temp
  memory, so a deep screen with a small partition **keeps the deep screen and loads shallower
  art** — the memory gate caps the *art variant*, never the display.
- **Budget:** `artBudget = grantedPartition − code/globals − residentCatalogPage(~150 KB) −
  rowIconCache − gworldFallbackReserve(only when TempFreeMem≈0, i.e. bare Sys6)`.
- **Authoritative guard = per-resource on-disk size check** (needs no baked metadata):
  `SetResLoad(false)` → `Get1Resource` (handle only) → `GetResourceSizeOnDisk(h)` vs remaining
  budget → load if it fits, else `ReleaseResource` and drop a tier.

## The plan

### P1 — ArtCaps probe + Status readout  — [x] DONE (2026-07-09)
**Goal:** pure measurement; produce the capability set and the real per-depth numbers for P3. No
art behaviour change.
- [x] Add `ArtCaps` struct + `art_caps_probe(ArtCaps *out, const Env *e)` — new `src/artcaps.{c,h}`.
  Query the **granted partition**: `GetProcessInformation` → `processSize`/`processFreeMem` on 7.x;
  `ApplicationZone()` extent + `FreeMem()`/`MaxBlock()` on 6.x (mirror `mem.c`). Compute `artBudget`;
  VRAM via `display_depths()`/`HasDepth`; fill `enabled[]`, `maxAffordableDepth`, `defaultMode`. The
  pure derivation is split into `art_caps_derive()` (no Toolbox) so the gating logic is host-tested
  across the profiles Snow can't emulate (1-bit compact, 24-bit Quadra) — see `tests/host_test.c`
  (`-DARTCAPS_HOST_TEST`).
- [x] Call it in `main()` after `env_probe` + display setup.
- [x] Surface it in `run_status_dialog` (main.c): partition/free/blk, artBudget, tiers on/off,
  maxAffordableDepth, defaultMode, per-tier peak estimate.
- [x] **Verify:** build clean + host tests green (108/108, +17 artcaps checks); harness at two
  partitions (`atrium size --pref/--min` 1024 vs 3072) on the **archive-built** MacAtrium-7.1-256color
  image (real art loads) — Status reports the right partition/budget/enabled each time, cross-checked
  with the `-DMEM_DEBUG=ON` peak overlay. Numbers recorded below.
- **Done when:** Status shows a correct capability set on both a small and a large partition, with
  no change to what art is drawn. ✅ 1024K → `on/on/off` default-8; 3072K → `on/on/on` default-24.

### P2 — Budget-aware art loader  — [ ]
**Goal:** never load art bigger than the partition holds; degrade one tier instead of OOM.
- [ ] `art_rsrc_order(depth, out)` (art.c) → also take `maxAffordableDepth`; restrict the try-order
  to depths ≤ that ceiling (keep exact → deeper → shallower *within* the ceiling).
- [ ] `art_load_rsrc` (art.c): `SetResLoad(false)` before `Get1Resource`; if
  `GetResourceSizeOnDisk(h) > budgetRemaining` → `ReleaseResource`, try next-shallower; else
  `SetResLoad(true)` + `LoadResource` + detach. Restore `SetResLoad(true)` on every exit path.
- [ ] Caller `load_item_art` (ui.c): pass effective depth = `min(screenDepth, maxAffordableDepth)`.
- [ ] **Verify:** small-partition harness run auto-loads a shallower variant with **zero OOM**
  (MEM_DEBUG peak inside partition); large-partition run loads the deepest available; the
  absent-variant fallback still works (delete a tier from a test fork and confirm it drops down).
- **Done when:** the small-partition run degrades gracefully and the large-partition run shows the
  deepest tier, both crash-free.

### P3 — Multi-OS `'SIZE'` numbers  — [ ]
- [ ] Set `src/macatrium.r` SIZE to **low-minimum / high-preferred** from P1 (start ~512 KB / ~3 MB;
  confirm against measurements).
- [ ] Confirm single-OS builds can still override via `app_mem_kb` (`atrium image` / `atrium size`).
- [ ] **Verify:** the multi-OS image launches at `minimum` on the smallest target **and** gets the
  full budget (deepest art) on a large one.

### P4 — (optional) Art Quality setting  — [ ]
- [ ] Settings row "Art Quality: Auto / 1-bit / 256 / Millions"; grey tiers outside
  `ArtCaps.enabled[]` (`HiliteControl`, as the OS chooser does); persist in prefs; loader clamps to
  the user's cap.

## Numbers (P1 measured 2026-07-09 → P3 input)

Measured in the Snow harness (Mac II + MDC 8•24, System 7.1, **8-bit** screen) on the
**archive-built** `MacAtrium-7.1-256color` image (real box-art PICTs resident), plus on-disk art
sizes read from the `256color` (384² art bound) and `fullcolor` (720² default bound) images. Snow
tops out at 8-bit, so the compact-1-bit and Quadra-24-bit *display* rows still need the 6.0.8 harness
/ q800 / real HW — but their on-disk art sizes are measured here.

| Target | System | Screen | Partition granted | Peak used | Peak art PICT (on-disk) | Note |
|---|---|---|---|---|---|---|
| Compact | 6.0.8 | 1-bit | — (needs 6.0.8 HW) | — | **13K** ABMP @384² · 44K @720² | bare Sys6: GWorld in-partition |
| LC / II | 7.1 | 8-bit | **3088K** (set 3072)<br>**1040K** (set 1024) | **486K** no-art<br>**558K** +8-bit art | **80K** box @384²<br>259–318K @720² · Δresident≈72K | measured; fits 1024K w/ ~480K headroom |
| Quadra | 7.5.5 | 24-bit | — (Snow caps 8-bit) | — | **1.34 MB** box @720² fullcolor | 24-bit *display* needs q800/HW; size + est recorded |

→ **Chosen multi-OS `'SIZE'` (P1 recommendation, P3 confirms): minimum ~1024 KB / preferred ~3072 KB.**
512 KB OOMs the shared 8-bit case (peak 558K); 1024 KB holds it with headroom; 3072 KB holds a
1.34 MB 24-bit PICT (budget 2595K). The per-build `app_mem_kb` still overrides for single-OS images.

### P1 findings

- **Budget formula validated to the KB on hardware:** `artBudget = partitionFree − 198K`
  (150K catalog page + 48K row-icon cache; +GWorld reserve only when temp is scarce). Status showed
  `2793−198=2595K` @3088 and `745−198=547K` @1040 — exact.
- **Capability set adapts, both axes correct:** `on/on/on` default-24 @3072 vs `on/on/off` default-8
  @1024 — at 1024K the real 1.34 MB 24-bit PICT genuinely exceeds the 547K budget, so 24-bit art is
  gated by *memory* while the screen keeps its depth (the two axes stay separate).
- **The zone grows on demand:** at probe time `FreeMem()`/`MaxBlock()` reflect the not-yet-grown app
  zone (`blk≈3–6K`) while `processFreeMem` already reports the true partition free — so the budget is
  based on `processFreeMem`, never `MaxBlock`. (This is why the readout's `blk` looks tiny.)
- **Conservative estimate brackets reality:** the startup `peakArtBytes` (uncompressed 720² pixmap:
  506K / 1519K for 8 / 24-bit) is spot-on for 24-bit (~1.34 MB real) but 1.6× (720²) to 6× (384²) the
  compressed 8-bit PICT. It errs safe; **P2's per-resource on-disk size check is the authoritative
  gate**, and P3 sizes off the *measured* numbers above, not the estimate.
- **Art must be built by `atrium image`:** a metadata-only library (`index.jsonl`+`cats/`, no
  `images/`) shows "(no art)" — the art pass converts the MacGarden archive (`apps/`,`games/<id>/`)
  into depth PICTs in `images/`. P1 verified against the archive-built demo images.

## Risks & edge cases

1. **`peakArtBytes` must be honest** — count the resident PICT **plus** the row-icon cache and
   catalog page, not the PICT alone. The per-resource on-disk check is the backstop.
2. **Bare System 6 has no temp memory** → the GWorld falls into the partition (render.c ladder).
   But S6 ⇒ compact ⇒ shallow ⇒ small GWorld, so it stays consistent — just subtract a
   `gworldFallbackReserve` from `artBudget` when `TempFreeMem()` ≈ 0.
3. **Don't over-restrict** — a deep screen on a small partition keeps the deep screen and drops
   *art* depth; screen and art are separate axes.
4. **`maxAffordableDepth` is computed once**, but resident art is one-at-a-time, so a mid-session
   fragmentation dip is caught by the per-resource load check, not the startup estimate.

## Verify (recipe)

Build + harness per [docs/04-toolchain-build.md](04-toolchain-build.md). Measure with
`cmake -DMEM_DEBUG=ON` (on-screen peak overlay, [mem.c](src/mem.c)) and `atrium size <bin>
--pref <KB> --min <KB>` (patch the partition without a rebuild). Snow tops out at 8-bit, so
validate the 24-bit budget path on the q800 / QEMU harness or real hardware.
