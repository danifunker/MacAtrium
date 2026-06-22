# 17 — Preferences persistence (theme / volume / selection)

MacAtrium's launcher preferences now survive a reboot — previously theme, volume,
and the selection reset every boot (docs/15). Implemented as **Track B**.

## What persists (and what doesn't)

| Pref | Restored at startup | Why |
|------|---------------------|-----|
| **Theme** (Dark/Light) | `render_set_theme` before first draw | a launcher preference |
| **Volume** (0..7 alert) | `sound_apply_vol` (silent — no boot beep) | SysBeepVolume resets on reboot (docs/15) |
| **Last selection** | `model_select(category, item-id)` | continuity |
| ~~Colour depth~~ | **not persisted** | startup matches the OS depth — the locked principle in docs/15; depth is a *system* setting (Monitors), not ours to force on every boot |

## The file

A tiny `key=value` text file, **`MacAtrium Prefs`** (type `pref`, creator `ATRM`),
in the System's Preferences folder — located with
`FindFolder(kOnSystemDisk, kPreferencesFolderType, …)`, so it follows the blessed
System Folder. CR-terminated; the reader tolerates CR/LF/CRLF and ignores unknown
keys, so the format can grow compatibly.

```
theme=light
volume=5
category=Games
item=lemmings
```

The selection is stored by **category name + item id**, not by index, so it
survives catalog rebuilds: a missing category falls back to "All", a missing item
to the first row (`model_select` returns 1 only on an exact hit).

## When it's written

`save_prefs()` snapshots theme + volume + current selection and writes the whole
file at the moments that matter, never per-keystroke:

- a persisted setting changes — the UI returns **`UI_PREFS_DIRTY`** from the `T`
  theme toggle and from Theme/Volume changes in the Settings panel (depth changes
  return `UI_NONE`);
- before handing control away — on **launch**, **Restart**, and **Shut Down**.

## Code

- `src/prefs.{c,h}` — load/parse + save (reuses `macfs_read_all`; writes via
  `FSpCreate`/`FSpOpenDF`/`FSWrite`/`FlushVol`).
- `src/model.c` — `model_select(category, item-id)` (pure C, unit-tested).
- `src/sound.c` — `sound_apply_vol` (set volume without the feedback beep).
- `src/ui.{c,h}` — `UI_PREFS_DIRTY` signals a persisted change to `main`.
- `src/main.c` — `prefs_load` + apply at startup; `save_prefs` at the triggers above.

## Verified (Snow, System 7.1, Mac II)

Each half of the round-trip was proven headlessly; both pass.

- **Load + apply** — a pre-seeded `MacAtrium Prefs` (`theme=light`,
  `category=Games`, `item=lemmings`) → the launcher boots in **light theme**, on
  **Games**, with **Lemmings** selected (defaults would be dark / "All" / first
  row). [evidence/prefs-restored-light-games-lemmings.png](evidence/prefs-restored-light-games-lemmings.png)
- **Save (no freeze)** — fresh boot (no prefs), `T` toggles the theme (a prefs
  write) and the UI stays responsive: navigation after the write moves the
  selection, a second toggle writes again — no hang.
  [evidence/prefs-save-no-freeze-light.png](evidence/prefs-save-no-freeze-light.png)

**Caveat (unchanged from docs/13 §6):** the headless `macatrium_harness` doesn't
sync guest writes back to the `.hda`, so the *full cross-boot round-trip* (write
in one boot, read in the next) can't be exercised here — it needs an interactive
Snow / MAME / real hardware run. Writes themselves complete with `err=0` and no
freeze, and the load+apply path is proven against a real on-disk file, so the
round-trip is expected to work; flag it for a non-headless confirmation.
