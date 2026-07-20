# 21 — Category Paging (lift the 256-title cap)

**Status:** design agreed 2026-06-26; **WORKING + Snow-verified 2026-06-27.** The
68k launcher reads the paged catalog end-to-end: boots into Recommended, pages a
category on the up/down arrow (loads `cats/<slug>.jsonl` from disk on demand),
items navigate within the page. Verified on a B&W 6.0.8 build in the 512/384 KB
Mac Plus partition. v1 keeps the full `CatItem` (struct-slim is a future v2).
Remaining: `atrium add`/migration paged-awareness (§10), and scale tests (a real
multi-page category, colour build).

## 0. Update — explicit category DB (supersedes deriving from genre)

Verifying Phase 1 on real data killed the "derive categories from genre" plan:
**315 games have no genre**, and many "genres" are folder/app artifacts
(`01 Sys 6`, `System`, `Network`). So navigation categories now live in an
**explicit, editable database** — multi-membership, hand/GUI-curatable — seeded
from a taxonomy rather than re-derived each build:

- **`data/taxonomy.json`** — the canonical ~15 categories + display order
  (Recommended first/default), a genre→bucket + kind seed map, a curated
  Recommended/`adds` seed, and a `catch_all_game` so no game is unreachable.
- **`data/categories.jsonl`** — the DB (`{id, categories[]}`), the **source of
  truth** for membership. Editable by hand and (next) in the GUI Library tab.
- **`atrium library categorize`** seeds/refreshes the DB from the library +
  compatibility facets + taxonomy, **preserving existing entries** on re-run.
- `catalog::run_paged` takes the DB + taxonomy: membership from the DB, pages
  ordered by the taxonomy. Both are embedded (bundled) like library/compatibility.

