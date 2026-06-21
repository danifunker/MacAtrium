# 13 — Iterate-Fast Build Session (run entirely on the Retro68 + Snow machine)

This is the prompt to paste when you want the **whole** build-test-iterate loop to
live on the build machine and run with minimal hand-holding. It extends the
handoff in [12-mvp-handoff.md](12-mvp-handoff.md) with a tight inner loop and an
autonomous working style. Clone the repo there first, then paste the block below.

---

## The prompt

> You are running the **entire MacAtrium build-and-test loop locally** on this
> machine — it has **Retro68 and Snow** (the hardware-level 68k Mac emulator with
> a trap-breakpoint debugger). MacAtrium is a keyboard-driven Finder-replacement
> launcher for classic Mac OS (68k, C). Your job is to get the **MVP working end
> to end in Snow** and keep iterating here — don't round-trip to another machine.
>
> Read `docs/README.md`, then `docs/01-decisions.md`, `03-architecture.md`,
> `06-content-pipeline.md`, `08-launching-system.md`, `09-roadmap.md`, and
> `11-derisk-log.md`. **The launch / Gestalt / QuickDraw APIs are already
> confirmed from Apple's headers in doc 11 — use those exact constants; don't
> rediscover them.**
>
> ### Optimize for iteration speed (do this first)
>
> 1. `export RETRO68=…` and confirm the 68k CMake toolchain file path.
> 2. Confirm Snow runs and **find its automation surface**: run `snow --help` (and
>    check `~/repos/snow`) — can it boot a disk image from the command line / a
>    config so you can relaunch in one step? Note the fastest way to (re)boot an
>    image.
> 3. Get a bootable **System 7 68k** image and keep a **pristine `base-sys7.dsk`**.
>    Each run copies it — never mutate the base. (We used Mac LC 7.1 / 7.5.5; see
>    doc 11 §B′.) Build `rb-cli` from `../rusty-backup` if it isn't present.
> 4. **Write `tools/dev.sh` — your one-command loop — and use it after every
>    change:**
>    ```sh
>    # tools/dev.sh  (build -> inject into a fresh copy -> boot in Snow)
>    cmake --build build                                   # 1. compile
>    cp base-sys7.dsk test.dsk                             # 2. fresh image
>    rb-cli put test.dsk build/MacAtrium.bin /MacAtrium/MacAtrium --type APPL --creator ATRM
>    rb-cli put test.dsk data/catalog.jsonl /MacAtrium/metadata/catalog.jsonl --type TEXT
>    # (+ put a real app or alias at /MacAtrium/Apps/... matching an entry)
>    snow test.dsk        # 3. boot (adjust to Snow's actual CLI)
>    ```
> 5. **Unit-test off-target for the fastest feedback:** compile and test `json.c`
>    and the layout math with host `gcc` (no emulator in the loop). Only the
>    Toolbox/UI path needs Snow.
>
> ### Working style (be autonomous)
>
> - Compile and run after every meaningful edit. **Fix your own compile/link/header
>   errors and rerun** — don't stop to ask about routine fixes (likely tweaks:
>   `StandardGetFile` signature, `ProcessInfoRec.processAppSpec`, the `qd` global).
> - Use **Snow's debugger** (trap breakpoint on `_Launch` = `A9F2`, memory/register
>   views, trap history) instead of guessing.
> - Keep a running **`docs/dev-log.md`** (what you did, what worked, what's next)
>   and a task list, so you can resume cleanly after a context reset.
> - **Commit at every green checkpoint** with a clear message and push (small,
>   frequent commits) so progress is syncable without interrupting you.
> - **Stop and ask only** at the CHECKPOINTs below or on a genuine blocker — a
>   decision that changes the plan, or something that contradicts the docs.
>
> ### Tasks (commit + push + update `dev-log` after each)
>
> **A. Prove the keystone.** Build `spikes/launch-return/` (its README + CMake).
> Run in Snow; confirm **RETURNS FROM LAUNCH** increments when a launched app
> quits; set a trap breakpoint on `_Launch` and watch control return. Fill the
> spike's matrix for 7.1 and 7.5.5. → **CHECKPOINT: report the matrix.**
>
> **B. MVP launcher in `src/`** (modules per doc 03, MVP scope per doc 09),
> reusing the spike's launch code:
> - `json.c` — tiny parser: strings, numbers, bools, flat objects, **string
>   arrays** (`categories`); CR/LF/CRLF-tolerant; MacRoman; host-unit-tested.
> - `catalog.c` / `model.c` — load `/MacAtrium/metadata/catalog.jsonl`; build
>   `category → items` incl. synthesized **"All"**; alphabetical sort
>   (recommendation-type categories keep dataset order).
> - `env.c` — `Gestalt` probes → backend choice, `gestaltLaunchCanReturn`, screen
>   bounds + `pixelSize`.
> - `ui.c` + `render_qd.c` / `render_cqd.c` — full-screen window, **Chicago**,
>   header (title · category · count), list with **↑↓ select, ←→ category, Return
>   launch, Esc menu**; layout from the screen rect; B&W + 256-color (256 first).
> - `launch.c` — resident `launchContinue` launch from the spike; resolve `app`
>   (relative to `/MacAtrium`) → `FSSpec`.
> - `sysctl.c` — Esc menu: **Restart** (`ShutDwnStart`), **Shut Down**
>   (`ShutDwnPower`); "Launch Finder" stub ok.
> - Built-in **"no catalog found"** safe screen.
>
> **C. End to end.** Inject the sample catalog + a real app via `dev.sh`; iterate
> until **Prince of Persia launches and returns**, selection preserved. →
> **CHECKPOINT: screenshot + report.**
>
> ### Constraints
>
> 68k / C / Universal Interfaces; pick one render backend at startup; always
> degrade to the B&W/text path; this is the **startup shell**, so any failure must
> show a recoverable on-screen state, never hang. Creator `ATRM`, type `APPL`.
> Don't gold-plate — MVP is "launch and return."
>
> ### Done
>
> Booting `test.dsk` in Snow shows the MacAtrium menu → arrow to Prince of Persia
> → Return launches it → quit returns to the menu with selection intact →
> Restart / Shut Down work. Commit, push, and summarize results in
> `docs/11-derisk-log.md` (launch matrix, L1/L3) and `docs/09-roadmap.md` (MVP
> items checked off).

---

## Why this is the fast version

- **One command (`tools/dev.sh`) per iteration** — build, inject into a fresh copy
  of a pristine base image, boot. No manual re-setup.
- **Host unit tests** for the parser/layout keep most edits off the emulator.
- **Autonomous loop** — fix-and-rerun without stopping for routine errors; only
  pause at real checkpoints — so the session stays on that machine and moves fast.
- **Frequent commits + `dev-log.md`** keep progress syncable and resumable.
