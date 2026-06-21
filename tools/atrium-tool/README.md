# tools/atrium-tool — `atrium`, the MacAtrium host build tool

The cross-platform, CI-able home for everything the 68k launcher can't do
itself (docs/06 content pipeline, docs/13 Priority 1). **Pure Rust, no native
dependencies** → builds and runs identically on macOS, Windows, and Linux, so a
contributor or a CI pipeline can rebuild the appliance image anywhere.

```sh
cd tools/atrium-tool
cargo build --release          # -> target/release/atrium
cargo test                     # unit tests (catalog facets, MacRoman, CR endings)
```

## Subcommands

| Verb | Status | What it does |
|------|--------|--------------|
| `catalog` | **done** | Compile a curated dataset → on-Mac `catalog.jsonl` (faceted categories, CR line endings, MacRoman encoding) |
| `pict` | planned | PNG/JPG → PICT, 1-bit + 8-bit depth variants (docs/06 Images) |
| `harvest` | planned | Pull apps out of a donor HFS image (the MacPack `.vhd`s) into `/MacAtrium/Apps` |
| `image` | planned | Orchestrate a full bootable build end-to-end (retire the bash `assemble.sh`) |

### `atrium catalog`

```sh
atrium catalog --src ../../data/library.jsonl --out /tmp/catalog.jsonl
# then write it onto a volume as type TEXT:
rb-cli put disk.hda /tmp/catalog.jsonl /MacAtrium/metadata/catalog.jsonl --type TEXT --creator ttxt
```

Reads the curated source dataset (UTF-8 JSONL, `data/library.jsonl`) and emits
the light index the launcher reads at boot. It **derives** the many-to-many
`categories` array from facet fields — the "facets + decade buckets" model:

| Source field | → derived category |
|--------------|--------------------|
| `kind` (`game`/`app`/`utility`) | `Games` / `Applications` / `Utilities` |
| `genre[]` | each genre verbatim (`Action`, `Puzzle`, …) |
| `color` (bool) | `Color` / `B&W` |
| `year` (int) | a decade bucket (`1980s`, `1990s`); the raw year is kept for sort |
| `vendor` | the publisher (`Broderbund`, …) |
| `mouse` (bool) | `Mouse Required` / `No Mouse` |
| `categories[]` | manual extras (e.g. `Recommended`, kept in dataset order) |

Output is validated against the on-device parser limits (≤256 items, ≤8
categories/item, ≤31 chars/category, name/path/desc lengths from `src/catalog.h`);
overflows are warned about (and categories clamped) so a generated catalog never
silently breaks the 68k reader. Strings are transcoded UTF-8 → MacRoman; any
character with no MacRoman equivalent becomes `?` and is counted in the summary.

Flags: `--lf` (LF endings for host debugging) / `--crlf` instead of the default
bare `CR`.

Verified end-to-end in Snow: see `docs/evidence/catalog-generated-all-12.png`,
`facet-*.png`, and `launch-return-generated-catalog.png`.
