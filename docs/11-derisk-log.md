# 11 — De-Risk Log

Living record of risk items from [10-open-questions.md](10-open-questions.md) as
they're verified. Method so far: read **Apple's own System 7.1 source headers**
on this machine (`~/Downloads/System 7.1 Source/Interfaces/CIncludes/`, CR-line
classic-Mac text) and **Snow's** repo (`~/repos/snow`). These confirm *API shape
and constants* (documentary). *Runtime behavior* still needs a build + emulator
(empirical) — see the bottom section.

Status: ✅ confirmed · 🔬 still needs on-target run.

## A. Confirmed from Apple headers (✅ documentary)

### Launch & return — the keystone

`LaunchApplication()` is the `_Launch` trap `0xA9F2` itself; setting
`launchContinue` keeps our process alive and returns control. One path for
System 6+MultiFinder and System 7.

| Fact | Value | Source |
|------|-------|--------|
| Launch call = trap | `LaunchApplication(const LaunchParamBlockRec*) = 0xA9F2` | `Processes.h` |
| Keep-alive flag | `launchContinue = 0x4000` | `Processes.h` |
| Provide-FSSpec flag | `launchNoFileFlags = 0x0800` | `Processes.h` |
| Extended block id | `extendedBlock = 'LC'`, `extendedBlockLen = sizeof(LaunchParamBlockRec) - 12` | `Processes.h` |
| Param block fields | `launchBlockID, launchEPBLength, launchControlFlags, launchAppSpec (FSSpecPtr), launchProcessSN, launchAppParameters` | `Processes.h` |
| **Runtime guard** | `gestaltLaunchCanReturn = 1`, `gestaltLaunchControl = 3` (bits in `gestaltOSAttr`) | `GestaltEqu.h` |

**Exact keystone call** (also implemented in `spikes/launch-return/`):

```c
LaunchParamBlockRec pb;
pb.launchBlockID      = extendedBlock;
pb.launchEPBLength    = extendedBlockLen;
pb.launchFileFlags    = 0;
pb.launchControlFlags = launchContinue | launchNoFileFlags;
pb.launchAppSpec      = &appFSSpec;        /* the app to run */
pb.launchAppParameters= NULL;
OSErr err = LaunchApplication(&pb);        /* returns HERE when the app quits,
                                              IF launchContinue is honored */
```

Guard before relying on it:

```c
long osAttr;
Boolean canReturn = (Gestalt(gestaltOSAttr, &osAttr) == noErr) &&
                    ((osAttr & (1L << gestaltLaunchCanReturn)) != 0);
```

### Return to Finder (✅)

Walk the process list, match the Finder's creator, bring it forward:

| Fact | Value | Source |
|------|-------|--------|
| Enumerate | `GetNextProcess(&psn)` | `Processes.h` |
| Inspect | `GetProcessInformation(&psn, &info)` → `info.processSignature` (OSType) | `Processes.h` |
| Finder creator | `'MACS'` | (well-known) |
| Bring forward | `SetFrontProcess(&psn)` | `Processes.h` |

### Environment / rendering (✅)

| Fact | Value | Source |
|------|-------|--------|
| QD generation selector | `gestaltQuickdrawVersion = 'qd  '` | `GestaltEqu.h` |
| B&W (classic QD) | `gestaltOriginalQD = 0x000` | `GestaltEqu.h` |
| 256-color Color QD | `gestalt8BitQD = 0x100` | `GestaltEqu.h` |
| Thousands/millions (32-bit QD) | `gestalt32BitQD = 0x200` (+ `…11/12/13`) | `GestaltEqu.h` |
| Current screen depth | `(*(*GetMainDevice())->gdPMap)->pixelSize` | `Quickdraw.h` |
| Main device | `GetMainDevice()`, `GetDeviceList()` | `Quickdraw.h` |

Backend select: `qd < gestalt8BitQD` → B&W backend; `>= gestalt8BitQD` → color;
`>= gestalt32BitQD` → thousands available.

### Shutdown / Restart (✅)

