# 09 — Roadmap & Milestones

Phased so each milestone is a runnable thing on a real System, lowest-risk first.
System 7 (resident, easiest) leads; System 6 and richer visuals layer on.

## MVP (Milestone 1) — "launch and return"

**Definition (locked):** 68k · System 7 · reads the catalog JSONL · launches an
app and returns · Shutdown/Restart · B&W + 256-color rendering.

Scope (✅ verified end-to-end on System 7.5.5 / Mac II in Snow, 2026-06-21 —
see [evidence/](evidence/)):

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

**Essentially done (2026-06-24):** MacAtrium boots and runs on bare System 6.0.8
(no MultiFinder, as the boot shell), verified headlessly in Snow. The port was a
stack of System-7-trap guards; each is checked off below.

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
- [x] **Guard the remaining System-7 traps.** Each was an "unimplemented trap" /
      `dsMemFullErr` on bare 6.0.8: the off-screen `GWorld`/MultiFinder-temp-memory
      (now off on System 6 — direct draw), the Process Manager
      (`GetCurrentProcess`/`SetFrontProcess` in `bring_self_front`, and the
      `GetNextProcess` "Show Finder" path), the AppleEvent handlers, and
      `FindFolder` (→ `GetVol` for the boot volume; prefs go in `/MacAtrium` on 6.x).
      Control Panels (FindFolder + odoc) gated to 7+.
- [x] **Auto-launch + validated on 6.0.8.** 6.0.8 has no Startup Items folder, so
      `atrium image finder_replace:true` installs the launcher **as the Finder**
      (in the System Folder, typed FNDR/MACS); the boot launches it as the shell.
      The `bbShellName` swap alone doesn't work — the boot launches the *file named
      Finder*, so we replace that file. **VERIFIED headlessly:** a plain boot of the
      `finder_replace` 6.0.8 image comes straight up in the MacAtrium launcher (full
      UI + 8-bit colour art), as the bare boot shell (no MultiFinder) — the
      strictest case, which also covers MultiFinder + double-click.
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
   confirmed bootable (S3).
3. Covering the Finder / hiding the menu bar behaves across systems. 🔬
4. Single 68k binary really runs unmodified on 6.0.8 + 7.x. 🔬

## Shipped since M1–M6 + consolidated backlog (2026-07-08)

The milestones above predate several features that have since shipped and are
Snow-verified (detail in the per-feature docs). Recently landed:
- **Paged catalog** — `index.jsonl` + `cats/<slug>.jsonl`, on-demand pages ([21](21-category-paging.md)).
- **Per-item image resource forks + in-disk OS chooser/blesser** — `bless.c` + `run_os_chooser`, swap-verified.
- **Compatibility data model** — 155-model table, 5-tier CPU→OS ceilings, project floor 6.0.8 → **6.0.4**;
  wired into `env.c` (tier probe) + chooser gating + swap warning ([38](38-compatibility-matrix.md), `data/os-tiers.json`).
- **Multi-disk libraries** — aggregate independent `/MacAtrium` volumes at startup, per-disk `[N]` tags,
  MacAtrium Status screen ([37](37-multi-disk-libraries.md); branch `multi-disk-libraries`, PR pending).
- **Cross-disk startup chooser** — Phase 0 PRAM spike (`SetDefaultStartup`) hardware-verified
  ([42](42-cross-disk-startup-chooser.md); branch `cross-disk-chooser`).

### Outstanding / deferred (consolidated from the retired resume docs)
Per-feature detail lives in the cited docs.

**Colour / display**
- **Phase 1 colour-path verify** — per-item image forks + colour PICT proven B&W-only; needs real HW or a
  boot-8-bit disk (Snow Mac II+MDC tops out at 8-bit; the q800 QEMU harness is the only headless deep-colour
  path). ([15](15-settings-and-color-depth.md))
- **Colour-depth survey** — PLAN ONLY, execute later.

**Boot / OS**
- **Host per-System startup placement** — the build only drops MacAtrium into 7.1.x's Startup Items; after a
  swap to another System Folder you land in the bare Finder. Install into *every* System Folder's startup.
- **Cross-disk chooser Phases 1–3** — cross-volume enumeration + UI + wire `SetDefaultStartup`; plus the
  spike's bad-value negative test ([42](42-cross-disk-startup-chooser.md)).
- **Multi-volume library (2 GB boot cap)** — BACKLOG, not started; complementary to multi-disk ([23](23-multi-volume-library.md)).

**UI**
- **Per-OS native control appearance** (compile-time `theme_sys{6,7,8}`) — Phase 3 of native controls.
- **Settings dialog won't fit a 512×342 9" screen** (~378 px tall) — matters now the floor is 6.0.4 (compact
  Macs); make it two-column/scrolling or shrink the row pitch (`SD_*` in `main.c`).

**Content pipeline**
- Milestone 5 items: `rb-cli scan/catalog`, artwork PNG→PICT depth variants, recommendations dataset +
  CONTRIBUTING, one-command image build.
- **Macintosh Garden**: wire MG into `enrich` + the scrub/attribution decision; `.zip`/inner-disk-image and
  `.sitx` (9.2.2-era) handling deferred.

**Build / tooling**
- **`curl` is dead code** in the atrium tool (downloads use Rust `ureq`/rustls) — rip out the `curl` field /
  `--curl` flag / `_curl` params.
- **`templates.json` / `donors.json`** are read from `data/` at runtime — a release running outside the repo
  needs them embedded.

**Data**
- **`oxyd-3-6`** is flagged `color:true` in `data/compatibility.jsonl` but the donor only has the mono app
  (cosmetic — lands in "Color"); flip to false for "Black & White".

**Hardware (Milestone 6)**
- MiSTer input mapping per core; real-68k-hardware test; performance pass.
