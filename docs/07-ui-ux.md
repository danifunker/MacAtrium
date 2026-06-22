# 07 — UI & UX Design

Authentic Mac feel, legible from a couch, operable from a tiny key set. "Be
creative" was the brief — this is a proposed direction, not a locked pixel spec.

## Visual language

- **Font:** **Chicago** (the classic system font) for titles, list rows, and
  chrome — the single biggest "this is a Mac" signal. Use `TextFont(systemFont)`.
  Chicago is a 12pt bitmap; at higher resolutions we keep it crisp by scaling
  *layout* (more rows, bigger margins) rather than smearing the glyphs. For
  large headers consider a second bitmap face; default everything to Chicago.
- **Chrome:** classic Mac cues — 1px black frames, the double-line "default
  button" ring on the focused item, drop-shadow rectangles, a title bar treatment
  reminiscent of System 7. Think "a Finder window that took over the screen."
- **Palette ("Mac" default, themable):**
  - **B&W backend:** black on white, selection = inverted (classic `InvertRect`
    highlight). Patterns (50% gray) for fills where color would carry meaning.
  - **Color backend:** a restrained default — System-7 platinum-ish grays, a
    blue accent for the selected row (echoing the classic selection blue), black
    text. All colors come from `theme` so users can re-skin (see Theming).
- **Layout (computed from screen rect, see [02](02-compatibility.md)):**

```
┌───────────────────────────────────────────────┐
│  MacAtrium            Games            14 items│  ← header: title · category · count
├───────────────────────────────────────────────┤
│   Dark Castle                            1986  │
│ ▶ Lemmings                               1991  │  ← selected row (highlight)
│   Prince of Persia                       1990  │
│   Shufflepuck Café                       1989  │
│   …                                            │
│                                                │
│   Lemmings — guide the lemmings to safety.     │  ← optional detail line for selection
├───────────────────────────────────────────────┤
│  ◀ ▶ category   ↑ ↓ select   ⏎ launch   ⎋ menu │  ← hint bar
└───────────────────────────────────────────────┘
```

At 512-wide we drop the year column and detail line; at 1024×768 we can show
two columns or a side detail/art pane.

## Navigation model (locked key set)

The control surface is intentionally tiny so MiSTer joystick→key mapping covers
it and a real keyboard feels obvious:

| Input | Action |
|-------|--------|
| **↑ / ↓** | Move selection within the current category |
| **← / →** | Switch category (and/or page a long list — see below) |
| **Return / Enter** | Launch the selected item |
| **Esc** | Open/close the top-level menu (Show Finder, Restart, Shut Down) |
| **I** | **More Info** card for the selection (Return launches; any other key returns) |
| **P** | Full-screen art preview |
| **T** | Toggle Dark / Light theme |
| **Cmd-Option-Q** | **Quit** the launcher entirely, returning exclusively to the Finder (kiosk-style hidden shortcut) |
| **Page Up / Page Down** | Page through a long list |
| Letter keys | Type-ahead jump to matching item (`T`/`P`/`I` are reserved) |
| Mouse click / double-click | Select / launch (never required) |

**Show Finder vs. Quit.** *Show Finder* (Esc menu) brings the resident Finder to
the front while MacAtrium keeps running underneath (and restores the menu bar so
the Finder is usable). *Cmd-Option-Q* fully **quits** the launcher (`ExitToShell`)
so the Finder becomes the sole shell — matched on the virtual key code (not the
char, which Option mangles).

**Detail line & More Info.** On wide screens the bottom of the list shows two
lines for the selection: a meta line (`year - developer - genre`) and the blurb
(`desc`). **I** opens a full **More Info** card — title, year/developer, genre,
the word-wrapped description, and the box art shown large. These read the new
display fields the catalog now carries (`vendor`, `genre` string; see
[06-content-pipeline.md](06-content-pipeline.md)) in addition to `desc`/`image`.
Additional artwork beyond the box front (e.g. screenshots) is a follow-up.

Left/Right doubles as category switch and, for very long single categories,
paging — final mapping to settle during implementation. Keep **one consistent
"back" affordance** (Esc) so there's never a dead end.

### The top-level menu (Esc)

A small centered panel:

```
        ┌────────────────────────┐
        │  Show Finder           │
        │  Restart               │
        │  Shut Down             │
        │  ── Settings ──        │
        │  Monitors              │
        │  Sound                 │
        │  Date & Time           │
        │  Mouse / Keyboard      │
        └────────────────────────┘
```

Same navigation rules (↑↓ + Return). System-settings entries that can't be
launched cleanly are visually **flagged** (see [08](08-launching-system.md)).

## Rendering backends

One backend chosen at startup from `env` (see [03](03-architecture.md)):

- **`render_qd` (B&W):** classic QuickDraw only — `FrameRect`, `PaintRect` with
  patterns, `InvertRect` for selection, `DrawString` in Chicago. Off-screen
  `BitMap` for flicker-free redraw, `CopyBits` to the window.
- **`render_cqd` (Color):** Color QuickDraw — `RGBForeColor`, filled/framed
  rects, optional `DrawPicture` for art. Compose in a `GWorld`, `CopyBits` out.
  Honors 16/256/thousands; at 16-color, snap theme colors to a safe set.

The `ui` layer calls a backend-agnostic API (`clear`, `fillRect`, `frameRect`,
`text`, `highlight`, `picture`) and never branches on depth itself.

## Theming

- A `theme` struct: background, panel, text, selection, accent, hint-bar colors,
  plus header/title font choices.
- **Defaults** baked into a resource; **overrides** loaded from a simple config
  on the volume (alongside the catalog — see [01](01-decisions.md)). For MVP a
  couple of presets (e.g., "Platinum", "High Contrast", "Green Phosphor") prove
  the system; full user editing can come later.
- On the B&W backend, theme collapses to patterns + invert; color fields are
  ignored gracefully.

## MiSTer / controller support

The MiSTer Mac cores deliver input to the Mac as **keyboard + mouse**. We do
**not** read a gamepad directly. Instead:

- Keep the whole UI driveable from **arrows + Return + Esc + Page**, so a MiSTer
  joystick→keyboard mapping (D-pad→arrows, A→Return, B→Esc) makes a controller
  "just work".
- Document a **recommended MiSTer key map** in the repo for users.
- Avoid requiring chorded/modifier keys or precise mouse for any core action.
- 🔬 Confirm how each target core surfaces buttons (some map a stick to the
  mouse); adjust the recommended mapping per core.

## Accessibility / legibility

- High-contrast default; large-enough rows at every resolution.
- Never rely on color alone to convey state (selection is also positional +
  framed), so the B&W backend loses nothing essential.
