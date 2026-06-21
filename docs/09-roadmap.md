# 09 — Roadmap & Milestones

Phased so each milestone is a runnable thing on a real System, lowest-risk first.
System 7 (resident, easiest) leads; System 6 and richer visuals layer on.

## MVP (Milestone 1) — "launch and return"

**Definition (locked):** 68k · System 7 · reads the catalog JSONL · launches an
app and returns · Shutdown/Restart · B&W + 256-color rendering.

Scope:

- [ ] Retro68 build producing a 68k `APPL`; `rb-cli` assembles a bootable System
      7 test image (see [04](04-toolchain-build.md)).
- [ ] Toolbox init + `env` probes (OS version, Color QD, screen bounds/depth,
      launch capability, Shutdown Mgr).
- [ ] `json.c` JSONL parser (CR/LF/CRLF-tolerant, MacRoman) + `catalog`/`model`
      loading a **hand-authored** `data/catalog.jsonl`.
- [ ] List UI: categories + "All", ↑↓ select, ←→ category, Return launch, Esc
      menu; layout computed from screen rect. Chicago font.
- [ ] Two render backends wired (B&W + Color), backend chosen at startup. (256
      first; 16 and thousands can be Milestone 3.)
- [ ] Sub-launch via the resident `Launch` path; return to a preserved selection.
- [ ] Esc menu with **Restart** and **Shut Down** via Shutdown Manager.
- [ ] The "no catalog / safe" fallback screen (so a bad boot is recoverable).
- [ ] **Runs as a normal app (dev mode)** over a normal Finder boot.

**Exit criteria:** boot the image (or run in dev mode) → pick a game → it
launches → quit it → back in the shell with selection intact → Restart works.

## Milestone 2 — actually the boot shell

- [ ] Startup-Items auto-launch (approach B) on a **copy** image; full-screen
      over the Finder, hide/cover the desktop.
- [ ] **Launch Finder** action (resident bring-to-front + reboot fallback).
- [ ] Settings menu: enumerate Control Panels, launch the app-like ones, flag
      the `cdev`-only ones ([08](08-launching-system.md)).
- [ ] Decide & document the default shipping mechanism (B vs boot-block swap C).

## Milestone 3 — visuals & breadth

- [ ] 16-color and thousands backends; theme system + a few presets.
- [ ] Real app icons (`ICN#`/`icl8`) next to entries.
- [ ] Type-ahead search; per-item hotkeys.
- [ ] Detail pane / two-column layout at 800×600 and 1024×768.
- [ ] Tune layouts across all five resolutions incl. 512×342 B&W.

## Milestone 4 — System 6.0.8

- [ ] Validate the single binary on 6.0.8 **+ MultiFinder**; fix Gestalt/trap
      assumptions; confirm the resident `Launch` path there.
- [ ] B&W path on a MacPlus-class config (Mini vMac / MacPlus core).
- [ ] If a single binary proves impractical, cut a 6.0.8 build variant (fallback
      only).

## Milestone 5 — content pipeline productionized

- [ ] New **`rb-cli scan`/`catalog`** subcommand: walk an HFS volume → emit
      `catalog.jsonl` (CR, MacRoman, `TEXT`), merging the recommendations dataset.
- [ ] Artwork: build-time PNG→PICT, depth variants, `art` wired into the UI.
- [ ] `data/recommendations/` dataset seeded + a CONTRIBUTING flow for PRs.
- [ ] One-command "build a ready-to-boot appliance image".

## Milestone 6 — MiSTer & hardware polish

- [ ] Verify input mapping on MacPlus / Mac LC / Mac II cores; ship a
      recommended joystick→key map.
- [ ] Test on real 68k hardware.
- [ ] Performance pass (redraw, large catalogs, memory footprint).

## Later / deferred (🕗)

- Kiosk lockdown (exit password, trap escapes).
- App-driven depth/resolution switching (Display Manager / `SetDepth`).
- In-app catalog/theme editing.
- Native PowerPC build.
- CI building + publishing images.

## Risk-ordered "verify early" list

These are the assumptions most likely to bite; prove them in Milestone 1–2 (full
list in [10-open-questions.md](10-open-questions.md)):

1. Resident `Launch` flags actually return control on each target. 🔬
2. Boot path is recoverable when the shell crashes on launch. 🔬
3. Covering the Finder / hiding the menu bar behaves across systems. 🔬
4. Single 68k binary really runs unmodified on 6.0.8 + 7.x. 🔬
