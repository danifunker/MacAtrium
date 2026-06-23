# 18 — Resume prompt (start here)

Paste/read this to pick MacAtrium back up fast. Supersedes the status parts of
[13-handoff.md](13-handoff.md) (which is still the best **environment** reference).
Working style: **commit directly to `main`** (no feature branches until the
project is stable); verify changes by **running them in Snow**, not just compiling.

---

## Where we are

The MVP and the **7.x feature push are essentially complete** and verified in Snow
(System 7.1, Macintosh II). Shipped:

- **Boot shell:** Startup-Items auto-launch (the locked 7.x mechanism, [16](16-startup-items.md));
  **true full-screen** (menu bar hidden via `hide_menu_bar()` — `LMSetMBarHeight(0)`
  **plus** reclaiming the bar strip into `GrayRgn` + `CalcVis`); **Show Finder**
  (fronts the resident Finder, restores the bar) and **Cmd-Option-Q → Quit to
  Finder** (`ExitToShell`, matched by virtual key code).
- **Launcher UX:** keyboard list nav, type-ahead, alias-resolved launch+return,
  dark/light theme, **More Info card** (`I`) + a two-line detail (developer/year/
  genre + blurb), inline art pane, full-screen art preview (`P`); **per-item
  launch hotkeys** (a `hotkey` char launches that title — doubles as a gamepad
  button map via MiSTer); **per-row icons** (1-bit ICN# + colour icl8, drawn in
  the list gutter, depth-matched).
- **Colour/art:** colour backend verified at every depth; `atrium pict` emits
  **1/4/8/16/24-bit**; the launcher picks the best variant for the screen and
  down-converts deeper masters. **Two artworks per title** (Box-Front + gameplay
  Screenshot), switchable via **Settings → Artwork**.
- **Settings panel:** Theme / Color Depth (runtime `SetDepth`) / Volume / Artwork
  / **Startup Sound** / **Shutdown Sound** (on/off, ≤7 s clips baked into the
  image) / **Control Panels** (enumerate the System `cdev`s, open via an `odoc`
  AppleEvent to the Finder).
- **Prefs persistence:** theme / volume / artwork / sound on-off / last-selection
  → `MacAtrium Prefs` in the Preferences folder ([17](17-prefs-persistence.md)).
- **App resource:** `SIZE (-1)` override (`src/macatrium.r`) — 4 MB partition,
  MultiFinder suspend/resume (re-hides the bar + hides our window on switch-away),
  HLE-aware (so we can send the Control Panels `odoc`).
- **Build pipeline:** the pure-Rust `atrium` crate (`harvest · enrich · merge ·
  set · pict · icon · snd · catalog · image`) — **harvest now recurses the whole
  app folder tree** (nested data: DOOM wads, data dirs) — + the **Management UI**
  (egui, full `atrium image` config incl. sounds + hotkeys, runs long ops on a
  worker thread) + **CI** that ships the CLI bundled inside each GUI package.

Sample appliance to run: **`~/MacAtrium-sample/`** (image + both ROMs + README +
`screenshots/`). Rebuild it with `atrium image --config /tmp/macatrium-sample/sample-image.json`.

## Environment & dev loop (all on this box; no display)

| Thing | Path / command |
|-------|----------------|
| Repo | `~/repos/MacAtrium` (branch `main`) |
| Retro68 | `export RETRO68=~/repos/Retro68-build` (toolchain file under `.../m68k-apple-macos/cmake/retro68.toolchain.cmake`) |
| Build launcher | from repo root: `cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=$RETRO68/...retro68.toolchain.cmake && cmake --build build` → `build/MacAtrium.bin` |
| Host core tests | `cd tests && make test` |
| atrium CLI | `tools/atrium-tool/` → `cargo build --release` (binary at `target/release/atrium`); `cargo test --release` |
| Management UI | `tools/macatrium-mgmt-ui/` → `cargo check` (needs a display to *run*) |
| rb-cli | `~/repos/rusty-backup/target/release/rb-cli` |
| Snow harness | canonical `tools/snow-harness/macatrium_harness.rs`; build copy at `~/repos/snow/testrunner/src/bin/` → `cargo build -r -p testrunner --bin macatrium_harness` |
| ROMs | `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom`, `/tmp/mdc/3410868.bin` (MDC 8·24) |
| Sample disks | `~/MacOS_SampleDisks/MacLC_7-1.hda` (+ 7.0.1 / 7.5.5 / 6.0.8) |
| Content | `~/macpack-work/*.vhd` (donors), `~/launchbox/Metadata.xml` (enrich) |

