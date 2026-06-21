# 13 — Handoff & Resume (post-MVP)

Where the project stands after the MVP build, how to pick it back up fast, and
the plan for the next two pushes: **(1) proper image-build tooling, then (2) knock
out the full 7.x feature set.**

Supersedes the original build prompt in [12-mvp-handoff.md](12-mvp-handoff.md)
(that goal — "launch and return" — is done).

---

## 1. Where we are — DONE ✅

The MVP is built, unit-tested, and verified end-to-end on real emulated hardware.

- **The launcher exists** (`src/`, 68k C, Retro68) and builds clean →
  `build/MacAtrium.bin` (creator `ATRM`, type `APPL`).
- **Portable core unit-tested off-target** — `tests/` (45 checks: JSON parser,
  catalog parse incl. CR/LF/CRLF, model categories/"All"/sort/many-to-many/nav).
- **Verified end-to-end in Snow, headlessly** on **System 7.0.1, 7.1, 7.5.5**
  (Macintosh II): boot → MacAtrium auto-launches (Startup Items) → arrow to an
  item → Return launches it (incl. the **real Prince of Persia**) → quit it
  (Cmd-Q) → control returns with selection intact. Plus: ←→ categories, Esc menu
  (Launch Finder / Restart / Shut Down), Restart reboots, graceful "Not found",
  the "no catalog" safe screen.
- **Redraw is double-buffered** (off-screen GWorld → one `CopyBits`); no flicker.
- **Headless verification harness** drives Snow's core directly
  ([../tools/snow-harness](../tools/snow-harness/)) — boot, inject keystrokes at
  cycle marks, dump framebuffer PNGs.
- Evidence: [evidence/](evidence/). Full empirical record:
  [11-derisk-log.md](11-derisk-log.md) §C′/§C″. Roadmap status:
  [09-roadmap.md](09-roadmap.md).

Branch: **`mvp-launcher`** (4 commits on top of the initial scaffold; working
tree clean; not yet merged to `main`).

---

## 2. Resume fast — the environment & loop

Everything is on this dev box.

| Thing | Path / command |
|-------|----------------|
| Repo | `~/repos/MacAtrium` (branch `mvp-launcher`) |
| Retro68 | `export RETRO68=~/repos/Retro68-build` ; toolchain file `$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake` (gcc 12.2.0) |
| Build launcher | `cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=$RETRO68/...retro68.toolchain.cmake && cmake --build build` |
| Host unit tests | `cd tests && make test` |
| rb-cli | `~/repos/rusty-backup/target/release/rb-cli` (image I/O: mkdir/put/put-binhex/put-macbinary/ls/bless/…) |
| Snow core | `~/repos/snow` (workspace target is warm) |
| Headless harness | `~/repos/snow/testrunner/src/bin/macatrium_harness.rs` (canonical copy in `tools/snow-harness/`); build: `cargo build -r -p testrunner --bin macatrium_harness` |
| Machine ROM | `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom` (Snow detects "Macintosh II (FDHD)") |
| Display-card ROM | `~/repos/mame/roms/nb_mdc824.zip` → `3410868.bin` (Mac II needs it for video; `ExtraROMs::MDC12`) |
| Sample disks | `~/MacOS_SampleDisks/MacLC_{6-0-8,6-0-8-POP,7-0-1,7-1,7-5-5,…}.hda` |
| Assemble test image | `tools/snow-harness/assemble.sh <src.hda> <out.hda> [startup_items_dir]` |
| Ready-to-test deliverable | `~/MacAtrium-7.1-test/` (image + both ROMs + README) |

**The dev loop:** edit → `cmake --build build` → `assemble.sh` onto a copy of a
sample disk → run `macatrium_harness` with `--keys` → `Read` the PNGs. No display
server required. (There is **no display** on this box — `snowemu` GUI can't be
used here; the harness is the way.)

⚠️ The harness file also lives **untracked** in the sibling `snow` repo where it
compiles. The canonical copy is `tools/snow-harness/macatrium_harness.rs`; keep
them in sync.

---

## 3. Code map (`src/`)

`json` · `catalog` · `model` — portable core (no Toolbox; host-tested).
`env` — Gestalt + screen/depth probe, backend select.
`macfs` — `/MacAtrium`-relative FSSpec + file read (⚠ System-7 FSSpec traps).
`render` + `render_qd`/`render_cqd` — backend-agnostic draw, B&W + Color, now
off-screen-composited. `ui` — layout, nav, Esc menu, safe screen.
`launch` — resident sub-launch (`launchContinue`). `sysctl` — Restart/Shut
Down/Launch Finder. `main` — init, full-screen window, event loop.
`mac_compat.h` — constants Retro68's leaner headers omit.

---

## 4. NEXT (when we resume) — Priority 1: complete image-build tooling

**Goal: one command turns the curated dataset + a base System disk into a
ready-to-boot appliance image** — no hand steps. This is roadmap
[Milestone 5](09-roadmap.md) + the [content pipeline](06-content-pipeline.md).

