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

**Releases / CI** — [`.github/workflows/release.yml`](../../.github/workflows/release.yml)
builds `atrium` for macOS / Windows / Linux on x86_64 + arm64 (native runners,
no cross toolchain — the crate has no C deps), builds the 68k `MacAtrium.bin` in
Retro68's published container, runs the C core's host tests, and publishes a
GitHub release with **the Mac launcher plus the per-platform build tools**.
Every push builds + tests; releases publish on `main` / tag pushes.

## Subcommands

| Verb | Status | What it does |
|------|--------|--------------|
| `catalog` | **done** | Compile a curated dataset → on-Mac `catalog.jsonl` (faceted categories, CR line endings, MacRoman encoding) |
| `harvest` | **done** | Pull apps out of a donor HFS image (the MacPack `.vhd`s) into `/MacAtrium/Apps`, both forks, + dataset stubs |
| `enrich` | **done** | Fill the dataset (year/vendor/genre + art URLs) from the LaunchBox Games Database |
| `merge` | **done** | Apply a manual overrides overlay (colour/mouse, corrections, unmatched titles) over the dataset |
| `pict` | **done** | PNG/JPEG → PICT at 1/4/8/16-bit (docs/06 Images) |
| `image` | **done** | Orchestrate a full bootable build end-to-end (retires the bash `assemble.sh`) |

The pipeline: **`harvest`** (bare stubs from a donor disk) → **`enrich`** (fill
metadata from LaunchBox) → **`merge`** (manual `overrides.jsonl`: colour/mouse +
corrections, which win) → **`catalog`** + **`pict`** → **`image`** ties it all
together into a bootable `.hda`.

### `atrium catalog`

```sh
atrium catalog --src ../../data/library.jsonl --out /tmp/catalog.jsonl
# or generate AND inject in one step, backing up any existing on-image catalog:
atrium catalog --src ../../data/library.jsonl --out /tmp/catalog.jsonl \
  --into disk.hda --backup-dir /tmp --rb-cli /path/to/rb-cli
```

`--into <image>` writes the catalog onto the volume as type `TEXT` (overwriting
with `--force`), after saving the existing one to `<backup-dir>/catalog-prev.jsonl`
via `rb-cli get` — so an on-volume catalog is never silently clobbered.

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

### `atrium harvest`

Pull real apps out of a donor HFS image — the MacPack `.vhd`s, or any sample
disk — into the MacAtrium tree. For each source app folder it finds the
launchable `APPL`, extracts it plus its data files with **both forks** (via
`rb-cli get-binhex`), skips bundled clutter (System/Finder, Desktop DB/DF, Icon),
and emits a `data/library.jsonl` stub (id/name/app path, with `year` and `kind`
inferred from the source path — e.g. `/Games/1986/…` → game, 1986).

```sh
# Harvest specific app folders, staging the forks + stubs to a dir:
atrium harvest --image ~/macpack-work/boot.vhd \
  --app "/Games/1986/Dark Castle 1.2" \
  --app "/Games/1991/Lemmings" \
  --stage /tmp/harvest --rb-cli /path/to/rb-cli

# Harvest every subfolder of a source dir, and inject straight into a target image:
atrium harvest --image ~/macpack-work/boot.vhd --scan "/Games/1986" \
  --stage /tmp/harvest --into target.hda --rb-cli /path/to/rb-cli
```

Flags: `--app <folder>` (repeatable) and/or `--scan <dir>`; `--stage <dir>` for
the `.hqx` forks + `harvested.jsonl`; `--into <image>` to also inject; `--rb-cli`
for the rb-cli binary path; `--apps-root` (default `/MacAtrium/Apps`).

Harvest is **incremental**, not one-shot: `--append-to data/library.jsonl` merges
the new stubs into the curated dataset, **de-duplicated by `id`** so a re-run
never clobbers hand-enriched entries and you can keep adding titles over time.
The emitted stubs are minimal — enrich them with `vendor`/`color`/`mouse`/`genre`
in `data/library.jsonl`, then run `catalog`.

Verified in Snow: harvested Prince of Persia (+ pack games) injected into a fresh
image, then launched and returned through the launcher
(`docs/evidence/harvest-pop-{selected,running,returned}.png`).

### `atrium pict`

Convert PNG/JPEG artwork to a classic-Mac **PICT** file (a 512-byte header +
PICT v2 picture data). QuickDraw `DrawPicture`s it directly — there's no
PNG/JPEG decoder on 68k.

```sh
atrium pict --input boxart.png --out boxart_8.pict --depth 8   # 1 | 4 | 8 | 16
```

