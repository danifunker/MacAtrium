# 16 — Startup Items: the 7.x shipping default

**Decision (locked ✅):** for **System 7.x**, MacAtrium ships as a **Startup
Item** (approach **B** in [05-finder-replacement.md](05-finder-replacement.md)).
The Finder boots normally and then launches MacAtrium, which goes full-screen.
The boot-block shell-swap (**C**) and the System 6 path stay **deferred** — C is a
later "pure appliance" option, and System 6 is [Milestone 4](09-roadmap.md).

Why B is the lock-in for 7.x:

- Proven end-to-end in Snow on 7.0.1 / 7.1 / 7.5.5 (see [13-handoff.md](13-handoff.md) §1).
- The Finder stays resident, so **Launch Finder** works (bring-to-front) and a
  crash on launch drops to a usable Finder instead of wedging the boot — the boot
  stays recoverable by design.
- Zero boot-block surgery; fully reversible by removing one item.
- Trade-offs we accept: a brief Finder-desktop flash before we cover it, and the
  Finder's background memory cost. Fine for a curated appliance.

## How Startup Items work (System 7)

System 7 launches everything inside **`{System Folder} ▸ Startup Items`** once the
Finder has finished loading, in **alphabetical order**. Items may be applications
*or aliases* to them (System 7 has the Alias Manager). `{System Folder}` means the
**blessed/active** System Folder — the one with the small Mac badge on its icon
(on some test images it's literally named e.g. `System Folder 7.0.1`).

> System **6** has no Startup Items folder — that's a System 7 feature. The 6.0.8
> path is the boot-block swap and is tracked under [Milestone 4](09-roadmap.md).

## Add MacAtrium by hand (real or emulated 7.x)

1. In the Finder, find the **MacAtrium** application.
2. **Make an alias** of it: select it, **File ▸ Make Alias** (⌘M). (An alias is
   preferred so the app can stay with its `/MacAtrium` tree; dropping the app
   itself in works too.)
3. Open the **blessed System Folder**, then its **Startup Items** folder. If
   there isn't one, create a folder named exactly `Startup Items` inside the
   System Folder.
4. Drag the alias (or the app) into **Startup Items**.
5. **Restart.** The Finder loads, then MacAtrium launches full-screen.

**Remove / disable:** drag the MacAtrium item out of Startup Items (Trash the
alias, or move the app back to where it lives) and restart. As a universal escape,
**Restart** always gets you a normal boot, and **Launch Finder** in the Esc menu
brings the Finder forward without rebooting.

**If multiple Startup Items exist:** they run alphabetically. MacAtrium is the
shell, so it normally wants to be the only one (or last); rename to control order
if needed.

## Add MacAtrium automatically (build tooling)

The image tooling does all of the above for you:

- **`atrium image --config build.json`** installs the launcher into Startup Items
  as the last step of the build. The target folder is the `startup_items` config
  key (default **`/System Folder/Startup Items`**); the tool creates it if missing
  and writes the launcher there as MacBinary. Point `startup_items` at the blessed
  folder's name if your base image differs (e.g.
  `"/System Folder 7.0.1/Startup Items"`).
- **`tools/snow-harness/assemble.sh <src.hda> <out.hda> [startup_items_dir]`** does
  the same for quick hand-built test images (defaults to
  `/System Folder/Startup Items`; falls back to the volume root when the folder
  doesn't exist, e.g. System 6).

Both place the launcher **directly** in Startup Items (not an alias) — simplest
for a generated appliance image, where the app's home *is* Startup Items.

## What the user sees at boot

Finder desktop appears briefly → MacAtrium launches and covers it full-screen →
keyboard/gamepad drives the list. The Finder is still running underneath: **Esc ▸
Launch Finder** brings it forward, and **Restart** / **Shut Down** are in the same
menu. Nothing here deletes or replaces the Finder file (we avoid approach D in
[05-finder-replacement.md](05-finder-replacement.md)).