The ~15 categories: Recommended · Action & Arcade · Adventure · Puzzle ·
Strategy & Sim · Role-Playing · Interactive Fiction · Card & Casino · Sports ·
Educational · Black & White · No Mouse Required · Games (catch-all) ·
Applications · Utilities. (§3's `cats/<slug>.jsonl` + slim record stand; only the
*source* of a title's categories changed — DB, not derived.)

**Goal:** let a MacAtrium disk hold far more than 256 titles and span every
category, while keeping the 68k launcher's RAM use **low and bounded on a
4 MB Mac Plus** — by loading **one category page at a time** from disk instead
of the whole catalog into RAM.

## 1. Why — the cap is RAM, not disk

The on-device catalog is capped at **256 items** (`src/catalog.h`
`#define MAX_ITEMS 256`) — but that ceiling binds the **legacy single-file**
catalog only. It comes from the width of fixed index arrays in the launcher's
*navigation model*, plus the size of the item records:

- **`gModel` is a static global** (`main.c` `static Model gModel`). `model.h`:
  `ModelCat cats[MODEL_MAX_CATS]` with `#define MODEL_MAX_CATS 128`, each holding
  an `int idx[MAX_ITEMS]` → **128 × 256 × 4 B = 128 KB of index arrays (≈138 KB
  for the whole `cats[]`), allocated always**, whatever the disk holds. In
  **paged** mode `idx[]` only ever indexes the CURRENT page, so at most
  `MAX_CAT_ITEMS` (128) entries are live — the array is sized for legacy mode.
- **`CatItem` is fat** — all fixed-size char buffers (no dynamic strings on 68k).
  The current struct is **≈1.74 KB/title**, so a resident page (`MAX_CAT_ITEMS`
  = 128) is **≈228 KB**, and a legacy 256-item catalog ≈445 KB. These are
  heap-allocated to the exact line count, so small disks are cheap.

**In paged mode the resident cost does not scale with the library.** Whether the
collection holds 96, 509 or 1,500 titles, the launcher keeps ~138 KB of model plus
ONE page (≤ ~228 KB) — only the current category page is loaded. What a bigger
collection costs is **disk** (apps + baked art) and category *pages* (bounded by
`MODEL_MAX_CATS`), **not** RAM. So there is no total-title cap in paged mode; the
generator (`catalog::run_paged`) deliberately exceeds `MAX_ITEMS` across pages.

A full *legacy* 256-title catalog in RAM ≈ 445 KB items + 138 KB model + the text
buffer ≈ **~600–800 KB of structures**, before UI/art. That is why the single-file
form was sized for a **colour** build's 1024/768 KB partition and does **not** fit
a **Mac Plus/SE** 512/384 KB partition. Paging removes that ceiling: the Plus holds
one ≤128-item page regardless of library size. The dominant remaining RAM term is
then **art depth**, not title count — a 384×384 cover is ~18 KB at 1-bit, ~147 KB
at 8-bit, ~440 KB at 24-bit (hence the 3 MB partition a 24-bit build wants).

## 2. Design — page by category

Stop loading the whole catalog. Keep resident only:

1. a tiny **category index** (names + counts), and
2. the **current category's page** (one heap `CatItem[]`).

Switching category frees the current page and loads the next from its own file.
The RAM ceiling becomes **one page**, which we cap *low* (§4). This also makes
**#2 (dynamic per-category idx) fall out for free** — there is no global
`idx[256]×65` model any more.

## 3. On-disk format (host / `atrium` change)

Replace the single `metadata/catalog.jsonl` with, under `metadata/`:

- **`index.jsonl`** — one line per category, in display order; loaded once at
  boot, **always resident** (≤ ~80 lines, a few KB):
  ```json
  {"name":"Games","slug":"games","count":128,"ordered":false}
  {"name":"Action","slug":"action","count":37,"ordered":false}
  {"name":"Action ▸ 2","slug":"action-2","count":12,"ordered":false}
  ```
- **`cats/<slug>.jsonl`** — the **slim** item records of one category, one file
  per category (and per sub-page). An item is **duplicated** into every category
  file it belongs to (disk is cheap; RAM is the scarce resource).
- **`hotkeys.jsonl`** — only the items that carry a launch `hotkey` (a handful);
  resident, so a hotkey launches directly without loading its page (§7.2):
  ```json
  {"key":"d","id":"dark-castle","name":"Dark Castle","app":"Apps/Dark Castle/Dark Castle"}
  ```
- **(future)** `desc/<art>.txt` — per-item description, read on demand (§6).

**Slim record schema** (one compact JSON object per line, MacRoman, CR — same
encoding rules as today). It drops the three 192-byte art paths and the
8×32-byte category array (an item in a category file doesn't need to know its
categories), and adds a small `art` base:
```json
{"id":"dark-castle","name":"Dark Castle","app":"Apps/Dark Castle/Dark Castle",
 "art":"dark-castle","year":1986,"vendor":"Silicon Beach","genre":"Action",
 "hotkey":"d","maxDepth":1,"desc":"…"}
```
`art` is the `fs_id` (HFS-safe ≤17 chars) the build already uses for baked files;
the launcher derives `images/<art>`, `images/<art>.shot`, `images/<art>.icon`
(+ depth variant) at draw time. Storing the base, not recomputing `fs_id` in C,
keeps host and launcher in lock-step.

## 4. In-RAM model and the memory bound

```c
typedef struct { char name[32]; char slug[32]; int count; int ordered; } CatRef;
typedef struct {
    CatRef cats[MODEL_MAX_CATS];   /* the index — resident */
    int    ncats, curCat, pendingCat;
    CatItem *page;                 /* current category, heap, sized to its count */
    int    pageCount, curItem, topRow;
} Model;
```

**`MAX_CAT_ITEMS` (compile-time, ~128)** caps any single page; the **generator
splits** a category larger than that into numbered sub-pages ("Action ▸ 1/2/…"),
which are themselves categories. So the launcher **never holds more than ~128
slim records**, regardless of library size.

Slim `CatItem` ≈ id 48 + name 64 + app 192 + art 20 + year 4 + vendor 40 +
genre 64 + type/creator 16 + hotkey 1 + maxDepth 4 + desc 256 ≈ **~720 B** (down
from ~1.5 KB). **128 × 720 B ≈ 92 KB per page** — fits the 384 KB Mac Plus
partition with room for the index, the transient file buffer, UI and render
state. One binary, one cap, sized for the smallest (B&W) target; colour machines
just get unused headroom and faster loads.

## 5. Cancelable category switching

Classic Mac is cooperative/single-threaded, so "cancelable" = **debounce +
chunked parse**, driven from the main event loop:

- Left/Right set `gModel.pendingCat = target` — they do **not** load.
- When the loop goes idle with `pendingCat != curCat`, it shows
  **"Loading Action…  ◀ ▶ to skip"** and begins loading.
- The parse runs in **chunks** (parse K records → `next_event()` → repeat). A
  Left/Right arriving mid-load updates `pendingCat` and **abandons the partial
  page**, so holding the arrow flips through categories instantly and only the
  one you *land on* loads.
- Category files are ≤ ~92 KB, so the read is one fast `PBRead`; the chunked
  parse keeps it interruptible. (Async `PBRead` is a later option if needed.)
- **Page cache:** v1 holds the **current page only** (lowest RAM). Keeping the
  previous page for instant "back" is a small LRU we can add later (size by
  target).

## 6. Slim `CatItem` (#1)

Dropped from RAM: the three 192-byte art paths (→ derived from `art`), and the
8×32-byte `cats[]` (membership is implicit in the file). `desc` stays **inline in
v1** (256 B in the record); moving it to on-demand `desc/<art>.txt` is an easy
follow-on if we want the page even smaller — not needed to hit the B&W budget.

`art.c` changes from reading explicit `image/shot/icon` catalog fields to
building them from `<art>`: try `images/<art>` (box), then `images/<art>.icon`
(app-icon fallback) — the same precedence the generator bakes today.

## 7. Decisions (defaults chosen; override before/while building)

1. **No global "All" view (v1).** "All" = the whole library, which can't be one
   page. The launcher lands in the first index category (the generator can place
   a curated "Recommended"/"Games" first). A windowed "All" (scroll-triggered
   page loads) can come later. *Chosen: drop "All".*
2. **Hotkeys: a tiny resident map; type-ahead stays per-page.** `hotkeys.jsonl`
   holds only the (few) items with a `hotkey`, with their `app` path — so a
   hotkey **launches directly**, no page load. `model_type_ahead` (first-letter
   jump) stays within the current page, which is the correct scope. *Chosen:
   resident hotkey map.*
3. **`MAX_CAT_ITEMS = 128`, generator auto-splits** oversized categories into
   numbered sub-pages. Single compile-time cap, sized for B&W. *Tunable.*
4. **Page cache = 1 (current only)** in v1. *Tunable later.*

## 8. Backwards compatibility / transition

The launcher detects the format by **presence of `index.jsonl`**: paged when
present, else the legacy single `catalog.jsonl`. During the transition the
generator emits **both** (legacy `catalog.jsonl` *and* the paged tree), so:
- the current bundled launcher keeps reading `catalog.jsonl`, and
- the paged files are ready for the new launcher.

Once the paged launcher is verified and bundled, the generator drops the legacy
file (a flag flips the default).

## 9. Implementation phases

1. ✅ **Host: paged generator + category DB** (`atrium catalog --paged-out`,
   `atrium library categorize`). Emits `index.jsonl` + `cats/<slug>.jsonl` +
   `hotkeys.jsonl`, slim records, split at `MAX_CAT_ITEMS`, HFS-safe slugs;
   membership from `data/categories.jsonl` ordered by `data/taxonomy.json`. Legacy
   `catalog.jsonl` still emitted ≤256. Unit-tested; verified on the full library.
   **Next host bit: a GUI category editor (Library tab) over `categories.jsonl`.**
2. **Launcher: index + paged model.** `catindex.c` (load `index.jsonl`), rework
   `model.{c,h}` to {index, current page}; `load_catalog` → `load_index` +
   `load_page(slug)`. Off-target model unit tests.
3. **Launcher: loading screen + debounced/chunked cancel** (§5).
4. **Launcher: slim `CatItem`** (§6) + `art.c` derivation + `hotkeys.jsonl`.
5. **Snow verification** on a **B&W** *and* a **colour** build: boot, navigate
   categories (loading screen), rapid-skip (cancel), launch + return, MEM_DEBUG
   partition readout. Confirm the B&W page bound holds.
6. **Ripple updates + flip default** (§10), drop legacy `catalog.jsonl`.

## 10. Ripple effects (host)

The catalog-format change touches more than the generator:
- **`catalog.rs`** — `build`/`render`/`run` become index + per-category emit;
  `compile`/`render_values`/`parse_compiled` (added for `atrium add`) get paged
  equivalents.
- **`image.rs`** — the catalog step (and `add_to_disk`'s compiled-catalog merge)
  merge per-category files instead of one file; `bake_art` already writes
  `images/<fs_id>.…`, which matches the derived `art` base.
- **GUI** — the Library tab's "Load Existing MacAtrium Disk" (`rb-cli get
  catalog.jsonl`) and the migration **import** (`catalog::parse_compiled`) read
  the catalog; both learn the paged layout (read `index.jsonl` + the `cats/`
  files), with the legacy path as fallback.
- `mgdb` / the Database tab are **unaffected** (they read the MG archive, not the
  on-disk catalog).

## 11. Risks

- This rewrites the **most Snow-verified** launcher code (catalog / model /
  navigation + launch-and-return). Every launcher phase (2–4) needs a Snow
  re-verification pass, which only runs on the user's machine — so it's a
  multi-step, verify-as-we-go effort, not one big drop.
- **Art derivation** must match the baked filenames exactly — mitigated by
  storing the `art` base in the record rather than recomputing `fs_id` in C.
- **HFS small-file overhead**: many `cats/*.jsonl` (≤ ~80) on a large-allocation
  volume waste some space; fine. Per-item `desc/*.txt` (future) would be one file
  per title — keep desc inline (v1) to avoid that until we need the saving.
- **Load latency** flipping categories: bounded by §5 (small files + chunked +
  debounce); async `PBRead` and a page cache are the escape hatches if needed.
