# 38 — Compatibility Matrix (what MacAtrium runs on)

The single-page reference for **what MacAtrium is and isn't compatible with**.
MacAtrium is a *universal* classic-Mac launcher: **one 68k C application** (built
with Retro68, [04](04-toolchain-build.md)) that probes its environment at
startup and adapts. The same binary is meant to run on **every 68K Mac** and, via
the built-in 68k emulator, on **PowerPC Macs** too — from a Mac Plus on System
**6.0.4** up to a Power Mac on **Mac OS 9.2.2**.

This doc turns the axes in [02-compatibility.md](02-compatibility.md) into
concrete Supported / caveat / Not-supported calls per CPU, model, OS, graphics
mode, and memory/storage configuration. It **builds on** the locked decisions in
[00-vision.md](00-vision.md) / [01-decisions.md](01-decisions.md) /
[03-architecture.md](03-architecture.md) and does not restate their reasoning —
follow the cross-links for the *why*.

> **Maintaining this doc.** This is a **hand-curated** reference, not generated —
> **edit the tables directly** when you learn something more precise. The columns
> are deliberately atomic (one fact per cell: CPU, Colour QD, screen, OS min, OS
> max, status) so a wrong value is a one-cell fix. Keep the `Notes` column to a
> single short clause. The machine-readable **per-title** compatibility (which
> *games* need colour / a min CPU / an OS range) lives separately in
> [`data/compatibility.jsonl`](../data/compatibility.jsonl) and is consumed by the
> build; this doc is the human-facing **hardware/OS** map for the launcher itself.

## How MacAtrium adapts (why one binary covers the range)

MacAtrium never hardcodes a machine. At launch, `env_probe()`
([src/env.c](../src/env.c)) reads:

| Probe (`Gestalt`/QD globals/LowMem) | Field | Drives |
|---|---|---|
| `gestaltSystemVersion` | `sysVers` | Launch strategy, `WaitNextEvent` vs `GetNextEvent`, `FindFolder` vs `GetVol` |
| `gestaltQuickdrawVersion` (`>= gestalt8BitQD`) | `hasColorQD` | Colour vs B&W render backend |
| `gestaltOSAttr` → `gestaltLaunchCanReturn` | `canLaunchReturn` | Resident sub-launch-and-return (the keystone) |
| `GetMainDevice` → `gdPMap.pixelSize` | `pixelSize` | Screen depth → art variant + layout |
| `qd.screenBits.bounds`, `LMGetMBarHeight()` | `screen`, `mbarHeight` | Layout (rows/pages/margins) |
| `gestaltSysArchitecture` + `gestaltNativeCPUtype` | `tier`, `maxOSbcd` | CPU→OS tier: the OS chooser greys Systems this Mac can't boot (docs/40) |

The two decisive derived flags:

- **`useColor = hasColorQD && pixelSize >= 4`** — the colour backend
  (`render_cqd`) is used only when Color QuickDraw is present **and** the screen
  is at least 4-bit. Everything else (1-bit, 2-bit, or no Color QD at all) takes
  the classic-QuickDraw **B&W** backend (`render_qd`). Colour art is simply not
  loaded on the B&W path; the 1-bit `.raw` variant is used instead
  ([15](15-settings-and-color-depth.md)).