| Action | Trap | Source |
|--------|------|--------|
| Power off | `ShutDwnPower() = {0x3F3C,0x0001,0xA895}` | `ShutDown.h` |
| Restart | `ShutDwnStart() = {0x3F3C,0x0002,0xA895}` | `ShutDown.h` |

### Control panels & folders (✅)

| Fact | Value | Source |
|------|-------|--------|
| Control Panels folder | `kControlPanelFolderType = 'ctrl'` | `Folders.h` |
| Locate it | `FindFolder(vRefNum, type, create, &vRef, &dirID)` — **ships with 6.0 glue** | `Folders.h` |
| Startup Items folder | `kStartupFolderType = 'strt'` | `Folders.h` |

`kStartupFolderType` confirms the **approach-B auto-launch** plan
([05](05-finder-replacement.md)) is a documented folder.

### Aliases (✅)

`AliasHandle`, `NewAlias(fromFile, target, &alias)`, `ResolveAlias(...)`,
`ResolveAliasFile(...)` all present (`Aliases.h`), with
`gestaltAliasMgrAttr = 'alis'` / `gestaltAliasMgrPresent = 0` guard
(`GestaltEqu.h`). Robust target resolution is available; guard for 6.0.8.

### Bonus finding

The headers themselves are **CR-line-ending MacRoman** text — direct
confirmation of the catalog-parser constraints in
[06-content-pipeline.md](06-content-pipeline.md) (tolerate CR, transcode MacRoman).

## B. Emulator decision: **Snow** ✅

