# 31 — Resume: native-controls UI overhaul DONE + 15-item samples (3 OS) + harvester fix

Paste into a fresh session to continue. **State: the launcher's native-controls
redesign (7 phases) is complete + emulator-verified; a `/`-in-app-name harvester
gap is fixed; and right-sized 15-item sample disks are built for 6.0.8 B&W / 7.1
colour / 7.5.5 Quadra (all boot + verified).** Supersedes docs/30. Working tree
CLEAN at `0892af5`.

## 0. Environment (don't re-learn)
- Build LOCAL: `cmake --build build` (Retro68) re-embeds
  `tools/atrium-tool/assets/MacAtrium.bin`; then `cargo build --release
  --manifest-path tools/atrium-tool/Cargo.toml` so disk builds pick it up. rb-cli =
  `/home/dani/repos/rusty-backup/target/release/rb-cli` (user updated it for `/`-in-
  name paths; `\/` escape or `:`-separated; rebuild it there if you touch it).
- **Verify in emulator** (memory `workflow-verify-in-emulator`): **Snow Mac II
  8-bit** (`tools/snow-harness/macatrium_harness.rs`, ROM `~/repos/lbmactwo_MiSTer/
  releases/MacIIFDHD.rom` + MDC ROM `/tmp/mdc/3410868.bin`) is the ONLY headless path
  that injects **mouse** (`--keys "CYC:click@X,Y"`; `meta_l+key` chords; NO `tab`).
  Window origin in GLOBAL coords = (1, mbarHeight+kTitleBarH) = (1, 39). **q800**
  (`tools/qemu-harness/q800_harness.py <rom> <disk> <out> <secs> --snap-every N
  --keys "T:key"`, ROM `/tmp/q800rom/f1acad13.rom`, boot ~50s, SHORT out dir, run
  from repo root) for 7.5.5/Quadra + deep colour (Snow tops out at Mac II 8-bit).
- Commit to main (memory `commit-directly-to-main`). Suspect our code, not Snow.

## 1. DONE + committed this session
### Native-controls UI overhaul (7 phases, `d94ea06`..`a457497`)
Research basis (HIG + Inside Macintosh): native widgets are the system CDEF/WDEF
defprocs from ROM; "no perf degradation" = the region-based update model (redraw
only the changed region). All Snow-verified.
- **P1 per-view click** (`d94ea06`): `BrowseView.click`; `ui_click` dispatches per
  view (was carousel-only). Click selects, re-click launches (= double-click-open).
- **P2 icon-grid split-pane + incremental** (`0fea981`): `render_end_rect` (partial
  blit); grid = ~2/3 icons + ~1/3 detail panel; smaller cells; `iconview_draw_sel`
  repaints only changed cells+detail+counter; `draw_sel`/`draw_art` vtable members;
  `browse_redraw` picks incremental vs full; `Ui.lastDrawn{Item,Top,Cat}`.
- **P3 List pane focus** (`0a7df67`): `Ui.listFocus`; Left/Right move focus, Up/Down
  within it; active pane filled / inactive outlined; `listview_draw_sel`.
- **P4 real Control-Manager controls** (`3e1299a`): `scrollBarProc` scroll bar +
  `pushButProc` Launch button (grid+list), created once (`ensure_controls`),
  positioned per view by `ui_paint_controls` AFTER the blit (Decision A); value=
  curItem/max=n-1 (inactive when content fits). main.c `FindControl`/`TrackControl`:
  Launch→UI_LAUNCH, arrows/page→`ui_scroll_step`, thumb→`ui_scroll_to`. Faux scrollbar gone.
- **P5 real title bar** (`abb84be`): `make_window` `plainDBox`→`noGrowDocProc`+go-away+
  "MacAtrium"; content `top = mbarHeight + kTitleBarH(19)`, inset 1px; immovable;
  `inGoAway`→confirm, `inDrag`→ignore.
- **P6 quit-confirm dialog** (`83c7dce`): `UI_MODE_QUITCONFIRM` render-layer panel +
  two real `pushButProc` buttons (Quit=default w/ `FrameRoundRect` ring, Cancel);
  close box / File>Quit / Cmd-Opt-Q → `ui_confirm_quit`; only confirmed Quit quits.
- **P7 category steppers** (`a457497`): the "^ Category v" band → category name +
  real `<` / `>` pushButProc buttons (`catPrev`/`catNext`) in the carousel+grid
  header (List uses its cat pane); `ui_step_category`. (Settings already has a View
  row — "configure views from the ESC/Settings menu", docs/29.)