**Headless verify loop:** build → inject (`rb-cli put*` / `atrium image`) →
`macatrium_harness <rom> <mdc> <hda> <out> <maxcycles> [--snap-every N] [--keys "CYC:KEY;..."]`
→ `Read` the PNGs. Keys: letters, `enter`/`esc`/`up`/`down`/`left`/`right`,
`cmd-<k>`, `cmd-opt-<k>`. Boot-to-launcher ≈ 2.4 G cycles.

## Gotchas (learned the hard way)

- **`cargo test` does NOT rebuild the `atrium` binary** — run `cargo build --release`
  before regenerating catalogs/images, or you'll silently use a stale tool.
- **Run `cmake --build build` from the repo root** (not `tests/`).
- **Harness lives in two places** — edit `tools/snow-harness/macatrium_harness.rs`
  then copy to `~/repos/snow/testrunner/src/bin/` and rebuild.
- **Menu-bar hide** must reclaim the bar strip into `GrayRgn` (+`CalcVis`), not
  just set `MBarHeight=0`, or the top stays clipped.
- **Modified-key shortcuts:** match the **virtual key code**, not the char —
  Option mangles it (Option-Q = "œ"). See `Cmd-Option-Q` in `main.c`.
- **Cross-boot prefs round-trip can't be verified headlessly** (the harness
  doesn't sync guest writes back to the `.hda`) — needs interactive Snow / real HW.
- **Mac strings are MacRoman** — keep UI source strings ASCII (no `·`/`—`).

## What's left

The 7.x polish list is **done** (hotkeys, list-row icons + `icl8`, `SIZE (-1)`
4 MB partition + suspend/resume, Settings → Control Panels) plus two extras
shipped on request: **configurable startup/shutdown sounds** and **recursive
structure-preserving harvest**. Each is verified in Snow (System 7.1, Mac II)
except where noted below.

Remaining 7.x odds and ends:
- **Per-resolution layout** — only the wide-screen art-pane scaling is done (and
  gated so 640×480 is unchanged). Real tuning for 512×342 B&W / 800×600 / 1024×768
  is unverified (the harness only renders 640×480) — needs runs at those modes.
- **7.6.1 run** — no 7.6.1 disk on the box yet (have 7.0.1 / 7.1 / 7.5.5 / 6.0.8).

Loose ends needing a **non-headless** run (code is done; emulator can't show it):
- **Control Panels open**: the `odoc` AppleEvent now *sends* without error and
  fronts the Finder (our window hides to reveal it), but the emulator's Finder
  doesn't visibly open the `cdev` — confirm the panel actually opens on real HW.
- **Re-hide menu bar on resume**: the SIZE `acceptSuspendResumeEvents` flag +
  osEvt handler (Hide/ShowWindow + re-hide bar) are in `main.c`; confirm with a
  real app-switch (Finder → back).
- Confirm the **prefs cross-boot round-trip** on interactive Snow / real hardware.

Bigger tracks:
- **Milestone 4 — System 6.0.8** (largest): port `macfs.c` off the System-7
  FSSpec traps (`PBGetCatInfo`/`PBHOpen`), trap-guard `WaitNextEvent`, validate on
  6.0.8 + MultiFinder ([09](09-roadmap.md), [11 §C″](11-derisk-log.md)).
- **Milestone 5 — content:** seed `data/recommendations/` + a CONTRIBUTING/PR flow.
- **Milestone 6 — MiSTer/hardware:** joystick→key map, real-hardware test, perf pass.

## 30-second resume

```sh
cd ~/repos/MacAtrium && export RETRO68=~/repos/Retro68-build
cd tests && make test && cd ..                      # core green?
cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
cmake --build build                                  # launcher builds?
(cd tools/atrium-tool && cargo build --release && cargo test --release)
# rebuild + boot the sample to confirm nothing regressed:
tools/atrium-tool/target/release/atrium image --config /tmp/macatrium-sample/sample-image.json
# then macatrium_harness on a copy of the .hda (see the table above).
```
