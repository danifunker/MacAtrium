# 35 — Resume: redraw-flash, header widgets, Settings-only PRAM, portable build tooling

Paste into a fresh session to continue. **State: a user-reported polish + tooling
batch is DONE and Snow-verified, committed to `main`. NOTE: git history was rewritten
this session to purge the launcher binary — see §3. Because of the rewrite, commit
hashes changed and `origin` diverged (a force-push is required to publish).**
Supersedes `docs/34-resume.md`.

## 0. Environment (don't re-learn)
- **Build the launcher LOCALLY:** `export RETRO68=$HOME/repos/Retro68-build && cmake
  --build build` → `build/MacAtrium.bin`. The atrium tool is **not** rebuilt when only
  the launcher changes (see §3 — the launcher is read from a path, not embedded).
- **rb-cli = `/home/dani/.local/bin/rb-cli`** (the tool's only external binary now;
  `atrium config` → `rb_cli = rb-cli` on $PATH resolves here). NOT the old
  `~/repos/rusty-backup/...` path.
- **Build disks:** `tools/atrium-tool/target/release/atrium image --config C.json`
  (run from the repo root — `data/templates.json` + `data/donors.json` are read as
  relative paths). Rebuild the tool only when its Rust source changes:
  `cargo build --release --manifest-path tools/atrium-tool/Cargo.toml`.
- **Verify in Snow (Mac II, off-screen colour):** `~/repos/snow/target/release/
  macatrium_harness <ROM> /tmp/mdc/3410868.bin <disk> <out> <cycles> --snap-every
  100000000 --keys "CYC:KEY;…"`. ROM `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom`.
  Keys: letters, arrows, enter/esc/space/tab, cmd-<k>, `click@X,Y`. Chooser ~2.6 G;
  `enter` picks Carousel. `click@` content origin ≈ (local.x, local.y+39).
  **Mac II + MDC always reports Color QD → off-screen path.** For the DIRECT-draw path
  (deferred art), boot a **6.0.8** disk (sysVers < 0x0700). Swap a fresh launcher into
  a full disk: 7.x → `rb-cli put-macbinary DISK L.bin --dst-dir "/System Folder/Startup
  Items" --force`; 6.0.8 → patch the MacBinary name to `Finder`, `put-macbinary … "/
  System Folder" --force`, `rb-cli chmeta … --type FNDR --creator MACS`.
- Commit to main (memory `commit-directly-to-main`). Suspect our code, not Snow.

## 1. DONE — launcher (src/ui.c, ui.h, main.c, display.c)
- **Incremental redraws copy AROUND the real controls** (new `push_around` splitter):
  `carousel_draw_sel` blits 4 bands straddling the Launch button + pager;
  `iconview_draw_sel`/`listview_draw_sel` blit cells/rows + `push_around(detail, launch)`.
  `ui_paint_controls(incremental)` only nudges the scroll thumb — no more Launch /
  stepper flash on every move.
- **Deferred cover load repaints ONLY the cover box** (`*_draw_art` + `ui_draw_art`
  dropped its `ui_paint_controls`). No more text→image→text re-render.
- **Settings-from-menu** skips the full browse re-blit (`handle_ui_command`
  UI_OPEN_MENU → if mc==UI_OPEN_SETTINGS, open straight away).
- **Settings glyph → real push button + gear** (`Ui.settingsBtn` pushButProc +
  `draw_gear_glyph`, focus ring on `settingsFocus`); **category `< >` → classic
  little-arrows stepper** (`draw_cat_stepper`/`cat_stepper_rect`, carousel+grid only;
  List keeps its pane). Removed catPrev/catNext, `ui_step_category`, `draw_settings_btn`.
