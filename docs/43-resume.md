# 43 — Resume: multi-disk libraries shipped; cross-disk startup chooser (docs/42) Phase 0 verified

Paste into a fresh session **on the Windows box** (the repo lives in WSL; this session
ran from Windows driving WSL). **State: two features. (1) Multi-disk libraries (docs/37)
is built, Snow-verified, committed, and pushed — PR still to open. (2) Cross-disk startup
chooser (docs/42) is designed and its one risky piece — the PRAM startup-device write — is
hardware-verified end-to-end (Phase 0 spike). The chooser UI + wiring is the next build.**

## 0. Environment (don't re-learn)
- **Windows + WSL split.** Repo at `\\wsl.localhost\Ubuntu-24.04\home\dani\repos\MacAtrium`
  (WSL: `~/repos/MacAtrium`). Run Linux commands via `wsl.exe bash -lc '...'` from PowerShell
  (or the Bash tool). Read/edit files through the `\\wsl.localhost\...` UNC paths.
- **68k build (Retro68 in WSL).** `$RETRO68` does NOT expand in the non-interactive shell —
  use the absolute toolchain path. Incremental: `cmake --build ~/repos/MacAtrium/build -j`
  → `build/MacAtrium.bin`. Fresh configure needs
  `-DCMAKE_TOOLCHAIN_FILE=/home/dani/repos/Retro68-build/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake`.
- **Host tests** (portable core): `cd ~/repos/MacAtrium/tests && make && ./host_test`
  (**91/91** this session).
- **Snow harness** (headless verify):
  `~/repos/snow/target/release/macatrium_harness <romdir>/macii.rom mdc.bin <diskA> <out> <cycles> [--disk2 <diskB>] [--snap-every N] [--wall-secs S] [--keys "CYC:key;CYC:key"]`.
  Work in `~/mac-mdverify/` (has `macii.rom`, `mdc.bin`, disk copies). Keys map to scancodes
  (`r`→0x0F). Output: `<out>/final.png`, `snap_*.png` (read them with the image tool).
- **Emulator assets are on the Windows side** (memory `macatrium-emulator-assets`): Mac II ROM
  `MacIIFDHD.rom` + MDC ROM `341-0868.BIN` at `/mnt/c/temp/mistercore/lbmactwo_MiSTer/releases/`;
  bootable HDDs at `/mnt/c/Temp/ClassicMacHDDs/` (`MacLC_7-5-5.hda`, `MacLC_7-1.hda`,
  `MacLC_6-0-8.hda`). Disk-build scripts: `~/mac-mdverify/build.sh` (multi-disk libraries),
  `~/mac-mdverify/build-spike.sh` (chooser spike).
- **git / GitHub**: `gh` is NOT in WSL but IS on Windows (authed as danifunker, `repo` scope).
  The WSL SSH key is absent, so push from **Windows git via gh's HTTPS helper**:
  `gh auth setup-git` then
  `git -c safe.directory='*' -C '\\wsl.localhost\Ubuntu-24.04\home\dani\repos\MacAtrium' push https://github.com/danifunker/MacAtrium.git <branch>`.
  PRs: `gh pr create -R danifunker/MacAtrium --base main --head <branch> ...` (or `gh api`).
- **Apple ROM source (SuperMario)**: `/mnt/c/temp/mistercore/supermario` — a git repo with **no
  checked-out working tree**. Read via `git -C <dir> grep <pat> HEAD` and
  `git -C <dir> show HEAD:<path>`. **MacLC_MiSTer PRAM notes**:
  `/mnt/c/temp/mistercore/MacLC_MiSTer/docs/` (`mame_pram_findings.md`, etc.).

## 1. Multi-disk libraries (docs/37) — DONE, PR to open
Built, Snow-verified, committed on branch **`multi-disk-libraries`** (commits `78709e5`
aggregate + `0aa3ac8` boot-first/Recommended-fallback), **pushed to origin**. **Open the PR**
(believed not yet created): `gh pr list -R danifunker/MacAtrium`; if none,
`gh pr create --base main --head multi-disk-libraries` (body = docs/37 + the two commit
messages — a draft was written this session but to an ephemeral scratchpad path).
- **Behaviour**: at startup enumerate every mounted HFS volume carrying `/MacAtrium/metadata`,
  aggregate into one library; each category tagged with its source disk; short `[N]` label
  (N = volume-table index, boot = 0) shown only when >1 disk mounted; a **MacAtrium Status**
  screen (Quick-Launch menu) is the legend. Selection restore = boot-first + **Recommended
  fallback** when the saved category's disk is gone (no persisted volume id).
- **Code**: `macfs_volumes(VolTable*)` + `macfs_make_spec_on` (macfs.c/.h); `.vol` tag on
  CatRef/ModelCat, `MODEL_MAX_CATS` 65→128, `model_select` Recommended fallback (model.c/.h);
  `load_index`/`load_page`/`do_launch`/`run_status_dialog` (main.c); art/launch take a `vref`.
  `prefs`/`sound`/`bless` stay boot-only. Harness gained `--disk2`.

