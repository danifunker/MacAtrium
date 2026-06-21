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
- **Dark / light theme** — dark by default; `T` toggles at runtime. Both backends
  honour it (Color QD via two palettes in `render_cqd.c`; B&W via a straight
  black/white inversion in `render_qd.c`). Verified in Snow on 7.1: boots dark →
  `T` → light → `T` → dark ([evidence/theme-dark-default.png](evidence/theme-dark-default.png),
  [theme-light-toggled.png](evidence/theme-light-toggled.png),
  [theme-dark-restored.png](evidence/theme-dark-restored.png)). Theme is not yet
  persisted (no prefs file) — resets to dark each boot.
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

**Decided & underway:** the host tool is a new **pure-Rust crate**,
[`tools/atrium-tool`](../tools/atrium-tool/) (binary `atrium`) — builds on
Mac/Windows/Linux, no native deps, CI-able. It owns catalog generation, art
conversion, app harvest, and image assembly via subcommands (rather than bolting
onto rb-cli, which stays the volume-I/O layer it calls). Data source decided too:
the **MacPack** collection (MiSTer MacPlus pack — APM/HFS `.vhd`s `rb-cli` reads
directly, organised by year/genre with a full text index).

1. **Generate `catalog.jsonl`** — **DONE ✅.** `atrium catalog` compiles the
   curated `data/library.jsonl` → faceted `categories` (the locked **"facets +
   decade buckets"** model: kind, genre, `Color`/`B&W`, decade bucket, vendor,
   `Mouse Required`/`No Mouse`; raw `year` kept for sort), CR/MacRoman, validated
   against the device parser limits. Unit-tested + **verified in Snow** with a
   12-title dataset: facet categories navigate and a real app launches/returns
   from the generated catalog ([evidence](evidence/): `catalog-generated-all-12.png`,
   `facet-{bw,color,decade-1980s,vendor-broderbund,no-mouse}.png`,
   `launch-return-generated-catalog.png`).
2. **Populate `/MacAtrium/Apps`** — **DONE ✅.** `atrium harvest` enumerates apps
   in a MacPack `.vhd` (or any donor disk) via `rb-cli ls`, extracts the launchable
   `APPL` + its data files with both forks (`get-binhex`), skips bundled clutter
   (System/Finder, Desktop DB/DF, Icon), optionally injects into a target image
   (`--into`), and emits `data/library.jsonl` stubs with `year`/`kind` inferred
   from the source path. **Verified in Snow:** harvested Prince of Persia + two
   pack games into a fresh image, then launched/returned PoP through the launcher
   ([evidence](evidence/): `harvest-pop-{selected,running,returned}.png`).
   *Still open:* bulk-scan curation at scale, and real **aliases** so moved files
   still launch.
3. **Convert artwork → PICT** — **DONE ✅** (native-Rust, in-repo). `atrium pict`
   turns PNG/JPEG into PICT v2 at **1/4/8/16-bit** (indexed PackBitsRect + colour
   table for 1/4/8; DirectBitsRect 1-5-5-5 for 16). The launcher gained a PICT
   **preview** (`P` key; `src/art.c`). **Verified in Snow:** 1-bit (Bayer-dithered),
   8-bit, 16-bit all DrawPicture correctly on the 1-bit screen
   ([evidence](evidence/): `pict-render-{1bit,8bit,16bit}.png`). *Known issue:*
   **4-bit** faults QuickDraw on a 1-bit screen (crash packed / hang unpacked)
   though the file is structurally valid — a QD/Snow 4→1-bit bug, not the encoder;
   real 4-bit check needs a colour-depth screen (ties to the §5 colour-backend item).
   *Next:* depth-matched variant selection in the launcher; median-cut palettes; resize.
4. **Install the launcher** — Startup Items now; add the **boot-block shell-swap**
   option (we proved the swap works, §C″/S3) for a true Finder-replacement build.
5. **Emit a bootable `.hda`** — `atrium image` (**deferred on purpose**): the
   one-command orchestrator (catalog + apps + art + launcher install + harness
   smoke test, retiring `assemble.sh`) is held until the **LaunchBox enrichment**
   lands, because that step defines the real dataset + art-source shape `image`
   will consume — building it first avoids rework. `assemble.sh` remains the quick
   hand-test path meanwhile.

**LaunchBox enrichment — DONE ✅.** `atrium enrich` streams LaunchBox's ~500 MB
`Metadata.xml` (SAX-style via quick-xml), filters `Platform == "Apple Mac OS"`
(731 games), matches our titles by normalised name (stripping parenthetical
qualifiers + `:` subtitles, preferring the most complete entry), and fills
`year`/`vendor`(Publisher)/`genre[]` **only where missing** (curation preserved;
`--overwrite` to force). Box-Front art URLs (joined by `DatabaseID`) go to an
optional manifest. **Colour/mouse aren't in LaunchBox → stay curated.** Approach
adapted from megatron-uk/x68klauncher's `tools/metadata.py`. Validated on real
data: 11/12 curated titles matched (only SimpleText, an app, missed); bare harvest
stubs → enriched (year/vendor/genre) → `catalog` → device C-parser parses clean
with the new facet categories. DB at `~/launchbox/Metadata.xml`.

**Immediate next step — `atrium image`** (now unblocked): the one-command
orchestrator tying `harvest` → `enrich` → curate → `catalog`/`pict` → bootable
`.hda` + harness smoke test, retiring `assemble.sh`.

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
      800×600 & 1024×768, tune layouts incl. 512×342. (M3)
      (dark/light **theme** is done — `T` toggles; persist it via a prefs file next.)
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
