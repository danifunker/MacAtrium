# 44 — Memory budget & the art-capability set — implementation plan

**Status: PLAN — ready to implement.** Opened 2026-07-09. A single **multi-OS binary**
(docs/36 image forks; docs/37 multi-disk) must size its own memory and pick art depth **at
runtime**, from a 4 MB 1-bit compact to a 32 MB 24-bit Quadra. This is the trackable plan —
tick the boxes as phases land; fill the numbers table as P1 produces measurements.

## Progress

- [ ] **P1 — ArtCaps probe + Status readout** — measurement only, no behaviour change; yields the real numbers for P3
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

### P1 — ArtCaps probe + Status readout  — [ ]
**Goal:** pure measurement; produce the capability set and the real per-depth numbers for P3. No
art behaviour change.
- [ ] Add `ArtCaps` struct + `art_caps_probe(ArtCaps *out, const Env *e)` — new `src/artcaps.{c,h}`
  (or fields on `Env`). Query the **granted partition**: `GetProcessInformation` →
  `processSize`/`processFreeMem` on 7.x; `FreeMem()`/`MaxBlock()` on 6.x (mirror `mem.c`). Compute
  `artBudget`; VRAM via `display_depths()`/`HasDepth`; fill `enabled[]`, `maxAffordableDepth`,
  `defaultMode`.
- [ ] Call it in `main()` after `env_probe` + display setup.
- [ ] Surface it in `run_status_dialog` (main.c): partition KB, artBudget KB, enabled modes,
  maxAffordableDepth.
- [ ] **Verify:** build clean + host tests green; in the harness patch the partition small then
  large (`atrium size --pref/--min`) and confirm Status reports the right partition/budget/enabled
  set each time; cross-check against the `-DMEM_DEBUG=ON` peak overlay and **record the numbers in
  the table below**.
- **Done when:** Status shows a correct capability set on both a small and a large partition, with
  no change to what art is drawn.

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

## Numbers to fill in (P1 output → P3 input)

| Target | System | Screen | Partition granted | Peak used | Peak art PICT | Note |
|---|---|---|---|---|---|---|
| Compact | 6.0.8 | 1-bit | | | | bare Sys6: GWorld in-partition |
| LC / II | 7.1 | 8-bit | | | | |
| Quadra | 7.5.5 | 24-bit | | | | Snow=8-bit; use q800 / real HW for 24-bit |

→ **Chosen multi-OS `'SIZE'`: minimum \_\_\_ KB / preferred \_\_\_ KB**

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
