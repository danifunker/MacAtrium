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

Long operations run inline for now (a brief freeze); moving them to a worker
thread, and box-art thumbnails (`egui_extras` image loaders), are follow-ups.
It builds for the same modern targets as the CLI; wiring it into the release
pipeline is optional (it pulls eframe/winit and needs display libs at runtime).
