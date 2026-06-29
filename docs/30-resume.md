# 30 — Resume: native window controls + view overhaul (6 phases DONE)

Paste into a fresh session to continue. **State: the launcher now uses REAL
Macintosh window chrome — a Window-Manager title bar, Control-Manager scroll bars
+ push buttons, mouse selection in every view, a split-pane icon grid with
incremental redraw, List-view pane focus, and a standard quit-confirm dialog. All
six planned phases are committed + Snow-verified.** Supersedes docs/29.

## 0. Environment (don't re-learn)
- Build LOCAL: `cmake --build build` (Retro68) re-embeds
  `tools/atrium-tool/assets/MacAtrium.bin`; then `cargo build --release
  --manifest-path tools/atrium-tool/Cargo.toml` so disk builds pick it up.
- **Verify in emulator**: **Snow Mac II 8-bit** (`tools/snow-harness/
  macatrium_harness.rs`, ROM `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom` +
  MDC ROM `/tmp/mdc/3410868.bin`) is the ONLY headless path that injects **mouse**
  (`--keys "CYC:click@X,Y"`, `meta_l` chords; NO `tab`). **q800** (`tools/qemu-
  harness/q800_harness.py`, keys-only) for 7.5.5 + deep colour rendering. Window
  origin in global coords = (1, mbarHeight + kTitleBarH) = (1, 39).
- rb-cli = `/home/dani/repos/rusty-backup/target/release/rb-cli`. Commit to main.

## 1. Done + committed (this redesign, commits `d94ea06`..`83c7dce`)
The research basis: Apple HIG + Inside Macintosh — native controls = real Toolbox
CDEFs/WDEFs (drawn from ROM; grep'd CDEF/WDEF/MDEF present in both ROMs), and "no
perf degradation" = the Mac update model (redraw only the changed region, never the
whole window). The old full-window-redraw-per-move was the root cause.

- **P1 per-view click hook**: `BrowseView.click`; `ui_click` dispatches per-view
  (was carousel-only -> grid/list ignored clicks). `grid_layout`/`list_layout`
  factored out. Click selects; re-click launches (= double-click-to-open).
- **P2 icon grid split-pane + incremental**: `render_end_rect` (partial blit);
  grid = ~2/3 icons + ~1/3 detail panel (screenshot + info), smaller cells (78x62,
  32px). `iconview_draw_sel` repaints only old+new cell + detail + header counter
  (Mac update model); `draw_sel`/`draw_art` vtable members; `browse_redraw` picks
  incremental vs full. `Ui.lastDrawn{Item,Top,Cat}`.
- **P3 List pane focus**: `Ui.listFocus` (0=cats, 1=items); Left/Right move focus,
  Up/Down within it; active pane filled, inactive outlined; `listview_draw_sel`.
- **P4 real Control-Manager controls**: `scrollBarProc` scroll bar + `pushButProc`
  Launch button (grid+list), created once (`ensure_controls`), repositioned per
  view (`ui_paint_controls`, after the blit — Decision A), value=curItem/max=n-1
  (inactive when content fits). main.c `FindControl`/`TrackControl`: Launch ->
  UI_LAUNCH; arrows/page -> `ui_scroll_step`; thumb -> `ui_scroll_to`. Faux `^/v`
  scrollbar removed.
- **P5 real title bar**: `make_window` `plainDBox` -> `noGrowDocProc` + go-away box
  + "MacAtrium"; content `top = mbarHeight + kTitleBarH(19)`, inset 1px; immovable
  (never DragWindow). `inGoAway` -> confirm; `inDrag` -> ignore.
- **P6 quit-confirm dialog**: `UI_MODE_QUITCONFIRM` render-layer panel + two real
  `pushButProc` buttons (Quit=default w/ hand-drawn `FrameRoundRect` ring, Cancel).
  Close box / File>Quit / Cmd-Opt-Q -> `ui_confirm_quit`; only confirmed Quit calls
  `quit_to_finder`. (Settings already has a **View** row — req "configure views
  from the ESC/Settings menu" — done in docs/29.)

Snow-verified each phase (grid/list click select + launch; split-pane + counter;
pane-focus swap; active scroll bar scrolls the 22-item Color category; Launch
button launches; title bar renders; quit dialog Cancel/Quit both work).

## 2. NEXT — remaining from the user's original list
- **Category nav as a standard control** (req: "the category buttons don't seem to
  be using standard window controls"). The carousel/grid "^ Category v" header band
  (`draw_browse_header` + carousel inline) is still hand-drawn. Options: a **popup
  menu** (`popupMenuProc`, System 7+, the most standard — needs a dynamic category
  MENU built after the model loads) vs a **Go/Category menu** in the menu bar
  (simpler, all-systems) vs real prev/next stepper buttons. **Open design choice —
  ask the user.**
- **Carousel control polish** (minor): the carousel still uses its hand-drawn
  arrows + Launch (the real controls are hidden there). For consistency, show the
  real `gLaunch` at `CarLayout.launchBtn` (+ remove the hand-drawn one); optionally
  a horizontal `scrollBarProc` pager replacing the ◀▶ arrows.
- Auto-repeat on scroll-bar arrow press-hold (currently one step per click — no
  action proc; a `NewControlActionUPP` would add hold-to-scroll).
- B&W (System 6 / direct path) smoke test of the controls + title bar + modal.

## 3. Backlog (pre-redesign, unchanged)
- Recommendations (~12/41 donors). 24-bit Millions full-scale on q800. Rebuild the
  3 beta1 finals with the new launcher.

## Memory to read
`classic-ui-redesign-views`, `qemu-q800-harness`, `build-and-snow-are-local`,
`workflow-verify-in-emulator`, `color-art-memory-budget`, `commit-directly-to-main`,
`suspect-our-code-not-snow`.
