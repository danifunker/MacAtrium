# 33 — Resume: 256-colour redraw audit + carousel filmstrip + real Settings window

Paste into a fresh session to continue. **State: a big "make it more Mac-like"
batch shipped (9 commits, `01d333b`..`1886153`); the NEXT chunk is a redraw/UX
rework with two design decisions already made by the user.** Working tree CLEAN at
`1886153`. Supersedes the earlier resumes for the launcher UI work.

## 0. Environment (don't re-learn)
- Build LOCAL, in this order (the tool EMBEDS the launcher + the data via
  `include_bytes!`, so BOTH rebuilds are required or disks ship a stale launcher):
  1. `cmake --build build` (Retro68) → recompiles + re-embeds
     `tools/atrium-tool/assets/MacAtrium.bin`.
  2. `cargo build --release --manifest-path tools/atrium-tool/Cargo.toml` → the tool
     picks up the new `MacAtrium.bin` (and `data/*.jsonl`).
  3. `./tools/atrium-tool/target/release/atrium image --config /tmp/sample-{608,71,755}.json`
     → builds the disk. rb-cli = `/home/dani/repos/rusty-backup/target/release/rb-cli`.
- **Verify in Snow** (256-colour path): `~/repos/snow/target/release/macatrium_harness
  <MacIIFDHD.rom> /tmp/mdc/3410868.bin <disk> <out> <cycles> --snap-every N --keys "…"`.
  Keys: letters/arrows/`return`/`esc`; `click@X,Y`; `hold@X,Y,DUR` (auto-repeat);
  `drag@X1,Y1,X2,Y2` (press-glide-release, for column/divider drags). Window origin
  in GLOBAL coords = (1, 39) → framebuffer = (local.x+1, local.y+39). Build the
  harness with `--features snow_core/mmap` if you need `--pram` persistence (the
  current binary lacks it; fresh boots bootstrap colour fine). SHORT out dir
  (AF_UNIX 108-char limit). The harness `.rs` source of truth is
  `tools/snow-harness/macatrium_harness.rs` → copy into `~/repos/snow/testrunner/
  src/bin/` + `cargo build -r -p testrunner --bin macatrium_harness` to rebuild.
- **DISK NAMING TRAP (the user hit this):** the CURRENT disks are
  `/home/dani/MacAtrium-sample-{608-bw,71-color,755-quadra}.hda`. The `final-*` beta
  disks and the `color-*`/`views-demo`/`setup-test` test disks on /home/dani are OLD
  (pre-Geneva launcher) — verify with `strings <bootlauncher rsrc> | grep -c Geneva`
  (1 = new, 0 = old). Sample configs at `/tmp/sample-{608,71,755}.json`.
- Commit to main (memory `commit-directly-to-main`). Suspect our code, not Snow.

## 1. DONE this session (committed `01d333b`..`1886153`)
- `01d333b` Settings toggles to **hide the menu bar / title bar** (GrayRgn-reclaim +
  CalcVis dance is back, gated; `set_menu_bar_state`/`rebuild_window`/
  `restore_system_menu_bar` in main.c).
