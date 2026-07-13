# docs/46 — Multi-depth (colour) tile icons — DONE

## Goal (achieved)
The browse-view **tile icons** (carousel + grid) now render in **colour** (8-bit) on a
colour screen, the way the box/screenshot **covers** already do. Previously the tiles were
1-bit black-and-white even on a colour screen — the last 1-bit thing in the UI. This was
mostly plumbing; the hard part (extracting the app's colour icon) was already written.

## How it works (the colour path, mirroring the covers)
1. **Extraction** — `tools/atrium-tool/src/icons.rs` pulls the app's **`icl8`** (32×32 8-bit
   colour Finder icon) from its resource fork — resolved the proper Finder way
   (`BNDL` → `FREF`(APPL) → that icon id, lowest-id fallback) — and decodes it through
   `mac_palette()` (the standard Mac 256-colour system table) to a 32×32 PNG
   (`app_icl8_png` → `write_icl8_png`/`icl8_rgba`).
2. **CLI** — `atrium icon --hqx X [--out RAW] [--png PNG]` (`main.rs`). `--out` writes the
   1-bit `ICN#` raw (as before); `--png` writes the colour `icl8` PNG. At least one is
   required; each is written only when the app has that icon, so a build can ask for both
   and fall back. Older apps have `ICN#` but no `icl8`.
3. **Games build** — `build_final.py` icon step: per app, `atrium icon --png … --out …`,
   then **prefer** the colour path — bake the PNG to a per-item resource fork with
   `atrium pict-rsrc --input <id>.icon.png --out <id>.icon.rsrc --depths 1,8 --max 32`
   (a 1-bit `ABMP` id 129 + an 8-bit `PICT` id 136), `rb-cli put` (empty data fork) +
   `setrsrc`, and set catalog `icon = images/<id>.icon.rsrc`. If there's no `icl8`, fall
   back to today's 1-bit `<id>.icon.raw` with catalog `icon = images/<id>.icon`.
4. **Launcher — no change.** `ui.c`'s `load_item_art` already dispatches a `.rsrc` icon path
   to `art_load_rsrc` (the same colour PICT loader the covers use: `art_rsrc_order` picks the
   deepest affordable ≤ screen depth, ids 132/136/144/152 + `ABMP` 129). The carousel tile
   (`draw_tile` → `row_icon` → `load_item_art`) and grid cell feed the resulting `Art` to
   `art_draw_fit`, which is depth-agnostic — a colour PICT `Art` blits in colour. The 1-bit
   fallback still resolves via the `<base>.raw` path.

Icons cap at 8-bit — the `ICN#`/`icl4`/`icl8` family has no 16/24-bit member — so the fork is
`--depths 1,8` (not `1,8,24`).

## Verify (done, 2026-07-13)
- `cargo test --lib` — 103 pass, incl. 5 new `icons.rs` tests (palette map, `icl8` BNDL vs
  lowest-id resolution, PNG round-trip to disk, no-`icl8` no-op).
- CLI on real staged apps: Apeiron → 32×32 colour PNG (9 colours); `pict-rsrc` → valid colour
  `.icon.rsrc`. Sweep: **56/95** staged apps ship a colour `icl8`.
- `build_final.py 2000` → games disk baked **56 colour `.icon.rsrc` + 33 1-bit fallback +
  4 no-icon** (93 records); all 56 colour files on the volume; **0 downgrades** (every app
  with an `icl8` gets a colour icon); fsck clean at 2000 MiB.
- Booted `MacAtrium-MultiOS-Games.hda` in the Snow harness (Mac II, 8-bit), read the Carousel:
  tiles render in **colour** (pickaxes, planet, hand, Apeiron's purple `icl8`) with the 1-bit
  `ICN#` fallback (e.g. a B&W mask) coexisting for apps without `icl8`.

### HFS 31-char filename limit (bug found + fixed during verify)
An adversarial cross-check (which 1-bit-fallback apps *actually* have an `icl8`?) caught **14**
colour-capable titles — Monkey Island, Indiana Jones, Prince of Persia 2, Pathways into
Darkness, … — silently getting 1-bit icons. Cause: their `<id>.icon.rsrc` filename exceeds
**classic HFS's 31-char limit**, so `rb-cli put` fails and `bake()` returns None → 1-bit
fallback. The *same* limit had been silently dropping those long-id titles' **covers**
(`<id>.rsrc`) and screenshots too. Fix (`build_final.py`): `art_base(rid)` derives a ≤21-char
on-disk basename — the id when it fits, else a 13-char prefix + 6-hex id hash (0 collisions
across 93 records) — applied uniformly to box/screenshot/icon filenames *and* their catalog
paths (the launcher loads whatever path the catalog names, so the on-disk name is free to be
short). Result after the fix: colour icons 42→**56** (the 14 recovered), covers 26→**31**,
screenshots 61→**84**, 0 downgrades. Short-id titles are unchanged (zero regression).

## Files changed
- `tools/atrium-tool/src/main.rs` — `icon` subcommand gains `--png`; `--out` now optional.
- `tools/atrium-tool/src/icons.rs` — split `write_icl8_png`/`icl8_rgba` out of `app_icl8_png`
  (testable from a synthetic fork); 5 new tests.
- `C:\Temp\macatrium-build\scripts\build_final.py` — icon step prefers colour `.icon.rsrc`,
  1-bit `.icon.raw` fallback; parametrised `bake(depths, mx)`; colour-count in the report;
  `art_base(rid)` HFS-safe on-disk names for box/screenshot/icon (fixes the 31-char overflow).
- Launcher (`src/ui.c`, `src/art.c`): **unchanged** — the `.rsrc` colour path already existed.

## Notes / gotchas (retained)
- **2 GB disk boot limit.** Keep the games disk **≤ 2000 MiB** — the Mac II ROM / classic HFS
  won't boot a > 2 GB volume (black screen). Colour icon forks add ~1 KB each (negligible).
- Not every app has an `icl8` (older/simple apps have only `ICN#`) → 1-bit fallback; never
  drop the tile.
- **HFS 31-char filename limit.** On-disk art names must be ≤ 31 chars or `rb-cli put` fails
  and the art is silently dropped. Use short bases (`art_base`), not the raw id — long ids
  (Monkey Island, Prince of Persia 2, …) overflow with the `.icon.rsrc`/`.rsrc` suffix.
- Keep the launcher partition at **3072/1024** (docs/44 budget) so `maxAffordableDepth ≥ 8`
  and colour art (covers *and* icons) stays affordable.
- The **`atrium image`** appliance build (`image.rs::bake_icon`) does colour icons a different
  way — loose `<id>.icon.raw` + `<id>.icon.8.pict` depth variants under catalog base
  `<id>.icon` — which `load_item_art`'s variant loader also resolves. `build_final.py` uses the
  `.icon.rsrc` fork instead, matching how it bakes covers. Both render in colour.
