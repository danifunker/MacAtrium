# 00 — Vision, Goals, Non-Goals

## The idea

A **keyboard-driven menu system** that boots in place of the Finder and turns a
vintage Mac into an appliance: power on, land in a clean, curated menu, pick a
game or app with the arrow keys, hit Return, play, quit, and you're right back
in the menu. The model is the **AmigaVision** boot shell — a fast, legible,
controller-friendly front end that hides the file system behind a curated list.

The target machine is as likely to be a **MiSTer FPGA core** (MacPlus, Mac LC,
Mac II) as it is real hardware, so the experience must feel good with a gamepad,
not just a keyboard and mouse.

## Goals

1. **Replace the Finder as the boot experience** — the user should never need to
   see the desktop to launch what they care about.
2. **Keyboard-first, controller-friendly** — the entire UI is driven by a tiny
   key set (arrows, Return, Esc, Page Up/Down) so a MiSTer joystick→key mapping
   covers it. Mouse works too but is never required for navigation.
3. **Curated, categorized library** — games and apps grouped into categories,
   with an "All" view, sourced from a data file rather than the file system.
4. **Broad OS coverage from one binary** — System 6.0.8 (with MultiFinder), 7.1,
   7.5.5, 7.6.1, ideally a single 68k build.
5. **Adaptive visuals** — looks right in B&W and in color, from a 512-wide
   compact screen to 1024×768, detecting and adapting at runtime.
6. **Authentic Mac feel, customizable** — Chicago font, classic chrome, a
   default "Mac" palette that the user can re-theme.
7. **Reproducible content pipeline** — the library (catalog + artwork) is
   generated and injected into the boot image by **rusty-backup**, so building a
   ready-to-run disk is scriptable.

## Non-goals (at least for now)

- **Not a file manager.** No desktop, Trash, copy/eject/format dialogs, or
  Finder AppleEvents. We deliberately give those up by replacing the Finder
  (see [05-finder-replacement.md](05-finder-replacement.md)); the escape hatch
  is "return to the real Finder, then reboot."
- **Not a general application.** It is a single-purpose shell, not a windowed
  multi-document app.
- **No networking / online catalog.** The library is local; curation happens at
  build time and via PRs to a recommendations dataset, not at runtime.
- **No PowerPC-native build initially.** 68k only — it runs everywhere these
  systems run, including on Power Macs via the 68k emulator and on MiSTer.
- **No sleep/power management UI.** Shutdown and Restart only.

## What "done" looks like for the MVP

Boot a System 7 disk image (68k) → the shell appears → arrow to a game in a
category → Return launches it → quit the game → back in the shell, selection
preserved → Shutdown/Restart work. The catalog comes from a JSONL file on the
volume. See [09-roadmap.md](09-roadmap.md).
