# 04 — Toolchain & Build

## Toolchain: Retro68

[Retro68](https://github.com/autc04/Retro68) is a GCC-based cross-compiler that
builds classic Mac OS 68k (and PPC) binaries from a modern host. It bundles the
Universal Interfaces, a `Rez`-compatible resource compiler, and CMake
integration, and can emit application binaries (with resource forks) plus disk
images.

Why it fits: ✅ compile on this Mac with no emulation in the loop, ✅ standard C,
✅ targets the exact 68k systems we need, ✅ scriptable for CI.

### Assumed environment

- Retro68 toolchain installed on the build machine (the prompt may run on a box
  that already has it). **Confirmed working** (2026-06-21) on the dev box:
  `RETRO68=~/repos/Retro68-build`, toolchain file
  `$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake`,
  `m68k-apple-macos-gcc` 12.2.0. The root `CMakeLists.txt` builds
  `build/MacAtrium.bin` from this; see also `spikes/launch-return/`.
  Note Retro68's multiversal headers are leaner than Apple's Universal
  Interfaces — a few constants (`launchNoFileFlags`, `gestaltLaunchCanReturn`,
  `kOnSystemDisk`, `fsRtDirID`) are supplied in `src/mac_compat.h`, and there is
  no `<Folders.h>`/`<Shutdown.h>`/`<QuickdrawText.h>` (those symbols arrive via
  any Toolbox shim, which pulls in the single combined header).
- Output we care about: a 68k `APPL` with the right **type/creator** so the OS
  treats it as the startup app, with our resources (`SIZE`, `vers`, menus,
  theme defaults, the built-in "no catalog" screen) attached.

## Project layout (proposed)

```
/                      (this repo)
├─ docs/               these planning docs
├─ src/                C sources (one .c/.h per module from 03-architecture.md)
│  ├─ main.c  env.c  catalog.c  model.c  theme.c
│  ├─ render.c  render_qd.c  render_cqd.c
│  ├─ ui.c  input.c  launch.c  sysctl.c
│  └─ json.c           minimal JSONL parser (see 06)
├─ rsrc/               Rez sources: app resources, SIZE, vers, dialogs, icons
├─ data/               recommendations dataset + sample catalog.jsonl for testing
├─ tools/              host-side helpers (artwork→PICT, catalog merge) — see 06
├─ cmake/              Retro68 toolchain glue, CMakeLists
└─ build/              (gitignored) build output: the .APPL, .dsk/.img test images
```

## Build pipeline

1. **Configure** with the Retro68 CMake toolchain file (selects the 68k target,
   Universal Interfaces, `Rez`).
2. **Compile** C sources → 68k objects → link the `APPL`.
3. **Rez** the resource fork (app resources + `SIZE` partition + `vers`).
4. **Package** into a runnable form: a `MacBinary`/`.bin` or a ready HFS image.
5. **Inject into a boot image** with `rb-cli` (see below) for the auto-launch
   scenario and for emulator testing.

Keep all of this behind a single `make`/script target so "edit → built test
image" is one command.

## Image assembly with rusty-backup

`rb-cli` (already built; sibling repo `../rusty-backup`) is the bridge from
build artifacts to a bootable HFS image:

```sh
# put the launcher app onto a System disk image with correct type/creator
rb-cli put system.dsk ./build/MacAtrium.bin /Applications/MacAtrium \
       --type APPL --creator ATRM
# put the catalog + theme so the shell can find them at runtime
rb-cli put system.dsk ./data/catalog.jsonl /Applications/catalog.jsonl --type TEXT
rb-cli ls  system.dsk /Applications
```

This is also how **game/app payloads and artwork** land in the image (the
"scan a tree and push files into the image" workflow): `rb-cli put`/`cp`/`untar`
populate the volume; the catalog references those on-disk paths. The new
`rb-cli scan`/`catalog` subcommand (spec in
[06-content-pipeline.md](06-content-pipeline.md)) reads the populated volume back
and emits the JSONL.

> **Type/creator note:** the launcher's creator code (placeholder `ATRM`) and
> `APPL` type must match what the boot mechanism in
> [05-finder-replacement.md](05-finder-replacement.md) expects. Lock the creator
> code before first release so prefs/files don't get orphaned.

## Testing loop

- **Fast inner loop:** build → `rb-cli` assemble a small System 7 68k image →
  boot in **Basilisk II** → observe. (System 6 image in **Mini vMac** for the
  B&W path; **MiSTer** core for hardware-accurate input later.)
- Keep a **dev-mode** build (runs as a normal app over a normal Finder boot, see
  [05](05-finder-replacement.md)) so most iteration doesn't touch the boot path.
- 🔬 Automating emulator boot/screenshot is desirable but unproven here; capture
  the manual steps first, automate once stable.

## CI (later)

Once the local build is solid, a GitHub Actions job can: build the 68k app with
Retro68, run any host-side unit tests (the JSON parser and layout math are
plain C and testable off-target), and publish the `.bin` + a sample image as
release artifacts. Not MVP.
