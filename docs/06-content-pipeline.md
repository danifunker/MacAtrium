# 06 — Content Pipeline & Catalog Format

How the curated library is defined, built, and read. Three actors:

```
  repo dataset (curated JSON, PR-driven,        host tooling (modern Mac):
  enriched from LaunchBox etc.)        ─────▶   build catalog.jsonl + convert
                                                 art to PICT, then rb-cli inject
                                                          │
                                                          ▼
                                          on-volume  /MacAtrium/{metadata,images,Apps}
                                                          │
                                                          ▼
                                          MacAtrium (68k) loads the light index at
                                          boot, lazy-loads images as you navigate
```

Key principle: **all the heavy lifting happens on the modern host.** The 68k app
just reads a lightweight index and lazily pulls curated files. Categories live in
**metadata, not the folder layout**, so one app can be in **many categories**.

## On-volume layout

A single root folder on the boot volume (name = the app):

```
/MacAtrium/
├─ Apps/        the actual apps & games (or aliases to apps installed elsewhere)
│   └─ Dark Castle/Dark Castle, Lemmings/Lemmings, ...
├─ metadata/    the catalog the app reads
│   └─ catalog.jsonl            (+ optional per-item detail files, later)
└─ images/      curated artwork, one (or a few depth variants) per item
    └─ dark-castle.pict, lemmings.pict, ...
```

`Apps/` is the only part the user might hand-populate (drop an app or an alias
in). `metadata/` and `images/` are produced by the host tooling. The whole
`/MacAtrium` tree is **self-contained and relocatable** — paths in the catalog
are relative to the `/MacAtrium` root.

## Catalog JSONL — schema v2

`metadata/catalog.jsonl`, newline-delimited, **one record per launchable item**.
This is the **light index** loaded at boot — keep it small; heavy data (images,
long text) is referenced and loaded lazily.

```json
{"id":"dark-castle","name":"Dark Castle","categories":["Games","Action","Recommended"],"app":"Apps/Dark Castle/Dark Castle","type":"APPL","creator":"DKCS","year":1986,"desc":"Platformer with throwing rocks.","image":"images/dark-castle.pict"}
```

| Field | Req | Notes |
|-------|-----|-------|
| `id` | ✅ | Stable slug; key into the repo dataset + prefs |
| `name` | ✅ | Display name (Chicago, may be truncated) |
| `categories` | ✅ | **Array** of category strings → many-to-many. `"All"` is synthesized, never stored |
| `app` | ✅ | Path **relative to `/MacAtrium`** to the app/alias to launch |
| `type`/`creator` | ◐ | OSTypes; help verify the target + pick a fallback icon |
| `year` | ○ | Integer; display/sort |
| `desc` | ○ | Short one-liner (keep it light; long text → a detail file later) |
| `image` | ○ | Path relative to `/MacAtrium` to the PICT artwork |

Reserved for later: `sort`, `hidden`, `detail` (path to long description),
`hotkey`, `requiresColor`, `minSystem`.

### Categories (many-to-many)

- Each item lists **all** the categories it belongs to. The app builds a
  `category → [items]` index at load; an item in `["Games","Puzzle"]` shows in
  both.
- **"All"** is the union of everything, generated at runtime.
- **Ordering** (from the locked decision): **alphabetical by default**, *except*
  recommendation-style categories (e.g. `"Recommended"`, `"Staff Picks"`), which
  preserve the **order the dataset lists them in**. The catalog emits items in
  dataset order; the app sorts alphabetically per category unless the category is
  flagged as list-ordered.

## Runtime loading (68k)

1. **Boot:** read `metadata/catalog.jsonl` → in-memory light index (id, name,
   categories, app path, image path, short desc). Hundreds of items is fine.
2. **Navigate:** when an item is selected, **lazily load** its `image` PICT (and,
   later, a detail file) directly from the volume — "loads the curated files as
   you move around." Purge images you've scrolled past to stay within the memory
   partition.
3. **Launch:** resolve `app` (relative → `FSSpec`/alias) and sub-launch
   ([08-launching-system.md](08-launching-system.md)).

