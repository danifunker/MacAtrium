# 22 — Resume: category paging + the build variants

Paste into a fresh session to continue. **Goal: the launcher now pages its catalog
by category (256-item cap lifted, docs/21) and we're cutting per-machine build
variants (B&W / 256-colour / Millions / Quadra-800) with progressively more games.**

## 0. CRITICAL environment facts (don't re-learn these the hard way)

**Build AND emulators are LOCAL on this box** — see memory `build-and-snow-are-local`.
- **Compile the 68k launcher:** `cmake --build build` (Retro68 at `~/repos/Retro68-build`).
  It rewrites `build/MacAtrium.bin` and re-embeds it to
  `tools/atrium-tool/assets/MacAtrium.bin` (copy_if_different — `git checkout` that
  asset if you rebuilt with no source change). After a launcher change, **rebuild
  `atrium-tool`** (`cargo build --release --manifest-path tools/atrium-tool/Cargo.toml`)
  so the new launcher is embedded into image builds.
- **Host-test the pure C** (json/catalog/model — incl. `catindex` + paged model):
  `cd tests && make && ./host_test` (88/88 currently).
- **Snow (Mac II-class, headless):** `~/repos/snow`. Build the harness once:
  `cp tools/snow-harness/macatrium_harness.rs ~/repos/snow/testrunner/src/bin/ &&
  cargo build -r -p testrunner --bin macatrium_harness`. Run:
  `~/repos/snow/target/release/macatrium_harness <rom> <mdc_rom> <hdd.img> <out_dir>
  <cycles> --snap-every N --keys "CYC:KEY;..."` → PNG snapshots. ROMs:
  `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom` + `/tmp/mdc/3410868.bin`.
  Carousel keys: **←/→ items, ↑/↓ category**. Boot ~2–3 G cycles.
