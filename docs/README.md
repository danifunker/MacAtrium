# Classic Mac Finder Replacement — Planning Docs

A keyboard-driven launcher shell for classic Mac OS, inspired by the
**AmigaVision** boot menu. It replaces the Finder as the startup application
and presents a curated, categorized list of games, applications, and system
settings, plus Shutdown/Restart — all navigable from a tiny key set so it works
equally well from a keyboard or a MiSTer gamepad.

Name: **MacAtrium** · creator code (placeholder): `ATRM` (register/confirm unique before release).

## How to read these docs

Read in order for the full picture; jump by topic once oriented.

**Working in the code (agents + contributors):** start with
[CODE_GUIDELINES.md](CODE_GUIDELINES.md) — repo map, conventions, and the
invariants/gotchas that aren't obvious from the design docs.

| # | Doc | What it covers |
|---|-----|----------------|
| — | [00-vision.md](00-vision.md) | Why this exists, goals, non-goals |
| — | [01-decisions.md](01-decisions.md) | **Authoritative** locked decisions (the spec) |
| — | [02-compatibility.md](02-compatibility.md) | OS × hardware × QuickDraw × depth × resolution matrix |
| — | [03-architecture.md](03-architecture.md) | Process/launch model, modules, event loop |
| — | [04-toolchain-build.md](04-toolchain-build.md) | Retro68, C, project layout, image injection |
| — | [05-finder-replacement.md](05-finder-replacement.md) | Becoming the startup app, dev mode, return-to-Finder |
| — | [06-content-pipeline.md](06-content-pipeline.md) | Catalog JSONL schema, recommendations dataset, rusty-backup, artwork |
| — | [07-ui-ux.md](07-ui-ux.md) | Visual design, Chicago, theming, navigation, layout, MiSTer input |
| — | [08-launching-system.md](08-launching-system.md) | Launch trap, control panels, shutdown/restart |
| — | [09-roadmap.md](09-roadmap.md) | Milestones & phases, MVP definition |
| — | [10-open-questions.md](10-open-questions.md) | Deferred decisions and things to verify on real targets |
| — | [15-settings-and-color-depth.md](15-settings-and-color-depth.md) | Settings panel (theme/depth/volume), runtime colour depth, the PICT word-align fix |
| — | [16-startup-items.md](16-startup-items.md) | **Locked: Startup Items (B) is the 7.x shipping default** + how to install MacAtrium there |
| — | [17-prefs-persistence.md](17-prefs-persistence.md) | Theme / volume / last-selection persist across reboot (`MacAtrium Prefs` file) |
| — | 20–45 | Topic/design docs: MultiFinder set-startup deep-dive (20), category paging (21), multi-volume backlog (23), classic-UI redesign (27), image forks + OS chooser (36), **multi-disk libraries (37)**, compatibility matrix (38), **cross-disk startup chooser (42)**, **memory budget & art modes (44)**, **[CD-based titles: BlueSCSI Toolbox disc switching (45)](45-cd-based-titles.md)** |
| — | [09-roadmap.md](09-roadmap.md#shipped-since-m1m6--consolidated-backlog-2026-07-08) | **The consolidated backlog / outstanding items** live here |

Spikes (focused experiments) live in [`../spikes/`](../spikes/) — `launch-return/`
(the keystone test that proves resident launch returns control) and `startup-disk/`
(the cross-disk startup-device PRAM spike, [docs/42](42-cross-disk-startup-chooser.md)).

## Status legend

Used throughout the docs:

- ✅ **Locked** — decided, build to this.
- 🔬 **Verify** — design is sound on paper but must be confirmed on a real
  System/emulator before we rely on it.
- 🕗 **Deferred** — intentionally out of MVP scope; revisit later.
- ❓ **Open** — needs a decision (tracked in [10-open-questions.md](10-open-questions.md)).

## One-paragraph summary

Build a single **68k C** binary with **Retro68** that runs on System 6.0.8
(with MultiFinder), 7.1, 7.5.5, and 7.6.1. It auto-launches in place of the
Finder, reads a **catalog JSONL** describing the curated library, renders an
adaptive list UI (B&W → 16 → 256 → thousands of colors; 512×342/384 →
1024×768), and launches a selected app as a sub-process that returns control to
the shell when it quits. The catalog and artwork are produced and injected into
the boot image by **rusty-backup**. MVP = launch an app and come back.