### Parser constraints (unchanged, confirmed empirically)

- **Line endings:** classic-Mac text is **CR**; tolerate CR / LF / CRLF. Host
  emits CR when targeting a Mac volume, file type `TEXT`. (Confirmed: Apple's own
  headers are CR — see [11-derisk-log.md](11-derisk-log.md).)
- **Encoding:** **MacRoman**; host transcodes UTF-8 → MacRoman at emit time.
- **Parser:** tiny hand-written JSON parser (`src/json.c`): strings, numbers,
  bools, flat objects, and **arrays of strings** (for `categories`). Liberal:
  skip blanks, ignore unknown fields, one bad line doesn't kill the catalog.

## Build-time: the repo dataset + enrichment

The curated metadata lives in this repo (`data/`), version-controlled and
**PR-friendly**, keyed by `id`. It is enriched from **existing public databases**
rather than hand-typed:

- **Primary: the LaunchBox Games Database** ("Apple Macintosh" platform) — names,
  release years, genres (→ our `categories`), descriptions, and box/screenshot
  art.
- **Supplements (as needed):** Macintosh Garden, MobyGames — better coverage for
  obscure classic-Mac titles than LaunchBox alone. 🔬 confirm LaunchBox's Mac
  coverage; plan for supplements.

A **host tool** (part of the MacAtrium tooling) does the work the 68k app can't:

1. Scan `Apps/` on a populated image (`rb-cli ls` gives names + type/creator).
2. Match each app to a dataset/LaunchBox entry (by name + year/creator).
3. Map source **genres → our `categories`**; merge curated overrides from the
   repo dataset (the PR'd corrections win).
4. Emit `catalog.jsonl` (CR, MacRoman, `TEXT`).
5. Download + convert artwork (next section).

The repo dataset is where a human fixes a bad match, adds a `"Recommended"` tag,
or writes a better blurb — and that's what users contribute via PR.

## Images

- **On-Mac format: PICT** — QuickDraw can `DrawPicture` it directly; no PNG/JPG
  decoder on 68k.
- **Host conversion at build time:** source art (LaunchBox PNG/JPG) → PICT,
  **sized to the target resolution** and **quantized per depth** (a 1-bit variant
  so B&W machines get a usable image; an 8-bit variant for color). The app picks
  by `env` depth.
- Stored in `images/`, referenced by the catalog's `image` field, **loaded
  lazily** on selection.
- **Roadmap:** text-only MVP → real app icons (`ICN#`/`icl8`, no assets needed) →
  curated PICT artwork.

## rusty-backup's role

- **Inject the tree:** existing `rb-cli put`/`cp`/`untar` push `Apps/`,
  `metadata/`, and `images/` into the HFS image with correct type/creator
  (`TEXT` for the jsonl, `PICT` for art).
- **Read back for matching:** `rb-cli ls` (type/creator) feeds the host matcher.
- A dedicated `scan`/`catalog` subcommand is **optional now** — the host tool can
  own catalog generation and call `rb-cli` for I/O. Revisit if we want it built
  into rusty-backup.

## End-to-end

```
data/ (curated JSON, PRs) ─┐
LaunchBox / Mac Garden ────┼─▶ host tool ─▶ catalog.jsonl (CR/MacRoman/TEXT)
Apps/ on image (rb-cli ls) ┘            └─▶ PICT art (depth/size variants)
                                              │ rb-cli put
                                              ▼
                              /MacAtrium/{metadata,images,Apps} on the boot image
                                              │
                                              ▼
                              MacAtrium reads metadata/catalog.jsonl at boot,
                              lazy-loads images/ as the user navigates
```

## Open bits (tracked in [10-open-questions.md](10-open-questions.md))

- LaunchBox "Apple Macintosh" coverage depth; which supplements to wire in.
- Confirm **PICT** as the artwork format (vs. storing art as resources) + the
  exact host converter (ImageMagick? a small tool in `tools/`?).
- Single `catalog.jsonl` vs. split per-category files if catalogs get large.
- Whether long descriptions move to per-item `detail` files (lazy-loaded) or stay
  inline.
