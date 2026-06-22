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

## Known limitation — 8-bit deferred (capped at 4-bit)

The picker is **capped at 4-bit** for now. At 8-bit the colour path has two
distinct defects, both 8-bit-specific (1/2/4-bit are clean):

1. **Off-screen GWorld blanks.** A 256-colour GWorld created with the default
   colour table doesn't line up with the screen's CLUT, so the blit comes out
   white. Giving the GWorld the screen's `pmTable` instead made it **hang**.
2. **Direct draw hangs.** Bypassing the GWorld to draw straight to the 8-bit
   screen hangs on the inline art's `DrawPicture` of the `.8.pict` — the same
   DrawPicture-to-screen unreliability documented in docs/14.

A proper 8-bit path needs: art via CopyBits of a colour raw bitmap (extend the
docs/14 `.raw` format to 8-bit + CLUT, never DrawPicture) **and** a GWorld whose
colour table matches the screen without the hang. Until then the cap keeps the
appliance safe (16-colour is real colour and fully verified). The device may
still *boot* at a higher depth; that path only blanks (offscreen), never hangs.

## Note: theme/volume aren't persisted

Both reset on reboot. Persisting them needs a prefs file; guest disk *writes*
work (verified — see docs/13 §6 correction) but the headless harness doesn't sync
them back to the `.hda`, so cross-boot persistence can't be verified here yet.
