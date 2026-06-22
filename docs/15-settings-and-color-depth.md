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

## Known limitation — 8-bit / 24-bit deferred (capped at 4-bit)

The picker is **capped at 4-bit**. 1/2/4-bit are flawless (colour verified at
4-bit); **every** ≥8-bit rendering approach tried either blanks or crashes, and
the *only* variable is the screen depth. The hardware/card itself isn't the
limit — the MDC 8•24 does 1/2/4/**8** (indexed) + **24** (direct); **16-bit**
("thousands") isn't on this card at all.

### What was tried (2026-06-21), all at an 8-bit screen

| Off-screen GWorld | blit destination | result |
|---|---|---|
| 8-bit, default CLUT | window PixMap | blank |
| 8-bit, default CLUT | live device PixMap | blank |
| 8-bit, **screen's** CLUT | either | **crash** |
| **4-bit** (valid; renders at a 4-bit screen) | live device PixMap | blank |
| 4-bit (valid) | window PixMap | **crash** |
| direct draw (no GWorld) | screen | **crash** |

Key isolation: a 4-bit GWorld that renders perfectly on a 4-bit *screen* goes
**blank** when the identical blit targets an 8-bit *screen*. So the source is
valid and the defect is on the 8-bit screen side (the displayed framebuffer the
driver/QuickDraw writes to ≠ what the emulator scans out).

### Two signatures, both pointing at the emulator at ≥8-bit

1. **Blank** — `CopyBits` to the 8-bit screen lands in a VRAM region the card
   isn't displaying. Snow's `core/src/mac/nubus/mdc12.rs` even carries an admitted
   `// not sure why this is off by 2 scanlines` fudge in its framebuffer-base
   maths, so its MDC base handling is known-imperfect at higher depths.
2. **Crash** — *deterministic*: every time the **same** instruction (`RTD` at
   `PC 0x0001CDB6`) returns to the **same** garbage PC (`0x11111129`), halting
   with *"I-cache enabled but PC unaligned"*. Random stack corruption would vary;
   an identical address every run smells like a Snow 68020 I-cache emulation edge
   triggered by the 8-bit colour-draw path.

### Real fixes found along the way (not yet shipped)

- The default app partition is **1 MB** (Retro68APPL.r) — too small for 8-bit
  colour (256-entry inverse tables, DrawPicture buffers). A `SIZE (-1)` override
  to 4 MB removed several of the crashes. (Reverted with the rest; 2 MB minimum is
  risky for low-RAM B&W Macs, so a shipped bump needs `min` left at 1 MB.)
- **Clamp the off-screen GWorld to ≤4-bit** regardless of screen depth — the UI
  needs only a few colours and a 4-bit GWorld depth-promotes onto a deeper screen.
  Good idea, but doesn't help while the 8-bit *screen* blit itself is broken.

### Conclusion / next step

The 8/24-bit blockers are in **Snow's MDC framebuffer-base + 68020 I-cache
emulation at ≥8-bit**, not the launcher. Needs Snow-side tracing (the displayed
`base`/`stride` vs `gdPMap.baseAddr` at 8-bit; the deterministic RTD→`0x11111129`
I-cache halt) or a real-hardware / other-emulator check. The 4-bit cap keeps the
appliance safe; a higher *boot* depth only blanks, never (in the shipped path)
hangs.

## Note: theme/volume aren't persisted

Both reset on reboot. Persisting them needs a prefs file; guest disk *writes*
work (verified — see docs/13 §6 correction) but the headless harness doesn't sync
them back to the `.hda`, so cross-boot persistence can't be verified here yet.