## 2. Cross-disk startup chooser (docs/42) — designed; Phase 0 hardware-verified
Today's blesser is **boot-volume-only** (`bless.c`: `bless_enumerate` walks only the boot
vref; `bless_set` writes the boot MDB). To boot an OS on ANOTHER disk you must set the
machine's **startup device** in PRAM — what Apple's **System Picker** does (it's on the sample
disks at `/System Picker/`).
- **Mechanism — confirmed in SuperMario `OS/StartMgr/StartSearch.a`**: `SetDefaultStartup`
  (OS trap **`$A07E`**) internally does `_WriteXPRam` of 4 bytes at XPRAM **`$78`** =
  `{ INTEGER drvNum; INTEGER refNum }`; `GetDefaultStartup` (**`$A07D`**) reads them back. For a
  SCSI disk, `refNum = ioVDRefNum` (from `PBHGetVInfo`). OS-mediated (no hardware poke), touches
  only `$78` (not the default-OS `$76` or the `NuMc`/`SPValid` signatures), and the ROM's own
  boot-fail mode is a recoverable "?" disk, not a black screen.
- **Phase 0 spike — DONE & hardware-verified** (`spikes/startup-disk/main.c` + `CMakeLists.txt`).
  On the 2-disk Snow harness (Mac II, 7.5.5→7.1, 8-bit): **Level 1** write+read-back matched
  (`{drvNum 8, refNum -34}`); **Level 2** `R`→`ShutDwnStart` **rebooted onto the target disk**
  (`SPIKE-A-755` → set `SPIKE-B-71` → came up "BOOTED FROM: SPIKE-B-71"). Evidence
  `docs/evidence/42-startup-disk-{level1-write-readback,level2-booted-B}.png`. The spike's
  inline-asm trap decls (`.short 0xA07E/0xA07D`, record ptr in A0, clobber d0-d2/a1) are ready
  to lift into the launcher — Retro68 has `DefStartRec` (Multiverse.h) but not the prototypes.
- **PRAM-safety doctrine** (from `display.c::display_set_default_depth` + the MacLC_MiSTer
  notes; full checklist in docs/42 §Safety): write only from ONE explicit user action, never at
  boot / speculatively; validate the target volume first; **read-back-verify before restart**;
  bless target → set device → `FlushVol` → restart; never write on failure; keep a recovery path.

## 3. REMAINING (next steps)
1. **Open the multi-disk PR** (§1) if `gh pr list` shows none.
2. **Commit the Phase-0 work on a fresh branch** (NOT into the multi-disk PR):
   `docs/42-cross-disk-startup-chooser.md`, `docs/43-resume.md`,
   `spikes/startup-disk/{main.c,CMakeLists.txt}` (NOT `build/` or `cfg.log`),
   `docs/evidence/42-startup-disk-*.png`. (Four pre-existing unrelated tool edits —
   `tools/compat-matrix/build.sh`, `tools/macgarden-scraper/{mg,scrape}.py`,
   `tools/snow-harness/assemble.sh` — predate this work; leave or commit separately.)
3. **Build the chooser** (docs/42 Phases 1–3):
   - `bless_enumerate` over ALL volumes (reuse `macfs_volumes`); `SysFolder` gains a `vref` +
     volume name; generalize `bless_set(vref, dirID)`.
   - New "boot this disk" action: bless the target folder on its volume → `SetDefaultStartup`
     with `ioVDRefNum` → `GetDefaultStartup` verify → `FlushVol` → `sysctl_restart`.
   - Chooser UI: list System Folders grouped by disk, scrollable (today's `run_os_chooser` is
     button-capped at ~8), compat-gated (reuse `osc_bootable` / `gEnv.maxOSbcd`), the running
     System bulleted.
   - Verify on the 2-disk harness (real System disks, `build-spike.sh` pattern).
4. **(Optional) spike negative test**: write a bogus refNum, restart, confirm a recoverable "?"
   (final nail in the anti-brick case; the ROM analysis already says it's recoverable).

## 4. Files & locked decisions
- **Docs**: docs/37 (multi-disk), docs/42 (cross-disk design + decisions + Phase-0 result), this file.
- **Code to reuse/extend**: `spikes/startup-disk/` (working PRAM spike), `src/bless.c/.h`
  (boot-only blesser to extend), `src/macfs.c` (`macfs_volumes`), `src/display.c` (PRAM-safety
  precedent), `src/main.c::run_os_chooser` (chooser UI to generalize).
- **docs/42 decisions (locked)**: proper PRAM `SetDefaultStartup` mechanism; all media incl.
  removable/CD; native in the chooser (not launching System Picker).
