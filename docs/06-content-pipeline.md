# 06 — Content Pipeline & Catalog Format

How the curated library is defined, built, and read. Three actors:

```
  repo dataset (curated JSON, PR-driven,        host tooling (modern Mac):
  enriched from LaunchBox/MG)          ─────▶   build the PAGED catalog (a category
                                                 index + one page per category) +
                                                 convert art to PICT, rb-cli inject
                                                          │
                                                          ▼
                                          on-volume  /MacAtrium/{metadata,images,Apps}
                                                          │
                                                          ▼
                                          MacAtrium (68k) loads the category INDEX at
                                          boot, then loads ONE category page at a time
```

Key principle: **all the heavy lifting happens on the modern host.** The 68k app
holds only the small category index plus the one category page it's showing, and
lazily pulls curated files. Categories live in **metadata, not the folder
layout**, so one app can be in **many categories** — and the catalog is **paged
by category** so a library can hold far more titles than a 4 MB Mac could ever
keep resident (see [21-category-paging.md](21-category-paging.md) for the why/RAM math).

## On-volume layout

A single root folder on the boot volume (name = the app):

```
/MacAtrium/
├─ Apps/        the actual apps & games (or aliases to apps installed elsewhere)
│   └─ Dark Castle/Dark Castle, Lemmings/Lemmings, ...
├─ metadata/    the PAGED catalog the app reads
│   ├─ index.jsonl              the category list (one line per category page)
│   ├─ cats/<slug>.jsonl        one file per category — its items (loaded on demand)
│   ├─ hotkeys.jsonl            the few items with a launch hotkey (resident)
│   └─ catalog.jsonl            legacy single-file catalog (≤256 items; back-compat)
└─ images/      curated artwork, one (or a few depth variants) per item
    └─ dark-castle.1.pict, dark-castle.8.pict, ...
```

`Apps/` is the only part the user might hand-populate. `metadata/` and `images/`
are produced by the host tooling. The whole `/MacAtrium` tree is **self-contained
and relocatable** — paths in the records are relative to the `/MacAtrium` root.

## The paged catalog

The on-Mac parser holds fixed-size records, so a single catalog is capped at
**256 items** and a 4 MB Mac can't hold even that comfortably. So the catalog is
**split by category** (docs/21):

### `metadata/index.jsonl` — the category index (resident)

One flat JSON object per line; this is the small, always-resident list the
launcher navigates left/right (up/down) through. In **display order** — the first
entry is the default landing view (Recommended).

```json
{"name":"Recommended","slug":"recommended","count":18,"ordered":true}
{"name":"Action & Arcade","slug":"action-arcade","count":128,"ordered":false}
{"name":"Action & Arcade (2)","slug":"action-arcade-2","count":72,"ordered":false}
```

| Field | Notes |
|-------|-------|
| `name` | Display name of the category page |
| `slug` | Filesystem-safe key → `cats/<slug>.jsonl` |
| `count` | Items in this page (for display until the page loads) |
| `ordered` | 1 = keep dataset order (Recommended/Featured); else alphabetical |

A category larger than the per-page cap (`MAX_CAT_ITEMS`, **128**) is split by the
generator into numbered sub-pages ("Action & Arcade (2)", …) — each its own index
entry + file — so the launcher never holds more than 128 records at once.

### `metadata/cats/<slug>.jsonl` — one category's items

Newline-delimited, **one record per launchable item** (schema below). An item is
**duplicated into every category file it belongs to** (disk is cheap; RAM is the
scarce resource). Loaded on demand when its category becomes current.

```json
{"id":"dark-castle","name":"Dark Castle","categories":["Recommended","Action & Arcade","Black & White"],"app":"Apps/Dark Castle/Dark Castle","year":1986,"vendor":"Silicon Beach Software","genre":"Action","desc":"Platformer with throwing rocks.","image":"images/dark-castle"}
```

| Field | Req | Notes |
|-------|-----|-------|
| `id` | ✅ | Stable slug; key into the dataset + the category DB + prefs |
| `name` | ✅ | Display name (Chicago, may be truncated) |
| `categories` | ✅ | **Array** of the item's categories (its DB membership; drives the tag line) |
| `app` | ✅ | Path **relative to `/MacAtrium`** to the app/alias to launch |
| `year`/`vendor`/`genre` | ○ | Display/sort metadata |
| `desc` | ○ | Short blurb (More-Info card) |
| `image`/`shot`/`icon` | ○ | Art base paths (`images/<id>`), resolved to a depth variant at draw time |
| `type`/`creator`/`hotkey`/`maxDepth` | ○ | OSTypes; launch hotkey; per-title screen-depth cap |

## Categories come from an editable database

Categories are **not** derived from the messy genre data at build time (315 games
have no genre; many "genres" are MacPack folder artifacts). They live in an
explicit, hand/GUI-editable **category DB**, seeded from a taxonomy:

- **`data/taxonomy.json`** — the **~15 canonical categories + display order**
  (Recommended first/default), plus the seed rules. Today: Recommended,
  Action & Arcade, Adventure, Puzzle, Strategy & Sim, Role-Playing, Interactive
  Fiction, Card & Casino, Sports, Educational, **Color**, **Black & White**,
  No Mouse Required, Applications, Utilities.
- **`data/categories.jsonl`** — the DB: `{id, categories[]}` per title, the source
  of truth for membership (multi-membership native).
