# data/ — the curated source dataset

Three JSONL files feed the build (`# …` / `// …` and blank lines are comments):

- **`library.jsonl`** — the **generated** catalog of titles: identity + descriptive
  metadata only (id, name, kind, year, vendor, genre[], desc, art, `app`,
  `harvest_src`). **Don't hand-edit it** — it's regenerated from the MacPack (see
  below). Put hand changes in `curated.jsonl` instead.
- **`curated.jsonl`** — the hand-maintained **overlay**: add-on / non-MacPack titles
  (full records) + corrections. Merged over the scrape, **curation wins**.
- **`compatibility.jsonl`** — per-title **requirements/facets**, keyed by `id`,
  merged over the library at build time (overlay wins). Hand-verified entries win
  over the auto-seeded ones. Fields: `color` (Color/B&W), `mouse` (Mouse Required),
  `maxDepth` (deepest screen bpp a title tolerates; launcher caps to it),
  `minOS`/`maxOS` (dotted range — out-of-range titles are dropped per target),
  `minMem` (KB), `minCPU`, `arch` (68K/PPC/BOTH).
- **`catalog.jsonl`** — a tiny hand-authored *output-format* sample from the MVP era.
  The real on-Mac catalog is **generated**, not authored.

## Regenerating the library (Library Builder)

```sh
# 1. scan the MacPack donor disks -> one record per title
atrium library scan --macpack ~/macpack-work --release 20240825-RC1 --out /tmp/scrape.jsonl
# 2. merge the hand-curated overlay (curation wins, non-MacPack add-ons kept)
atrium merge --base /tmp/scrape.jsonl --overlay data/curated.jsonl --out data/library.jsonl
# 3. enrich vendor/genre/desc/colour from the Macintosh Garden archive (gaps-only)
atrium mg --src data/library.jsonl --mg-archive ~/macgarden-archive --out data/library.jsonl
# 4. move the requirement/facet fields into compatibility.jsonl (hand entries win)
atrium library split --library data/library.jsonl --compat data/compatibility.jsonl
```

`atrium library scan` derives `kind` (game/app/utility) from the MacPack tree — the
single exclusive bucket — and `year` from `/Games/<year>`; `genre` is a multi-valued
(non-exclusive) tag list seeded from the Applications/Utilities category folder.
`harvest_src` records the donor by its **original filename** (`boot.vhd`,
`Supplement.vhd`) + path, so a build resolves it against the configured MacPack folder.

## Source → on-Mac catalog

`atrium catalog` compiles `library.jsonl` (+ the merged `compatibility.jsonl`) into the
light index the launcher reads at boot, **deriving** the many-to-many `categories`
from the facet fields:

| Field | From | → derived category / use |
|--------------|-----|--------------------------|
| `id` / `name` / `app` | library | slug / display / launch path |
| `kind` | library | `Games` / `Applications` / `Utilities` |
| `genre[]` | library | genre categories (`Action`, `Puzzle`, …) — multi-valued |
| `year` | library | decade bucket (`1980s`/`1990s`); raw year kept |
| `vendor` | library | publisher category |
| `color` | compatibility | `Color` / `B&W` |
| `mouse` | compatibility | `Mouse Required` / `No Mouse` |
| `maxDepth` | compatibility | launch-time screen-depth cap |
| `minOS`/`maxOS`/`minMem`/`arch` | compatibility | preflight compatibility vs the target |

The tool transcodes to **MacRoman**, emits **CR** line endings, and validates against
the on-device parser limits (`src/catalog.h`). See the
[tool README](../tools/atrium-tool/README.md).

## Provenance

Titles come from the **MacPack** collection (`MacPack-20240825-RC1`, the MiSTer
MacPlus pack on Archive.org — HFS disk images organised by year/genre that `rb-cli`
reads directly). Metadata is enriched from the local **Macintosh Garden** archive
(68K-only) and/or the **LaunchBox Games Database**; both only fill gaps, so curation
wins. `color`/`mouse`/`maxDepth` and the other requirements are curated/verified in
`compatibility.jsonl` (no public metadata source carries them).
