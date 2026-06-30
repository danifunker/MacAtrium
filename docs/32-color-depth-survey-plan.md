# 32 — Plan: empirical colour-depth survey of MacPack (maxDepth harvesting)

**Status: PLAN ONLY — to execute later.** Goal: for every launchable app in
MacPack, empirically find the deepest screen depth it tolerates by booting it at
256 colours (8-bit) and thousands (16-bit), screenshotting, and detecting any
depth-warning dialog or system bomb. The result populates the `maxDepth` cap in
`data/compatibility.jsonl` so the launcher drops the screen before launch (the
mechanism already exists — `main.c do_launch`, docs proven; this just feeds it
data). Supersedes the hand-curation we did for `arkanoid-1-10` / `dark-castle-1-2`.

## 0. Why empirical (vs the static scan)
The resource-fork scan (docs proven PoC) catches two of three cases cheaply, so
run it FIRST as a pre-filter — but it cannot catch the third, which is exactly
why we need to launch:
- **Self-announcers** (Arkanoid): a `STR `/`DITL` says "set the main screen to two
  colors…". Static scan finds the string → known `maxDepth:1`, no launch needed.
- **Colour/B&W facet**: colour resources (`icl4`/`icl8`/`cicn`/`clut`/`pltt`) vs
  B&W-only (`ICN#`/`PAT `). More accurate than today's screenshot-based guess.
- **Silent bombers** (Dark Castle): NO warning string anywhere — it just crashes
  above 1-bit. ONLY a real launch reveals this. ← the reason this survey exists.

## 1. Scope
- **Universe**: every real app file (`APPL`) inside the MacPack `.vhd` donor images
  (`~/MacPack-20240825-RC1.zip`), de-duplicated by app. This is the *installable*
  set — we can only test what we can boot. (The wider ~1489-title MacGarden catalog
  includes ~1204 with no donor; those stay scan-/curation-only, see
  [[macpack-vs-macgarden-corroboration]].) Phase 0 produces the exact count.
- **Depths tested**: 8-bit (256) and 16-bit (thousands). 24-bit ("millions") is an
  optional later extension; `maxDepth` already supports 32.
- **Output**: a per-app result row → a *suggestions* file
  (`data/compatibility.suggested.jsonl`), reviewed by hand, then merged into
  `compatibility.jsonl`. Suggestions NEVER auto-merge — hand-verified entries win,
  and `compatibility.jsonl` is embedded via `include_bytes!` so a tool rebuild is
  required to take effect (the gotcha that bit the manual fix).

## 2. Emulator split (depth → harness)
Snow tops out at Mac II / 8-bit, so the two depths use two harnesses:
| Depth | Harness | Notes |
|-------|---------|-------|
| 8-bit (256) | Snow `macatrium_harness` (Mac II + MDC) | fast boot ~45s; `click@`/keys; `hold@` |
| 16-bit (thousands) | q800 `q800_harness.py` (Quadra/68040, macfb over QMP) | what Snow can't do; keys via QMP, no mouse; boot ~50s |

Boot depth is set per-disk (not at runtime): slot PRAM spID (XPRAM 0x48, see
[[color-depth-in-slot-pram]]) or the System Folder Monitors pref. **Decision to
finalize at impl:** bake the target depth into each test disk's System (most
deterministic) vs. persist it via the harness PRAM. Calibrate the spID values for
8/16-bit on each card before the full run (§6).