- **1/4/8-bit** → indexed `PackBitsRect` (0x0098) with an embedded colour table:
  1-bit uses an ordered (Bayer) dither; 4-bit the classic Mac 16-colour CLUT;
  8-bit a 6×6×6 cube + grey ramp. `--no-pack` stores rows uncompressed.
- **16-bit** → `DirectBitsRect` (0x009A), 1-5-5-5 "thousands" pixels.
- Adaptive (median-cut) palettes and resizing are a future quality pass.

### `atrium enrich`

Fill the curated dataset from the **LaunchBox Games Database** — streams the
~500 MB `Metadata.xml` (SAX-style, low memory) and matches our titles by name.

```sh
# one-time: grab the DB
curl -L https://gamesdb.launchbox-app.com/Metadata.zip -o Metadata.zip && unzip Metadata.zip
atrium enrich --src data/library.jsonl --metadata Metadata.xml --out data/library.jsonl \
  --art-manifest /tmp/art.jsonl     # optional Box-Front art URLs (id, databaseID, art)
```

Filters to `--platform "Apple Mac OS"` (default), then fills **`year`** (ReleaseYear/
ReleaseDate), **`vendor`** (Publisher), and **`genre[]`** (Genres, `;`-delimited) —
**only where missing**, so hand-curated values survive (use `--overwrite` to force).

**`--detect-color`** auto-fills the `color` facet by downloading a **gameplay
screenshot** (not box art — box art is colourful even for B&W games) and measuring
its colourfulness. It's a heuristic over the actual screenshot (early-Mac shots
are often B&W even for titles that later got colour), so it's overridable via
`merge`. **Mouse-required isn't derivable and stays curated.**

Matching strips parenthetical qualifiers, `:` subtitles, leading/trailing articles,
and dotted version suffixes — so our clean titles hit LaunchBox's disambiguated
ones ("Prince of Persia (Brøderbund Software)", "Deja Vu: A Nightmare Comes
True!!", "Hobbit, The", "Glider 4.0") — preferring the entry with the most
complete data. Unmatched titles are reported for manual fixing. Approach adapted
from megatron-uk/x68klauncher's `tools/metadata.py`.

### `atrium merge`

Apply a manual overrides overlay onto the dataset — the home for hand-captured
data: the **colour/mouse** facets LaunchBox lacks, corrections to anything
`enrich` got wrong, and whole records for titles it couldn't match.

```sh
atrium merge --base data/library.jsonl --overlay data/overrides.jsonl --out data/library.jsonl
```

`overrides.jsonl` holds **partial records keyed by `id`** — only the fields you
set are applied, and the overlay **wins** (use `--fill-missing` to only fill
gaps). Overlay ids not present in the base are appended as new records. So the
full metadata flow is: `enrich` fills from LaunchBox (gaps only) → `merge` lays
your manual corrections on top (authoritative).

### `atrium image`

The one-command bootable build — composes every verb above from a JSON config:

```sh
atrium image --config example-image.json
```

It (1) copies the base `system` → `out`; (2) `harvest`s each donor's `apps` into
the image (appending stubs to a **throwaway copy** of the dataset — the build
never mutates `data/library.jsonl`); (3) `enrich`es from `metadata` (LaunchBox);
(4) `merge`s the manual `overrides`; (5) converts `art_dir/<id>.{png,jpg}` →
PICT at `art_depth` and wires the catalog `image` field; (6) generates + injects
the `catalog` (backing up any existing one); (7) installs the `launcher` into
`startup_items`. See [`example-image.json`](example-image.json) for the schema;
all fields except `system`/`out`/`launcher`/`dataset` are optional.

**Verified in Snow:** a full `atrium image` run (~2 s) produced a bootable image
that boots into the faceted catalog, renders the built-in art, and launches a
harvested Prince of Persia — `docs/evidence/image-built-{catalog,art,pop-running}.png`.

The launcher previews the selected item's `image` PICT with the **P** key.
**Verified rendering in Snow** (1-bit screen): 1-bit (dithered), 8-bit, and
16-bit all `DrawPicture` correctly — `docs/evidence/pict-render-{1bit,8bit,16bit}.png`.
**Known issue:** a **4-bit** PICT faults this emulator's QuickDraw when drawn onto
a *1-bit* screen (crash when packed, hang when unpacked) — both modes; the file
itself is structurally valid (round-trip-decodes, identical layout to the working
8-bit). 1/8/16-bit on the same screen are fine, so it's a QD/Snow 4→1-bit
conversion bug, not an encoder defect; 4-bit's real check awaits a colour-depth
screen. In production the launcher should load the art variant matching the
screen depth (docs/06), so a 1-bit screen gets the 1-bit variant.
