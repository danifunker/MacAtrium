# 08 — Launching, Control Panels, Shutdown/Restart

Concrete behavior for the actions the shell performs: sub-launching apps, opening
system settings, and powering the machine down/up.

## Sub-launching an app (the core action)

Unified resident-launch path (see [03-architecture.md](03-architecture.md) for
why this works on 6+MultiFinder and 7+):

1. Resolve the catalog `path` (and optional `type`/`creator`) to an **`FSSpec`**.
   Prefer alias resolution where available so moved files still launch; fall back
   to path resolution. 🔬 confirm Alias Manager availability per system.
2. Build a `LaunchParamBlockRec`:
   - `launchBlockID = extendedBlock`, `launchEPBLength = extendedBlockLen`
   - `launchControlFlags = launchContinue | launchNoFileFlags`
     (`launchContinue` keeps **us** alive; we resolve the file ourselves)
   - `launchAppSpec = &theFSSpec`
3. Call the extended `Launch` trap (`LaunchApplication(&pb)` on System 7).
4. On return, bring our window forward, redraw, resume the event loop. Selection
   and scroll position are preserved (we never quit).
5. On failure (`launchErr`, file moved, not an app) → non-fatal alert, stay put.

**Capability guard:** check `Gestalt(gestaltOSAttr)` →
`gestaltLaunchControl` / `gestaltLaunchCanReturn` at startup. We require
MultiFinder precisely so this is always true; if it's somehow false, disable
launching with an explanatory message rather than doing the non-returning launch
that would quit the shell. 🔬 verify selectors/flags on each target.

## Opening system settings / control panels

The brief: surface **Monitors, Sound, Date & Time, Mouse, Keyboard**; launch the
ones that behave as standalone apps, **flag** the ones that don't.

Reality of classic control panels (✅ measured — see
[11-derisk-log.md](11-derisk-log.md) §B′ C1):

- On **6.0.8 and 7.5.5, all five target panels are type `cdev`** (Monitors
  `cdsc`, Sound `soun`, Date & Time `time`, Mouse `mous`, Keyboard `keyb`) — *not*
  launchable apps. The "App-ified control panel" trend is mostly post-7.6, so
  across our whole range expect `cdev`s. The "launch the app-like ones" idea
  yields **zero** of the five.
- A `cdev` is opened by the **Finder** treating it as a document. So our realistic
  paths are: send the Finder an **`odoc` ("open document") AppleEvent** for the
  cdev file (works when the Finder is resident — approach B), or route via
  **"Launch Finder"**. There is no public Toolbox "open control panel" call.

Approach:

1. Enumerate the **Control Panels** folder (`FindFolder` →
   `kControlPanelFolderType`, has 6.0 glue) and read each item's type/creator.
2. For any genuine `APPL` (rare), launch it via the sub-launch path above.
3. For `cdev`s (the norm): if a Finder is resident, **open via `odoc` AppleEvent
   to the Finder** (`'MACS'`); otherwise show them and route through "Launch
   Finder". Never pretend a cdev launched when it didn't.
4. 🔬 Remaining to verify on-target: that the `odoc`-to-Finder open actually
   works from our backgrounded shell, per system.

This keeps the promise honest: mouse-needing settings are reachable, and we're
upfront about the ones the shell can't drive directly.

## Shutdown & Restart

The **Shutdown Manager** makes this easy and is the right, clean way (it runs
shutdown procs, flushes volumes, etc.):

- **Restart:** `ShutDwnStart()` — restarts the machine.
- **Shut Down:** `ShutDwnPower()` — powers off (or shows the "you may now switch
  off" screen on machines without soft power).
- Confirm Shutdown Manager presence via `Gestalt`/trap check before wiring the
  menu items (it's present on all our targets, but guard anyway).
- **No Sleep** action (per [01-decisions.md](01-decisions.md)).
- Put both behind a confirmation or at least a deliberate menu position so
  they aren't hit by accident from a controller. (Place them in the Esc menu,
  not on a stray keypress.)

## Show Finder / Quit

Two escape hatches (detail in [05-finder-replacement.md](05-finder-replacement.md)):

- **Show Finder** (Esc menu): if the Finder is resident (Startup-Items approach),
  bring it forward via `SetFrontProcess` after locating its `ProcessSerialNumber`
  (creator `MACS`), restoring the menu bar so its menus work. MacAtrium keeps
  running underneath. (`sysctl_show_finder`.)
- **Quit to Finder** (`Cmd-Option-Q`): fully quit the launcher with `ExitToShell`
  so the Finder becomes the sole shell. Restores the menu bar first; matched on
  the virtual key code (Option mangles the character). A deliberately hidden,
  kiosk-style shortcut.
- If we ever fully replaced the shell, Show Finder would instead **launch** the
  Finder app through the standard sub-launch path.
- Always offer **Restart** as the universal fallback to get a normal boot.

## Summary of guarded capabilities

Probe once at startup, store in `env`, branch later:

| Capability | Probe | Fallback |
|------------|-------|----------|
| Resident launch | `gestaltLaunchCanReturn` | disable launching, explain |
| Color QuickDraw | `gestaltQuickdrawVersion` | B&W backend |
| Alias Manager | `Gestalt`/trap | path-only resolution |
| Shutdown Manager | `Gestalt`/trap | hide Shutdown/Restart (shouldn't happen) |
| Control Panels folder | `FindFolder` | omit settings menu |
