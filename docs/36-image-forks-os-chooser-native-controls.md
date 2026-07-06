# 36 — Plan: per-item image resource forks, OS chooser/blesser, per-OS native controls

Working plan for three stacked pieces of work, all built and tested in **WSL only**
(Ubuntu-24.04). Supersedes nothing; extends the pipeline in
[06-content-pipeline.md](06-content-pipeline.md), the build in
[04-toolchain-build.md](04-toolchain-build.md), and the UI in
[07-ui-ux.md](07-ui-ux.md)/[27-classic-ui-redesign.md](27-classic-ui-redesign.md).

## Locked decisions (2026-07-06)
1. **Images → per-item resource fork.** Each title's depth variants become resources
   inside one `images/<id>.rsrc` file (not loose data-fork `.pict`/`.raw` files).
   Chosen for the simplest 68k change and the smallest resident footprint (only the
   current cover is ever in RAM; the resource map per file is tiny). *Not* one big
   library file (map would stay resident) and *not* per-category (deferred).
2. **Per-OS native controls → compile-time themes.** Three separately-built APPLs,
   `-DATRIUM_THEME=sys6|sys7|sys8`, each hard-locked to one design language. "Publish
   3 versions" is literal. The in-UI OS chooser still works on a multi-System image
   because **each System Folder carries its own theme-matched binary** as its startup
   app — bless+reboot lands on the right System *and* the right-looking launcher.

Order of work (per request): **setup → image forks → OS chooser → native controls.**

---

## Phase 0 — WSL environment setup (first build on this box)

Everything is present except four apt packages and three not-yet-built Rust binaries.

### Install (the only blocker — needs your sudo password)
```bash
sudo apt update && sudo apt install -y ruby libmpfr-dev libmpc-dev libboost-all-dev
```
`ruby` = Retro68's multiversal header generator; `mpfr`/`mpc` = gcc deps; `boost` =
Retro68 host tools (Rez/LaunchAPPL/…). Already present: cmake, gcc-13, make, bison,
flex, texinfo, libgmp-dev, zlib1g-dev, cargo/rustc 1.96.

### Build Retro68 (68k only — MacAtrium is a 68k APPL; skips PPC+Carbon, ~½ the time)
```bash
cd ~/repos && mkdir -p Retro68-build && cd Retro68-build
../Retro68/build-toolchain.bash --no-ppc --no-carbon      # ~20–40 min on 20 cores
export RETRO68=~/repos/Retro68-build                       # add to ~/.bashrc
```
gcc/binutils/hfsutils are **vendored in-tree** (no extra submodules); the
`multiversal` submodule is already initialized. Toolchain file lands at
`$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake`.

### Build the Rust tooling (all sources present; none built in this WSL yet)
```bash
cargo build --release --manifest-path ~/repos/rusty-backup/Cargo.toml       # rb-cli
install -m755 ~/repos/rusty-backup/target/release/rb-cli ~/.local/bin/rb-cli # on PATH
cargo build --release --manifest-path ~/repos/MacAtrium/tools/atrium-tool/Cargo.toml
cargo build --release --manifest-path ~/repos/snow/Cargo.toml               # Snow harness
```

### Smoke test the launcher build
```bash
cd ~/repos/MacAtrium
cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
cmake --build build            # -> build/MacAtrium.bin (MacBinary)
```

### Snow testing (ROMs confirmed in this WSL)
- **Main CPU ROM:** `~/repos/boot0.rom` — Mac II **non-FDHD**, 256 K.
- **Video card ROM:** `~/repos/341-0868.BIN` — 8•24 (MDC) declaration ROM, 32 K.
```bash
~/repos/snow/target/release/macatrium_harness \
  ~/repos/boot0.rom ~/repos/341-0868.BIN <disk> <out.png> <cycles> \
  --snap-every 100000000 --keys "CYC:KEY;…"      # click@X,Y / arrows / enter / esc / cmd-<k>
```
Note (from [35-resume](35-resume.md)): Mac II + MDC always reports **Color QD** → the
off-screen render path. To exercise the **direct-draw / B&W** path (the `ABMP`
fast-path and the sys6 theme) boot a **6.0.8** disk (`sysVers < 0x0700`).

---

## Phase 1 — Images into a per-item resource fork