- **`atrium library categorize`** seeds/refreshes the DB from the library +
  compatibility facets + taxonomy, **preserving hand/GUI edits**. Colour/B&W come
  from the colour facet or the pre/post-1987 era; Applications/Utilities from
  `kind`; the rest from a genre→bucket seed + the curated Recommended list.

There is **no synthesized "All"** category (the whole library can't be one page);
the launcher lands in the first index category. **Ordering** within a category is
alphabetical except recommendation-style ones (Recommended/Featured/Staff Picks),
which keep dataset order.

## Runtime loading (68k)

1. **Boot:** read `metadata/index.jsonl` → the resident category list
   (`catindex_parse` → `CatRef[]`), and load the **first** category's page. If
   `index.jsonl` is absent, fall back to the legacy single `catalog.jsonl`.
2. **Navigate categories** (up/down): `model_move_cat` fires the **page loader** —
   a brief "Loading <category>…" notice, then read `cats/<slug>.jsonl` into the one
   resident page (`gCat`, a `CatItem[MAX_CAT_ITEMS]` allocated once and reused) via
   `catalog_parse_into`, and `model_set_page` installs it. Only the current page is
   in RAM.
3. **Navigate items** (left/right) within the loaded page; **lazily load** the
   selected item's `image` PICT (purged as you scroll) directly from the volume.
4. **Launch:** resolve `app` (relative → `FSSpec`/alias) and sub-launch
   ([08-launching-system.md](08-launching-system.md)).

The model's public interface (`model_cur_cat`/`model_cur_item`/`model_move_item`)
is unchanged, so the UI (`ui.c`) didn't change — paging is a callback the model
fires; `ui.c` just renders the current category + page as before.

### Parser constraints (unchanged, confirmed empirically)

- **Line endings:** classic-Mac text is **CR**; tolerate CR / LF / CRLF. Host
  emits CR, file type `TEXT`.
- **Encoding:** **MacRoman**; host transcodes UTF-8 → MacRoman at emit time (index,
  pages, and hotkeys are all MacRoman/CR).
- **Parser:** tiny hand-written JSON parser (`src/json.c`): strings, numbers,
  bools, flat objects, arrays of strings. Liberal: skip blanks, ignore unknown
  fields, one bad line doesn't kill the page. A category **page** is the same
  record format as the legacy catalog, so `catalog_parse_into` reads it unchanged;
  only the index needed a new parser (`catindex_parse`).

## Build-time: the repo dataset → the paged tree

The curated metadata lives in this repo (`data/`), PR-friendly, keyed by `id`,
enriched from public databases (**LaunchBox** + **Macintosh Garden**), then
categorized and paged by the host tool:

1. `atrium library scan` enumerates the MacPack → `data/library.jsonl` (identity +
   descriptive metadata); `mg`/`enrich` fill year/vendor/genre/desc.
2. `atrium library split` moves requirement facets (color/mouse/maxDepth/min·maxOS)
   into `data/compatibility.jsonl`.
3. `atrium library categorize` seeds/refreshes `data/categories.jsonl` from the
   taxonomy + facets (preserving edits).
4. `atrium image` (or `atrium catalog --paged-out`) emits the **paged tree** —
   `index.jsonl` + `cats/<slug>.jsonl` + `hotkeys.jsonl` (split at `MAX_CAT_ITEMS`,
   CR/MacRoman/`TEXT`) — and `catalog::inject_paged` writes it to `metadata/`.
   A legacy `catalog.jsonl` is also written when ≤256 items (old-launcher safety).
5. Art is converted to PICT depth variants (next section) and injected to `images/`.

The dataset + the category DB are where a human fixes a bad match, moves a title
between categories, or adds a `"Recommended"` tag — contributed via PR (or the
GUI's Library tab, planned).

## Images

- **On-Mac format: PICT** (1-bit ships as a raw CopyBits bitmap) — QuickDraw draws
  it directly; no PNG/JPG decoder on 68k.
- **Host conversion at build time:** source art (MacGarden/LaunchBox PNG/JPG) →
  PICT, sized to the target resolution and **quantized per depth** (a 1-bit variant
  for B&W machines, 8-bit+ for colour). The record stores an **art base**
  (`images/<id>`); the launcher resolves `<base>.<depth>.pict` for the screen depth.
- Stored in `images/`, **loaded lazily** on selection.

## rusty-backup's role

- **Inject the tree:** `rb-cli put`/`cp`/`untar` push `Apps/`, `metadata/`
  (index + every `cats/*.jsonl` + hotkeys, via `inject_paged`), and `images/` into
  the HFS image with correct type/creator (`TEXT` for jsonl, `PICT`/`ABMP` for art).
- **Read back for matching:** `rb-cli ls` (type/creator) feeds the host scanner.

## Open bits (tracked in [10-open-questions.md](10-open-questions.md))

- ~~Single `catalog.jsonl` vs. split per-category files~~ → **resolved: paged by
  category** ([21-category-paging.md](21-category-paging.md)).
- Long descriptions: inline today; a per-item on-demand `desc` file is a future
  RAM optimization (docs/21 §6), as is the slim `CatItem` (drop the in-RAM art
  paths, derive from the id base).
- `atrium add` / OS-migration need paged-awareness (merge into the per-category
  files, not just the legacy `catalog.jsonl`) — docs/21 §10.
