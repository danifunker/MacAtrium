# 09 — Roadmap & Milestones

Phased so each milestone is a runnable thing on a real System, lowest-risk first.
System 7 (resident, easiest) leads; System 6 and richer visuals layer on.

## MVP (Milestone 1) — "launch and return"

**Definition (locked):** 68k · System 7 · reads the catalog JSONL · launches an
app and returns · Shutdown/Restart · B&W + 256-color rendering.

Scope (✅ verified end-to-end on System 7.5.5 / Mac II in Snow, 2026-06-21 —
see [11 §C′](11-derisk-log.md) and [evidence/](evidence/)):

- [x] Retro68 build producing a 68k `APPL` (`build/MacAtrium.bin`, creator
      `ATRM`); `rb-cli` assembles a bootable System 7 test image
      ([tools/snow-harness](../tools/snow-harness/)).
- [x] Toolbox init + `env` probes (OS version, Color QD, screen bounds/depth,
      launch capability, Shutdown Mgr).
- [x] `json.c` JSONL parser (CR/LF/CRLF-tolerant, MacRoman) + `catalog`/`model`
      loading `data/catalog.jsonl` (+ 45 off-target unit tests in `tests/`).
- [x] List UI: categories + "All", ↑↓ select, ←→ category, Return launch, Esc
      menu; layout computed from screen rect. Chicago font.
- [x] Render backends wired; backend chosen at startup. **B&W exercised** (the
      7.5.5 test ran at 1-bit); the Color (256) backend is built but still needs
      a colour-depth run to exercise.
- [x] Sub-launch via the resident `Launch` path; **return to a preserved
      selection** (the keystone — proven).
- [x] Esc menu with **Restart** and **Shut Down** via Shutdown Manager (Restart
      verified to reboot cleanly).
- [x] The "no catalog / safe" fallback screen (built in `ui.c`; shown when the
      catalog is missing/empty).
- [x] **Runs auto-launched over a normal Finder boot** (Startup Items) — the
      dev-mode-plus path; double-click-as-normal-app is the same binary.

**Exit criteria — met:** booted the image → selected SimpleText → it launched →
Cmd-Q quit it → back in the shell with selection intact → Restart reboots. ✅

## Milestone 2 — actually the boot shell

- [x] Startup-Items auto-launch (approach B) on a **copy** image; full-screen
      over the Finder, hide/cover the desktop. (Proven in M1.)
- [x] **Decided & documented the default shipping mechanism: Startup Items (B)
      is the 7.x default**; boot-block swap (C) deferred to a later "pure
      appliance" build / System 6. See [16-startup-items.md](16-startup-items.md).
- [x] **Show Finder** action (resident bring-to-front, restores the menu bar) +
      **Quit to Finder** (`Cmd-Option-Q`, `ExitToShell`). Restart is the fallback.
- [ ] Settings menu: enumerate Control Panels, launch the app-like ones, flag
      the `cdev`-only ones ([08](08-launching-system.md)).

## Milestone 3 — visuals & breadth

- [ ] 16-color and thousands backends; theme system + a few presets.
- [~] Real app icons — `ICN#` harvested from the app and shown as **fallback
      art** when a title has no box art (`atrium icon` + launcher `.icon.raw`).
      Still open: icons *next to* list entries, and `icl8` colour icons.
- [ ] Type-ahead search; per-item hotkeys.
- [ ] Detail pane / two-column layout at 800×600 and 1024×768.
- [ ] Tune layouts across all five resolutions incl. 512×342 B&W.

## Milestone 4 — System 6.0.8

Groundwork done (see [11 §C″](11-derisk-log.md)): 6.0.8 boots on the Mac II and
MultiFinder activates via the boot-block shell swap (S3 ✅). Two confirmed
blockers to fix:

- [x] **Replace FSSpec calls for System 6.** Done — `macfs.c` resolves paths by
      walking dir IDs (`PBGetCatInfo`) and opens/reads/creates via
      `HOpen`/`HGetFInfo`/`HCreate` (classic HFS calls that work on 6.0.8 **and**
      7.x — one code path, no FSSpec trap). Converted the other call sites too:
      `launch.c` (`FSpGetFInfo`→`macfs_get_finfo`, plus a Gestalt-guarded
      `ResolveAliasFile` — the Alias Manager is 7-only), `prefs.c` (build the spec
      directly + `macfs_create`/`macfs_open_df`), `sound.c`
      (`FSpOpenResFile`→`HOpenResFile`). **No FSSpec-trap calls remain.**
- [x] **Guard `WaitNextEvent`** — version-gated: System 7+ uses `WaitNextEvent`,
      6.x falls back to `GetNextEvent` + `SystemTask` (which still yields under
      MultiFinder). Set from the probed `gestaltSystemVersion`.
- [x] **No regression** — the new file code is verified on **System 7.1** (Mac II,
      Snow): boots, loads the 12-item catalog, renders metadata + art.
- [ ] **Deploy + validate on 6.0.8 + MultiFinder** (the open piece). The code is
      ready; the blocker is *deployment*: Startup-Items auto-launch is System-7-only.
      🔬 **Tried the boot-block shell swap and it did NOT take in Snow on
      `MacLC_6-0-8.hda`** (2026-06-23): the Apple_HFS partition / boot block is at
      sector 96 (byte 49152, confirmed via the partition map), and rewriting
      `bbShellName` there to **"MacAtrium"** *or* **"MultiFinder"** (verified the
      bytes persisted) both still booted the **plain Finder** — so this Snow build
      ignores `bbShellName` for this disk, contradicting the earlier §C″ note.
      Next: investigate Snow's 6.0.8 shell selection (boot `2` code resource? a
      `bless`/`make-bootable` step? interactive Snow?) or a different auto-launch
      (a real MultiFinder "Set Startup"). Until then the launcher is **code-ready
      but unverified-at-runtime on 6.0.8**. (A `set_boot_shell` helper was drafted
      then reverted — patching the field is correct, but it doesn't drive the boot,
      so it wasn't committed.)
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

1. Resident `Launch` flags actually return control on each target. ✅ **7.0.1,
   7.1, 7.5.5** confirmed (Mac II, Snow) — incl. the real Prince of Persia;
   6.0.8 needs a Milestone-4 port; 7.6.1 not yet run.
2. Boot path is recoverable when the shell crashes on launch. ✅ for the
   Startup-Items deployment (a crash drops to the Finder); boot-block-swap also
   confirmed bootable (S3, see [11 §C″](11-derisk-log.md)).
3. Covering the Finder / hiding the menu bar behaves across systems. 🔬
4. Single 68k binary really runs unmodified on 6.0.8 + 7.x. 🔬
