# 42 — Cross-disk startup chooser (boot a System Folder on any mounted disk)

**Status: DESIGN LOCKED — PRAM mechanism verified (Phase 0 DONE), ready to build.**
Requested + decided 2026-07-08. Follows the multi-disk-libraries work (docs/37), which
added the volume table this feature reuses. Chosen path: the proper **Startup Disk (PRAM)
mechanism** via `SetDefaultStartup` — the Phase-0 spike is hardware-verified end-to-end
(see Phasing) — all media including removable/CD, native in the chooser (decisions below).

## What it is

Extend MacAtrium's **System Folder Chooser** so it can boot into a System Folder on
**any mounted disk**, not just the disk MacAtrium booted from — i.e. Apple's *System
Picker* capability, native to the launcher. Today the chooser only re-blesses a folder
on the boot disk and restarts.

## The two settings (why cross-disk is a different thing)

Booting is governed by **two** separate settings:
- **Bless** — *per-volume*: which folder on a volume is its System Folder
  (`ioVFndrInfo[0]`, the dir ID). This is what `bless_set` changes today.
- **Startup disk** — *machine-wide*, stored in **PRAM**: which disk the ROM boots.

Re-blessing works *within* the boot disk (the boot disk is already the startup disk).
Booting a **different** disk means changing which disk boots — a separate step the
current chooser never performs.

## How System Picker does it (the model to copy)

From System Picker's own notes (Apple, K. Aitken; found on the 7.5.5 sample disk): it
scans every mounted disk for blessable System Folders, and on Restart writes **two**
things —
1. **the startup disk** — "*just as if the user had selected it using the Startup Disk
   Control Panel*" (the PRAM startup-device setting), and
2. **the bless** — "*the folder … recorded on that disk by saving its ID to disk*".

Plus a **PRAM-free fallback** for machines with no Startup Disk support (the Mac Plus):
it **deblesses every *other* disk's System Folder**, leaving only the chosen one
blessed, so the ROM's boot scan lands on the sole bootable volume. Its own caveat: if
the chosen disk is then absent, the Mac shows the `?` disk until something is reblessed.

## Current state (grounded)

- `bless_enumerate` (`src/bless.c:113`) — walks **only** the boot volume's root
  (`macfs_boot_vref`, `fsRtDirID`) for folders holding a `System` file.
- `bless_set` (`src/bless.c:155`) — writes `ioVFndrInfo[0]` on the **boot** volume's MDB
  (`PBHSetVInfo`) + `FlushVol`. `bless_and_restart` = that + `sysctl_restart`.
- `SysFolder` (`src/bless.h:17`) — `{dirID, name, blessed, version, macatriumReady}`,
  **no volume field**. `BLESS_MAX_SYS = 16`.
- `run_os_chooser` (`src/main.c`) — a movable-modal button list; greys folders outside
  `[0x0604, gEnv.maxOSbcd]` (compat gate `osc_bootable`), warns on `!macatriumReady`
  ("boots to Finder"), blesses + restarts on selection. Fixed buttons, `QL_MAXITEMS = 8`.
- **Reuses:** `macfs_volumes(VolTable*)` (docs/37) already enumerates every mounted
  library-or-any volume — the cross-disk enumeration primitive is already here.

## What it touches

- **`bless.h/.c`:** `SysFolder` gains a **volume** (`short vref` + a display name /
  volume-table index). Refactor the per-volume root-walk into a helper and have
  `bless_enumerate` loop it over the mounted volumes. Generalize `bless_set(vref, dirID)`.
  Add a **set-startup-device** call (the PRAM write; target's `ioVDRefNum` from
  `PBHGetVInfo`) and a cross-disk boot entry point; keep a **debless** helper
  (`bless_set(vref, 0)`) for the per-model fallback.
