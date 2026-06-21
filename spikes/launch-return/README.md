# Spike: launch-return

**Proves:** an app can sub-launch another app with `launchContinue` and **control
returns** to it when the child quits — the keystone of the resident-shell
architecture ([../../docs/03-architecture.md](../../docs/03-architecture.md)).

If this works on System 6.0.8+MultiFinder, 7.1, 7.5.5, and 7.6.1, the whole
"shell stays alive, launch a game, come back" model is sound and we have **one**
code path. If it *doesn't* return on some system, we learn that now — before
building the real thing — and fall back to the non-resident model there.

The exact API was already confirmed from Apple's headers
([../../docs/11-derisk-log.md](../../docs/11-derisk-log.md) §A). This spike
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

## Matrix to fill in

Run on each target and record (promote results into the de-risk log):

| System | Model (Snow) | launchCanReturn | Returns? | Notes |
|--------|--------------|-----------------|----------|-------|
| 6.0.8 +MultiFinder | SE/30 | ? | ? | |
| 7.1 | II / SE/30 | ? | ? | |
| 7.5.5 | II / SE/30 | ? | ? | |
| 7.6.1 | SE/30 | ? | ? | |
