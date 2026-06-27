# tools/macatrium-mgmt-ui — MacAtrium Management UI

A thin GUI over the [`atrium`](../atrium-tool/) library. **Every action calls the
same `atrium` functions the CLI exposes** — the CLI stays the source of truth;
this is just a nicer way to drive it. (It deliberately does *not* link
rusty-backup; like the CLI, it invokes the unmodified `rb-cli` binary, so the
heavy C deps never enter this build.)

```sh
cd tools/macatrium-mgmt-ui
cargo run --release      # opens a window (needs a display — not the headless dev box)
```

## Workflow — six jobs

A top tab bar picks the **job**; a status/progress line sits at the bottom. Each
action calls the same `atrium` function the CLI does, and long operations run on a
**worker thread** so the window stays responsive (buttons disable with a spinner).
The bundled library + compatibility data are baked in, so a build needs no data
paths — just a Target, some titles, and an output.

1. **Build** — pick a **Target** (a Mac profile: base OS + art depths + launcher
   RAM, from `data/targets.json` ⊕ your own), choose the titles from the bundled
   ~1500-title library (search + **kind/genre** filters, optional box-art
   thumbnails), set the output `.hda`, and **Build disk** (`atrium::image`). The
   donor/dir/tool plumbing lives behind **Advanced**. **Migrate / clone from an
   existing disk** imports another MacAtrium disk's titles and can **Scrub** the
   ones the chosen Target's OS can't run (minOS/maxOS), then builds afresh.
2. **Add to disk** — pick a built MacAtrium `.hda`, a matching Target, and more
   titles → **Add to disk** (`atrium::image::add_to_disk`): injects the new titles
   and merges them into the disk's catalog, leaving the existing titles (and their
   baked art) intact.
3. **Library** — browse the bundled catalogue and edit each title's compatibility
   facets (**Colour/B&W**, **Mouse**, launch **hotkey**); **Save** writes the
   compatibility overlay (`atrium::merge::set` → `data/compatibility.jsonl`).
   **Load Existing MacAtrium Disk** extracts a built disk's catalog for editing
   (`rb-cli get` on `/MacAtrium/metadata/catalog.jsonl`).
4. **Database** — explore the **Macintosh Garden** archive (~21k titles)
   cross-referenced against MacPack (`atrium::mgdb`): a filterable table — type,
   architecture, OS, category, year, colour — that flags which titles are
   **missing from MacPack** (the "what are we missing" view). **Detect colour**
   fills Colour/B&W offline from screenshots. Needs the MG-Archive set.
5. **Attain** — register the **MacPack** folder (the donor disks a build harvests
   from) and run the **Macintosh Garden downloader** to cache the selected titles'
   software (`atrium::fetch`, gated on a valid MG-Archive).
6. **⚙ Settings** — MacPack / MG-Archive / cache / `rb-cli` paths, saved to
   `~/.macatrium.json`; a **Targets** editor (add/update/remove your own profiles
   over the bundled defaults); and a read-only Templates list. A **first-run
   wizard** auto-detects `rb-cli` and prompts for the source folders.

**Save config… / Load config…** (on the Build screen) round-trip the whole form to
a `builds/*.json` — the exact schema `atrium image --config` consumes — so you can
build in the GUI then run/version it from the CLI, or open an existing build to
tweak.

### Launcher RAM (the `SIZE` partition)

A **Target** pins this for you; to tune it by hand, **Build → Advanced → launcher
RAM KB** sets the preferred / minimum memory partition baked into the launcher
(`app_mem_kb`). Blank keeps the binary's 2 MB / 1 MB; the **Colour** (1024/768)
and **Compact B&W** (512/384) presets fill the measured per-target values, and the
**Mac Plus / SE (B&W only)** toggle auto-applies Compact when the fields are blank.
Compact machines (4 MB total) are starved by 2 MB, so shrinking this is what lets a
Mac Plus/SE build leave room for System 6 + the game.

### Art depths

A **Target** sets these too; under **Build → Advanced** the **art depths**
checkboxes choose which PICT variants get baked per title — `1` (dithered B&W raw),
`4`/`8` (indexed, adaptive palette), `16` (Thousands), `24` (Millions). A deeper
variant down-converts to shallower screens at draw time, so the launcher always has
something to show. Tick a single box to bake just that one depth.

## Releases

The Manager ships in the release pipeline (`.github/workflows/release.yml`),
mirroring how rusty-backup packages its GUI. **Each GUI package also bundles the
[`atrium`](../atrium-tool/) CLI** built from the same target, so one download per
platform delivers both tools:

- **Windows** (x64 + arm64) — a per-user Inno Setup installer
  (`MacAtrium-Manager-Setup.exe`) that installs into `%LocalAppData%` without
  admin rights, plus a plain `.zip`. Both carry `macatrium-mgmt-ui.exe` and
  `atrium.exe` side by side. Script: `installer/macatrium-mgmt-ui.iss`.
- **macOS** (arm64 + x64) — a `MacAtrium Manager.app` bundle wrapped in a
  `.dmg`. The CLI lives at `Contents/MacOS/atrium` (signed inside-out with the
  bundle). Developer ID codesigning + notarization activate only when the
  `MACOS_*` repo secrets are present; otherwise the `.app` is ad-hoc signed and
  still runs locally.
- **Linux** (x64 + arm64) — a portable `.AppImage` (Anylinux / quick-sharun)
  that bundles both binaries. sharun dispatches on `argv0`, so the CLI is
  reachable by symlinking the AppImage to `atrium`.

The standalone `atrium-<platform>` archives (from the `build-tool` job) stay for
headless / CLI-only use.

App icons live in `assets/icons/macatrium-*` (placeholders —
[backlog](../../docs/10-open-questions.md#backlog-deferred--low-priority-not-blocking)).