- **`main.c` `run_os_chooser`:** list folders across disks (labelled / grouped by disk);
  the "running now" folder (boot volume's blessed) marked distinctly from each volume's
  own blessed folder; the boot action drives the chosen mechanism below.
- **Compat gating:** reuse `osc_bootable` per folder (it's disk-agnostic — version vs CPU
  tier ceiling).
- **UI scale:** N disks × M folders can exceed `BLESS_MAX_SYS`/the 8 fixed buttons — needs
  a scrollable/paged list rather than one-button-per-folder.

## The boot mechanism — Start Manager `SetDefaultStartup`  *(identified 2026-07-08)*

On choosing folder F on volume V:
1. **Bless F on V** — `bless_set(V.vref, F.dirID)` (generalized per-volume bless; HFS, no PRAM).
2. **Set the startup device** — the **documented Start Manager trap `SetDefaultStartup`**
   (OS trap `$A07E`), *not* a raw PRAM poke. The SuperMario ROM source
   (`OS/StartMgr/StartSearch.a`) confirms it internally does `_WriteXPRam` of a 4-byte record
   to XPRAM **`$78`** — `[ DriveId | PartitionId | RefNum(word) ]` — so the OS writes PRAM in
   the model's own format (clock-chip on a II, Egret on the LC); we never pick an offset or
   touch hardware, and `$76` (default-OS) and the `NuMc`/`SPValid` signatures are left alone.
   Build the `DefStartRec` (Retro68 `Multiverse.h`) `scsiDev` arm with `sdRefNum = ioVDRefNum`
   (the driver refNum from `PBHGetVInfo` on V). Same abstraction `display.c` uses for the video
   Control call. **Read it back with `GetDefaultStartup` (reads the same 4 bytes at `$78`) and
   verify before restarting.** (The ROM's sibling `SetOSDefault` at `$76` even added a password
   gate after a PRAM-corruption-bricks-boot bug — the exact hazard we're guarding against.)
3. **Restart** (`sysctl_restart`) after `FlushVol`-ing V.

At boot the ROM reads this via `FindStartupDevice → GetDefaultStartup` (confirmed in the Mac
LC ROM disassembly, MacLC_MiSTer `MacLC_ROM_Boot_Sequence_Analysis.md`). Other disks'
blessings are untouched, and a locked CD (a SCSI device too) works because it's only
*selected*, not modified.

### Safety — why this can't brick (research-verified 2026-07-08)
The colour-depth history taught the whole doctrine (`src/display.c:101-129`: two real bricks,
both fixed — a hardcoded mode-id guess, and speculative writes from four sites). We hold the
startup-device write to the same rules:
- **Documented trap, never a guessed offset** — `SetDefaultStartup`, like the video Control
  call, does the correct model-specific PRAM write; we never hand-poke XPRAM or touch the
  `NuMc` (0x0C) / `SPValid` (0x10) signatures. There is no PRAM checksum — corrupting a
  signature just makes the OS wipe the settings region (a recoverable self-heal; MAME/ROM
  ground truth, MacLC_MiSTer `mame_pram_findings.md`).
- **One write site only** — the chooser's explicit "boot this disk" action; never at startup,
  on resume, or speculatively (the colour bug was four incidental sites).
- **Prove the value first** — only from a live-mounted, blessed, compat-passing System Folder
  whose `ioVDRefNum` came back from a successful `PBHGetVInfo`; reuse `folder_has_system` /
  `blessed_dir` / `osc_bootable`.
- **Read-back-verify** (`GetDefaultStartup`) before restart; **never write on any error**;
  don't disturb other disks' blessings, so the previous startup path still works.
- **Recoverable failure by design** — a bad startup-device value yields the blinking **"?"
  disk** (the ROM falls through to its own scan), *not* the black-screen-until-PRAM-reset a
  bad *video* id causes. Never combine this write with any live depth change (so it can't also
  blank the screen); surface the ⌘-⌥-P-R hint; keep the target blessed so a re-bless recovers.
  **Model-gate**, and where the trap isn't proven, fall back to debless-others (below).

**Per-model fallback (debless-others).** Where `SetDefaultStartup` isn't proven for a model,
System Picker's PRAM-free trick still boots a fixed HD: bless F on V and clear every *other*
mounted volume's `ioVFndrInfo[0]` so the ROM boot-scan lands on V (caveats: a yanked V → `?`
boot; can't boot a locked CD).

## Decisions (locked 2026-07-08)

- **Mechanism: the Startup Disk (PRAM) write via `SetDefaultStartup`.** Set the machine's
  startup *device* through the documented Start Manager trap (OS-mediated, like the Startup
  Disk control panel), so it's correct across the LC's *main* XPRAM and a card's *slot* PRAM
  alike; other disks' blessings stay intact. NOT the debless-others trick. The trap +
  `DefStartRec` are identified (research 2026-07-08); Phase 0 harness-verifies before UI.
- **Media: all disks, incl. removable / CD-ROM.** A locked CD can't be deblessed and can
  only be booted by *selecting* it as the startup disk — which is exactly why the PRAM
  mechanism (not debless-others) is required. Consistent with the mechanism choice.
- **Native** in the System Folder Chooser (no System Picker launch interim).
- Still open (design-time): chooser UI layout (group-by-disk vs per-row label; a
  scrollable list replacing the 8 fixed buttons); and the per-model **fallback** where the
  PRAM startup-device write is unavailable/unsafe (use debless-others there — see Risks).

## Risks

1. **PRAM write (lower than first feared).** The mechanism is the documented
   `SetDefaultStartup` trap — OS-mediated, so it's correct across the LC's *main* XPRAM (Egret,
   depth at 0x58, no checksum, `NuMc`/`SPValid` signatures) and a card's *slot* PRAM without us
   knowing offsets — and its failure mode is a **recoverable "?" disk**, not a black-screen
   brick (see Safety). Residual risk is the trap's model coverage; the Phase-0 spike
   harness-verifies it and we fall back to debless-others where unproven.
2. **Removable/CD** — a CD is bootable but can't be deblessed; only setting it as the startup
   device selects it (this is why PRAM). An ejected target after selection is a recoverable `?` boot.
3. **Locked/removable media** — can't debless a CD; booting *from* a CD needs (B).
4. **Target OS incompatible with the Mac** — reuse the existing compat gate (`osc_bootable`).
5. **MacAtrium not on the target System** → boots to Finder — the existing `macatriumReady`
   warning applies per folder, now across disks.
6. **UI scale** — folders × disks vs the fixed-button chooser; needs a scrollable list.
7. **Clean restart** — quit safely; `FlushVol` every touched volume before `sysctl_restart`.

## Phasing

0. ✅ **PRAM spike — DONE (2026-07-08, `spikes/startup-disk/`).** Verified end-to-end on the
   2-disk Snow harness (Mac II, System 7.5.5 → 7.1, 8-bit): the tiny 68k test picks the
   non-boot volume, calls `SetDefaultStartup({drvNum = ioVDrvInfo, refNum = ioVDRefNum})`,
   `GetDefaultStartup` read-back **matches**, and on `R` → `ShutDwnStart` the machine
   **restarts onto the target disk** — booted `SPIKE-A-755`, set `SPIKE-B-71`, rebooted showing
   "BOOTED FROM: SPIKE-B-71". Evidence: `docs/evidence/42-startup-disk-{level1-write-readback,
   level2-booted-B}.png`. Traps emitted inline (`$A07E`/`$A07D` — Retro68 has `DefStartRec` but
   not the prototypes). Remaining for a later pass: the deliberately-bad-value negative test
   (expect a recoverable "?"), and a non-II model.
1. **Cross-volume enumeration + chooser UI** — `bless_enumerate` over the `VolTable`;
   `SysFolder` gains a volume; the chooser lists every disk's folders, labelled by disk,
   compat-gated, in a scrollable list; `bless_set(vref, dirID)` generalized. Boot-disk
   behaviour unchanged.
2. **Wire the boot action** — choosing a folder blesses it on its volume + sets the startup
   device (Phase 0's mechanism) + restarts; handle an absent/removable target and the
   per-model fallback. Verify cross-disk boot on the 2-disk harness.
3. **Polish** — removable/CD specifics, the debless-others fallback path, `?`-safety.

## Verify

The docs/37 2-disk Snow rig is the exact test: disk A (boot, MacAtrium) + disk B (a
different System, e.g. `--disk2 MacLC_7-1.hda`). Open the chooser, pick disk B's System,
confirm the machine **restarts into disk B** (screenshot B's desktop / its launcher). The
harness already exercises restart; scripted keys drive the chooser.
