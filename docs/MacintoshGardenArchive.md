# MacintoshGarden Archive — working notes

> **Living doc.** Findings about the Macintosh Garden data dump we're evaluating as
> a supplemental content source, plus a running log of what we decide. Append to
> the **Discussion log** at the bottom as we go.
>
> Status: **investigation only — no code/data changes made yet.**

## The source file

- **Tarball:** `~/Infinite-Mac_20260312_214929.tar.gz` (≈ 24 MB compressed, ≈ 122 MB
  extracted). This is the "file in `~/` with MacintoshGarden information in it."
- **What it actually is:** the **[Infinite Mac](https://infinitemac.org)** project's
  software library, which is itself a structured **scrape of
  [Macintosh Garden](https://macintoshgarden.org)** (macintoshgarden.org). So the
  metadata *is* Macintosh Garden's, repackaged as JSON.
- The dump snapshot is dated **2026-03-12** (mtimes inside the tar).
- Extracted layout:

  ```
  Infinite-Mac/
    apps.ndjson            27 MB  — one JSON object per line (authoritative)
    games.ndjson           15 MB  — one JSON object per line (authoritative)
    Applications/app.<nid>.json   13,733 files (one per title)
    Games/game.<nid>.json          8,010 files (one per title)
  ```

  The per-title `*.json` files are **identical** to the `data` payload of the
  matching ndjson line (verified for a sample). The ndjson is the easiest thing to
  process; the split files are just a convenience mirror.

## Record schema

Each ndjson line is `{"nid": <int>, "data": {"game"|"app": { … }}}`. `nid` is
Macintosh Garden's node id and matches the `*.json` filename.

**Games** (18 fields, all present on every record):

| field | example | notes |
|-------|---------|-------|
| `title` | `"Barry the Bear"` | display name |
| `description` | HTML string | **contains HTML** (`<a>`, `<i>`, entities) + MG-internal links |
| `url_alias` | `"games/barry-the-bear"` | the **Macintosh Garden page slug** |
| `year` | `"1994"` | string; present on 7,917 / 8,010 |
| `engine` | `"Apple Media Tool"` | runtime/engine (games only) |
| `external_download_url` | usually `null` | off-site download (only 36 games) |
| `author` / `publisher` | `["Living Media", …]` | arrays |
| `composer` / `designer` | arrays (often empty) | games only |
| `perspective` | `["Point & Click"]` | games only |
| `category` | `["Early Childhood"]` | **29 distinct** game categories |
| `system` | `["Mac OS 6"]` | min OS list |
| `architecture` | `["68k"]` | `68k` / `PPC` |
| `emulation` | `["Basilisk II (68k)", …]` | emulator hints (MG/Infinite-Mac context) |
| `files` | `[{filename, filemime, filesize}]` | the downloadable disk image(s) |
| `manuals` | `[{filename, filemime, filesize}]` | PDFs etc. |
| `screenshots` | `[{filename, filemime, filesize}]` | see **Images** below |

**Apps** differ slightly: **no** `engine` / `composer` / `designer` / `perspective`
/ `external_download_url`; **adds** `compatibility` (free-text, e.g. "FAT – both
68k and PPC") and uses `category_app` instead of `category`.

## The numbers

| | Games | Apps |
|---|------:|-----:|
| records | 8,010 | 13,733 |
| with ≥1 screenshot | 7,856 | 12,558 |
| total "plain" screenshots | 24,092 | 30,732 |
| with box/cover art | 1,033 | 742 |
| total box/cover images | 1,711 | 1,149 |
| with downloadable file(s) | 7,970 | 13,562 |
| with manual(s) | 1,998 | 2,484 |
| with year | 7,917 | 12,808 |

Combined: **21,743 titles**, ~**55k screenshots** + ~**2.9k box/cover images**
referenced. (Coverage dwarfs our current LaunchBox enrichment for obscure Mac
titles — which is exactly why the docs list Mac Garden as a supplement.)

## Images & screenshots — the important caveat

**The data gives image *filenames*, not direct URLs.** Each screenshot/file/manual
entry is just `{filename, filemime, filesize}` — e.g.
`{"filename":"Barry_the_Bear.png","filemime":"image/png","filesize":242053}`.

To actually fetch an image we have to **construct** the URL from Macintosh Garden's
hosting convention. Two things we *do* have:

- The per-title page URL is `https://macintoshgarden.org/<url_alias>`
  (e.g. `…/games/barry-the-bear`).
- MG serves uploaded files from a `…/files/…` path. The exact directory
  (screenshots vs downloads) and any subfoldering is **⚠️ not yet confirmed** — we
  should verify the real URL pattern (by fetching one page) before relying on it.

**Box art vs gameplay screenshot** is recoverable from the filename:

- Box/cover names match `box_front` / `box_back` / `cover` / `_front` / `_back`
  (samples: `Monkey_2_box_front.jpg`, `Lemmings_box_back.jpg`,
  `caesar_ii_coverart.png`, `Shufflepuck_Cafe_-_Box_Front.jpg`).
- Everything else is a gameplay screenshot. This maps cleanly onto our existing
  **Box-Front + gameplay Screenshot** two-artwork model (the screenshot is what we
  use for Color/B&W detection).
- Heuristic, not perfect: `_front`/`_back` also catch things like
  `uninvited_reference_front.jpg` (a reference card, not a box).

## Macintosh-Garden-specific content (for the "scrub after ingest" goal)

If we ingest this and then strip anything MG-specific, these are the carriers:

| carrier | where | scrub action |
|---------|-------|--------------|
| `url_alias` slugs | every record | drop (or repurpose only as our internal `id` seed) |
| internal links `<a href="/games/…">` | **2,488** game descriptions | strip/flatten the `<a>` tags |
| any HTML in `description` | **5,791** games have tags; 40 have `&ndash;`/`&amp;` etc. | de-HTML to plain MacRoman-safe text (we need plain text anyway) |
| literal `"macintoshgarden"` | **257** game descriptions | strip/rewrite |
| `external_download_url` | 36 games | drop |
| `emulation` / `engine` hints | all | irrelevant to our 68k/System 7 target — drop |
| `files` / `manuals` | all | these are MG-hosted downloads; **we harvest binaries from MacPack**, not from MG, so drop the references (keep only as a coverage cross-check if useful) |

Net: keep the *facts* (title, year, author/publisher, category, cleaned
description prose, the screenshot/box images once downloaded); discard the
MG plumbing (slugs, internal links, download refs, emulation hints, branding).

> ⚠️ **Attribution / licensing flag.** Macintosh Garden screenshots are largely
> **user-contributed**. "Scrub anything specific to MG" should be a deliberate
> decision, not a default — we may want to *keep* a provenance/attribution note
> even if we strip the operational slugs/links. Decide before shipping.

## How it maps onto our dataset (`data/library.jsonl`)

Our facet schema (see `data/README.md`) vs MG fields:

| our field | from MG | notes |
|-----------|---------|-------|
| `id` | derive from `title`/`url_alias` | our existing slug rule still wins |
| `name` | `title` | |
| `app` | — | **not in MG**; comes from the MacPack harvest (launch path) |
| `kind` | games→`game`, apps→`app`/`utility` | from which file the record is in |
| `genre` | `category` / `category_app` | needs a category→our-genre map (29 game cats) |
| `color` | — | **not in MG**; we detect from the gameplay screenshot |
| `year` | `year` | direct |
| `vendor` | `publisher` (or `author`) | pick one; arrays |
| `mouse` | — | **not in MG**; hand-set |
| `desc` | `description` | **must de-HTML + transcode to MacRoman** |
| `image` | `screenshots` (box-front + gameplay) | filenames → download → PICT, same as LaunchBox path |

`enrich` only fills *missing* fields today (curation wins), so MG would slot in as a
second enrichment source behind/beside LaunchBox.

## Open questions / decisions to make

1. **Image URL pattern** — confirm the real `macintoshgarden.org/…/files/…` path
   (one fetch) before building anything that downloads art.
2. **Match key** — how do we line up MG records with the apps we actually harvest
   from MacPack? By name (fuzzy) + year, like the LaunchBox matcher?
3. **LaunchBox vs MG precedence** — MG as fallback for titles LaunchBox misses, or
   preferred for Mac-specific art? (MG art is era-accurate Mac screenshots.)
4. **Scrub vs attribute** — strip all MG traces, or keep a provenance note? (see flag above)
5. **Scope** — all 21.7k titles, or only the ~200 we actually ship?

## Image URL scheme (CONFIRMED live)

Full-resolution screenshots and box art both live in one flat namespace:

```
https://macintoshgarden.org/sites/macintoshgarden.org/files/screenshots/<filename>
```

where `<filename>` is exactly the `screenshots[].filename` from the record
(URL-encode it). Confirmed `HTTP 200 image/*` for games and apps. Notes:
- There are also cached thumbnails (`…/files/imagecache/{thumbnail,small}/screenshots/…`)
  — we take the originals.
- The `filesize` in the JSON is **stale** (differs from the live file), so resume
  is keyed on file existence, not size.
- The title page itself is `https://macintoshgarden.org/<url_alias>`.

## The scraper (`tools/macgarden-scraper/`)

Built 2026-06-23. Python 3, stdlib only; **not committed yet**. Rate-limited
(global ceiling, default 10 img/s), concurrent worker pool (default 24) so the cap
— not per-request latency — is the binding constraint, resumable (skips files
already on disk). Images only (not the `.sit`/`.iso` downloads or PDF manuals).

- **Output folder:** `~/macgarden-archive/` (outside the repo — ~20–30 GB; matches
  where `~/launchbox` / `~/macpack-work` live). Layout: `metadata/{games,apps}.ndjson`,
  `<kind>/<nid>/info.json` + image files, `manifest.csv`, `scrape.log`.
- **Observed throughput:** ~7 img/s, ~250–550 KB/image. Full run ≈ 2–2.5 h.
- Transient `503`s are logged as `error` and retried on the next run (file absent).

## Lookup & search — where to find things

Everything keys on **`nid`** (Macintosh Garden node id): it's the folder name
(`<kind>/<nid>/`), the `nid` field in the ndjson and each `info.json`, and the
`nid` column in `manifest.csv` and the index. That's the join key for the whole
dataset (and for wiring MG into `enrich`).

Three ways to look a title up, fastest first:

1. **`index.jsonl` / `index.csv`** (at the archive root, built by `mg.py index`) —
   one row per title with the searchable fields + the local `dir`. `index.csv`
   opens in a spreadsheet / greps cleanly; `index.jsonl` also carries the image
   filename list. **This is the lookup table** and the intended input to `enrich`.
   Columns: `nid, kind, title, year, publisher, category, n_images, n_box,
   on_disk, url_alias, dir`.
2. **`mg.py`** (`tools/macgarden-scraper/mg.py`) — a small search CLI over the index:
   ```sh
   mg.py index                         # (re)build the index (21,743 titles)
   mg.py search "monkey island"        # substring on title
   mg.py search --vendor broderbund --kind games --with-art
   mg.py show 1                        # full record + image list + folder path
   ```
3. **per-folder `info.json`** — the complete record co-located with that title's
   images, for "I have the nid, give me everything."

`manifest.csv` is the **download audit log** (one row per fetch attempt), not a
content index — use it for provenance/QA (what 404'd, byte sizes), not lookup.

## Integration plan (locked 2026-06-23)

MG becomes a first-class source in `atrium` (**Option B**), parallel to LaunchBox.

**Decisions**
- **Scope:** 68K-compatible titles only — filter to MG `architecture ⊇ "68k"`
  (4,429 games + 7,607 apps = 12,036 eligible). **OS 9.2.2 / PPC support comes later.**
- **Attribution:** *visible* — carry a `source: "Macintosh Garden"` field; the
  launcher's More Info card may show "via Macintosh Garden". (MG screenshots are
  largely user-contributed; we credit rather than scrub.) On-screen descriptions
  are still de-HTML'd to plain MacRoman text (strips `<a>`/entities/internal links).
- **Binary extraction:** **shell out to rb-cli** (not a crate import — rusty-backup
  is AGPL-3.0; shelling out keeps MacAtrium's license boundary clean, and atrium
  already depends on rb-cli for all HFS I/O). rb-cli's `archive` verb handles
  StuffIt (classic+v5) / Compact Pro / MAR / `.sea` / BinHex-wrapped `.hqx`;
  `put-macbinary` handles `.bin` (MacBinary I/II/III, full Finder info); the
  image/optical stack reads disk images (`.dmg/.iso/.img/.dsk/.toast`); atrium does
  `.zip` itself (pure-Rust). `archive extract --format binhex` emits `.hqx`, exactly
  what `harvest` already ingests.
- **Format coverage in 68K scope:** only ~31 titles (0.26%) have *only* unsupported
  formats. `.sitx` is a PPC/OS9-era format — just **11** of 111 `.sitx` titles are
  68K, and those have alternates or fall in that 31. So `.sitx` is a non-issue now.
- **`.sitx` / unsupported tail → deferred to the OS 9.2.2 phase.** Idea on record:
  use **Snow** to extract them — boot a PPC classic Mac with a real StuffIt Expander
  6/7 (the only thing that opens `.sitx`), expand in-guest, read the result back.
  Blocked on two things that the OS 9.2.2 phase will have anyway: (1) `.sitx` needs
  PPC-era StuffIt (won't run on 68K System 7); (2) the **headless Snow harness
  doesn't persist guest writes to the `.hda`** ([[snow-harness-guest-writes-freeze]]),
  so grabbing files back needs a non-headless / save-state path.

**Phases**
- **Phase 1 — metadata + art (pure-Rust):** `enrich`/`image` gain `--mg-archive`;
  match shipping `id`s → MG records (reuse the name+year matcher), fill
  year/vendor/genre/desc (gaps-only, de-HTML'd) + `source`, detect color offline
  from the local screenshot, feed box-front + a gameplay shot into the existing
  `pict`→PICT→catalog path. MG preferred for Mac art, LaunchBox fallback.
- **Phase 2 — MG as content/donor:** `atrium fetch` builds the static mirror URL
  (`gardenmirror.oldapplestuff.com/<kind>/<file>`) → download per shipping 68K title
  into `~/macgarden-archive/downloads/<kind>/<nid>/` (local, **not git**) →
  `rb-cli archive extract`/`put-macbinary` (or disk-image path) → hand forks to the
  **existing** harvest staging → `--into` the image.

---

## Discussion log

- **2026-06-23** — Located the source (`~/Infinite-Mac_20260312_214929.tar.gz`), an
  Infinite Mac / Macintosh Garden scrape: 8,010 games + 13,733 apps as ndjson +
  per-title JSON. Documented schema, counts, image situation (filenames not URLs;
  box vs screenshot detectable by filename), the MG-specific fields to scrub, and
  the mapping onto `data/library.jsonl`.
- **2026-06-23** — Confirmed the live screenshot URL scheme. Built
  `tools/macgarden-scraper/` (Python, rate-limited 10/s, concurrent, resumable;
  **uncommitted** per request) and kicked off the full run into `~/macgarden-archive/`.
  Smoke-tested green (167/168, one transient 503 that resume will retry).
  Still TODO: the scrub/attribution decision and wiring MG into `enrich`.
- **2026-06-23** — Per request, switched to **games first**: stopped the combined
  run and relaunched `--kinds games` only (~25.8k game images; ~2.9k already on
  disk). Apps (~31.9k images) held until games finish. Gotcha noted: killing the
  background shell orphans the Python child — `pkill -9 -f macgarden-scraper/scrape.py`
  to stop it for real.
- **2026-06-23** — **Games done:** 25,427 / 25,803 game images on disk (6.2 GB,
  ~47 min @ 7.9/s). **0 genuine 404s**; 407 failures, all transient (406× HTTP 503
  + 1 timeout) from sustained load. Running a gentler resume pass (rate 5 / 8
  workers) to recover them. Apps still pending.
- **2026-06-23** — Gentle resume recovered 365/376; **games now 25,792 / 25,803
  (99.96%)**, 11 transient 503 stragglers left (no 404s). Then **kicked off apps**
  (`--kinds apps`, rate 10) — chained to start only after the games resume exited,
  so no overlapping load. ~31.9k app images, ETA ~1.5 h. Plan: one final unified
  resume pass (`--kinds games,apps`) after apps to mop up all stragglers at once.
- **2026-06-23** — **Apps done:** 30,968 / 31,881 images (97.1%), 8.5 GB, ~70 min
  @ 7.4/s. 907 transient 503s + **6 genuine 404s** (those 6 app images truly don't
  exist on MG). Running the final unified gentle resume (rate 5 / 8 workers) for
  the 11 game + 907 app 503 stragglers (~918 files).
- **2026-06-23** — **SCRAPE COMPLETE.** After the unified pass + a slow single-stream
  mop-up (rate 2 / 3 workers / 5 retries), **57,648 / 57,654 images on disk (~18 GB),
  0 remaining errors.** Only 6 unrecoverable 404s (images that don't exist on MG):
  `apps/30001 Picture_2_124.png`, `apps/31885 Finder_3_19.gif`,
  `apps/25769 Screen_Shot_2023-09-07_at_7.48.41_PM.png`, `apps/6684 opti.jpg`,
  `apps/17712 Dave.jpg`, `apps/15208 DVD_RAM_Tune_Up.jpg` — each of those titles
  still has its other screenshots. Archive is ready for the enrichment pipeline.
- **2026-06-23** — Added **`tools/macgarden-scraper/mg.py`** (index + search + show)
  and built `~/macgarden-archive/index.{jsonl,csv}` (21,743 titles; 20,409 with
  images). `nid` is the universal join key. Documented the lookup under "Lookup &
  search" above. Still uncommitted.
- **2026-06-23** — Chose **Option B** (first-class MG source in `atrium`) and locked
  the integration plan above. Confirmed rusty-backup already has the extraction we
  need: `rb-cli archive` (StuffIt/CompactPro/MAR/SEA/BinHex) + `put-macbinary`
  (MacBinary I/II/III). Decided **shell out to rb-cli** (AGPL boundary), **68K-only**
  scope first, **visible attribution**, and **defer `.sitx`/Snow-extraction** to the
  OS 9.2.2 phase. Verified the 68K subset (12,036 titles) and that rb-cli + zip +
  disk-image paths cover 99.7% of it. Next: build Phase 1.
