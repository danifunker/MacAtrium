# 29 — Resume: real System menu bar + Settings View row + Settings redraw fix

Paste into a fresh session to continue. **State: the classic chrome now has a real
Toolbox menu bar (Apple / File / Edit / View / Special) on every variant, the
Settings panel gained a "View" row (Carousel / Icon Grid / List), and the Settings
panel's flickery full-panel re-fill is fixed (partial redraw, like the Esc menu).
All q800-verified + committed.** Supersedes docs/28 for the UI-redesign thread.

## 0. Environment (don't re-learn)
- Build is LOCAL: `cmake --build build` compiles the 68k launcher (Retro68
  `~/repos/Retro68-build`) → re-embeds `tools/atrium-tool/assets/MacAtrium.bin`.
  Then `cargo build --release --manifest-path tools/atrium-tool/Cargo.toml` so disk
  builds pick up the new launcher. CMake stamps the git hash into the version
  (shows "-dirty" with uncommitted changes).
- **Verify in emulator** (memory `workflow-verify-in-emulator`): **q800** for 7.5.5
  (`tools/qemu-harness/q800_harness.py <rom> <disk> <out> <secs> --snap-every N
  --keys "T:key;..."`, ROM `/tmp/q800rom/f1acad13.rom`, boot ~55s; **run from the
  repo root** + SHORT out dir e.g. `/tmp/q8x`). NEW: `--keys` now supports CHORDS
  via `+`, e.g. `64:meta_l+i` = Cmd-I (`meta_l` = the Command key) — that's how the
  menu-key path gets exercised headlessly. Still no mouse, so a pulled-down menu
  (MenuSelect) isn't screenshot-verified; the same `do_menu()` dispatch IS verified
  via the Cmd-key (MenuKey) path.
- rb-cli = `/home/dani/repos/rusty-backup/target/release/rb-cli` (HEAD). Commit to
  main (memory `commit-directly-to-main`). Suspect our code, not Snow.

## 1. What's DONE + committed (this session)
- **Real System menu bar** (main.c): `install_menus()` builds Apple/File/Edit/View/
  Special programmatically (no MENU resources). Apple = About + DAs (AppendResMenu
  'DRVR'); File = Launch (Cmd-L) / Get Info (Cmd-I) / Show Finder / Quit (Cmd-Q)
  — the last two gated on `canLaunchReturn` (absent on the System-6 boot shell);
  Edit = standard, disabled (present for DAs, routes to `SystemEdit`); View =
  Carousel / Icon Grid / List (✓ current via `sync_view_menu`) + Settings…;
  Special = Restart / Shut Down. `do_menu(MenuSelect/MenuKey result)` dispatches —
  most items funnel into the existing `handle_ui_command` / new `ui_show_*` hooks.
- **Window now sits BELOW the bar** (`make_window`: `b.top += mbarHeight`). The old
  `hide_menu_bar()`/`restore_menu_bar()` + GrayRgn-reclaim + `CalcVis` dance is
  GONE; on return from a sub-launch / on osEvt resume we just `DrawMenuBar()` to
  repaint ours (the child/Finder drew its own). Suspend just `HideWindow`.
- **Event loop**: mouseDown `inMenuBar` → `sync_view_menu()` + `MenuSelect`;
  `inSysWindow` → `SystemClick` (DA windows). keyDown with `cmdKey` → `MenuKey`
  (Cmd-Opt-Q still quits first); unmatched Cmd-combos are swallowed, not passed to
  the UI as plain keys.
- **New UI hooks** (ui.c/ui.h): `ui_show_about` (new `UI_MODE_ABOUT` card +
  `draw_about`), `ui_show_settings`, `ui_show_info`, `ui_set_view`. Thin
  state+draw wrappers so main's menu dispatch keeps the UI draw-only.
- **Settings "View" row** (ui.c): new row 6 (after the sound rows) cycles
  Carousel/Icon Grid/List; `SET_N` 9→10, `SET_ROW_VIEW=6`, Categories/Carousel
  shifted to 7/8, `SET_ROW_CTLPANELS=9`. `kViewName[]`/`kViewDesc[]` moved up so
  `set_row_text` can name the view.
- **Settings redraw fix** (ui.c): `draw_settings` now does the same partial redraw
  as `draw_menu` — `draw_settings_row` + `draw_settings_hint` + an `overlayDrawn`
  fast path that repaints only the changed row(s) + the hint band instead of
  re-filling the whole panel every keystroke. A value change that invalidates the
  background (theme/depth/view/artwork/categories/carousel) clears `overlayDrawn`
  in `ui_draw`, so it correctly falls through to a full redraw.
- **Redundant header title dropped**: the browse views no longer repaint
  "MacAtrium" at the top-left (it's in the menu bar now). Gear + "^ Category v" +
  N/M counter stay. (draw_safe + the About card keep their own title.)
- **Harness**: `q800_harness.py` `send_key` learned `+` chords (see §0).

q800-verified on `/home/dani/setup-test.hda` (7.5.5, 7 titles, fresh): menu bar on
the chooser + all 3 views; first-run chooser; Settings shows the View row; changing
View switches the browse view behind with a clean redraw; Tab still cycles; Cmd-I
opens Get Info. No Type-28 / Sad Mac.

## 2. NEXT — remaining classic chrome (task #5 leftovers, lower priority)
- **Dithered scroll bars**: the grid/list scroll bars (`draw_scrollbar_v`) are flat;
  add a 50%-gray pattern track (render layer has no dither primitive yet — QuickDraw
  `qd.gray`).
- **Faux window frame / title bar** (optional now the menu bar carries identity).
- **Reconcile category nav with a menu**: a Go/Library/Category menu could replace
  or complement the "^ Category v" header band + ↑↓ (carousel/grid) / left pane
  (list). Open design choice.
- **Edit menu + DAs**: Edit items are statically disabled, so a DA (e.g. Note Pad)
  can't Cut/Copy/Paste. Authentic fix = enable Edit when the front window is a DA
  (system window) and disable otherwise. Skipped (appliance rarely opens DAs).
- **Verify the mouse menu pulldown** in Snow/on a display (harness has no mouse).

## 3. Backlog (pre-redesign, still open — unchanged from docs/28)
- Recommendations: only ~12/41 install (donors); `data/recommendations.md` wishlist.
- 24-bit Millions at full scale on q800 (computed, not run). Multi-volume (docs/23).
- Rebuild the 3 beta1 finals (`MacAtrium-final-{bw-608,color-71,quadra-755}.hda`)
  with the new launcher — they still carry the docs/26 build.

## Memory to read
`classic-ui-redesign-views`, `qemu-q800-harness`, `build-and-snow-are-local`,
`workflow-verify-in-emulator`, `beta1-finals-and-recommendations`,
`commit-directly-to-main`, `color-art-memory-budget`.
