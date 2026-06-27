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

> **Note:** the mockup above is the original *list* sketch. The shipped build
> renders a horizontal **carousel** of item icons with a detail/art pane below, so
> the axes are the reverse of that sketch — **← / → move items, ↑ / ↓ change
> category** (the on-screen hint reads `← → game   ^v category`).

| Input | Action |
|-------|--------|
| **← / →** | Move the selection through items in the current category page |
| **↑ / ↓** | Change **category** — loads that category's page on demand (see *Category paging*) |
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

**Artwork (box art vs. screenshot).** `atrium image` bakes two artworks per title
— Box-Front (`image`) and a gameplay Screenshot (`shot`) — at every depth. The
**Artwork** row in the Settings panel (Theme / Color Depth / Volume / **Artwork**)
chooses which the inline pane, More Info card, and `P` preview show; the choice
persists in prefs. Each falls back to the other when only one exists.

### Category paging

Categories are the spine of navigation, and the catalog is **paged by category**
so a big library stays within a 4 MB Mac's RAM (the full design + RAM math live in
[21-category-paging.md](21-category-paging.md); the on-disk format in
[06](06-content-pipeline.md)). What that means for the UI:

- The launcher boots holding only the **category index** (names + counts) and the
  **first** category's items — it **lands on Recommended**, the curated default.
  There is no "All" view (it would be the whole library — too big for one page).
- **↑ / ↓ change category.** Each change loads that category's page from disk
  (`cats/<slug>.jsonl`) on demand, showing a brief **"Loading <category>…"**
  notice; only the current page is ever resident. So flipping categories is how you
  move through the library, and the set of categories comes from the editable
  **category DB** (a title can appear in several — Bolo is in *Action & Arcade*,
  *Strategy & Sim*, *Color*, and *Recommended* at once).
- A category bigger than the per-page cap (128) is split by the build into
  numbered **sub-pages** ("Action & Arcade (2)"), each just another category in the
  ↑/↓ list — so "page a long category" *is* moving to its next sub-page.
- **← / →** move within the loaded page; **Page Up / Page Down** jump a screenful;
  **letter keys** type-ahead within the page. Keep **one consistent "back"
  affordance** (Esc) so there's never a dead end.

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
