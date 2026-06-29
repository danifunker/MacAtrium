# 28 — Resume: classic UI redesign (multi-view + theme DONE; chrome next)

Paste into a fresh session to continue. **State: the launcher now has 3 switchable
browse views behind an MVC vtable, a first-run interaction chooser, and an
authentic light theme — all q800-verified + committed. The remaining piece is the
classic CHROME (menu bar + window frame + dithered scroll bars), which needs one
design decision (see §5).** Supersedes docs/26 for the UI-redesign thread; beta1
(docs/26) is the prior milestone.

## 0. Environment (don't re-learn)
- Build is LOCAL: `cmake --build build` compiles the 68k launcher (Retro68
  `~/repos/Retro68-build`) → re-embeds `tools/atrium-tool/assets/MacAtrium.bin`.
  Then `cargo build --release --manifest-path tools/atrium-tool/Cargo.toml` so
  disk builds pick up the new launcher. **CMake stamps the git hash into the menu
  version (re-run cmake to refresh; shows "-dirty" with uncommitted changes).**
- **Verify in emulator** (memory `workflow-verify-in-emulator`): **q800** for
  7.5.5 (`tools/qemu-harness/q800_harness.py <rom> <disk> <out> <secs>
  --snap-every N --keys "T:key;..."`, ROM `/tmp/q800rom/f1acad13.rom`, boot ~50s;
  **run it from the repo root** + use a SHORT out dir e.g. `/tmp/q8x`). **Snow**
  for B&W/colour direct-draw (`tools/snow-harness/README.md`; B&W = the direct
  path where flip/menu flashes live). Keys: letters/arrows/ret/esc/tab/spc.
- rb-cli = `/home/dani/repos/rusty-backup/target/release/rb-cli` (HEAD). Commit to
  main (memory `commit-directly-to-main`). Suspect our code, not Snow.

## 1. What's DONE + committed (this redesign thread)
- `a72bcc8`,`1ae07df` **docs/27 + wireframes** (`docs/wireframes/*.png`): classic
  HIG research + 4 directions; **decision = a View menu offering Carousel + Icon
  Grid + List, user-configurable, screenshot-focused, keyboard works in all,
  keep the ESC quick-menu** (docs/27 "Decision").
- `55c0def` **First-run startup screen** (`UI_MODE_SETUP`): "How would you like to
  browse?" Carousel/Icon/List, keyboard+mouse, persists. Shown when prefs have no
  `view` (`!haveView`). Files: ui.c `draw_setup`/`setup_row_rect`, the SETUP
  branches in ui_draw/ui_key/ui_click; main.c first-run trigger; prefs `view=`.
- `c96ba94` **BrowseView vtable** (MVC, no-op extraction). See §3.
- `7717c8d` **Icon Grid + List views** — all 3 live + switchable. **Tab cycles**
  Carousel→Icon→List (interim until the View menu; persisted).
- `5edb406` **Authentic light theme + made it the default** (render_cqd.c `kLight`:
  white interiors, black-on-white, PALE `#c6cfef` selection w/ black text, black
  rules). Fixes the "bad light colours". 'T' still toggles to dark.

All q800-verified. Working tree CLEAN at `5edb406`.

## 2. Tasks (task list state)
1–4 DONE (foundation, startup screen, vtable, grid+list). **#5 IN PROGRESS:
classic chrome + View menu + (dithered) scroll bars + window frame.** Theme (part
of #5) is done.

## 3. The MVC architecture (so adding views is easy)
- **Model = `model.c`** (shared, untouched): current category/item, paging;
  `model_cur_cat/cur_item/move_item/move_cat`, `m->curCat/curItem`, `m->cat->items`,
  `cat->idx[]`, `cat->count`.
- **View = `BrowseView { name, draw, nav, idle }`** (ui.c, just above `ui_draw`).
  `gCarouselView`/`gIconView`/`gListView` + `gViews[VIEW_N]` + `cur_view(u)`.
  `ui_draw`→`cur_view->draw`, `ui_idle`→`->idle`, `ui_key` delegates ARROW nav to
  `->nav` (shared keys Return/Esc/P/I/type-ahead stay in ui.c). `Ui.view` +
  `VIEW_CAROUSEL/ICON/LIST` enum (ui.h). **To add a view: write 3 functions +
  one gViews[] slot.**
- Grid: `draw_iconview`/`iconview_nav` (2-D arrows, scroll derived from selection),
  `grid_metrics`, `draw_scrollbar_v` (basic), `draw_browse_header` (shared header).
  List: `draw_listview`/`listview_nav` (↑↓ item, ←→ category) + a SCREENSHOT detail
  strip (box art on P), `vbar`, `LPANE`. `listview_idle` returns UI_IDLE_FULL.

## 4. Try it (review disks)
- `/home/dani/setup-test.hda` + `/home/dani/MacAtrium-views-demo.hda` (7.5.5, 7
  titles) — fresh, so first boot shows the chooser; `/tmp/setup-test.json` rebuilds.
  q800: boot → pick a view → **Tab** cycles all three. The light theme is default.
- The 3 beta1 finals (`MacAtrium-final-{bw-608,color-71,quadra-755}.hda`, docs/26)
  still carry the OLD launcher — rebuild them once the redesign lands.

## 5. NEXT — task #5 chrome (the one open DECISION)
The launcher **deliberately hides the system menu bar** (`main.c hide_menu_bar`:
`LMSetMBarHeight(0)` + reclaims the GrayRgn strip) to own the full screen as a
clean appliance. So "restore the menu bar" is a real trade-off — **pick one:**
- **(A) Real system menu bar** — stop hiding it, position the window below it
  (`b.top = mbarHeight`), set up Apple/File/Edit/View/Library menus
  (NewMenu/InsertMenu/DrawMenuBar), handle `inMenuBar`/MenuSelect + MenuKey in
  main.c. Most authentic (Fitts top-edge); more invasive; the View menu replaces
  the Tab hack. Redundant with the current gear/title header (clean that up).
- **(B) Faux menu bar** drawn at the launcher's window top (y=0..20) — identical
  LOOK since the appliance owns y=0; keeps the full-screen boot; dropdowns reuse
  the overlay pattern. Lower risk. (Recommended for the appliance.)
Then: **dithered scroll bars** (add a 50%-gray pattern fill — render layer has no
dither primitive yet; QuickDraw `qd.gray`), **window frame/title bar** (faux), and
reconcile the **category nav** (carousel/grid use the "^Category v" header + ↑↓;
list has the left pane) with whatever the menu bar adds (a Go/Category menu).

## 6. Backlog (pre-redesign, still open)
- Recommendations: only ~12/41 install (donors); `data/recommendations.md` wishlist
  (memory `beta1-finals-and-recommendations`, `macpack-vs-macgarden-corroboration`).
- 24-bit Millions at full scale on q800 (computed, not run). Multi-volume (docs/23).
- Polish: list genre column width, grid name truncation, per-view targeted art
  redraw (list uses UI_IDLE_FULL → a flash on the direct path).

## Memory to read
`beta1-finals-and-recommendations`, `qemu-q800-harness`, `build-and-snow-are-local`,
`workflow-verify-in-emulator`, `mgmt-ui-redesign`, `commit-directly-to-main`,
`color-art-memory-budget`.