### Today
Loose data-fork files loaded by path in `src/art.c`:
`images/<id>.<depth>.pict` (colour PICT, read from file offset 512) and
`images/<id>.raw` (1-bit, custom 12-byte `"AB"` header, drawn via `CopyBits`). The
catalog `image` field is a base (`images/<id>`), resolved to a variant at draw time.

### Target — `images/<id>.rsrc` (empty data fork; resources in the resource fork)
| Resource | id | Payload | Draw path |
|----------|----|---------|-----------|
| `ABMP` | 128 | the existing `"AB"` 1-bit bitmap | `CopyBits` (B&W fast path) |
| `PICT` | 128 + depth-index (1bpp=0…16bpp=4) | colour PICT per depth | `DrawPicture` |

IDs ≥128 (0–127 are Apple-reserved). The launcher loads the best resource **≤ the
current screen depth**, falling back to `ABMP` 128 for 1-bit.

### Host changes (Rust, `tools/atrium-tool`)
- Generalize the resource-fork writer already used by `snd.rs::build_resfork_from_wav`
  into `src/resfork.rs` — input `[(OSType, id, name?, data)]`, output the classic
  resource-fork binary (header + data + map + type list + ref lists).
- `image.rs`/`pict.rs`: per item, build the `ABMP`+`PICT` set → one `.rsrc` blob →
  create the volume file (empty data fork) → **`rb-cli setrsrc`** it in (the primitive
  already exists, `rbcli.rs:172`; mirror the `snd` injection at `image.rs:246`).
- Catalog: `image` base stays `images/<id>`; only the *resolution* changes (→
  `images/<id>.rsrc` + resource id by depth). No schema break.

### 68k changes (`src/art.c`, wired via `src/model.c`)
- New `art_load_rsrc(base, depth)`: `FSpOpenResFile` `<base>.rsrc` → pick `PICT` for
  depth (fall back down, else `ABMP` 128) → `Get1Resource`/`GetPicture` →
  **`DetachResource`** (own the handle, purge independently) → `CloseResFile`. Fills the
  existing `Art` struct; **draw code (`art_draw_fit`) is unchanged.**
- Keep path-based `art_load` as a fallback: try `.rsrc` first, fall back to the old
  `.pict`/`.raw` so pre-existing images still render during migration.
- RAM: identical to today — one cover in RAM, purged on scroll; no always-open map.

### Verify
6.0.8 disk (ABMP/CopyBits path) + 7.x/colour disk (PICT path) in Snow; covers land in
the cover box only, fast-scroll never blocks on a decode.

---

## Phase 2 — OS chooser / blesser (built before the theming)

Goal: one image holding several System Folders; an in-UI screen picks one, **blesses**
it, and reboots into it.

### Host — assemble the multi-System image
- New `atrium` config/target that lays down multiple System Folders on one HFS volume
  from the existing per-OS bases (`data/templates.json`): e.g. `System 6.0.8`,
  `System 7.5.5`, `System 8.1`, plus the `/MacAtrium` tree. One folder blessed by
  default.
- Place the **theme-matched** launcher as each System's startup app: sys6 binary in the
  6.0.8 folder (Finder-replacement trick, [20](20-system6-multifinder-set-startup-on-disk.md)),
  sys7 in 7.x Startup Items, sys8 in 8.x Startup Items ([05](05-finder-replacement.md),
  [16](16-startup-items.md)).
- Emit `metadata/systems.jsonl` (`{name, folder, sysVers}`) so the 68k chooser lists
  systems without scanning the volume.

