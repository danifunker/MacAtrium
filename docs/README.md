# Classic Mac Finder Replacement — Planning Docs

A keyboard-driven launcher shell for classic Mac OS, inspired by the
**AmigaVision** boot menu. It replaces the Finder as the startup application
and presents a curated, categorized list of games, applications, and system
settings, plus Shutdown/Restart — all navigable from a tiny key set so it works
equally well from a keyboard or a MiSTer gamepad.

Name: **MacAtrium** · creator code (placeholder): `ATRM` (register/confirm unique before release).

## How to read these docs

Read in order for the full picture; jump by topic once oriented.

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
| — | [11-derisk-log.md](11-derisk-log.md) | **De-risk findings** — APIs confirmed from Apple headers, emulator choice (Snow), what's left to test |
| — | [12-mvp-handoff.md](12-mvp-handoff.md) | The original "build the MVP" prompt (done) |
| — | [13-handoff.md](13-handoff.md) | **Start here to resume** — post-MVP status, environment, and the build-out (image tooling, GUI, CI, then full 7.x) |
| — | [13-iterate-session.md](13-iterate-session.md) | Paste-in prompt for a **sustained, fast-iterating** session there (one-command loop + autonomy) |
| — | [14-art-render-snow-bug.md](14-art-render-snow-bug.md) | **Resume prompt** — in-launcher art `DrawPicture` crashes Snow on some valid art; fix via CopyBits |
| — | [15-settings-and-color-depth.md](15-settings-and-color-depth.md) | Settings panel (theme/depth/volume), runtime colour depth, the PICT word-align fix |
| — | [16-startup-items.md](16-startup-items.md) | **Locked: Startup Items (B) is the 7.x shipping default** + how to install MacAtrium there |
| — | [17-prefs-persistence.md](17-prefs-persistence.md) | Theme / volume / last-selection persist across reboot (`MacAtrium Prefs` file) |

Spikes (focused experiments) live in [`../spikes/`](../spikes/) — currently
`launch-return/`, the keystone test that proves resident launch returns control.

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
