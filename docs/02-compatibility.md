# 02 — Compatibility Matrix

The job: **one 68k binary** that detects what it's running on and adapts. This
doc enumerates the axes and the runtime checks that drive adaptation.

## Operating systems

| System | Era | Process model | Notes |
|--------|-----|---------------|-------|
| 6.0.8 | 1991 | Single app, or **MultiFinder** (we require MultiFinder) | Oldest *validated* target; envelope floor is **6.0.4** (see below) |
| 7.1 | 1992 | Process Manager (always multitasking) | Baseline "modern" target |
| 7.5.5 | 1996 | Process Manager | Most-used classic release |
| 7.6.1 | 1997 | Process Manager | Last before Mac OS 8; 68k still supported |

The **envelope floor is 6.0.4** — the first System with the Gestalt Manager the
runtime probe needs (6.0.8 is the oldest *validated* build; 6.0.4–6.0.7 are
in-envelope but untested). OS support clusters by **CPU/ROM into five tiers**
(68k-early / 68030 / 68040 / PPC-old-world / PPC-new-world), each with an OS
ceiling; `env_probe` detects the tier (`gestaltSysArchitecture` +
`gestaltNativeCPUtype`) and the System Folder Chooser greys out Systems this Mac
can't boot. Full grid + data: [38-compatibility-matrix.md](38-compatibility-matrix.md),
`data/os-tiers.json`.

**Detection:** `Gestalt(gestaltSystemVersion, …)` for the OS version;
`Gestalt(gestaltOSAttr, …)` → `gestaltLaunchControl` / `gestaltLaunchCanReturn`
to confirm the resident-launch capability before relying on it. Always check
trap availability before calling (`TrapAvailable` pattern) rather than assuming
from version.

## Color QuickDraw & bit depth

Bit depth is **not** uniformly available — it depends on the machine and on
QuickDraw generation:

| Depth | Requires | Where |
|-------|----------|-------|
| 1-bit B&W | Classic QuickDraw (always present) | Every Mac, incl. compact 68000 (MacPlus core) |
| 4-bit (16) | **Color QuickDraw** | 68020+ with color ROM/hardware (Mac II family, LC, MiSTer Mac LC/II cores) |
| 8-bit (256) | Color QuickDraw | same |
| 16-bit (thousands) | **32-bit QuickDraw** | Built into System 7; an installable INIT on 6.0.5+ with Color QuickDraw |

**Detection:**
- `Gestalt(gestaltQuickdrawVersion, …)` — presence of Color QuickDraw
  (`gestaltOriginalQD` means classic/B&W only; higher = Color QD; the 32-bit QD
  bit indicates thousands/millions capability).
- Per-screen depth from the `GDevice` / `PixMap` of `GetMainDevice()` /
  `GetDeviceList()`; read `(**(**gd).gdPMap).pixelSize`.
- If Color QuickDraw is absent, take the **pure classic QuickDraw path** (no
  `GWorld`, no `RGBColor`, B&W only). This is the MacPlus / compact case.

**Implication:** the renderer has two backends — a **classic-QD B&W backend**
and a **Color-QD backend** — selected once at startup. See
[07-ui-ux.md](07-ui-ux.md).

## Screen resolutions

We never hardcode a resolution; we read `GetMainDevice()` bounds (or
`screenBits.bounds` on the classic path) and compute layout. Targets to look
good at:

| Resolution | Typical source |
|------------|----------------|
| 512×342 | Compact built-in B&W (MacPlus core) — *tolerate*, B&W |
| 512×384 | 12" RGB display (Mac LC era), color |
| 640×480 | 13" RGB / VGA — **default design size** |
| 800×600 | SVGA |
| 1024×768 | Larger displays |

Layout is a function of `(width, height, depth)`: list rows, columns/pages,
margins, and font size derive from the available content rectangle. Account for
the menu bar height (`GetMBarHeight()` / `LMGetMBarHeight()`).

## Machine / input context

- **Real hardware:** any 68k Mac meeting the OS/QD requirements above.
- **Emulators (primary):** Mini vMac (System 6, B&W, 68000), Basilisk II
  (System 7, 68020/40, color), SheepShaver (PPC — runs our 68k binary under
  emulation too).
- **MiSTer FPGA cores:** MacPlus (B&W 512×342), Mac LC / Mac II-class cores
  (color). Input reaches the Mac as **keyboard + mouse**; gamepad navigation is
  achieved by mapping joystick → keys at the MiSTer level, so our UI must be
  fully operable from arrows + Return + Esc + Page keys. See
  [07-ui-ux.md](07-ui-ux.md).

## Compatibility checklist (runtime, at startup)

1. `Gestalt` system version → pick launch strategy.
2. `Gestalt` QuickDraw version → pick render backend (classic B&W vs Color QD).
3. Confirm resident-launch capability (`gestaltLaunchCanReturn`) before using it;
   else fall back to the non-returning launch path (bare System 6 only — should
   not happen since we require MultiFinder, but guard anyway).
4. Read main device bounds + pixel depth → compute layout + palette.
5. Check Shutdown Manager availability before wiring Shutdown/Restart.

Every one of these is a 🔬 **verify-on-target** item: the design is sound but the
exact Gestalt selectors, trap numbers, and behaviors must be confirmed against
each emulator/core. See [10-open-questions.md](10-open-questions.md).
