# 01 — Locked Decisions (the Spec)

This is the **authoritative** record of what we agreed. If another doc disagrees
with this one, this one wins. Status markers: ✅ locked, 🔬 verify on target,
🕗 deferred, ❓ open.

## Toolchain & language

- ✅ **Retro68** as the cross-compiler (build 68k Mac binaries from a modern Mac;
  no emulation needed to compile). See [04-toolchain-build.md](04-toolchain-build.md).
- ✅ **C** against the Universal Interfaces (Toolbox). No C++/Pascal.
- ✅ **68k only.** One architecture, runs on every target system and on Power
  Macs / MiSTer via 68k. Native PPC is a 🕗 non-goal.

## Target systems

- ✅ System **7.1, 7.5.5, 7.6.1** — first-class, built and validated first.
- ✅ System **6.0.8** — hard requirement, delivered **after** 7.x is working.
- ✅ **MultiFinder is required** under System 6 (gives us a resident shell and
  one launch code path with System 7 — see [03-architecture.md](03-architecture.md)).
- ✅ **Prefer a single binary** across all four systems. If a clean single
  binary proves impossible, separate release binaries are acceptable but
  explicitly the fallback, not the goal.

## Color & resolution

- ✅ Support **B&W (1-bit)**, **16-color (4-bit)**, **256-color (8-bit)**, and
  **thousands (16-bit)**. Detect bit depth at runtime and adapt.
- ✅ Support **512×384, 640×480 (default), 800×600, 1024×768**, plus tolerate the
  **512×342** compact built-in screen. Layout is computed from the actual screen
  bounds, not hardcoded. See [02-compatibility.md](02-compatibility.md).
- 🕗 The app **setting** depth/resolution itself (Display Manager / `SetDepth`)
  is a future nice-to-have. MVP renders at whatever the user configured.

## Finder replacement & lifecycle

- ✅ **Keep the Finder installed; auto-launch our shell over it.** Cleaner than
  deleting/renaming Finder and far safer to iterate on.
- ✅ Provide a **"Launch Finder"** action; the user reboots to get the normal
  Finder boot back. Treated as **full replacement** day to day, with the real
  Finder reachable on demand.
- ✅ A **dev mode**: run the shell as an ordinary app over a normal Finder boot,
  so we can iterate without committing to the boot path. See
  [05-finder-replacement.md](05-finder-replacement.md).
- 🕗 **Kiosk lockdown** (password to exit, can't drop to Finder) — deferred,
  revisit later.
- ✅ **Shutdown** and **Restart** actions. ✅ **No Sleep.**

## Content model

- ✅ One on-volume root folder **`/MacAtrium/`** with `Apps/` (the apps & games /
  aliases), `metadata/` (the catalog the app reads), `images/` (curated PICT art).
  Self-contained + relocatable; paths in the catalog are relative to this root.
- ✅ App reads a **light index** `metadata/catalog.jsonl` at boot and **lazy-loads
  images** from `images/` as the user navigates. Heavy lifting is host-side; the
  68k app stays light. Schema in [06-content-pipeline.md](06-content-pipeline.md).
- ✅ **Categories live in metadata, not the folder layout** → an item can be in
  **many categories** (`categories` array). Synthetic **"All"** = the union.
- ✅ Curated metadata is a **JSON dataset in this repo** (`data/`), PR-friendly,
  enriched at build time from **existing databases — LaunchBox** (Apple Macintosh
  platform) primary, Macintosh Garden / MobyGames as supplements.
- ✅ **Host tooling** (modern Mac) generates `catalog.jsonl` + converts art to
  PICT; **rusty-backup** injects the `/MacAtrium` tree. A built-in `scan` in
  rusty-backup is optional (host tool can own generation + call `rb-cli` for I/O).
- ✅ Per-item metadata: **id, name, categories[], app path** (required);
  **type/creator, year, description, image** (optional).

## UI & input

- ✅ **Chicago** as the primary font for the authentic Mac look.
- ✅ A default **"Mac" palette**, **customizable** via theme settings.
- ✅ Navigation: **Up/Down** moves selection, **Left/Right** pages, **Return**
  launches, **Esc** backs out. Type-ahead and per-item hotkeys are nice-to-have.
- ✅ **Keyboard-first; mouse also clickable.** Navigation never requires the mouse.
- ✅ **MiSTer controller support** = keep the control surface to a minimal key set
  so the core's joystick→keyboard mapping drives it. We do **not** read a gamepad
  directly. See [07-ui-ux.md](07-ui-ux.md).
- ✅ Artwork roadmap: **text-only MVP → real app icons → custom artwork**.

## System settings exposed

- ✅ Surface **Monitors, Sound, Date & Time, Mouse, Keyboard**. Launch the ones
  that behave as standalone apps; **flag** the classic `cdev`-only ones that are
  awkward to open without the Finder. See [08-launching-system.md](08-launching-system.md).

## Config & persistence

- ✅ Config/prefs live in the **app's own folder or a shared location** on the
  volume (not buried in System prefs for MVP), externally editable.
- 🕗 In-app editing and final prefs location — revisit with kiosk mode.

## MVP

- ✅ **68k, System 7, reads the catalog JSONL, launches an app and returns**,
  with Shutdown/Restart. B&W + 256-color rendering. Everything else layers on.
  Full breakdown in [09-roadmap.md](09-roadmap.md).