### Harvester `/`-in-name fix (`0892af5`) — RESOLVED (memory `slash-in-app-name-skips-inject`)
A donor APPL with `/` (Oxyd's `Oxyd™ b/w`) used to be skipped; the user's rb-cli +
harvest.rs escape/sanitize fixed the INSTALL, but the harvest gave it an app-name id
(`oxyd-b-w`) disconnected from the curated `oxyd-3-6` → installed but uncategorized/
bare. Fix: `selection::harvest_plan` returns a path→selected-id map; `harvest::run`
keeps the **curated id** on the stub; `merge_stubs` corrects that record's `app` to
the sanitized install path in place; `filter_present_apps` de-dups by install path
(a library with two records for one game — `dark-castle` + `dark-castle-1-2` — lists
once). 93 tests pass. Snow-verified: Oxyd shows as "Oxyd 3.6" w/ screenshot + 1992/
Dongleware/Puzzle, categorized, **and launches** (Dongleware splash).

### 15-item sample disks (build artifacts on /home/dani, NOT committed)
Canonical 15 = bolo-0-99-2, glider-3-1-2, tetris-1-2, prince-of-persia, lode-runner-1-2,
3-in-three-1-2, scarab-of-ra-1-4, jewelbox-2-0-2, flappy-mac-1-1, diamonds-1-52,
glypha-3-0, mathematica, arkanoid-1-10, **oxyd-3-6**, dark-castle-1-2.
Configs `/tmp/sample-{608,71,755}.json` (`atrium image --config …`); base resolved
via `data/templates.json` (6.0.8→MacLC_6-0-8-POP, 7.1→MacLC_7-1, 7.5.5→MacLC_7-5-5).
- 6.0.8 B&W: `/home/dani/MacAtrium-sample-608-bw.hda` — 71 MB, art `["1"]`, 512/384 KB. ✅ Snow boot (direct/B&W path: title bar+steppers+carousel render in 1-bit).
- 7.1 colour: `/home/dani/MacAtrium-sample-71-color.hda` — 76 MB, art `["1","8"]`, 1024/768 KB. ✅ Snow boot, Oxyd browsable+launchable.
- 7.5.5 Quadra: `/home/dani/MacAtrium-sample-755-quadra.hda` — 131 MB, art `["1","8","24"]`, 3584/3072 KB. ✅ q800 boot (icon grid + scroll bar + colour art).
`disk_size_mb` is the (fully-allocated, non-sparse) VOLUME size; right-sized to base
+ content + ~2× headroom. Most of each disk is the base System (6.0.8 ~7 MB vs 7.5.5
~50 MB), not the 13 MB of games; the 24-bit Millions art adds ~10 MB on the Quadra.
Can trim to ~45/55/95 MB if minimal is wanted.

## 2. NEXT / open items
- **Carousel control polish** (minor, deferred from P6): the carousel still uses its
  own hand-drawn ◀▶ item-arrows + Launch (the real `gLaunch` is hidden there). Show
  the real Launch + (optionally) a horizontal `scrollBarProc` pager for consistency.
- **Scroll-arrow auto-repeat**: one step per click today (no action proc); a
  `NewControlApProc` would add hold-to-scroll.
- **Sample disks**: decide whether to commit/tag them as the new beta finals (the 3
  beta1 finals `MacAtrium-final-{bw-608,color-71,quadra-755}.hda` carry the OLD pre-
  redesign launcher — rebuild them with this launcher). Optionally trim disk sizes.
- **Curation nit**: `data/compatibility.jsonl` marks `oxyd-3-6` `color:true` but the
  donor only has the *mono* app, so Oxyd lands in "Color" (cosmetic). Flip to false
  to land it in "Black & White" if desired.

## 3. Backlog (pre-redesign, unchanged)
Recommendations (~12/41 donors). 24-bit Millions full-scale on q800 (computed, this
session rendered colour but depth unconfirmed). Multi-volume (docs/23).

## Memory to read
`classic-ui-redesign-views`, `slash-in-app-name-skips-inject`, `qemu-q800-harness`,
`build-and-snow-are-local`, `workflow-verify-in-emulator`, `color-art-memory-budget`,
`build-tool-mvc-architecture`, `commit-directly-to-main`, `suspect-our-code-not-snow`.