What exists today is a *prototype*: `tools/snow-harness/assemble.sh` hard-codes
a hand-written catalog and two hand-extracted apps. The real tool should:

1. **Generate `catalog.jsonl` from the repo dataset** (`data/`), not by hand —
   merge curated metadata, map genres→categories, emit CR/MacRoman/`TEXT`
   (schema [06](06-content-pipeline.md)). Decide: a standalone host tool in
   `tools/` vs. a new **`rb-cli scan`/`catalog`** subcommand (06 leaves this
   open; rb-cli is the natural home for volume I/O).
2. **Populate `/MacAtrium/Apps`** from a source tree or by extracting/aliasing
   apps already on a disk (both forks — `get-binhex`/`put-binhex` is the proven
   path; consider real **aliases** so moved files still launch).
3. **Convert artwork → PICT** (depth variants) into `/MacAtrium/images` — host
   PNG→PICT step (06 leaves the exact converter open; pick one).
4. **Install the launcher** — Startup Items now; add the **boot-block shell-swap**
   option (we proved the swap works, §C″/S3) for a true Finder-replacement build.
5. **Emit a bootable `.hda`** (+ optionally a Snow workspace / `hddN.img` layout)
   and run the harness as a smoke test.

Deliverable: something like `make image DATASET=… SYSTEM=… OUT=…` (or a
`tools/build-image` script) that's reproducible and CI-able. Fold the working
bits of `assemble.sh` into it; keep `assemble.sh` as the quick test path.

---

## 5. NEXT — Priority 2: knock out everything for 7.x

Pull from roadmap Milestones 2 & 3 (the 7.x-relevant ones). Concrete checklist,
roughly highest-leverage first:

- [ ] **Become the real boot shell** (not just Startup Items): decide & implement
      Startup-Items (B) vs boot-block shell-swap (C) as the shipping default;
      finish **Launch Finder** (resident bring-to-front + reboot fallback). (M2)
- [ ] **Exercise the Color (256) backend at a colour depth** — every run so far
      was 1-bit. Boot a colour-depth screen (set Monitors depth, or via the next
      item) and confirm `render_cqd` looks right; bump the `SIZE` partition for
      the bigger colour GWorld (1 MB is fine at 1-bit; ~8-bit needs more, or use
      a temp-mem GWorld).
- [ ] **Per-item display depth via `SetDepth`/`HasDepth`** (the "fullscreen"
      lever from the chat): optional catalog `depth`; set before `Launch`,
      restore on return. Guarded by Color QD. (docs/01 deferred item)
- [ ] **Hide the launcher's own menu bar** for true full-screen
      (`LMSetMBarHeight 0`, restore on Launch Finder). (S1/S2)
- [ ] **Settings menu** — enumerate Control Panels (`FindFolder`), open the
      `cdev`s via an `odoc` AppleEvent to the resident Finder (C1, M2).
- [ ] **Aliases for launch targets** — `ResolveAliasFile` so moved/aliased apps
      still launch; fall back to path. (08)
- [ ] **Artwork** — app icons (`ICN#`/`icl8`, no assets needed) → curated PICT,
      lazy-loaded; wire into the UI. (M3)
- [ ] **UI polish** — type-ahead jump, per-item hotkeys, detail/art pane at
      800×600 & 1024×768, theme presets, tune layouts incl. 512×342. (M3)
- [ ] **16-bit / thousands** backend variant. (M3)
- [ ] **7.6.1 run** to finish the L1 matrix (needs a 7.6.1 disk — not on this box).

---

## 6. Known gaps / watch-outs

- **System 6.0.8 is Milestone 4, not 7.x.** It boots on the Mac II and
  MultiFinder activates via the boot-block swap (S3 ✅), but the launcher needs a
  port first: FSSpec calls are the System-7 trap `0xAA52` (rewrite `macfs.c` with
  older File-Manager `PB…` calls or add glue), and `WaitNextEvent` must be
  trap-guarded (fall back to `GetNextEvent`). Details in §C″.
- **Color backend is unverified** (1-bit only so far) — see Priority 2.
- **Some games are copy-protected** (e.g. PoP ships a `codes.jpg` wheel) — fine
  for proving launch/return, but blocks deep automated play.
- **Off-screen path needs Color QD** (true on Mac II). Original-QD-only compact
  machines fall back to direct drawing; a classic off-screen-BitMap path is the
  Milestone-4 follow-up.
- The **harness duplication** between this repo and the `snow` repo (see §2).

---

## 7. The 30-second resume

```sh
cd ~/repos/MacAtrium && git checkout mvp-launcher
export RETRO68=~/repos/Retro68-build
cd tests && make test && cd ..                       # core still green?
cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
cmake --build build                                   # launcher builds?
# then: tools/snow-harness/assemble.sh + macatrium_harness to run it (see §2)
```
Then start on **Priority 1 (image tooling)**, then **Priority 2 (7.x)**.
