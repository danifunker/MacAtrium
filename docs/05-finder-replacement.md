# 05 — Becoming the Boot Experience

Goal: when the machine boots, **our shell** comes up instead of the Finder —
while keeping the real Finder installed and reachable. This doc covers the
mechanisms, the safe dev path, and the return-to-Finder escape hatch.

## Background: how the Mac chooses a "Finder"

On classic Mac OS the **boot blocks** (the first two logical blocks of a
bootable HFS volume) name the **System file** and the **shell** application —
by default `"Finder"`. The ROM/System launches the System file, which launches
the named shell. The shell is whatever process "owns" the user session; replace
it and you replace the boot experience.

There are a few ways to put ourselves in that seat, in increasing intrusiveness:

| Approach | What it does | Risk | Reversibility |
|----------|--------------|------|---------------|
| **A. Dev mode** | Ship as a normal `APPL`; user double-clicks it over a normal Finder boot | none | trivial — just quit |
| **B. Startup Items** | Drop an alias in **System Folder → Startup Items**; Finder still boots, then launches us; we go full-screen and hide Finder | low | remove the alias |
| **C. Boot-block shell swap** | Patch the volume's boot blocks to name our app as the shell instead of `Finder` (Finder file untouched on disk) | medium | restore boot blocks / rename |
| **D. Replace `Finder`** | Rename/replace the Finder file itself | high | need the original Finder back |

**Decision (from [01-decisions.md](01-decisions.md)):** keep the Finder
installed; auto-launch ours. We use **A** for development and **B/C** for the
"real" appliance experience. We do **not** delete or overwrite the Finder (avoid
**D**). ✅ **Settled for 7.x: B (Startup Items) is the shipping default** — proven
end-to-end (M1). **C** (boot-block swap) is deferred to a later "pure appliance"
build / System 6. Install details: [16-startup-items.md](16-startup-items.md).

### Why B (Startup Items) is attractive first

- Finder boots normally, then launches us → we still have a healthy Process
  Manager and a Finder we can return to.
- Zero boot-block surgery; fully reversible by deleting one alias.
- Trade-off: the Finder desktop flashes briefly before we cover it, and the
  Finder stays resident in the background (memory cost). For a curated appliance
  that's usually fine.

### Why C (boot-block swap) is the "pure" version

- No Finder desktop appears at all; our shell is *the* shell.
- Cleaner appliance feel, closer to the AmigaVision experience.
- `rusty-backup` already manipulates HFS images at the block level, so a
  `rb-cli` helper to read/rewrite the boot-block shell name is a natural fit
  (potential new subcommand). 🔬 Verify the boot-block layout we target and that
  swapping the shell name behaves across 6.0.8/7.x.

## Going "full screen" over the Finder (approach B)

When launched as the shell-on-top we want to look like *the* environment:

- Open a window covering the whole `GetMainDevice()` bounds (or draw to the
  screen under the menu bar). Decide whether to **hide the menu bar** (
  `LMSetMBarHeight(0)` style tricks) or keep a minimal one. 🔬 menu-bar hiding is
  fiddly and version-sensitive — verify.
- Under MultiFinder/Process Manager, optionally hide other layers so the Finder
  desktop isn't visible behind us.
- Pull keyboard focus and run our event loop as the foreground app.

## Return to Finder (escape hatch)

A top-level **"Launch Finder"** action that brings the real Finder forward:

- If Finder is still resident (approach B), bring it to front
  (`SetFrontProcess` on the Finder's `ProcessSerialNumber`, found by scanning
  the process list for creator `MACS`).
- If we are the *only* shell (approach C), launch the Finder app explicitly via
  the same sub-launch path we use for everything else (see
  [08-launching-system.md](08-launching-system.md)).
- Document the simple, reliable fallback for the user: **reboot** to get the
  normal boot back (or a normal boot if the appliance disk is configured for it).

🔬 Exact Process Manager calls and Finder visibility behavior differ across
systems — verify on each target.

## Safety rules while developing

1. **Never** test boot-block/shell changes on a disk you can't recreate. Always
   work on a **copy** of the System image (we generate them with `rb-cli`).
2. Keep a known-good **rescue image** that boots straight to Finder.
3. Prefer **dev mode (A)** for day-to-day work; only exercise B/C when
   specifically testing the boot path.
4. Because the shell is the startup app, a crash on launch can wedge the boot —
   build the "no catalog / safe" screen (see [03-architecture.md](03-architecture.md))
   early so failures are recoverable on-screen rather than a hang.

## Kiosk lockdown (deferred 🕗)

Later we may add At-Ease-style lockdown: suppress "Launch Finder", require a
password to exit, trap command-key escapes. Out of MVP scope; tracked in
[10-open-questions.md](10-open-questions.md).
