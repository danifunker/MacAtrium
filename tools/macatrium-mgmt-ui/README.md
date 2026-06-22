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

## Workflow

1. **Pick an `.hda`** and **Extract catalog** — runs `rb-cli get` on
   `/MacAtrium/metadata/catalog.jsonl` (or **Open** a `data/library.jsonl` directly).
2. The table lists each title with **Color / B&W** and **Mouse** checkboxes — the
   two facets LaunchBox can't provide — plus name/year/vendor/genre.
3. **Enrich (LaunchBox)** fills year/vendor/genre from `Metadata.xml`
   (`atrium::enrich`).
4. Toggle the checkboxes, then **Save overrides** writes them to
   `data/overrides.jsonl` (`atrium::merge::set`).
5. **Build image** assembles a bootable `.hda` (`atrium::image`) from the base
   system + launcher + dataset + overrides.

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

Long operations run inline (a brief freeze); moving them to a worker thread, and
box-art thumbnails (`egui_extras` image loaders), are still pending.
