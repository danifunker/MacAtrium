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
2. **Per-OS native controls → runtime themes, one universal binary.** *(Revised
   2026-07-07 — supersedes the original compile-time / 3-binary plan.)* The single
   `MacAtrium.bin` selects the sys6/sys7/sys8 chrome **at startup** from the detected
   System version (`env.sysVers`), consistent with the locked single-binary goal
   ([01](01-decisions.md)) and the runtime backend/tier detection already in
   `env`/`render`. The look is a configurable **Appearance** setting (Auto / System 6 /
   System 7 / Platinum) in Settings + prefs, with an optional per-build default baked by
   `atrium`. On a multi-System image the *same* binary drops into every System Folder and
   always matches the booted OS — no per-folder binary juggling. On 8+ it delegates to
   the **Appearance Manager** for true Platinum.

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

### Implemented (2026-07-06)
Done and Snow-verified: `bless.c` (`bless_enumerate` + `bless_set` via `PBHGetVInfo` /
`PBSetVInfo` `ioVFndrInfo[0]` + `FlushVol` + `sysctl_restart`; the mechanism was
de-risked offline first with `rb-cli bless`, which switched HD20SC 7.1.2 → 6.0.8 in
Snow) and `run_os_chooser` — a **built-in-widget** modal (standard push buttons + focus
ring) reachable from the **Quick-Launch list** ("System Folder Chooser") **and** the
**Special menu**. Choosing a folder blesses it and restarts into it (verified: picked
6.0.8 → the Mac rebooted into System 6). Scope change: enumeration is **live** (walk the
volume root for folders holding a `zsys` System file), so **no host `systems.jsonl` is
needed**. The chooser shows each folder's **name + real System version** (read from the
`System` file's `vers` resource), and the Quick-Launch menu + chooser show a
**"MacOS Version: X"** header (running Gestalt version; also in About). Commits
`d32dad5` / `a8cd9c2` / `16a003c` / `e4a6b6e`.

### Compatibility gating (chooser) — TODO
Show **every** System Folder, but classify each for **this machine** (Gestalt
`gestaltMachineType` / CPU / Color-QD × the folder's System version, using the
[compatibility matrix](38-compatibility-matrix.md)):
- **Compatible** → normal, selectable.
- **Needs a System Enabler** → selectable but **flagged** — a marker (e.g. `⚠`) plus a
  status line *"System Enablers must be installed in the selected System Folder for it to
  boot correctly."* We can't tell offline whether the enabler is actually present, so we
  **warn, not block** (the image may already have it; builds assume enablers are present).