Snow (`~/repos/snow`, `/Applications/Snow.app`; site snowemu.com) emulates 68k
Macs at the **hardware level — no ROM patching, no syscall interception**. That
is exactly what we want for verifying `_Launch`/Process Manager behavior: what we
observe is what real hardware does (unlike Basilisk II, which patches the ROM and
intercepts traps and could mask the very behavior we're testing).

### Matrix coverage

| Our target | Snow model | Notes |
|------------|-----------|-------|
| System 6.0.8, B&W (1-bit) | Plus / SE / Classic (68000) | also the MacPlus-class B&W path |
| System 7.1 / 7.5.5, color | **Macintosh II / IIcx / SE/30** + Macintosh Display Card 8•24 | Color QD, 8-bit (256); higher depths per card/monitor |
| Thousands (16-bit) | II-class + 8•24 card | 🔬 confirm depth/monitor combo reaches 16-bit |
| System 7.6.1 | **SE/30 / IIcx (68030)** | 7.6.1 needs 68030+; SE/30 is the classic 7.6.1 box |

CPUs 68000/020/030 + 68881/882 FPU + 68851 PMMU. Snow tops out at SE/30-class
(no 68040/Quadra), but **SE/30 (68030) covers 7.6.1**, so the matrix holds. Large
screens (1024×768) depend on the emulated monitor options — 🔬 verify.

### Why Snow is ideal for *this* de-risk — the debugger

Snow's debugger has **system-trap breakpoints + a trap-history viewer**, plus
single-step / memory / register views. We can:

- Break on `_Launch` (`A9F2`) and **watch control return** to our shell after the
  child quits — direct empirical proof of the keystone.
- Inspect our `LaunchParamBlockRec` in memory before the trap.
- Confirm `Gestalt(gestaltOSAttr)` bit `gestaltLaunchCanReturn` at runtime.

**Plan:** Snow = primary. **Mini vMac** (`/Applications/Mini vMac.app`, installed)
= quick secondary for pure compact B&W / System 6. **Basilisk II** = avoid for
trust-sensitive trap behavior (fine for quick functional smoke tests only).

## B′. Empirical results from sample disks (✅ no-build, via rb-cli)

Done on this machine against `~/Documents/MacOS_SampleDisks/` (Mac LC images:
6.0.8, 7.0.1, 7.1, 7.5.5) with the built `rb-cli` (`~/repos/rusty-backup/target/debug`).
All read-only. `rb-cli ls` prints **type + creator** and the **blessed System
Folder**; `rb-cli bless show` and a raw boot-block read filled in the rest.

### C1 — control panels are `cdev`s, *not* apps (course-correction)

On **7.5.5**, all five target panels are type `cdev`: Monitors `cdev cdsc`,
Sound `cdev soun`, Date & Time `cdev time`, Mouse `cdev mous`, Keyboard
`cdev keyb`. The only `APPL` in the folder is an oddball (Desktop Patterns
`APPL dskp`). On **6.0.8** the panels are loose `cdev`s in the System Folder
(Monitors, Sound, Mouse, Keyboard, Brightness, Color, General `cdev sysc`, …).

→ **The "launch the app-like ones" plan yields zero of the five.** We must open
`cdev`s another way (send an `odoc` AppleEvent to a resident Finder, or route via
"Launch Finder"). [08-launching-system.md](08-launching-system.md) updated.

### MultiFinder on the 6.0.8 disk — available, not active

`MultiFinder` (`ZSYS MACS`), `Backgrounder`, and `DA Handler` are all present in
the System Folder, but the **boot-block shell name is "Finder"** → it boots plain
Finder. To get our required resident model, set Finder's *Set Startup →
MultiFinder* (rewrites the shell name) or patch it ourselves. (Confirm by booting
in Snow.)

### S3 — boot-block shell field located precisely

Raw read of the HFS partition (starts at LBA 96) boot blocks:

```
0x00  4C 4B ...            bbID = 'LK'  (valid)
0x06  00 17                bbVersion
0x0A  06 "System"          bbSysName  (Str15, space-padded)
0x1A  06 "Finder"          bbShellName  (Str15)  ← the field to swap for approach C
```

So the Finder-swap is a 16-byte Str15 at **partition offset 0x1A**. Tooling hooks
already exist in rb-cli: `bless` (set blessed System Folder), `make-bootable`
(`--boot-from` donor copies boot blocks), `chmeta` (type/creator). A small
"rewrite bbShellName" helper would be the cleanest. [05](05-finder-replacement.md).

### Bonus: 32-Bit QuickDraw present on 6.0.8

The 6.0.8 disk has the **`QD32 LEAK` "32-Bit QuickDraw"** INIT installed → this
machine can do **thousands** of colors under System 6, empirically confirming the
R1 plan (thousands = add-on INIT on 6.x, built-in on 7).

### Content-tooling confirmation

`rb-cli ls` exposes everything our content pipeline needs — per-file type/creator
and the blessed folder — and `bless`/`chmeta`/`put` cover injection. Good signal
for the [06-content-pipeline.md](06-content-pipeline.md) approach.

## C. Still needs a build + run (🔬 empirical)

These can't be settled from headers; they need the spike binary in Snow:

| # | Item | How Snow verifies it |
|---|------|----------------------|
| L1 | `launchContinue` actually **returns control** on 6.0.8+MF, 7.1, 7.5.5, 7.6.1 | trap breakpoint on `A9F2`; spike's return-counter increments |
| L3 | Startup-app crash is **recoverable** (no wedged boot) | force a fault as startup app; confirm rescue path |
| S1/S2 | Covering the Finder desktop / **hiding the menu bar** behaves per system | run shell full-screen, observe |
| S3 | **Boot-block shell swap** behaves (and whether `rb-cli` should gain a helper) | swap on a copy image, boot in Snow |
| C1 | ~~classify control panels~~ ✅ all five are `cdev`s (§B′). **Remaining:** does an `odoc` AppleEvent to a resident Finder open a cdev from our backgrounded shell? | run on-target |
| I1 | MiSTer Mac core button→key mapping | separate, needs MiSTer (not Snow) |

### The toolchain — decided

The empirical loop runs entirely on **the other machine**, which has both
**Retro68 and Snow**. This (host) Mac is for documentary de-risk (Apple headers)
and content tooling (rusty-backup). So the deliverable is a **self-contained
runbook**: `spikes/launch-return/` carries the source, a Retro68 `CMakeLists.txt`,
and step-by-step build → `rb-cli` inject → Snow run/debug instructions. Pull the
repo onto the build machine and execute. (`rb-cli` is needed there too, or build
the image on the Mac that has rusty-backup and copy it across.)