- `0516e2d` carousel footer = **Macintosh key-caps** (render_round_frame / render_arrow
  / render_return; Chicago has NO arrow/return/esc glyphs so they're drawn).
- `b0c6adf` **overlay/chooser flashing fixed** — overlays (menu/Settings/quit) blit
  ONLY their panel (`u->panelRect` + `render_end_rect`); Menu→Settings matches
  View→Settings (`overlay_panel_rect`+`rect_contains`); chooser repaints only changed
  rows; empty clicks don't redraw.
- `256bb41` **Geneva content font** + **Text Size Small/Medium/Large** (`render.c`
  contentFont/textSize; `ROW_H` is now runtime `g_rowH`; `ui_set_text_size`). Default
  Medium(10). Menus/title/controls stay Chicago.
- `db6c515` **two Icon Grid styles** (Finder / At Ease Tiles), `Settings > Grid Style`,
  `grid_name_2` 2-line wrap.
- `9d2622c` **sortable List columns** (Name/Type/Year, `model_sort_page`, ▲▼) +
  **aligned Settings values** (`draw_settings_row` splits label/value at col 16).
- `e07b1f2` **List name filter** (type to find; `model_filter_page`/`ci_substr`;
  intercept printables in the List view before t/p/i/hotkey/type-ahead; Esc clears).
- `da7894d` **click the title bar → menu** (inDrag opens the menu hub; menu access
  when the bar is hidden).
- `e2fc929` **resizable List columns** (drag the Name|Type divider, XOR feedback;
  `list_col_x`; harness gained `drag@`).
- `1886153` **no repaint before a depth-capped launch** (dropped the "Setting up the
  display" notice; `show_switch_message` removed). Verified Arkanoid → 1-bit clean.

All three sample disks rebuilt at each step. Host tests 88/88.

## 2. NEXT — the agreed batch (two user messages, design decisions MADE)
### Redraw audit (the user asked for it; findings)
Full-screen repaint triggers and verdicts:
- **Carousel ←/→ move = full `ui_draw` EVERY move** ← the remaining flashing. Cause:
  `browse_redraw` (ui.c ~2393) falls back to full draw because the carousel has no
  `draw_sel`, and it *can't* incrementally redraw while it **re-centers** the selection
  (every tile shifts). → fixed by the filmstrip rework (2.1).
- Startup **1-bit→8-bit depth bump posts an extra updateEvt → double-paint** = the
  "refreshes when the app first loads". Reducible (2.3).
- Category change / view / text-size / grid-style / theme → full repaint = NECESSARY
  (layout reflows).
- Menu/Settings open+nav, empty click, chooser nav → already panel-only / no-redraw
  (done in `b0c6adf`).
- Picture+text: the off-screen (256-colour) path ALREADY loads the cover synchronously
  in `draw_carousel` (`if (u->r->useOffscreen) ensure_art_loaded(u)`, ~line 622) — so
  it *should* be one pass. The user still reports a text-then-text+image double-pass;
  **instrument/confirm** during 2.1 (likely resolved by the filmstrip's incremental
  detail repaint, or a stray second `ui_draw`).

### 2.1 Carousel → "filmstrip + moving selection" (USER PICKED THIS)
Replace the centred-hero carousel with a **fixed horizontal strip of equal tiles**:
- Tiles at FIXED positions (no re-centring). The **selected** tile is highlighted (box).
- ←/→ move the selection by ONE; when it crosses the visible page edge, the strip
  **pages by a full screenful** (the user's "scroll by the full screen").
- Page model like the grid: `page = curItem / nTiles`; visible = `[page*nTiles,
  +nTiles)`; selected tile at `curItem % nTiles`.
- Add **`carousel_draw_sel`** (incremental): a move that stays on the page repaints
  only the old+new tile + the detail pane and blits that union (model on
  `iconview_draw_sel`); a page change falls back to full draw. This kills the per-move
  flash. Wire it into the `gCarouselView` vtable (the 6th slot, currently `0`).
- Detail pane (screenshot + meta + Launch) below, unchanged, loaded synchronously.
- Touch points: `CarLayout`/`carousel_layout` (fixed row + page scroll, drop the
  hero/side-tile + `nside` wrap math), `draw_carousel` (fixed tiles + highlight),
  `carousel_nav` (page logic), new `carousel_draw_sel`, `carousel_click` (hit-test the
  fixed tiles), `ui_paint_controls` carousel branch (pager value=page or curItem),
  `ui_scroll_step` carousel page size. Mid-read of `carousel_layout` (ui.c ~399) when
  this resume was written.

### 2.2 Settings → a REAL Mac window + keyboard nav (USER PICKED THIS)
Replace the custom overlay panel with a **real window** using live Toolbox controls
(checkboxes / popup menus / a Done push-button), AND add custom keyboard handling so
arrows move focus + Space/Return toggle the focused control — one UI that looks
standard and is still fully keyboard/gamepad-drivable. (Rejected: keep-panel+add-window
duplicate; restyle-panel-as-window.) This is a big rework of `UI_MODE_SETTINGS` /
`draw_settings` / `settings_adjust` + `ensure_controls`-style control creation.

### 2.3 ESC menu + Settings → key-caps to match the carousel
Apply the carousel key-cap style (`draw_keycap`/`draw_keyhints`, ui.c) to the hint
lines in the ESC menu + Settings (e.g. the `^v row  <> change  Esc back` line and the
menu nav hints), so the on-screen key hints are consistent.

### 2.4 Startup double-paint (the depth bump)
Collapse the boot 1-bit→8-bit transition so the first frame isn't painted twice (the
OS updateEvt). Risky reorder of the boot-depth logic in `main()` (set depth before the
window exists, or swallow the first self-induced updateEvt) — do carefully, verify the
colour bump still engages.

## 3. Recommended order
2.1 carousel filmstrip (biggest flash win) → confirm 2.0 picture/text along the way →
2.4 startup double-paint → 2.3 key-caps (quick) → 2.2 real Settings window (largest).

## Memory to read
`classic-ui-redesign-views`, `color-art-memory-budget`, `build-and-snow-are-local`,
`workflow-verify-in-emulator`, `overrides-db-maxdepth`, `commit-directly-to-main`,
`suspect-our-code-not-snow`, `shrink-size-partition-per-config`.