### 68k — bless + reboot (`src/sysctl.c` already restarts via `ShutDwnStart()`)
- New `src/bless.c`: resolve chosen folder → dirID (`FSMakeFSSpec` + `PBGetCatInfo`
  `ioDrDirID`); read `ioVFndrInfo` via `PBHGetVInfo`, set `ioVFndrInfo[0]` = System
  Folder dirID (the volume's blessed-folder field), write back `PBHSetVInfo`; then
  `sysctl_restart()`. **Verify exact fields** against Inside Macintosh: Files / a known
  blesser (and whether boot blocks 0/1 need the System/Finder names updated).
- New chooser screen in `src/ui.c` (reachable from the Special menu; optional first-run
  prompt): list `systems.jsonl`, mark the current blessed one, pick → confirm → bless →
  restart.

### Verify
Boot the multi-System image in Snow; drive the chooser with `--keys` (arrows/enter,
`click@`); confirm the post-reboot snapshot shows the chosen System's launcher.

---

## Phase 3 — Per-OS native window controls (compile-time themes)

### Selection
`-DATRIUM_THEME=sys6|sys7|sys8` (default `sys7`) compiles one of
`src/theme_sys{6,7,8}.c` against a shared `src/theme.h`; stamped into `vers`/About and
the binary name (`MacAtrium-sys6.bin`, …).

### `theme.h` interface (metrics + draw hooks)
Title-bar height/style, close/zoom/collapse box rects + draw, window-frame draw,
scroll-bar (width + draw, or delegate to the real Control Manager), push-button proc +
draw, default-button ring, list/grid cell chrome, background fill/pattern, focus ring.

### The three looks
- **sys6** — System 6 flat B&W: 1px rectangular frames, square lined close box, lined
  title bar, plain B&W scroll arrows + 25%-gray gutter, rounded-rect buttons, thick
  default ring. Pure QuickDraw.
- **sys7** — **baseline = today's UI extracted into a theme.** Zoom box, System-7 title
  bar, standard proportions. Extract first, prove pixel-parity in Snow, *then* branch.
- **sys8** — Platinum (8–9.2.2): where the **Appearance Manager** is present (Gestalt
  `gestaltAppearanceAttr`, built in on 8+), delegate to native primitives
  (`DrawThemeWindowFrame`/`DrawThemeButton`/`DrawThemeScrollBar`/…) for true Platinum
  chrome; hand-drawn Platinum fallback otherwise. (sys7 does **not** rely on Appearance
  — it's only an optional 7.x extension.)

### ui.c refactor (staged to de-risk the 137 KB file)
1. Add `theme.h`; route existing chrome/button/scrollbar drawing through `theme_*`,
   with `theme_sys7.c` reproducing today's look 1:1 — verify **no visual change** in Snow.
2. Add `theme_sys6.c`; verify on a 6.0.8 disk.
3. Add `theme_sys8.c`; verify on an 8.x disk.

### Build matrix
A small CMake function loops the theme list → `build/MacAtrium-sys{6,7,8}.bin`. The
`atrium` tool (`targets.rs`/`templates.json`, already OS-keyed) selects the theme-matched
binary per target when assembling disks.

---

## Release matrix
| Deliverable | System Folder(s) | Launcher binary |
|-------------|------------------|-----------------|
| 6.0.8 disk | System 6.0.8 | `MacAtrium-sys6` |
| 7.x disk | System 7.x | `MacAtrium-sys7` |
| 8–9.2.2 disk | System 8/9 | `MacAtrium-sys8` |
| **Multi-OS chooser disk** | all three | each folder's theme-matched binary |

No separate "adaptive" binary — the compile-time-theme decision means the multi-OS image
just reuses the same three binaries, one per System Folder.

## Risks / to verify
- **Bless fields:** `ioVFndrInfo[0]` + possible boot-block name update — confirm against a
  known blesser before trusting it.
- **Multiple System Folders on one HFS volume** booting cleanly per OS — verify in Snow
  and on MiSTer/real hardware.
- **ui.c refactor regression** — mitigated by "extract sys7 = current look, prove parity
  first."
- **Snow = Color QD on Mac II+MDC** — B&W/direct-draw (ABMP + sys6) must be tested on a
  6.0.8 disk.

## Task checklist
**Phase 0** — [ ] apt deps · [ ] Retro68 (`--no-ppc --no-carbon`) · [ ] rb-cli · [ ] atrium tool · [ ] snow harness · [ ] smoke-build `MacAtrium.bin`

**Phase 1** — [ ] `resfork.rs` writer · [ ] host per-item `.rsrc` + `setrsrc` inject · [ ] `art_load_rsrc` + fallback · [ ] Snow verify (6.0.8 + colour)

**Phase 2** — [ ] multi-System image assembly + `systems.jsonl` · [ ] per-System theme-matched startup app · [ ] `bless.c` (verify fields) · [ ] chooser screen · [ ] Snow verify switch+reboot

**Phase 3** — [ ] `theme.h` + extract sys7 (parity) · [ ] `theme_sys6` · [ ] `theme_sys8` (Appearance) · [ ] CMake 3-binary matrix · [ ] atrium picks per-target binary · [ ] per-OS Snow verify
