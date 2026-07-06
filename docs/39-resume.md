# 39 — Resume: image forks done, OS chooser done — what's left

Paste into a fresh session to continue MacAtrium. **State: Phase 1 (per-item image
resource forks) and the Phase 2 OS chooser/blesser are DONE and Snow-verified on
`main`.** Remaining: chooser compatibility gating, host per-System startup placement,
the Phase 1 colour-path HW verify, the multi-disk epic (docs/37), and Phase 3 native
controls. The compatibility matrix (docs/38) was spun out to a separate agent session.

## 0. Environment (don't re-learn) — full "Verify recipe" is in docs/36
- MacAtrium is a **68k C** launcher built with **Retro68** in **WSL** at
  `~/repos/MacAtrium`. Drive from the Windows box via `wsl.exe -e bash -lc '…'` for
  commands and `\\wsl.localhost\Ubuntu-24.04\home\dani\repos\MacAtrium` for file ops.
- **Build launcher:** `export RETRO68=~/repos/Retro68-build && cd ~/repos/MacAtrium &&
  cmake --build build -j` → `build/MacAtrium.bin`.
- **Build tool:** `cargo build --release --offline --manifest-path
  tools/atrium-tool/Cargo.toml`. DNS is flaky in this WSL — build **`--offline`**; if a
  new crate is needed, retry `cargo fetch` a few times to catch a DNS window.
  `rb-cli` = `~/.local/bin/rb-cli`.
- **Snow harness (headless verify):** source is `tools/snow-harness/macatrium_harness.rs`
  (NOT in the fresh snow clone). Rebuild from a **neutral cwd** (snow pins Rust 1.95.0):
  `cp tools/snow-harness/macatrium_harness.rs ~/repos/snow/testrunner/src/bin/ && cd ~ &&
  cargo build -r --manifest-path ~/repos/snow/Cargo.toml -p testrunner --bin
  macatrium_harness`. Run: `macatrium_harness <mainROM> <mdcROM> <disk> <outdir> <cycles>
  --snap-every N --keys "CYC:KEY;…"` (keys: `esc up down left right enter return space l
  f r q`, `click@X,Y`, `drag@X1,Y1,X2,Y2`).
- **Test disk / ROMs:** `/mnt/c/Temp/mistercore/HD20SC-With-Benchmarking-and-CDROM.vhd`
  (multi-System: 6.0.8 / 7.0.1 / **7.1.2 folder = actually 7.1.1** / 7.5.5 + pre-6;
  blessed 7.1.2; ships a System Picker). Main ROM `~/repos/boot0.rom` (Mac II non-FDHD);
  MDC `~/repos/341-0868.BIN`. Boots **B&W** — 8-bit colour can't be reached at runtime
  here (needs a boot-8-bit disk / real HW). Assemble: `cp` HD20SC, `rb-cli put-macbinary
  build/MacAtrium.bin --dst-dir "/System 7.1.2/Startup Items"`, prefs `view=0` in
  `/System 7.1.2/Preferences/MacAtrium Prefs`.
- **`rb-cli bless set|show <img> "<folder>"`** blesses/reads a disk's boot System Folder
  offline (how blessing was de-risked; `make-bootable` also blesses).

## 1. DONE (on `main`)
- **Phase 1 — per-item image resource forks** (`art_forks` default ON): host bakes one
  `images/<id>.rsrc` (1-bit `ABMP` + a `PICT` per colour depth) via `resfork.rs` +
  `rb-cli setrsrc`; 68k `art_load_rsrc` loads it. B&W path Snow-verified; the colour
  `PICT` path is proven-equivalent to the loose-`.pict` render but **live colour verify is
  blocked** in Snow (needs HW / boot-8-bit disk). `atrium pict-rsrc` bakes a standalone
  `.rsrc`. Commits `1b75d30` `0886427` `92ac20a` `c768048`.
- **Phase 2 — OS chooser/blesser**: `bless.c` (`bless_enumerate` + `bless_set`
  `PBSetVInfo` `ioVFndrInfo[0]` + `FlushVol` + restart) and `run_os_chooser` — a
  built-in push-button modal (current folder bulleted, each showing **name + real System
  version** read from its `System` file's `vers`). In the **Quick-Launch list**
  ("System Folder Chooser") **and** the **Special menu**. **Swap verified** (chose 6.0.8 →
  the Mac rebooted into System 6). **"MacOS Version: X"** header on the Quick-Launch menu +
  chooser, and in About. Commits `d32dad5` `a8cd9c2` `16a003c` `e4a6b6e`.

## 2. REMAINING
1. **Chooser compatibility gating** (docs/36 §"Compatibility gating"): show every System
   Folder but **gray out incompatible** ones (`HiliteControl(ctl,255)`) and **flag
   enabler-needed** ones with a status line — *"System Enablers must be installed in the
   selected System Folder for it to boot correctly."* Uses the compatibility matrix
   (**docs/38**, being written by a separate agent) + Gestalt machine/CPU/Color-QD in
   `env`. Assume enablers are present (warn, don't block).
2. **Host per-System startup placement** (docs/36 Phase 2): the build must drop MacAtrium
   into **each** System Folder's startup so a swap lands back in MacAtrium (today it only
   auto-runs under 7.1.2; after swapping to 6.0.8 you get the bare Finder).
3. **Phase 1 colour verify** on real HW / a boot-8-bit disk, then it's fully closed.
4. **Multi-disk epic** — scoped in **docs/37**; top risk is RAM/paging vs aggregation.
5. **Phase 3 — per-OS native controls** (docs/36 Phase 3): compile-time `theme_sys{6,7,8}`.
6. (pending) **Compatibility matrix** — spawned task; lands as **docs/38**.

## Docs + memory to read
Plan: **docs/36** (forks/chooser/controls + verify recipe) · **docs/37** (multi-disk) ·
docs/38 (compat matrix, pending). Memory: `commit-directly-to-main`,
`build-and-snow-are-local`, `workflow-verify-in-emulator`, `snow-harness-verify-gotchas`.