- **Incompatible** on this machine (e.g. a pre-6 System on a 68020+, or an OS the ROM
  can't run) → **grayed out** (disabled push button via `HiliteControl(ctl, 255)`), shown
  for transparency but not blessable.
- Implementation: extend `env` with Gestalt machine/CPU/Color-QD; a small compat table
  distilled from docs/38; disabled-button rendering + a per-selection status line in
  `run_os_chooser`.

**Host-side blessing (rusty-backup):** independent of the in-UI chooser, `rb-cli bless
set <img> "<folder>"` and `rb-cli bless show <img>` change/read a disk's blessed System
Folder **offline** — for build scripts and to fix/verify a disk without booting (exactly
how the mechanism was de-risked). `rb-cli make-bootable` also blesses a System Folder as
part of its auto-repair.

---

## Phase 3 — Per-OS native window controls (runtime themes, one binary)

*(Binary model revised 2026-07-07 to runtime selection — see Locked decision #2.)*

### Selection
`src/theme.h` defines the chrome interface; `src/theme_sys{6,7,8}.c` each implement it,
and a runtime `Theme` (function-pointer set) is chosen once at startup. The default is
resolved from `env.sysVers` (`< 0x0700` → sys6; `>= 0x0800` with the Appearance Manager
→ sys8; else sys7); a **prefs `appearance`** value (Auto/sys6/sys7/sys8) overrides it and
is surfaced as a Settings row. `atrium` may bake a default `appearance` per target, but
ships **one** binary.

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

### Build
One binary as today (`build/MacAtrium.bin`) — all three `theme_sys{6,7,8}.c` link in (the
extra chrome code is a few KB, negligible even in a Mac Plus partition). No per-theme build
matrix; `atrium` places the same binary in every System Folder and may set a default
`appearance` per target.

**Appliance cleanup (8.x).** `atrium image` removes the Mac OS 8 **Control Strip** +
**Launcher** control panels from the target System Folder (config `disable_control_panels`,
default `["Control Strip","Launcher"]`, case-insensitive, no-op where absent) so they don't
float over the full-screen launcher. Verified on the q800 (the strip is gone after the
build; `rb-cli rm` under the hood). The boot-time modal-dialog case (e.g. a QuickTime
version mismatch that blocks Startup Items) is left to whoever packages the disk.

### Implemented — Increment 1 (2026-07-07): runtime Appearance foundation
Landed and Snow-verified on the Mac II (7.1.1):
- **`src/theme.{h,c}`** — the `Appearance` model (`APPEAR_AUTO/SYS6/SYS7/SYS8`), a data-driven
  `Theme` trait table, `appearance_resolve()` (AUTO → sys6 `<7.0` / sys8 `>=8.0` w/ Appearance
  Mgr / else sys7), `appearance_name()`. Pure C, host-testable.
- **`env`** — `hasAppearanceMgr` (`gestaltAppearanceAttr`) so sys8 only claims true Platinum
  where the Appearance Manager is real.
- **`render`** — `Render.appearance`/`.look`; resolved from `env.sysVers` at `render_init`
  (auto-match the OS), overridable via `render_set_appearance`.
- **`prefs`** — an `appearance` key (`auto`/`sys6`/`sys7`/`sys8`); a saved value beats the OS
  default (`main.c`). (Pre-seedable for headless verify.)
- **First themed trait** — `render_round_frame` reads `look->capCorner`: **sys7 = rounded
  (6px, exact parity with today)**, **sys6 = square** — visibly flips the Icon-Grid tiles and
  the key-cap hints. Verified: sys7 rounded == baseline; sys6 square.

### Implemented — Increment 2 (2026-07-07): in-UI Settings row
A **Appearance** stepper row (Auto / System 6 / System 7 / Platinum) in the Settings panel,
live-applied via `render_set_appearance` and persisted (`prefs appearance`). `Render` now
carries the pref (`appearancePref`) as well as the resolved look, and `render_set_appearance`
takes the pref + `Env` (re-resolves). Snow-verified on 7.1.1: the row shows **"Auto (System
7)"**, cycling it to **System 6** turns the Icon-Grid tiles + key-caps square, and the choice
survives the panel close (saved).

### Implemented — Increment 3 (2026-07-07): tiles + Platinum frame, verified on the q800
- **Flat sys6 tiles** — the raised At-Ease tile is a sys7/sys8 affordance (`Theme.tileRaised`);
  sys6 draws the flat by-icon look (icon + name, name-highlighted selection). Snow-verified.
- **Soft Platinum frame (sys8)** — `Theme.frameBevel` makes `render_frame` draw a softer grey
  frame (via a new `cqd_set_line_shade`) instead of the hard-black 1px rule — the Platinum
  group-box look, robust on any background. (A true 2-tone engraved bevel wants a grey surface;
  the light theme uses white interiors where a white highlight edge vanishes, so we use the
  soft-grey frame.) Verified in colour on the **q800 (Mac OS 8.1)** via QEMU-for-Windows + QMP.

**Remaining (next increments):** selection style + background/desktop pattern per era; deeper
**sys8 Platinum** via the Appearance Manager (`DrawThemeButton`/`DrawThemeWindowFrame`) where a
grey surface makes a real engraved bevel worthwhile. Consider splitting the trait table into
`theme_sys{6,7,8}.c` once the per-era drawing outgrows a data table.

---

## Release matrix
One universal `MacAtrium.bin` for every deliverable; the look is chosen at runtime from the
booted System (overridable via the Appearance setting).
| Deliverable | System Folder(s) | Launcher binary | Look (default) |
|-------------|------------------|-----------------|----------------|
| 6.0.8 disk | System 6.0.8 | `MacAtrium.bin` | sys6 |
| 7.x disk | System 7.x | `MacAtrium.bin` | sys7 |
| 8–9.2.2 disk | System 8/9 | `MacAtrium.bin` | sys8 (Platinum) |
| **Multi-OS chooser disk** | all | the same `MacAtrium.bin` in each | matches each booted System |

## Risks / to verify
- **Bless fields:** `ioVFndrInfo[0]` + possible boot-block name update — confirm against a
  known blesser before trusting it.
- **Multiple System Folders on one HFS volume** booting cleanly per OS — verify in Snow
  and on MiSTer/real hardware.
- **ui.c refactor regression** — mitigated by "extract sys7 = current look, prove parity
  first."
- **Snow = Color QD on Mac II+MDC** — B&W/direct-draw (ABMP + sys6) must be tested on a
  6.0.8 disk.

## Status (2026-07-06)
**Phase 0 DONE**; **Phase 1 DONE** — B&W/`ABMP` path Snow-verified; colour/`PICT` proven-equivalent
(live verify blocked in this Snow config, see below); `art_forks` now defaults ON. Commits: resfork
`1b75d30`, per-item forks `0886427`, `pict-rsrc` tool `92ac20a`. (Env: memory `macatrium-wsl-env`.)

### Verify recipe (this WSL)
- **Rebuild the Snow harness** — source is `tools/snow-harness/macatrium_harness.rs`; it is NOT in the
  fresh snow clone (only `snowemu`). Snow pins Rust 1.95.0 via `rust-toolchain.toml`, so build from a
  neutral cwd to use the installed stable:
  ```sh
  cp tools/snow-harness/macatrium_harness.rs ~/repos/snow/testrunner/src/bin/
  cd ~ && cargo build -r --manifest-path ~/repos/snow/Cargo.toml -p testrunner --bin macatrium_harness
  ```
- **Test disk:** `/mnt/c/Temp/mistercore/HD20SC-With-Benchmarking-and-CDROM.vhd` — multi-System,
  **blessed to System 7.1.2**, boots B&W on the Mac II ROM (`~/repos/boot0.rom` + MDC `~/repos/341-0868.BIN`).
  It ships **System Picker 1.1a3** — a working multi-System chooser to study for Phase 2.
- **Bake a fork + assemble + boot** (harness key `return` picks Carousel):
  ```sh
  atrium pict-rsrc --input art.png --out foo.rsrc --depths 1,8
  rb-cli put DISK empty /MacAtrium/images/foo.rsrc --type rsrc --creator ttxt
  rb-cli setrsrc DISK /MacAtrium/images/foo.rsrc --from-file foo.rsrc
  # + catalog item {"image":"images/foo.rsrc",...}; launcher in "/System 7.1.2/Startup Items"
  macatrium_harness ~/repos/boot0.rom ~/repos/341-0868.BIN DISK out/ 2200000000 \
      --snap-every 200000000 --keys "1000000000:return"
  ```
- **Colour `PICT` path — proven-equivalent; live verify blocked here.** A colour boot can't be reached
  in this Snow config: HD20SC boots 1-bit and the guest's runtime `SetDepth(8)` doesn't switch this MDC
  (a `view=0` pref *is* honoured — so prefs load — but `depth=8` doesn't raise the screen). The PICT
  *resource* is byte-identical to the proven loose-`.pict` render: same `build_pict` data minus the
  512-byte file header the loose loader already skips; both end at `art->pic → DrawPicture`; only
  `DetachResource` is new. **Recommend a colour smoke-test on real hardware or a boot-8-bit disk.**
- **`art_forks` now defaults ON** (`config.rs d_art_forks`): builds pack art into per-item `.rsrc`
  unless a config sets `"art_forks": false`. The loose-file fallback stays in `art.c` — a missing
  `.rsrc` variant yields a blank cover, never a crash.

## Task checklist
**Phase 0** — [x] apt deps · [x] Retro68 (`--no-ppc --no-carbon`) · [x] rb-cli · [x] atrium tool · [x] snow harness · [x] smoke-build `MacAtrium.bin`

**Phase 1 DONE** — [x] `resfork.rs` writer · [x] host per-item `.rsrc` + `setrsrc` inject · [x] `art_load_rsrc` + fallback · [x] Snow verify B&W/`ABMP` · [x] flip `art_forks` default ON · [~] colour `PICT` verify (proven-equivalent; live verify → HW / boot-8-bit disk)

**Phase 2** — [x] `bless.c` (enumerate + `PBSetVInfo`; de-risked vs `rb-cli bless`) · [x] chooser UI (built-in widgets, Quick-Launch + Special menu) · [x] Snow verify swap (7.1.2 → 6.0.8) · [x] per-folder System version + MacOS-version header · [ ] compatibility gating (gray incompatible, flag enabler-needed — needs docs/38) · [ ] host per-System startup placement · [ ] filter/handle pre-6 Systems in the chooser

**Phase 3** (runtime themes, one binary) — [~] `theme.h` + route chrome through it (capCorner + flat tiles + Platinum frame done; selection/bg TODO) · [x] runtime select from `env.sysVers` + prefs override + **Settings Appearance row** · [~] `theme_sys6` (square caps + flat tiles ✓) · [~] `theme_sys8` (soft Platinum frame ✓; Appearance Mgr delegation TODO) · [ ] `atrium` optional per-target default · [~] verify (sys6/sys7 Snow 7.1.1 ✓; sys8 q800/8.1 ✓)