## 3. Test rig: one app, auto-launched, at a fixed depth
For a clean signal, BYPASS the MacAtrium launcher — make the app itself the
startup item so it launches directly at boot (no launcher variables). Reuse the
existing auto-launch placement (Startup Items on 7.x; Finder-replace on 6.0.8,
see [[system-608-boot-shell]]).
- **Test System**: pick one canonical System per app era. Default 7.1 (broad
  Color-QD compat). Apps tagged "01 Sys 6" retest under 6.0.8/7.1 if they fail to
  launch under 7.5 (don't conflate a System-incompat failure with a depth limit).
- **Per-app disk**: `atrium image` with `selection=[app]`, `art_depths` irrelevant,
  System Folder boot depth pre-set to the target. Add a survey/auto-launch flag
  (app-as-startup, no launcher) + a "set boot depth" step — small extensions, see §7.
- **Isolation**: each run uses a fresh disk copy (Snow) or QEMU `-snapshot`
  (q800, already non-mutating) so a bomb can't bleed into the next app.

## 4. Capture & classification
Per run: boot → app auto-launches → take screenshots at **+5s, +12s, +25s** post-
launch (dialogs can appear after init; some only once you start play) → `final.png`.
Classify each app/depth into one of:
- **clean** — app UI up, no modal dialog.
- **warn** — modal dialog with depth text (soft; e.g. Arkanoid still runs).
- **bomb** — system-error/bomb dialog, Sad Mac, or frozen/blank screen (hard).
- **fail (unrelated)** — didn't launch: "not enough memory", "requires newer
  system", app-not-found, missing data file. NOT a depth verdict — re-queue.

Automated classifier: OCR (tesseract, with upscale+threshold preprocessing for
Chicago/Geneva at 1/8-bit) → keyword match:
- depth: `colors|256|thousands|millions|monitor|two colors|black.?and.?white|16`
- bomb: `system error|restart|bomb|sorry`
- fail: `not enough memory|requires|newer version|can.t be (opened|found)`
Cross-check against the static resource scan (§0) as a prior. Every **warn/bomb/
fail** gets human spot-check; plus a random sample of **clean** to bound OCR FP/FN.

## 5. maxDepth derivation (pipeline order: 8-bit first)
- 8-bit **warn|bomb** → `maxDepth:1`. Skip the 16-bit run (already capped low).
- 8-bit **clean** → run 16-bit:
  - 16-bit **warn|bomb** → `maxDepth:8`.
  - 16-bit **clean** → no cap (`maxDepth` omitted / ≥16). Optional 24-bit later.
- 8-bit **fail (unrelated)** → re-queue under an alternate System; no verdict.
Record warn-vs-bomb in the suggestion note (curation cares: soft pref vs hard
crash). Also emit the static colour-facet so the `color` field can be corrected
in the same pass (fixes screenshot mis-tags like Arkanoid).

## 6. Calibration FIRST (gate before the full run)
Validate the rig end-to-end on a known 4-title set, one per outcome, BEFORE
spending the batch budget:
- `arkanoid-1-10` → must read **warn** at 8-bit.
- `dark-castle-1-2` → must read **bomb** above 1-bit.
- `prince-of-persia` → **clean** at 8-bit, expect warn/bomb at 16 → `maxDepth:8`.
- a known 16-bit-happy colour title → **clean** at 16-bit (no cap).
If the rig mis-reads any of these (depth not actually set, OCR miss, bomb not
detected), fix before scaling. This is the single most important step.

## 7. Tooling to build (the survey is mostly new infra)
1. `atrium survey enumerate` — every launchable MacPack app (id, donor, path) +
   static facets (colour resources, depth-warning strings) from the §0 scan.
2. Test-disk builder — extend `atrium image` with `--survey` (app-as-startup, no
   launcher) and `--boot-depth {1,8,16}` (sets PRAM spID / Monitors pref).
3. Batch runner — worker pool spinning K headless harness instances (Snow for 8,
   q800 for 16), boot → timed screenshots → kill-on-timeout → collect PNGs+logs.
4. Classifier — OCR+keyword → `{clean,warn,bomb,fail}` + extracted message, joined
   with the static prior; one result row per (app, depth).
5. Reducer — rows → `data/compatibility.suggested.jsonl` (maxDepth + corrected
   colour facet, with provenance + screenshot links). Never auto-merges.
6. Review contact-sheet — HTML/markdown grid (app · depth · verdict · message ·
   screenshot thumbnail) for fast human accept/reject, then merge → rebuild tool →
   rebuild disks.

## 8. Cost & parallelism (overnight batch)
Per run ≈ boot+launch+capture ≈ 60s (Snow) / 70s (q800). 8-bit pass = N runs;
16-bit pass = only the 8-bit-clean subset (≈ the colour titles). For N≈300:
single-thread ≈ 6h (8-bit) + ~3h (16-bit); with K parallel workers ≈ /K. Runs
headless, so a K=4–8 worker pool turns it into an overnight job. Phase 0 fixes N.

## 9. Risks
- **OCR on Mac fonts** → preprocess + static-scan prior + human spot-check of all
  non-clean.
- **Depth not actually engaging** in the emulator → §6 calibration is the guard.
- **Input-gated warnings** (dialog only appears once you start a game) → multi-
  screenshot timeline + optional auto-advance (inject Return/click); static scan
  backstops the ones we miss in-game.
- **Unrelated launch failures** → classified separately, re-queued under the right
  System; never derive maxDepth from a failed launch.
- **Bombs wedging the emulator** → per-run timeout + fresh disk/-snapshot isolation.
- **Scale of MacPack vs donors** → only the donor-backed set is testable; the rest
  fall back to static scan + curation (state the dropped count, don't imply 100%).

## 10. Execution checklist (when we run it)
1. Build §7.1 enumerate + static scan → app list + facets + announcer pre-flags.
2. Build §7.2 test-disk builder; §6 calibrate on the 4-title set. **Gate.**
3. Build §7.3 runner; 8-bit pass over all apps (Snow).
4. §7.4 classify; 16-bit pass over the 8-bit-clean subset (q800).
5. §7.5 reduce → suggestions; §7.6 contact sheet → human review.
6. Merge accepted → `compatibility.jsonl` → rebuild tool → rebuild disks → spot-
   verify a few in-emulator.

## Memory / infra to read first
[[qemu-q800-harness]] (16-bit path), [[color-depth-in-slot-pram]] (boot depth),
[[overrides-db-maxdepth]] (the cap mechanism), [[macpack-data-source]] +
[[macpack-vs-macgarden-corroboration]] (the universe), [[build-and-snow-are-local]],
[[system-608-boot-shell]] (auto-launch), [[workflow-verify-in-emulator]].
