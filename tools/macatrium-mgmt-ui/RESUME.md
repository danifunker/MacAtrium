# Resume — MacAtrium Management UI, job-based rebuild

## STATUS 2026-06-26 — the rebuild is BUILT (shell + most jobs). What's left:

- **Add-to-disk inject backend** — the screen (disk picker + title picker) is
  scaffolded but the "Add to disk" button is disabled. Implement the inject:
  `selection::harvest_plan` → `harvest::run(.., into: Some(disk), append_to: stub)`
  for the MacPack donors → regenerate the on-disk catalog (`catalog::run` +
  `catalog::inject`). Best done as a new `atrium image`-side controller fn
  (e.g. `image::add_to_disk(&AddConfig)`) so the CLI gets it too (MVC/DRY).
- **Genre/tag filters + box-art thumbnails** in the title picker (`egui_extras`
  image loaders); `LibRow` dropped `genre`/`vendor` — re-add when the filter lands.
- **OS-migration mode** — load a built disk, retarget to a newer OS, scrub
  incompatible titles via `maxOS`.
- **Verify the GUI visually** (Snow / on a display) — the headless box can't.

Everything below is the original plan; steps 0–4 are DONE, 5–6 partial. Commits:
`tooling: build Targets…`, `mgmt-ui: rebuild around jobs…`, `deps: upgrade…`.

---

Paste into a fresh session. **Goal: re-architect the egui Management UI
(`tools/macatrium-mgmt-ui`) from the current pipeline-stage tabs
(Library / Enrich / Build) into job-based screens, on top of the data + tooling
foundation that's already built.** The user found the current UI confusing because
it mirrors the CLI verbs and exposes raw machine paths/plumbing.

## Target information architecture (agreed with the user)

- **Build a disk** — pick a **Target** → pick titles from the Library → write the
  MacAtrium output disk.
- **Add to a disk** — pick an existing MacAtrium output `.hda` → pick titles →
  inject (extend an already-built disk).
- **Library** — browse/edit titles + their compatibility facets; **Load Existing
  MacAtrium Disk** imports a built disk's catalog/metadata here for editing.
- **Attain** — *acquire source software* (separate from Build): register/locate the
  **MacPack** folder; run the **MG downloader** (gated on a valid MG-Archive). The
  MG downloader caches once and the bits may need manual install — not a slide-in.
- **⚙ Settings** — Targets & Templates, tool paths, MacPack/MG-Archive/cache
  locations; persisted to `~/.macatrium.json`. A **first-run wizard** auto-detects
  `rb-cli` and prompts for the MacPack/MG folders.

Plumbing currently on the Build tab (harvest sources, donor disks, apps_root/
metadata_dir/images_dir/stage, rb-cli path) moves to Settings/Advanced — a normal
user shouldn't see it.

## Foundation already in place (build the GUI ON this — don't rebuild it)

The CLI is the source of truth; the GUI is a thin view over `atrium::config::BuildConfig`
+ `atrium` functions (MVC — see memory `build-tool-mvc-architecture`).

- **`$HOME` settings** — `tools/atrium-tool/src/settings.rs`: `Settings`
  { `macpack_dir`, `mg_archive`, `rb_cli`, `cache_dir` }, load/save to
  `~/.macatrium.json` (`$MACATRIUM_CONFIG` overrides the path). CLI `atrium config
  [--macpack-dir|--mg-archive|--rb-cli|--cache-dir]` shows/sets it. `image::run`
  already reads `settings.macpack_dir` (donor resolver) + `settings.rb_cli`.
- **Bundled data** — `config.rs` `EMBEDDED_LIBRARY` / `EMBEDDED_COMPAT` /
  `EMBEDDED_LAUNCHER` (`include_bytes!`). `dataset` / `launcher` / `overrides`
  (the `compatibility` key) are all `Option` → bundled when absent. So a build
  config needs only `out` + `selection` (+ a Target). Helpers: `dataset_bytes()`,
  `compatibility_bytes()`, `launcher_bytes()`.
- **Donor resolver** — `selection.rs` `resolve_donor`: a `donors.json` alias OR a
  disk *filename* (e.g. `boot.vhd`) under `macpack_dir`. So scrape titles harvest.
- **Library Builder backend** — `atrium library scan` (MacPack → `library.jsonl`,
  ~1489 titles) + `atrium library split` (move requirement facets into
  `compatibility.jsonl`). See `library.rs` + `data/README.md` for the pipeline.
- **Data model (3 files, all bundled)** — `data/library.jsonl` (generated: identity
  + descriptive metadata, no color/mouse), `data/curated.jsonl` (hand overlay, wins),
  `data/compatibility.jsonl` (requirements keyed by id: `color`/`mouse`/`maxDepth`/
  `minOS`/`maxOS`/`minMem`/`minCPU`/`arch`).
- **`atrium size`** (patch launcher SIZE), per-config `app_mem_kb`, curl→`ureq`
  (no external curl), the GUI **scroll fix** (status in a bottom `TopBottomPanel`,
  content in a `ScrollArea`).

## Current GUI (what you're replacing) — `tools/macatrium-mgmt-ui/src/main.rs`

