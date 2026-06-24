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

## Workflow — three steps

A top tab bar walks you through it; a status/progress line sits at the bottom of
every step. Each action calls the same `atrium` function the CLI does.

1. **Library** — **Pick an `.hda`** + **Extract catalog** (`rb-cli get` on
   `/MacAtrium/metadata/catalog.jsonl`), or **Open** a `data/library.jsonl`. The
   table lists each title; toggle the **Color / B&W** and **Mouse** facets (the two
   LaunchBox can't provide) plus an optional launch **hotkey**, then **Save
   overrides** (`atrium::merge::set` → `data/overrides.jsonl`).
2. **Enrich** — fill metadata (gaps-only) from public sources:
   - **LaunchBox** — `Metadata.xml` → year/vendor/genre (`atrium::enrich`), with an
     optional Color/B&W auto-detect.
   - **Macintosh Garden** — a local scrape archive → year/vendor/genre/desc + the
     `source` attribution, colour detected offline (`atrium::mg`, 68K-only).
   - **Fetch software from Macintosh Garden** — downloads + extracts + injects a
     title's software into the **output** `.hda` and appends a catalog stub
     (`atrium::fetch`).
3. **Build** — three essentials up front (base system / launcher / output), an
   optional **content sources** group (MG archive + LaunchBox), and **Build image**
   (`atrium::image`). Everything else — dataset/overrides paths, platform, startup
   items, **art-depth variants**, art max-px, local art dir, sounds, harvest
   sources, and the dirs/tools (rb-cli, curl, apps/metadata/images dirs, stage) —
   lives under **Advanced**. Optional fields are omitted when blank so the CLI
   defaults apply.

### Art depths

The **art depths** checkboxes choose which PICT variants get baked per title —
`1` (dithered B&W raw), `4`/`8` (indexed, adaptive palette), `16` (Thousands),
`24` (Millions). The default is **1 / 8 / 24**; a deeper variant down-converts to
shallower screens at draw time, so the launcher always has something to show.
Tick a single box to bake just that one depth.

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

App icons live in `assets/icons/macatrium-*` (placeholders for now — swap in a
real one later).

## Follow-ups

Long operations (Extract / Enrich / Build) now run on a **worker thread** — the
window stays responsive, the action buttons disable with a spinner, and the
result is applied when the thread finishes. Box-art thumbnails in the table
(`egui_extras` image loaders) are still pending.
