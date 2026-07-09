# Spike: launch-return

**Proves:** an app can sub-launch another app with `launchContinue` and **control
returns** to it when the child quits — the keystone of the resident-shell
architecture ([../../docs/03-architecture.md](../../docs/03-architecture.md)).

If this works on System 6.0.8+MultiFinder, 7.1, 7.5.5, and 7.6.1, the whole
"shell stays alive, launch a game, come back" model is sound and we have **one**
code path. If it *doesn't* return on some system, we learn that now — before
building the real thing — and fall back to the non-resident model there.

The exact API was already confirmed from Apple's headers. This spike
confirms the **runtime behavior** the headers can't.

## What it shows on screen

- `System version` / `QuickDraw version` (Gestalt sanity check).
- `launchCanReturn: YES/NO` — the `gestaltLaunchCanReturn` guard.
- **`RETURNS FROM LAUNCH: N`** — increments every time control comes back from a
  launched app. **A growing N is the proof.**
- `last LaunchApplication err` and the last app launched.

Keys: `L` launch an app · `F` bring Finder forward · `R` restart · `Q` quit.

## Runbook — all on the build machine (Retro68 + Snow live there)

This whole spike runs on the machine that has Retro68 **and** Snow. Pull this
repo there and follow these steps. The C is a DRAFT against Retro68's Universal
Interfaces — expect to nudge a header/field if the compiler complains (likely
suspects: `StandardGetFile` signature, `ProcessInfoRec.processAppSpec`).

### 1. Build (Retro68 + CMake)

```sh
export RETRO68=/path/to/Retro68-build      # your install
cmake -S . -B build \
  -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
cmake --build build
# -> build/launch-return.bin  (and .APPL / .dsk)
```

See [`CMakeLists.txt`](CMakeLists.txt) for the output details and toolchain-path
note.

### 2. Inject into a System image (rusty-backup)

Work on a **copy** of a bootable System image, and make sure a small target app
(TeachText / SimpleText) is on it to launch:

```sh
cp system7.dsk test.dsk
rb-cli put test.dsk build/launch-return.bin /launch-return --type APPL --creator LRSP
rb-cli ls  test.dsk /
```

(If `rb-cli` isn't on the build machine, build the image on the Mac that has it
and copy `test.dsk` over — or `cargo build` rusty-backup there.)

### 3. Run + observe in Snow

1. Boot `test.dsk` in **Snow** (model per the matrix below), run `launch-return`.
2. Press `L`, pick the target app, quit it → watch **RETURNS FROM LAUNCH**
   increment. Repeat a few times to be sure it's stable.
3. **Debugger proof (the strong evidence):** in Snow, set a **system-trap
   breakpoint on `_Launch` (`A9F2`)`**, step over it, and confirm execution
   returns to our event loop. Inspect the `LaunchParamBlockRec` in the memory
   viewer before the trap fires, and check the trap-history view.
4. Also note the on-screen `launchCanReturn` flag matches whether it actually
   returned.

## Status (2026-06-21)

- **Builds clean** with Retro68 (fixed for Retro68's leaner multiversal headers:
  dropped the nonexistent `<QuickdrawText.h>`/`<Shutdown.h>`, and `#define`d
  `launchNoFileFlags` / `gestaltLaunchCanReturn` with the values; a `Str255` can't
  take a `"\p"` static initialiser so the label is seeded at runtime).
- **The keystone is confirmed across the whole System 7 family (7.0.1 / 7.1 /
  7.5.5)** — via the **MVP launcher** (`src/`), which reuses this exact
  `LaunchParamBlockRec`/`launchContinue` code and is far easier to drive
  headlessly than this spike's interactive `StandardGetFile` dialog. The launcher
  launched the **real Prince of Persia** (and SimpleText) and control returned
  with selection intact, run automatically in Snow. See
  [../../docs/evidence/](../../docs/evidence/). The spike remains as the minimal
  reference for the call.

## Matrix

Run on each target and record (promoted into the de-risk log):

| System | Model (Snow) | launchCanReturn | Returns? | Notes |
|--------|--------------|-----------------|----------|-------|
| 6.0.8 +MultiFinder | Macintosh II | n/a | n/a | Boots ✅; MultiFinder activates via boot-block shell swap (S3 ✅). Launcher itself needs Milestone-4 port (FSSpec = System-7 trap `0xAA52`; `WaitNextEvent` needs MultiFinder) |
| 7.0.1 | **Macintosh II (FDHD)** | **YES** | **YES** ✅ | real **Prince of Persia** launched & returned, selection intact |
| 7.1 | **Macintosh II (FDHD)** | **YES** | **YES** ✅ | real **Prince of Persia** launched & returned, selection intact |
| 7.5.5 | **Macintosh II (FDHD)** | **YES** | **YES** ✅ | SimpleText + real Prince of Persia, both launched & returned, selection intact |
| 7.6.1 | SE/30 | ? | ? | not yet run (no 7.6.1 disk on this box) |
