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
  genre + blurb), inline art pane, full-screen art preview (`P`).
- **Colour/art:** colour backend verified at every depth; `atrium pict` emits
  **1/4/8/16/24-bit**; the launcher picks the best variant for the screen and
  down-converts deeper masters. **Two artworks per title** (Box-Front + gameplay
  Screenshot), switchable via **Settings → Artwork**.
- **Settings panel:** Theme / Color Depth (runtime `SetDepth`) / Volume / Artwork.
- **Prefs persistence:** theme / volume / artwork / last-selection → `MacAtrium
  Prefs` in the Preferences folder ([17](17-prefs-persistence.md)).
- **Build pipeline:** the pure-Rust `atrium` crate (`harvest · enrich · merge ·
  set · pict · icon · catalog · image`) + the **Management UI** (egui, full
  `atrium image` config, runs long ops on a worker thread) + **CI** that ships the
  CLI bundled inside each GUI package (win/mac/linux).

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

**Next up (designed, ready to build): per-item launch hotkeys / gamepad mappings**
— optional `hotkey` per item in `overrides.jsonl` → catalog; the launcher launches
that title when the key is pressed (MiSTer maps buttons→keys, so it doubles as
per-item button mapping); set it via a "Hotkey" column in the Management UI.

Other 7.x polish:
- **Settings → Control Panels** (enumerate via `FindFolder`, open `cdev`s with an
  `odoc` AppleEvent to the resident Finder).
- Icons *next to* list rows + `icl8` colour icons; per-resolution layout tuning
  (800×600, 1024×768, 512×342 B&W); `SIZE (-1)` 4 MB partition bump; 7.6.1 run.

Loose ends needing a **non-headless** run (not code-blocked):
- Re-hide the menu bar when returning from **Show Finder** (MultiFinder
  suspend/resume; needs the SIZE `acceptSuspendResumeEvents` flag + app-switch test).
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
