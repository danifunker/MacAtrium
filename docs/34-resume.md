# 34 — Resume: docs/33 batch shipped (filmstrip + real Settings window)

Paste into a fresh session to continue. **State: the entire docs/33 batch (2.1–2.4)
is DONE, committed, and Snow-verified — 4 commits `ef2aefc`..`9dc2705` on top of the
docs/33 resume base. Working tree CLEAN at `9dc2705`.** Supersedes `docs/33-resume.md`.

## 0. Environment (don't re-learn) — unchanged from docs/33
- Build LOCAL, in this order (the tool EMBEDS the launcher + data via `include_bytes!`,
  so BOTH rebuilds are required or disks ship a stale launcher):
  1. `cmake --build build` (Retro68) → recompiles + re-embeds
     `tools/atrium-tool/assets/MacAtrium.bin`.
  2. `cargo build --release --manifest-path tools/atrium-tool/Cargo.toml`.
  3. `./tools/atrium-tool/target/release/atrium image --config /tmp/sample-{608,71,755}.json`.
  rb-cli = `/home/dani/repos/rusty-backup/target/release/rb-cli`.
- **Verify in Snow:** `~/repos/snow/target/release/macatrium_harness <ROM> /tmp/mdc/3410868.bin
  <disk> <out> <cycles> --snap-every N --wall-secs S --keys "CYCLE:KEY;…"`.
  ROM = `/home/dani/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom`.
  Keys: letters/`up`/`down`/`left`/`right`/`return`/`enter`/`esc`/`space`/`l f r q`;
  `click@X,Y`; `hold@X,Y,DUR`; `drag@X1,Y1,X2,Y2`. Boot reaches the first-run chooser
  ~2.0 G cycles; `return` there confirms Carousel. SHORT out dir (AF_UNIX 108-char).
  Disk-window content origin in GLOBAL coords for `click@` = `(local.x+1, local.y+39)`
  for the MAIN window; the **Settings dialog** is a separate centred window — compute
  from its bounds (CW=360, centred; content top ≈ 51 on 640×480).
- Current disks: `/home/dani/MacAtrium-sample-{608-bw,71-color,755-quadra}.hda`
  (all rebuilt this batch). Sample configs `/tmp/sample-{608,71,755}.json`.
- Commit to main (memory `commit-directly-to-main`). Suspect our code, not Snow.
- Host tests: `cd tests && make && ./host_test` (88/88).

## 1. DONE this session (committed `ef2aefc`..`9dc2705`)
- `ef2aefc` **Carousel → filmstrip + moving selection.** Fixed strip of equal tiles
  (no re-centring hero); page = `curItem/nTiles`, ←/→ move one, page-edge crosses
  scroll a full screenful. New `carousel_draw_sel` (incremental: old+new tile + name
  + header counter + detail band, union-blit) wired into the `gCarouselView` vtable;
  `carousel_layout` reworked (equal columns, dropped nside/hero); new
  `carousel_tile_box`/`carousel_name_band`/`draw_carousel_tile`/`draw_carousel_detail`;
  `carousel_click` hit-tests fixed tiles (re-click = launch). Verified: in-page moves,
  page jump, partial last page, wrap-around — colour + B&W.
- `10f537c` **Boot double-paint fix.** `ValidRect(&gWin->portRect)` after main()'s
  self-draw swallows the window-creation + 1-bit→8-bit depth-bump updateEvt.
- `13d78f7` **Key-cap hints in ESC menu + Control Panels.** `draw_keyhints` split into
  `draw_keyhints_box(g,n,xL,xR,top)`; ESC menu grew (+84) for a nav line + build stamp.
- `9dc2705` **Settings is a real Mac window.** `run_settings_dialog` in main.c — a
  `movableDBoxProc` modal of checkBoxProc checkboxes + `<`/`>` pushButProc steppers +
  Control Panels + default Done. Own modal loop; arrows move a focus ring, Space/Return
  toggle, Left/Right step, mouse drags+clicks. Live apply; chrome toggles deferred to
  `rebuild_window` on close. ui.c exposes `ui_setting_count/kind/label/checked/value/step`
  + `UI_OPEN_SETTINGS`; old `draw_settings*`/`set_row_text`/`UI_MODE_SETTINGS` deleted.
  **Steppers not popups** (user decision: 6.0.8 has no popupMenuProc). Verified on Mac II
  AND the 6.0.8 build: render, focus nav, checkbox toggle (kbd + mouse), stepper step,
  apply-on-close (theme/categories), and Hide-menu-bar → relayout on close.

## 2. Open follow-ups (none blocking; pick by priority)
- **Settings dialog height vs small screens.** Content is ~378 px tall (14 rows). Fine
  on 640×480, but **won't fit a 512×342 9" screen** (compact Macs / some 6.0.8). If the
  launcher must run there, make the dialog two-column or scrolling, or shrink the row
  pitch. `SD_*` constants + `sd_row_top` in main.c. Not hit by the current samples.
- **Dialog hint is plain text** (`"Arrows move … Space/Return change … Esc closes"`).
  main.c can't call the static `draw_keyhints_box` (ui.c). Could expose a key-cap
  drawing helper to make the dialog's hint match the menu/footer key-caps (cosmetic).
- **Untested-but-low-risk dialog paths:** the Control Panels action button → cdev list
  (`SD_OPEN_CDEVS` → `UI_MODE_CTLPANELS`), and the Done button via mouse. Code is
  straightforward; verify opportunistically.
- **Quadra/755 not emulator-verified** (no Mac II harness boot of the Quadra build).
  Use the q800 harness (`tools/qemu-harness/q800_harness.py`, memory `qemu-q800-harness`)
  to smoke-test the filmstrip + Settings dialog at 24-bit if needed.
- **Beta disks** (`final-*` on /home/dani) still carry the pre-filmstrip launcher —
  rebuild for any release (memory `beta1-finals-and-recommendations`).

## Memory to read
`classic-ui-redesign-views` (updated with this batch), `color-art-memory-budget`,
`build-and-snow-are-local`, `workflow-verify-in-emulator`, `overrides-db-maxdepth`,
`commit-directly-to-main`, `suspect-our-code-not-snow`, `qemu-q800-harness`.
