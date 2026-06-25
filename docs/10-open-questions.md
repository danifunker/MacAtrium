# 10 — Open Questions & Verify-on-Target

Tracked decisions still open (❓) and design points that are sound on paper but
must be confirmed on a real System/emulator/core before we depend on them (🔬).

> **De-risk in progress — see [11-derisk-log.md](11-derisk-log.md).** Most API-shape
> items below are now ✅ confirmed from Apple's System 7.1 headers (L1 launch
> shape, L2 Gestalt, L4 aliases, R1/R2 depth, S4 return-to-Finder, shutdown,
> control-panel folders). Emulator chosen: **Snow**. The remaining 🔬 items are
> *runtime behavior* needing a build + emulator run.
>
> **Q0 (toolchain for the empirical spike) — RESOLVED ✅:** build + Snow both run
> on the other machine (Retro68 + Snow live there). The repo carries a
> self-contained runbook in `spikes/launch-return/` (source + `CMakeLists.txt` +
> build/inject/debug steps). Next action: pull the repo onto that machine and run
> the launch-return spike to settle the L1/L3/S* runtime items below.

> **🔬 PARKED ISSUE — System-6 launches need the Process Manager (MultiFinder).**
> The bare 6.0.8 appliance runs MacAtrium as the shell with no Finder/MultiFinder,
> so the classic Segment-Loader launch can't set a launched app's working directory
> → companion-file apps bomb with **`File System error -43`** (verified: Prince of
> Persia), and there's no launch-and-return. Fix needs MultiFinder (already
> installed on the base, not activated). **Spike plan + handoff:
> [19-multifinder-process-manager-spike.md](19-multifinder-process-manager-spike.md)**
> (Spike A: MacAtrium as the shell under MultiFinder — priority; B: startup app;
> C: keep 6.0.8 for self-contained games, route the rest to 7.1/9.2.2).

## Open decisions (❓)

| # | Question | Leaning / notes |
|---|----------|-----------------|
| Q1 | **Name = MacAtrium** ✅. **Creator code** placeholder `ATRM` — register/confirm unique before release so prefs/files aren't orphaned. | |
| Q2 | Canonical **path form** in the catalog: colon-style `Vol:Games:X` vs slash-style. | The *on-Mac* file is read by the shell → likely colon-style; `rb-cli` examples use `/`. Pin one and have rusty-backup emit it. |
| Q3 | Default **shipping boot mechanism**: Startup-Items (B) vs boot-block shell swap (C). | Start with B (reversible); evaluate C for the "pure" appliance. |
| Q4 | **Recommendations dataset** on-disk format (one file per title? TOML/JSON?). | Optimize for clean PR diffs. |
| Q5 | **Config/prefs** final location (app folder vs shared vs System prefs) + in-app editing. | Deferred with kiosk mode; MVP uses an externally-edited file near the catalog. |
| Q6 | **Kiosk lockdown** scope (exit password, trap cmd-keys) — if/when. | Deferred 🕗. |
| Q7 | Catalog **scale** expectations (dozens? hundreds? thousands of items?). | Drives memory strategy + whether paging/search is mandatory early. |
| Q8 | Single binary vs **per-system binaries** if 6.0.8 forces divergence. | Single preferred; per-system is the accepted fallback. |

## Verify on target (🔬)

Group these into a short "spike" before/within Milestone 1–2. Each needs
checking on the relevant emulator/core and ideally real hardware.

### Launch & lifecycle
- L1: Extended `Launch` with `launchContinue` **returns control** on System 7.1,
  7.5.5, 7.6.1 **and** 6.0.8+MultiFinder — same flags, same behavior.
- L2: `Gestalt(gestaltOSAttr)` selectors `gestaltLaunchControl` /
  `gestaltLaunchCanReturn` mean what we think on each system.
- L3: Crash-on-launch of the **startup app** is recoverable (rescue image, safe
  screen) — don't wedge a boot.
- L4: **Alias Manager** availability/behavior across targets for robust target
  resolution.

### Becoming the shell
- S1: Startup-Items auto-launch + covering/hiding the Finder desktop behaves on
  each system.
- S2: **Hiding the menu bar** (or running full-screen under it) — version
  sensitivity.
- S3: Boot-block **shell-name swap** layout and behavior (and whether `rb-cli`
  should gain a helper for it).
- S4: **Return to Finder** — `SetFrontProcess` on Finder (creator `MACS`) when
  resident; explicit launch otherwise.

### Rendering & environment
- R1: `Gestalt(gestaltQuickdrawVersion)` thresholds for Color QD vs 32-bit QD
  (thousands) on each target.
- R2: Reading per-screen **depth** from `GetMainDevice()` PixMap reliably.
- R3: Chicago at higher resolutions — does scaling layout (not glyphs) look
  right at 1024×768? Need a bigger header face?
- R4: `GWorld` (color) and off-screen `BitMap` (B&W) compose + `CopyBits`
  cleanly at every depth/resolution, incl. 512×342.

### System settings
- C1: Which of Monitors / Sound / Date & Time / Mouse / Keyboard are **launchable
  apps** vs **`cdev`s** on each system.
- C2: Whether a reliable "open this `cdev` from a non-Finder shell" path exists,
  or we only flag-and-defer them.

### Content pipeline
- P1: rusty-backup **`scan`/`catalog`** subcommand design + that emitted JSONL
  (CR, MacRoman, `TEXT`) parses on-Mac.
- P2: **UTF-8 → MacRoman** transcoding for names/desc at emit time.
- P3: PNG → **PICT** build-time conversion + depth variants render correctly.

### Input
- I1: How each **MiSTer Mac core** surfaces gamepad buttons (keys vs mouse);
  finalize the recommended joystick→key map per core.

## How we'll burn these down

Most 🔬 items resolve by building Milestone 1 (forces L*, R*, env probes) and
Milestone 2 (forces S*, C*). Keep this doc updated as each is confirmed —
promote answers into the relevant numbered doc and mark the item ✅.