- **QEMU q800 (Quadra 800 / 68040 — for the 7.5.5 variant; Snow can't do 68040):**
  `qemu-system-m68k -M q800` (QEMU 8.2.2 here). ROM extracted to
  `…/scratchpad/q800rom/f1acad13.rom` (CRC `4e70e3c0`) from `~/repos/mame/roms/macqd800.zip`.
  QEMU launches with it; a **boot+screenshot harness is NOT yet built** (see §4).

## 1. What's DONE + committed (the paging system, Snow-verified)

The launcher reads a **paged catalog** instead of one ≤256-item file:
- **On disk** (`/MacAtrium/metadata/`): `index.jsonl` (the resident category list) +
  `cats/<slug>.jsonl` (one page per category, loaded on demand) + `hotkeys.jsonl`;
  legacy `catalog.jsonl` still written when ≤256 (back-compat). Docs: **docs/21**
  (design) + **docs/06** (format, rewritten).
- **Category DB** (the editable source of truth, NOT derived from genre):
  `data/taxonomy.json` (15 canonical categories + order, Recommended default) +
  `data/categories.jsonl` (`{id, categories[]}`), seeded by `atrium library
  categorize` (preserves edits). Categories: Recommended · Action & Arcade ·
  Adventure · Puzzle · Strategy & Sim · Role-Playing · Interactive Fiction · Card &
  Casino · Sports · Educational · **Color · Black & White** (facet-or-pre/post-1987)
  · No Mouse Required · Applications · Utilities. No "All".
- **Host (Rust):** `catalog::run_paged` (full per-category records, split at
  `MAX_CAT_ITEMS=128`), `catalog::Taxonomy`, `catalog::inject_paged`; `image.rs`
  emits + injects the tree; `library::categorize`. `atrium catalog --paged-out`,
  `atrium library categorize`. `config.rs` embeds taxonomy + categories.
- **Launcher (C):** `model.{c,h}` paged — `model_index_init` + `model_set_page` +
  a `PageLoader` callback (fired by `model_move_cat`/`model_select`); **ui.c
  unchanged** (public model API preserved). `catindex_parse` reads `index.jsonl`.
  `main.c` `load_index`/`load_page` + "Loading <category>…" notice; legacy
  `load_catalog` is the fallback. `MAX_CAT_ITEMS` in `catalog.h`.
- **Carousel polish:** wraps (end items show on the left at the default view;
  `model_move_item` wraps) + **per-category cursor memory** (`ModelCat.savedItem`).
- **Build speedup:** art bakes only **installed** titles (present-filter moved
  before the art pass) — small builds went from timing-out → ~18 s.
- **Snow-verified:** B&W 6.0.8 (Mac Plus/SE, 512/384) AND Colour 7.1 (Mac LC/II,
  1024/768, 8-bit art) — boot into Recommended, ↑/↓ pages a category off disk,
  ←/→ wraps items, per-category memory holds. Screenshots were inspected.

## 2. Targets / variants (`data/targets.json`, `data/templates.json`)

| Target | base_os | art_depths | partition KB | machine / emulator | status |
|---|---|---|---|---|---|
| Mac Plus/SE — 6.0.8 (B&W) | 6.0.8 | `1` | 512/384 | Snow (Mac II runs it 1-bit) | built+verified (small) |
| Mac LC/II — 7.1 (Colour) | 7.1 | `1,8` | 1024/768 | Snow + MDC | built+verified (small) |
| Mac II — 7.1 (Millions) | 7.1 | `1,8,24` | 1536/1024 | Snow + MDC (8-bit screen) / QEMU | built (small), 24-bit art bakes |
| Quadra 800 — 7.5.5 (full) | 7.5.5 | `1,8,24` | 2048/1536 | **QEMU q800 only** | FULL build in flight (§3) |

Templates: `6.0.8` (finder_replace), `7.1`, `7.5.5` (both Startup-Items). 7.5.5 base
= `/home/dani/MacOS_SampleDisks/MacLC_7-5-5.hda` (APM disk).

## 3. IN FLIGHT — the 7.5.5 "full" build

`atrium image --config <scratchpad>/755-full.json` → `/home/dani/macatrium-755-full.hda`,
`Selection::All` (~1489 titles), 24-bit art, 2 GB. Was at ~700/1489 harvested when
this was written; ETA ~20–30 min. Log: `<scratchpad>/755-full.log`. **On resume:
check if it finished** (`grep "image built" <log>`); the harvest is the long pole.
The full library → ~29 pages (< the 65-category `MODEL_MAX_CATS` limit), so it fits.

(scratchpad = `/tmp/claude-1000/-home-dani-repos-MacAtrium/<session>/scratchpad` — a
fresh session has a new path; the disk + log paths above are absolute and survive.)

## 4. NEXT — two open decisions + the remaining work

1. **QEMU q800 verification harness** for the Quadra/7.5.5 variant (Snow can't do
   68040). Needed: bless the built disk for SCSI boot (`rb-cli make-bootable`),
   boot `qemu-system-m68k -M q800 -bios .../f1acad13.rom -m 256 -drive
   file=<disk>,format=raw,if=scsi…` headless, capture the framebuffer (QMP
   `screendump` to PNG). Classic-MacOS-on-q800 boot timing is finicky — build it
   like the Snow harness. **User to decide: build this harness, or hand off the
   .hda + a recipe.**
2. **"Way more games into all 3 variants"** — the 7.5.5 is the all-games showcase.
   **User to decide** whether B&W / 256 / Millions get full-library rebuilds too
   (each ~20–30 min) or stay curated.
3. **`atrium add` / OS-migration paged-awareness** (docs/21 §10) — they still merge
   the legacy `catalog.jsonl`, not the per-category files. Make them paged-aware.
4. **v2 (deferred):** slim `CatItem` (drop in-RAM art paths, derive from id) for more
   B&W headroom; cancelable/debounced rapid category-skip (v1 loads synchronously,
   fine for ≤128 pages); on-demand `desc` files.

## 5. How to cut + verify a variant (recipe)

```sh
# 1. (if launcher changed) rebuild launcher + re-embed, then atrium-tool
cmake --build build && cargo build --release --manifest-path tools/atrium-tool/Cargo.toml
# 2. build a disk from a JSON config (base_os + selection + art_depths + app_mem_kb)
./tools/atrium-tool/target/release/atrium image --config build.json   # -> out .hda
# 3a. Snow (Plus/SE/LC/II): copy the .hda, run the harness, inspect snapshots
# 3b. QEMU q800 (Quadra): bless + qemu-system-m68k -M q800 (harness TBD)
```
Settings: `~/.macatrium.json` has `macpack_dir=/home/dani/macpack-work`,
`mg_archive=/home/dani/macgarden-archive`, `rb_cli=rb-cli` (on PATH at `~/.local/bin`).

## Memory to read
`build-and-snow-are-local` (the toolchain!), `21-category-paging`, `mgmt-ui-redesign`,
`workflow-verify-in-emulator`, `color-depth-in-slot-pram`,
`shrink-size-partition-per-config`, `commit-directly-to-main`.