- **Slot PRAM is written from EXACTLY ONE place: the Settings "Color Depth" stepper**
  (`ui.c apply_depth`). Startup, per-game depth caps, and osEvt resume now only set the
  LIVE depth, never the boot default. `display_set_default_depth` also refuses to write
  unless that depth is already live and persists the card's real `cscGetMode` id (not a
  hardcoded 128..133 guess). This is the fix for "some machines need a PRAM reset to
  boot" — the launcher no longer scribbles boot-default modes on every boot / game cap.
- **Verified in Snow** (7.1 Mac II 8-bit off-screen + 6.0.8 direct-draw): all 3 views
  render clean, no control flash, deferred cover lands in just the cover box; gear
  opens the menu + little-arrows steps the category (both via real `click@`).

## 2. DONE — build tooling (tools/atrium-tool/src/*.rs, CMakeLists.txt, .gitignore)
- **Launcher is no longer embedded in the tool / committed to git.** `config.rs`
  dropped `EMBEDDED_LAUNCHER` (include_bytes!); `launcher_bytes()` reads a path:
  `config.launcher` → `$MACATRIUM_LAUNCHER` → **`build/MacAtrium.bin`** (default). Build
  it with Retro68 or drop a release binary there. Removed the CMake POST_BUILD copy and
  the `assets/MacAtrium.bin` asset; `.gitignore`s it. 93/93 tool tests pass.
- **No more `cp` shell-out** (Windows-portable). `image.rs copy_sparse()` streams the
  base image in 1 MiB blocks and seeks over all-zero blocks — byte-identical to the
  source, holes preserved (real sparse on Unix; correct-but-not-sparse on Windows).
  **rb-cli is now the ONLY external binary the tool spawns.**
- **curl is vestigial** — downloads use native Rust `ureq`/rustls. The `curl` config
  field / `--curl` flag / `_curl` params are dead code. *Follow-up: rip them out* (I
  left them since the user initially said "curl is okay", before we found it's unused).

## 3. DONE — history rewrite (destructive; READ THIS)
- `tools/atrium-tool/assets/MacAtrium.bin` was **purged from ALL git history** with
  `git filter-repo --path … --invert-paths` (45 commits had touched it). **Every commit
  hash changed** and `filter-repo` dropped the `origin` remote.
- A full pre-rewrite snapshot is in the **bundle backup** (`$CLAUDE_JOB_DIR/tmp` or the
  path printed in the session) — `git clone backup.bundle` to recover if needed.
- **`origin` is git@github.com:danifunker/MacAtrium.git.** It still has the OLD history.
  Publishing requires re-adding the remote and a **force push** (`git push --force`),
  which rewrites the public GitHub history — do this only when ready; it breaks any
  other clone/fork.

## 4. Requirements to build disks from a release (no source build)
Beyond the MacAtrium app + rb-cli: a bootable **base OS image** per OS (`data/
templates.json`), **donor image(s)** for the games (`data/donors.json`; PoP's donor is
the 6.0.8 PoP base, which doubles as the 6.0.8 base), and an optional **art source**
(MacGarden archive `mg_archive`, or LaunchBox, or a local `art_dir`). Embedded in the
tool: library/compatibility/taxonomy/categories/targets. `templates.json` + `donors.json`
are read from `data/` at runtime (a release running outside the repo needs them —
consider embedding them too).

## 5. Artifacts
- **9 minimal (Prince-of-Persia-only) disks** in `/home/dani/MacAtrium-minimal/`
  (3 OS × 3 depths, 49–87 MB), built with this session's launcher. Generator +
  configs in `$CLAUDE_JOB_DIR/tmp/gen_minimal.py` + `min-*.json` (not committed).
- The committed 5-game demo matrix is `builds/gen_configs.py` + `builds/gen-*.json`.

## Memory to read
`classic-ui-redesign-views`, `color-depth-in-slot-pram`, `color-art-memory-budget`,
`build-tool-mvc-architecture`, `build-and-snow-are-local`, `workflow-verify-in-emulator`,
`snow-harness-verify-gotchas`, `qemu-q800-harness`, `commit-directly-to-main`,
`suspect-our-code-not-snow`.
