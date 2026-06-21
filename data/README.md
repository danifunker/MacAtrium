# data/ — the curated source dataset

Two files live here:

- **`library.jsonl`** — the **curated source dataset** (the thing humans edit /
  PR). UTF-8 JSONL keyed by `id`, carrying *facet* fields. `# …` / `// …` lines
  and blank lines are comments. This is the input to the host tool.
- **`catalog.jsonl`** — a tiny hand-authored *output-format* sample kept from the
  MVP era (schema v2). The real on-Mac catalog is now **generated**, not authored.

## Source → on-Mac catalog

The host tool [`atrium`](../tools/atrium-tool/) compiles `library.jsonl` into the
light index the launcher reads at boot, **deriving** the many-to-many
`categories` from the facet fields — the "facets + decade buckets" model:

```sh
atrium catalog --src data/library.jsonl --out /tmp/catalog.jsonl
rb-cli put disk.hda /tmp/catalog.jsonl /MacAtrium/metadata/catalog.jsonl --type TEXT --creator ttxt
```

| Source field | Req | → derived category / use |
|--------------|-----|--------------------------|
| `id` | ✅ | stable slug |
| `name` | ✅ | display name |
| `app` | ✅ | launch path, relative to `/MacAtrium` |
| `kind` | ○ | `game` (default) / `app` / `utility` → `Games` / `Applications` / `Utilities` |
| `genre` | ○ | array → genre categories (`Action`, `Puzzle`, …) |
| `color` | ○ | bool → `Color` / `B&W` |
| `year` | ○ | int → decade bucket (`1980s` / `1990s`); raw year kept for sort/display |
| `vendor` | ○ | publisher → its own category (`Broderbund`, …) |
| `mouse` | ○ | bool → `Mouse Required` / `No Mouse` |
| `categories` | ○ | manual extras (e.g. `Recommended`), preserved in dataset order |
| `desc` / `image` / `type` / `creator` | ○ | passed through to the catalog |

The tool transcodes to **MacRoman**, emits **CR** line endings, and validates
against the on-device parser limits (`src/catalog.h`). See the
[tool README](../tools/atrium-tool/README.md).

## Provenance

The starter titles in `library.jsonl` are real Macintosh classics from the
**MacPack** collection (the MiSTer MacPlus pack — a curated set of HFS disk
images, organised by year/genre, that `rb-cli` reads directly). The metadata is
a curated starter; refine `vendor`/`year`/etc. via PR. Harvesting the actual app
forks out of the pack into `/MacAtrium/Apps` is the `atrium harvest` step (docs/13).

`year`/`vendor`/`genre` can be filled automatically with **`atrium enrich`** from
the **LaunchBox Games Database** (it only fills missing fields, so curation wins).
`color` (Color/B&W) and `mouse` (Mouse Required/No Mouse) aren't in LaunchBox —
set those by hand.