- **`canLaunchReturn`** — gates the resident launch path. When absent (bare
  System 6, no MultiFinder) MacAtrium still runs, but a launch is a *cold* one
  (see [§2](#2-system-software-608--922) and [§8](#8-feature-level-caveats-cross-cutting)).

Because adaptation is at runtime, "compatibility" is mostly about **whether the
required capability physically exists** on that machine/OS, not about a separate
build.

### Assumption: system software is already complete

Throughout this matrix, **assume the target disk image already has all required
System Enablers, ROM updates (e.g. MODE32 for the "dirty-ROM" 68030 machines),
and standard extensions installed** to reach its target OS. A model is **never**
marked incompatible merely because it needs an Enabler or a point-release update
to boot its OS — those are treated as present. Only *genuine* incompatibilities
are flagged: CPU too old or too new for the OS, no Color QuickDraw for colour, an
OS outside the 6.0.4–9.2.2 envelope, too little RAM, or an unimplemented feature.

### Legend

| Mark | Meaning |
|---|---|
| ✅ | **Supported** — works as designed |
| ⚠️ | **Supported with caveats** — works, with a documented limitation |
| ❌ | **Not supported** — a genuine incompatibility |
| 🔬 | Supported *by design* but **on-target validation still pending** ([09-roadmap.md](09-roadmap.md)) |

**OS min / OS max** columns give each machine's realistic range **clamped to
MacAtrium's 6.0.4–9.2.2 envelope**: `OS min` is the earliest System that machine
supports (or 6.0.4, the envelope floor, whichever is later); `OS max` is the
highest it can boot (a genuine CPU/ROM hardware ceiling — see [§1](#1-cpu--architecture)).
The floor is **6.0.4** — the first System with the **Gestalt Manager** the whole
`env` probe relies on; **6.0.8 stays the oldest *validated* System-6** (6.0.4–6.0.7
are in-envelope but untested; below 6.0.4 would need a `SysEnvirons` fallback,
deferred). The CPU→OS tiers live in [data/os-tiers.json](../data/os-tiers.json).

**Validated on-target so far** (Snow / QEMU harnesses, per [09](09-roadmap.md)): System **6.0.8** (as the Finder, 8-bit colour), **7.0.1
/ 7.1 / 7.5.5** (Mac II / Quadra-class), across depths **1 / 2 / 4 / 8 / 16 /
24-bit**, resident launch-and-return, Restart/Shut Down. Everything marked 🔬
below (7.6.1, Mac OS 8.x/9.x, real hardware, MiSTer) is architecturally covered
by the same probes but has **not yet been booted** on that exact target.

---

## 1. CPU / architecture

The processor sets three things: the **lowest** and **highest** OS it can boot,
and whether **Color QuickDraw** can exist at all. `OS min`/`OS max` here are the
CPU-family bounds (individual models floor higher — see [§4](#4-models)).

| CPU | Colour QD | OS min | OS max | MacAtrium | Notes |
|---|---|---|---|---|---|
| **68000** | ❌ never | 6.0.4 | 7.5.5 | ✅ B&W only | Original QuickDraw only; compacts/portables; 4 MB RAM cap. Always the B&W backend |
| **68020** | ✅ (card/ROM) | 6.0.4 | 7.5.5 | ✅ | First Color QD generation; 7.6 needs a 68030. Colour needs a colour card/display |
| **68030** | ✅ | 6.0.4 | 7.6.1 | ✅ | The broad classic middle; MODE32 assumed for dirty-ROM models (Mac II/IIx/IIcx/SE/30) |
| **68040 / 68LC040** | ✅ | 7.1 | 8.1 | ✅🔬 8.x | No 68040 Mac boots System 6 (min 7.1); 8.5 needs PowerPC |
| **PowerPC 601/603/604** | ✅ always | 7.1.2 | 9.2.2 | ✅🔬 | Runs the **68k binary under the built-in 68k emulator** — no native PPC build ([00](00-vision.md)). 601/603/604 top at **9.1**; only a G3/G4 reaches 9.2.2 |

**Envelope note.** No single machine spans 6.0.4 → 9.2.2 — each CPU tops out
where the table says. "68k + Mac OS 8.5/9.x" is therefore ❌ **as a hardware
combination** (8.5+ is PowerPC-only), not a MacAtrium limitation.

---

## 2. System software (6.0.4 → 9.2.2)

MacAtrium's launch model ([03](03-architecture.md)) wants a **Process Manager**
so the shell stays resident and control **returns** when a launched app quits.
That exists on System 7+ and on System 6 **with MultiFinder**. Bare System 6 has
no Process Manager, so MacAtrium is instead installed **as the Finder**
([05](05-finder-replacement.md), [09 §M4](09-roadmap.md)) and a launch is
non-returning.

| OS | Process model | MacAtrium | Reason / limitation |
|---|---|---|---|
| **6.0.8 + MultiFinder** | MultiFinder (resident) | ✅ | Resident sub-launch returns; `canLaunchReturn` true. Oldest first-class target |
| **6.0.8 bare** (no MultiFinder) | Single app | ⚠️ | Installed **as the Finder**; boots & runs (8-bit colour verified). Launch is *cold*: the app replaces the shell, and on quit the OS relaunches MacAtrium, which **rebuilds state from prefs** ([17](17-prefs-persistence.md)) rather than returning warm. `WaitNextEvent`, Process Manager, Alias Manager, `FindFolder`, AppleEvents all absent → guarded off |
| **7.0 / 7.0.1 / 7.1** | Process Manager | ✅ | Baseline "modern" target; resident return verified |
| **7.1.x (incl. 7.1.2 first PPC)** | Process Manager | ✅ | 7.1.2 is the earliest PowerPC system |
| **7.5 – 7.5.5** | Process Manager | ✅ | Most-used classic release; primary test base |
| **7.6 / 7.6.1** | Process Manager | ✅🔬 | 68k still fully supported; needs a 68030+. Not yet booted on-target |
| **8.0 / 8.1** | Process Manager | ✅🔬 | 68040 or PowerPC only. Same resident launch API; not yet validated |
| **8.5 / 8.6** | Process Manager (PPC) | ✅🔬 | PowerPC-only OS → MacAtrium runs under the 68k emulator |
| **9.0 – 9.2.2** | Process Manager (PPC) | ✅🔬 | PowerPC-only. Top of the envelope; 9.2.x needs G3/G4 hardware |
| **< 6.0.4** (System 4/5, ≤ 6.0.3) | — | ❌ | Below the 6.0.4 Gestalt-Manager floor the `env` probe needs |
| **Mac OS X / Classic env.** | — | ❌ | Out of scope; not a classic boot shell |

> On System 6, `WaitNextEvent` is a MultiFinder/System-7 trap, so the event loop
> falls back to `GetNextEvent + SystemTask` (still yields under MultiFinder) —
> `main.c`, set from the probed version.

**The launcher enforces the range at runtime.** `env_probe` detects the CPU tier
from `gestaltSysArchitecture` + `gestaltNativeCPUtype` (correct even under the PPC
68k emulator, where `gestaltProcessorType` reports the emulated 68LC040) and
derives `maxOSbcd`, the highest System this Mac can boot. The **System Folder
Chooser** ([main.c](../src/main.c) `run_os_chooser`) then **greys out** any System
Folder whose version falls outside `[0x0604, maxOSbcd]`, and shows a **swap
warning** — *"MacAtrium not installed - boots to Finder"* — when a bootable target
folder has no launcher in its Startup Items. Tier ceilings are baked from
[data/os-tiers.json](../data/os-tiers.json); per-machine data lives in
[data/models.jsonl](../data/models.jsonl). See docs/40.

---

## 3. Graphics — QuickDraw generation, depth, and backend

Bit depth is **not** uniformly available: it depends on the QuickDraw generation
and the screen. MacAtrium selects **one** backend at startup and matches the art
variant to the live depth ([15](15-settings-and-color-depth.md)).

| Depth | Requires | Backend chosen | Art variant | MacAtrium |
|---|---|---|---|---|
| **1-bit (B&W)** | Original QuickDraw (every Mac) | B&W `render_qd` | `<id>.1.raw` (CopyBits) | ✅ everywhere |
| **2-bit (4 greys/colours)** | Color QuickDraw | **B&W** `render_qd` (`useColor` needs ≥ 4) | 1-bit raw (down-mapped) | ✅ renders via the B&W path |
| **4-bit (16)** | Color QuickDraw | Colour `render_cqd` | `<id>.4.pict` or down-convert | ✅ |
| **8-bit (256)** | Color QuickDraw | Colour `render_cqd` | `<id>.8.pict` | ✅ MVP colour target; default `art_depths` `["1","8"]` |
| **16-bit (Thousands)** | 32-bit QuickDraw | Colour `render_cqd` (direct) | `.16` / down-convert | ✅ |
| **24-bit (Millions)** | 32-bit QuickDraw | Colour `render_cqd` (direct) | `<id>.24.pict` | ✅ |

**Backend selection = `useColor` = `hasColorQD && pixelSize >= 4`.** Key
consequences:

- **68000 machines** (no Color QuickDraw) → always B&W, at any "depth" they
  report. This is by design, not a defect.
- A **2-bit** screen (Color QD present) still uses the **B&W** backend — the
  colour path starts at 4-bit.
- 32-bit QuickDraw is built into System 7; on 6.0.5+ it is an installable INIT
  (assume present) — so **Thousands/Millions** need both Color QD *and* 32-bit QD.
- The off-screen composition **GWorld** (colour path) is allocated from
  **MultiFinder temp memory** (`useTempMem`); on System 6 it draws direct (no
  GWorld). See [§6](#6-ram--memory-partition).

Startup **matches the OS depth** (never forces one); the Settings panel can
switch depth live via `SetDepth`, and the art re-colours on the next draw
([15](15-settings-and-color-depth.md)).

### Screen resolutions

Layout is a function of `(width, height, depth)` read from `GetMainDevice`
bounds — never hardcoded ([02 §Screen resolutions](02-compatibility.md)). All of
**512×342** (compact B&W, *tolerated*), **512×384**, **640×480** (default design
size), **800×600**, and **1024×768** are supported; larger OS 9 displays scale
up. The menu-bar height is accounted for (`GetMBarHeight`).

---

## 4. Models

One row per machine, atomic columns for easy correction. `OS min → OS max` is the
range clamped to the 6.0.4–9.2.2 envelope; `Status` is MacAtrium's call. Colour
depends on [§3](#3-graphics--quickdraw-generation-depth-and-backend); memory on
[§6](#6-ram--memory-partition). Per-model `OS min` is that unit's practical earliest
System (≥ the 6.0.4 envelope floor — some cells show the tested-6.0.8 floor);
authoritative per-machine minimums are in [data/models.jsonl](../data/models.jsonl).

**`Colour QD` values:** `Yes` = colour on the built-in display · `Ext` = colour
only on an external monitor · `Card` = needs a NuBus video card + display ·
`No` = B&W only (no Color QuickDraw).

**`OS max` follows the CPU ceiling** ([§1](#1-cpu--architecture)): 68000/68020 →
7.5.5, 68030 → 7.6.1 (MODE32 assumed), 68040 → 8.1, PowerPC → 9.1 (601/603/604).

### 4.1 Compact & portable — 68000 (B&W only)

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Mac Plus | 68000 | No | 512×342 B&W | 6.0.8 | 7.5.5 | ✅ | 4 MB cap; use the compact partition |
| Mac SE | 68000 | No | 512×342 B&W | 6.0.8 | 7.5.5 | ✅ | as Plus |
| Mac Classic | 68000 | No | 512×342 B&W | 6.0.8 | 7.5.5 | ✅ | as Plus |
| Mac Portable | 68000 | No | 640×400 B&W | 6.0.8 | 7.5.5 | ✅ | wide B&W layout |
| PowerBook 100 | 68000 | No | 640×400 B&W | 6.0.8 | 7.5.5 | ✅ | as Portable |

### 4.2 Compact & all-in-one — 68030

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Mac SE/30 | 68030 | Ext | 512×342 B&W | 6.0.8 | 7.6.1 | ✅ | colour only on an external monitor; MODE32 |
| Mac Classic II | 68030 | No | 512×342 B&W | 6.0.8 | 7.6.1 | ✅ | CQD in ROM but 1-bit built-in, no external video → B&W; min via 6.0.8L |
| Color Classic / CC II | 68030 | Yes | 512×384 colour | 7.1 | 7.6.1 | ✅ | first colour compact |

### 4.3 Mac II family

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Mac II | 68020 | Card | none (NuBus) | 6.0.8 | 7.5.5 | ✅ | colour needs a video card + display, else B&W; MODE32 |
| Mac IIx | 68030 | Card | none (NuBus) | 6.0.8 | 7.6.1 | ✅ | needs a video card; MODE32 |
| Mac IIcx | 68030 | Card | none (NuBus) | 6.0.8 | 7.6.1 | ✅ | needs a video card |
| Mac IIci | 68030 | Yes | built-in | 6.0.8 | 7.6.1 | ✅ | 32-bit-clean ROM, built-in video |
| Mac IIsi | 68030 | Yes | built-in | 6.0.8 | 7.6.1 | ✅ | built-in video |
| Mac IIfx | 68030 | Card | none (NuBus) | 6.0.8 | 7.6.1 | ✅ | fast 030; needs a video card; MODE32 |
| Mac IIvi / IIvx | 68030 | Yes | built-in | 7.1 | 7.6.1 | ✅ | built-in video |

### 4.4 LC family

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Mac LC | 68020 | Yes | 512×384 colour | 6.0.8 | 7.5.5 | ✅ | 10 MB RAM cap |
| Mac LC II | 68030 | Yes | 512×384 colour | 6.0.8 | 7.6.1 | ✅ | min via 6.0.8L |
| Mac LC III | 68030 | Yes | 512×384 / 640×480 | 7.1 | 7.6.1 | ✅ | |
| Mac LC 475 | 68LC040 | Yes | 640×480+ colour | 7.1 | 8.1 | ✅🔬 8.x | = Quadra 605 / Performa 475 board |
| Mac LC 575 | 68LC040 | Yes | 640×480 colour (AIO) | 7.1 | 8.1 | ✅🔬 8.x | Color-Classic-style all-in-one |

### 4.5 Quadra / Centris (incl. AV)

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Centris 610 / 650 | 68(LC)040 | Yes | onboard / card | 7.1 | 8.1 | ✅🔬 8.x | |
| Quadra 605 / 610 / 650 / 700 / 800 / 900 / 950 | 68040 | Yes | onboard / card | 7.1 | 8.1 | ✅🔬 8.x | fastest 68k |
| Quadra 660AV / 840AV | 68040 | Yes | onboard | 7.1 | 8.1 | ✅🔬 8.x | AV models; the DSP is irrelevant to the 68k app |

### 4.6 PowerBook & Duo

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| PowerBook 1xx (140–180) | 68030 | mono/gray | passive panel | 7.0.1 | 7.6.1 | ✅ | B&W/grayscale → B&W backend; 180c is colour |
| PowerBook Duo 2xx (210–280c) | 68030 / 68040 | gray / colour | panel | 7.1 | 7.6.1 / 8.1 | ✅🔬 | Duo 280/280c are 68040 → 8.1 |
| PowerBook 520 / 540(c) | 68LC040 / 68040 | gray / colour | panel | 7.1 | 8.1 | ✅🔬 8.x | |

### 4.7 Performa

Performa models are **rebadged LC / Quadra / PowerPC** boards — a Performa
**inherits the row of its underlying machine** and gets that board's CPU /
Colour-QD / OS range / status. Examples: Performa 475 = LC 475 (68LC040, 8.1);
Performa 630 = 68040; Performa 5200/6200 = PowerPC ([§4.8](#48-powerpc-68k-app-under-the-built-in-emulator)).
✅ per the underlying board.

### 4.8 PowerPC (68k app under the built-in emulator)

All PowerPC Macs ship with Color QuickDraw and run MacAtrium's **68k binary under
the built-in 68k emulator** — no native build ([00](00-vision.md)).

| Model | CPU | Colour QD | Built-in display | OS min | OS max | Status | Notes |
|---|---|---|---|---|---|---|---|
| Power Mac 6100 / 7100 / 8100 | PPC 601 | Yes | onboard / card | 7.1.2 | 9.1 | ✅🔬 | first-gen "PDM"; 9.2.x needs G3/G4 |
| Performa/Power Mac 52xx / 62xx / 63xx | PPC 603 / 603e | Yes | onboard | 7.5 | 9.1 | ✅🔬 | |
| Power Mac 7500 / 8500 / 9500 | PPC 604 | Yes | onboard / card | 7.5 | 9.1 | ✅🔬 | |
| PowerBook 5300 | PPC 603e | Yes | colour panel | 7.5.2 | 9.1 | ✅🔬 | first PPC PowerBook |

> Reaching **9.2.2** specifically needs a **G3/G4** (beyond the 601/603/604 parts
> listed); those still run MacAtrium — they are simply above this table's parts.

---

## 5. Emulators & FPGA (test/target surfaces)

Not hardware, but the primary way MacAtrium is exercised ([02](02-compatibility.md)).

| Surface | Config | Status |
|---|---|---|
| **Snow** (dev harness) | Mac II / Quadra, System 6.0.8 / 7.x, 1–24 bit | ✅ primary validation |
| **QEMU Quadra 800** | System 7.5.5, multi-SCSI | ✅ boot/volume-cap harness ([23](23-multi-volume-library.md)) |
| Mini vMac | MacPlus, System 6, B&W, 68000 | ✅🔬 B&W path |
| Basilisk II | System 7, 68020/40, colour | ✅🔬 |
| SheepShaver | PPC, runs the 68k binary under emulation | ✅🔬 |
| **MiSTer FPGA** | MacPlus (B&W 512×342), Mac LC / Mac II cores | ✅🔬 input via keyboard/mouse; joystick→key at the core |

---

## 6. RAM / memory partition

The launcher bakes a `'SIZE' (-1)` partition into the build
([config.rs](../tools/atrium-tool/src/config.rs), `effective_app_mem`). Two
presets plus the binary default:

| Preset (`app_mem_kb`) | preferred / minimum | For | Basis |
|---|---|---|---|
| **`COLOR_APP_MEM_KB`** | **1024 / 768 KB** | 7.x colour builds | Measured peak ~472 KB; the 8-bit GWorld comes from **temp memory**, not this partition. Headroom for a larger library |
| **`COMPACT_APP_MEM_KB`** | **512 / 384 KB** | Mac Plus/SE B&W appliance | 4 MB-cap compacts draw 1-bit direct (no GWorld) and load only small 1-bit `.raw` art. Verified on 6.0.8 |
| launcher built-in default | 2048 / 1024 KB (2 MB / 1 MB) | when `app_mem_kb` unset | Generous default; shrink it for compacts |

Minimum realistic **machine RAM** (partition + OS + a launched game):

| Target | Practical machine RAM | Notes |
|---|---|---|
| Compact B&W, System 6 | **2–4 MB** | 512 KB launcher + System 6 (~1 MB) + a small game. 4 MB is the compacts' cap |
| Compact B&W, System 7.1 | **4 MB** | System 7 wants ~2 MB; tight but the compact preset is sized for it |
| Colour, System 7.x | **4 MB min, 8 MB comfortable** | 1 MB partition + System 7 + ~300 KB 8-bit GWorld (temp mem) + game |
| 68040 / PowerPC, OS 8/9 | **8–16 MB+** | Ample; never the constraint |

> **Art vs. RAM:** the default art bound is 720 px; an 8.5 MB PoP cover *failed to
> load* on a 6.0.8 machine (`DEFAULT_ART_BOUND` note, [config.rs](../tools/atrium-tool/src/config.rs)).
> A 1-bit-only build (`art_depths` `["1"]`) skips colour PICTs entirely, shrinking
> both the image and the launcher's runtime memory for the compact target.

---

## 7. Storage, boot volume & media

| Aspect | Limit | Status | Reason |
|---|---|---|---|
| **Boot volume size** | **≤ 2 GB** | ✅ (build capped at `MAX_DISK_MB` = 2048) | Classic ROMs won't *boot* a >2 GB startup volume — a 3.0 GB 7.5.5 disk Sad-Macs on the q800 ([23](23-multi-volume-library.md)). HFS structure goes to ~4 GiB, but **booting** is the wall |
| **Non-boot data volume** | ~4 GiB (HFS) | 🕗 backlog | Not capped at 2 GB; spanning the library across extra SCSI volumes is [23](23-multi-volume-library.md) (not started) |
| **Multiple `/MacAtrium` disks** | — | 🕗 scoping | Aggregating N independent library disks at startup: [37](37-multi-disk-libraries.md) |
| **SCSI / IDE hard disk** | — | ✅ | The normal library medium; read-write (prefs persist, [17](17-prefs-persistence.md)) |
| **Floppy (400 K–1.4 MB)** | tiny + **read-only** | ⚠️ / ❌ as a library | **Floppies are read-only** for our purposes → **prefs don't persist** (theme/volume/last selection); a real library won't fit 1.4 MB. Fine only as a minimal boot/test stub |
| **CD-ROM** | read-only | 🕗 | Removable/read-only media handling is an open question in [37](37-multi-disk-libraries.md) |

---

## 8. Feature-level caveats (cross-cutting)

The constraints that recur above, in one place:

1. **Resident launch-and-return needs a Process Manager** — i.e. **System 7+**,
   or **System 6 + MultiFinder**. Guarded by `canLaunchReturn`
   ([env.c](../src/env.c) / [03](03-architecture.md)). Where it's false, a launch
   is *cold* (relaunch-from-prefs), not warm.
2. **Bare System 6 → installed as the Finder.** No Startup Items folder on 6.0.8,
   so `atrium image finder_replace:true` puts MacAtrium in the System Folder as
   `FNDR`/`MACS` and the boot launches it as the shell ([05](05-finder-replacement.md),
   [09 §M4](09-roadmap.md)). Process Manager, `WaitNextEvent`, Alias Manager,
   `FindFolder`, and AppleEvents are all guarded off there.
3. **Colour artwork needs Color QuickDraw** *and* a ≥ 4-bit screen (`useColor`).
   Otherwise the **B&W backend** renders and the 1-bit `.raw` art is used — the
   designed 68000 / low-depth path, not a failure.
4. **Floppies are read-only** → no prefs persistence; not a library medium
   ([§7](#7-storage-boot-volume--media)).
5. **No file-manager features.** No desktop/Trash/copy/format, no Finder
   AppleEvents — deliberately given up by replacing the Finder
   ([00 non-goals](00-vision.md)).
6. **No native PowerPC build.** PPC Macs run the 68k binary under emulation
   ([00](00-vision.md)); a native build is a deferred non-goal.
7. **App-driven depth/resolution switching** beyond the Settings panel's
   `SetDepth` (e.g. Display Manager mode changes) is deferred 🕗 ([01](01-decisions.md)).

---

## 9. Genuine "Not supported" — the short list

Everything else in the envelope is ✅/⚠️. The real incompatibilities:

| Not supported | Why |
|---|---|
| Macs older than the **Plus** (128K, 512K/512Ke) | 64 KB ROM, too little RAM, can't boot 6.0.8/7.x — below the floor |
| **System < 6.0.4** (System 4/5, ≤ 6.0.3) | Below the 6.0.4 Gestalt-Manager floor the `env` probe needs |
| **68k CPU + Mac OS 8.5 / 8.6 / 9.x** | 8.5+ is **PowerPC-only** — no such hardware combination exists; not a MacAtrium limit |
| **Mac OS X (native) / other OSes** | MacAtrium is a *classic* boot shell only |
| **Native PowerPC execution** | No PPC build; PPC runs the 68k binary under the emulator |
| **Colour on a 68000 machine** | No Color QuickDraw hardware exists → B&W only, by design |
| **Booting a > 2 GB volume** | Classic ROM boot limit ([23](23-multi-volume-library.md)) |

---

## Cross-references

- **Why & scope:** [00-vision.md](00-vision.md), [01-decisions.md](01-decisions.md)
- **Axes & runtime checks:** [02-compatibility.md](02-compatibility.md)
- **Launch model & modules:** [03-architecture.md](03-architecture.md), [08-launching-system.md](08-launching-system.md)
- **Build & memory partition:** [04-toolchain-build.md](04-toolchain-build.md), [config.rs](../tools/atrium-tool/src/config.rs)
- **Becoming the boot shell:** [05-finder-replacement.md](05-finder-replacement.md), [16-startup-items.md](16-startup-items.md)
- **Colour depth & backends:** [15-settings-and-color-depth.md](15-settings-and-color-depth.md)
- **Prefs persistence:** [17-prefs-persistence.md](17-prefs-persistence.md)
- **Volume caps & multi-disk:** [23-multi-volume-library.md](23-multi-volume-library.md), [37-multi-disk-libraries.md](37-multi-disk-libraries.md)
- **Per-title compatibility data:** [`data/compatibility.jsonl`](../data/compatibility.jsonl)
- **CPU→OS tier model & data:** [data/os-tiers.json](../data/os-tiers.json), [data/models.jsonl](../data/models.jsonl), [src/bless.c](../src/bless.c) (chooser gating)
- **Roadmap / what's validated:** [09-roadmap.md](09-roadmap.md)
- **Runtime probe source:** [src/env.c](../src/env.c), [src/main.c](../src/main.c)
