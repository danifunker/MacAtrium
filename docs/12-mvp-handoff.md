# 12 — MVP Build Handoff (run on the Retro68 + Snow machine)

Paste the prompt below into a fresh agent session on the machine that has
**Retro68 and Snow**. It's self-contained but leans on the docs in this repo, so
make sure the repo is cloned/pulled there first.

---

## The prompt

> You are continuing **MacAtrium**, a keyboard-driven Finder-replacement launcher
> for classic Mac OS (**68k, C, built with Retro68**). It boots in place of the
> Finder and shows a curated, categorized menu of games/apps; you arrow to one,
> press Return, it launches, you quit, and you're back in the menu.
>
> **This machine has Retro68 and Snow** (the hardware-level 68k Mac emulator with
> a trap-breakpoint debugger). The plan and all decisions live in `docs/`. Read,
> in order: `docs/README.md`, `docs/01-decisions.md`, `docs/03-architecture.md`,
> `docs/06-content-pipeline.md`, `docs/08-launching-system.md`,
> `docs/09-roadmap.md`, and especially `docs/11-derisk-log.md` (the launch/Gestalt/
> QuickDraw APIs are already confirmed from Apple's headers — use those exact
> constants).
>
> **Goal (MVP):** boot a System 7 image in Snow → the MacAtrium menu appears →
> arrow to **Prince of Persia** → Return launches it → quit it → back in the menu
> with selection intact → Restart/Shut Down work.
>
> **Environment to set up / confirm:**
> - `export RETRO68=…` (your Retro68 build); confirm the 68k toolchain file path.
> - A bootable **System 7 68k** disk image (we developed against Mac LC images:
>   6.0.8 / 7.1 / 7.5.5 — see `docs/11-derisk-log.md` §B′). Always work on a
>   **copy**.
> - `rb-cli` from the `rusty-backup` repo for injecting files (build it if needed).
>
> **Do these in order:**
>
> 1. **Prove the keystone.** Build `spikes/launch-return/` (see its README +
>    CMakeLists). It's a DRAFT — fix any header/field issues the compiler flags
>    (likely `StandardGetFile` or `ProcessInfoRec.processAppSpec`). Inject it into
>    a copy of a System 7 image with `rb-cli`, run in Snow, and confirm
>    **RETURNS FROM LAUNCH** increments when a launched app quits. Set a
>    system-trap breakpoint on `_Launch` (`A9F2`) and watch control return. Fill
>    in the matrix in the spike README for 7.1 and 7.5.5.
>
> 2. **Write the MVP launcher in `src/`** (modules per `docs/03-architecture.md`),
>    reusing the spike's exact launch code. Keep it to MVP scope — "launch and
>    return," no gold-plating:
>    - `json.c` — tiny JSON parser: strings, numbers, booleans, flat objects, and
>      **string arrays** (for `categories`). Tolerate CR/LF/CRLF; MacRoman. Make
>      it unit-testable off-target (host `gcc`).
>    - `catalog.c` / `model.c` — load `/MacAtrium/metadata/catalog.jsonl` (schema
>      in `docs/06`) into items; build a `category → items` index including the
>      synthesized **"All"**; alphabetical sort (recommendation-type categories
>      keep dataset order).
>    - `env.c` — `Gestalt` probes: system version, QuickDraw version → pick B&W vs
>      Color backend, `gestaltLaunchCanReturn`, main-device bounds + `pixelSize`.
>    - `ui.c` + `render_qd.c` / `render_cqd.c` — full-screen window, **Chicago**
>      font, header (title · category · count), list with **↑↓ select, ←→ category,
>      Return launch, Esc menu**; layout computed from the screen rect. B&W and
>      256-color backends (256 is fine first).
>    - `launch.c` — the resident sub-launch (`launchContinue`) from the spike;
>      resolve an item's `app` path (relative to `/MacAtrium`) to an `FSSpec`.
>    - `sysctl.c` — Esc menu: **Restart** (`ShutDwnStart`), **Shut Down**
>      (`ShutDwnPower`); "Launch Finder" can be a stub for now.
>    - A built-in **"no catalog found"** safe screen (this is the startup app —
>      never dead-end the user).
>
> 3. **Run it end to end.** Build with Retro68. Inject `data/catalog.jsonl` →
>    `/MacAtrium/metadata/catalog.jsonl` and put a real app (or alias) at
>    `/MacAtrium/Apps/Prince of Persia/Prince of Persia` (or temporarily point an
>    entry's `app` at a known-present app like **SimpleText**). Boot in Snow and
>    iterate until Prince of Persia launches and returns.
>
> 4. **Record results.** Fill the launch matrix and mark L1/L3 in
>    `docs/11-derisk-log.md`; check off MVP items in `docs/09-roadmap.md`. Commit
>    on a branch.
>
> **Hard constraints:** 68k / C / Universal Interfaces; pick one render backend at
> startup; always degrade gracefully to the B&W/text path; the app is the startup
> shell, so a failure must show an on-screen recoverable state, never a hang. MVP
> creator code placeholder is `ATRM`, type `APPL`.

---

## Notes for whoever runs it

- The launcher source doesn't exist yet on purpose — it's written *there*, where
  it can be compiled and run, not blind.
- If `rb-cli` isn't on that machine, build `rusty-backup` (`cargo build`) or build
  the disk image on the Mac that has it and copy it over.
- Sample disks we used live (on the original dev Mac) at
  `~/Documents/MacOS_SampleDisks/`.
