# 15 — Settings panel, runtime colour depth, and volume

A `Settings` affordance reachable from the list screen, holding **Theme**,
**Color Depth**, and **Volume** — and, as a bonus, the colour render backend
(`render_cqd.c`) is finally **verified on a colour-depth screen** (it had only
ever run at 1-bit). Builds on the CopyBits art work (docs/14).

## UX

- A little 3-slider **gear** sits at the header's left. Pressing **Left** at the
  first category focuses it (a frame appears); **Return** opens the panel;
  **Right** unfocuses back to the categories.
- The **Settings panel**: `^v` move between rows, `<>` (or Return) change the
  selected row's value live, `Esc` returns to the list.
  - **Theme** — Dark / Light (`render_set_theme`).
  - **Color Depth** — cycles the screen depths the device supports; applies via
    `SetDepth` and re-fits rendering.
  - **Volume** — 0..7, the system alert volume (`SetSysBeepVolume`), beeps once
    at the new level. Shows `n/a` if the Sound Manager lacks SysBeepVolume.

## Code

- `src/display.{c,h}` — `GetMainDevice` + `HasDepth`/`SetDepth` + `gdPMap`
  (confirmed against SuperMario `QuickDraw/GDevice.a`: `gdDevType` bit 0 =
  mono/colour). `display_depths` enumerates {1,2,4,8,16,32} via `HasDepth`.
- `src/sound.{c,h}` — `Get/SetSysBeepVolume` (0..0x100) mapped to a 0..7 scale.
- `src/render.{c,h}` — `render_reset_for_depth` disposes the off-screen GWorld and
  reselects colour vs B&W; `render_end` now blits to a colour window's PixMap
  (not the overlapping old `portBits`). The GWorld is allocated from **temp
  (MultiFinder) memory** (`useTempMem`) — a 640×480×8 GWorld is ~300 KB and won't
  fit the default app partition.
- `src/main.c` — the window is a **colour window** (`NewCWindow`) when Color QD is
  present, so >1-bit blits work.
- `src/ui.{c,h}` — the gear, the `UI_MODE_SETTINGS` panel, Left-to-focus, and the
  row value logic.

## Verified in Snow (System 7.1, Mac II)

Boot → Left (gear) → Return (panel) → all three rows render; Volume reads 7/7
(SysBeepVolume works on 7.1). Changing **Color Depth** 1→2→4 switches the screen
live, and at **4-bit the colour backend renders** — cyan selection, the dark
theme in colour ([evidence/settings-color-4bit.png](evidence/settings-color-4bit.png);
1-bit panel: [evidence/settings-panel-1bit.png](evidence/settings-panel-1bit.png)).
This is the colour-backend verification deferred since docs/13 §5.

## Colour at every depth — root cause was our PICT encoder (RESOLVED)

All depths now render in colour — **1 / 2 / 4 / 8 (256) / 24-bit (Millions)** —
with depth-matched colour art via `DrawPicture`, runtime depth-switching, and no
crashes. Verified in Snow on a Mac II.

**The bug was ours, not Snow's, and not `DrawPicture`.** Our PICT encoder
violated a PICT-format rule: *"Because opcodes must be word-aligned in version 2
and extended version 2 pictures, a byte of 0 is added after odd-size data"*
(Imaging With QuickDraw, Appendix A). PackBitsRect pixel data is frequently
odd-length; we appended `OpEndPic` straight after it with **no pad byte**, so the
final opcode sat on an odd offset and `DrawPicture` mis-parsed it. Fix: pad the
picture data to even before `OpEndPic` (one `if` in `pict.rs::build_pict`, plus a
unit test that every depth yields even-length picture data).

This single one-byte-per-picture fault produced *every* symptom we'd chased and
mis-blamed on the emulator:
- the original **1-bit PICT crash** (docs/14 — worked around with the raw bitmap),
- the **4-bit blank**, the **8-bit blank/crash**, and
- the **"8-bit chrome blank" on a runtime depth switch** — the misaligned
  `.8.pict` `DrawPicture` was corrupting the shared off-screen GWorld, taking the
  whole frame (chrome included) down with it. With aligned PICTs it renders.

16-bit happened to be even-length already (DirectBitsRect, 2 bytes/pixel,
unpacked), which is why docs/13 once saw 16-bit "work" while indexed depths
didn't — a clue we initially mis-read.

Evidence: [evidence/color-art-4bit-pict.png](evidence/color-art-4bit-pict.png),
[evidence/color-art-8bit-pict.png](evidence/color-art-8bit-pict.png).

### Architecture (per the depth design)

- **Startup matches the OS depth** (`env_probe` → `render_init`); we never force a
  depth at launch. Set the screen via the Monitors control panel and the launcher
  follows.
- **Runtime change recalculates** (`render_reset_for_depth` rebuilds the GWorld +
  reselects colour/B&W; `apply_depth` updates `env`).
- **Art** is depth-matched: `<id>.1.raw` (raw bitmap, CopyBits) on a 1-bit screen,
  `<id>.<N>.pict` (colour PICT, DrawPicture) on colour screens. `atrium image`
  default `art_depths` is `["1","8"]` (1-bit raw + 256-colour PICT); the picker
  offers whatever the device reports via `HasDepth` (no artificial cap).
- The off-screen GWorld at 8-bit needs more heap than the **1 MB** default app
  partition; this works in current testing, but a `SIZE (-1)` bump (preferred
  4 MB, `min` left at 1 MB for low-RAM B&W Macs) is the safe follow-up if 8-bit
  ever runs short.
- **Colour fidelity.** The off-screen GWorld is created with the *screen's*
  colour table (`render.c` passes `gdPMap.pmTable` to `NewGWorld` at indexed
  depths) so theme colours map through one translation, not two — the GWorld
  default table mapped our greys washed-out at 8-bit and brown at 4-bit. Theme
  palettes (`render_cqd.c`) use neutral greys that land on the system grey ramp:
  a near-black dark theme and a platinum light theme, both with an azure
  selection ([evidence/theme-dark-8bit.png](evidence/theme-dark-8bit.png),
  [evidence/theme-light-8bit.png](evidence/theme-light-8bit.png)). Indexed
  palettes are coarse in the dark range, so the dark theme leans on grey frames
  for structure rather than subtle panel-fill differences.

## Note: theme/volume aren't persisted

Both reset on reboot. Persisting them needs a prefs file; guest disk *writes*
work (verified — see docs/13 §6 correction) but the headless harness doesn't sync
them back to the `.hda`, so cross-boot persistence can't be verified here yet.