- `enum Tab { Library, Enrich, Build }`; `tab_library` / `tab_enrich` / `tab_build`.
- `App` struct holds every field as a `String`/bool; the model bridge is
  **`to_config() -> BuildConfig`** and **`apply_config(BuildConfig)`** (keep these —
  they're the view↔model mapping and are unit-tested: `config_round_trips_through_gui`).
- `save_config()` / `load_config()` round-trip a `builds/*.json` (same schema the CLI
  runs). `app_mem_kb()` / `art_depths()` derive build fields. Long ops run on a
  worker thread (`spawn_job`/`poll_job`/`Done`).
- The Library tab's "Extract catalog" (`rb-cli get` of `/MacAtrium/metadata/
  catalog.jsonl`) is the seed of **Load Existing MacAtrium Disk**.

## The one thing NOT yet built: Targets & Templates

This is **step 0** of the rebuild. Today the build resolves `base_os` via the
**templates registry** (`data/templates.json` → keys `6.0.8`, `7.1`; each = base
`.hda` + `finder_replace` + `startup_items`; resolved in `templates.rs`).

**Proposed model:** a **Target** = a named build *profile* that references a template
and pins the machine settings:
```jsonc
// data/targets.json (bundled defaults) + user targets in ~/.macatrium.json
{
  "Mac Plus/SE (B&W)": { "base_os": "6.0.8", "art_depths": ["1"],
                          "app_mem_kb": [512,384] },
  "System 7.1 (Colour)": { "base_os": "7.1", "art_depths": ["1","8"],
                            "app_mem_kb": [1024,768] }
  // future: screen size, default maxDepth, disk_size_mb
}
```
Picking a Target fills those `BuildConfig` fields. Bundle a couple of defaults +
let the user add/edit in Settings (persist to `~/.macatrium.json`). Decide:
targets registry as a `data/targets.json` (like `templates.json`/`donors.json`,
bundled via `include_bytes!`) **plus** user overrides in settings — recommended.

## Build order (keep the GUI compiling/working between steps)

0. ✅ **Targets/Templates model** — `targets.rs` (registry + bundled defaults +
   user targets in `Settings`); `Target::apply_to(&mut BuildConfig)`; CLI `targets`.
1. ✅ **Settings screen + first-run wizard** — `rb-cli` detect, MacPack/MG-Archive
   folders → `~/.macatrium.json`; Targets editor + read-only Templates list.
2. ✅ **Restructure tabs → jobs**: Build / Add-to-disk / Library / Attain / Settings.
   `to_config`/`apply_config` kept verbatim as the controller.
3. ✅ **Build screen** — Target combo → virtualised/searchable/kind-filtered title
   picker over the bundled ~1489 titles → output. Plumbing behind Advanced.
4. ✅ **Library screen** — browse (library ⊕ compat) + edit facets; Load Existing
   MacAtrium Disk imports its catalog. (genre filter + box art still pending.)
5. ⏳ **Add-to-disk** — pick an existing output `.hda` → pick titles → inject. UI
   scaffolded; **inject backend is the next task** (harvest-into-existing + catalog
   regen — see STATUS at the top; design it as an `atrium` controller fn).
6. ◑ **Attain** — `macpack_dir` register (done); MG downloader caches the *selected*
   titles into the cache (`fetch::run` with `into:None`), gated on MG-Archive.

## Open design questions to settle as you go

- Targets vs Templates exact split + storage (bundled `targets.json` + user-in-settings?).
- "Add to existing disk": the build pipeline today writes a fresh image; injecting
  into an existing MacAtrium `.hda` needs harvest `--into` + catalog regen — design it.
- OS-migration mode (load a MacAtrium disk, retarget to a newer OS, scrub
  incompatible via `maxOS`) — the user flagged this as a richer *future* mode.
- Title browsing for ~1500 entries: search + facet filters + a virtualized list.
- Box-art thumbnails (`egui_extras` image loaders) — still pending (GUI README).
- `kind` is the single exclusive bucket; `genre`/tags are multi-valued (slice-and-dice).

## Build / verify

```sh
cd ~/repos/MacAtrium
cargo build --release --manifest-path tools/atrium-tool/Cargo.toml
cargo build --release --manifest-path tools/macatrium-mgmt-ui/Cargo.toml   # GUI
cargo test  --release --manifest-path tools/macatrium-mgmt-ui/Cargo.toml   # incl. config round-trip
# The GUI needs a display — the headless dev box can't screenshot it; the USER runs
# `cargo run --release -p macatrium-mgmt-ui` to verify visually. Keep the
# to_config/apply_config round-trip test green as the regression guard.
```
A build needs `rb-cli` on PATH (now at `~/.local/bin/rb-cli`) and `~/.macatrium.json`
with `macpack_dir` set (for harvesting MacPack/scrape titles).

## Memory
Read: `mgmt-ui-redesign` (the agreed IA + data model), `build-tool-mvc-architecture`,
`macpack-data-source`, `macgarden-archive`, `shrink-size-partition-per-config`,
`workflow-verify-in-emulator`. Commit straight to `main` (`commit-directly-to-main`).
