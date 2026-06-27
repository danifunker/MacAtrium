/*
 * model.h — the in-memory library the UI navigates: categories (with the
 * synthesized "All"), the per-category item lists, and the current cursor.
 *
 * Pure C, unit-tested off-target. Ordering rule (docs/01, docs/06): items sort
 * alphabetically within a category, EXCEPT recommendation-style categories
 * ("Recommended", "Staff Picks") which preserve dataset order.
 */
#ifndef MACATRIUM_MODEL_H
#define MACATRIUM_MODEL_H

#include "catalog.h"

#define MODEL_MAX_CATS (64 + 1)   /* +1 for the synthesized "All" */

/* Paged catalog (docs/21): one entry of the resident category index
 * (metadata/index.jsonl) — a category PAGE the launcher loads on demand. The
 * items themselves are NOT held here; main.c loads cats/<slug>.jsonl into a
 * Catalog when this category becomes current. */
typedef struct {
    char name[ITEM_CAT_LEN];
    char slug[ITEM_CAT_LEN];      /* cats/<slug>.jsonl */
    int  count;                   /* titles in this page (from the index) */
    int  listOrdered;             /* keep dataset order (Recommended/Featured) */
} CatRef;

/* Parse the paged catalog index (metadata/index.jsonl): one flat JSON object per
 * line {name, slug, count, ordered} into refs[cap]. Returns the number parsed.
 * Pure C (json.c), allocation-free, host-testable. */
int catindex_parse(const char *buf, long len, CatRef *refs, int cap);

typedef struct {
    char name[ITEM_CAT_LEN];
    char slug[ITEM_CAT_LEN];      /* paged: cats/<slug>.jsonl ("" in legacy mode) */
    int  idx[MAX_ITEMS];          /* indices into Catalog.items (current page in paged mode) */
    int  count;                   /* items in this category (from the index in paged mode) */
    int  listOrdered;             /* 1 = keep dataset order, don't sort */
    int  savedItem;               /* last cursor position in this category (restored on return) */
} ModelCat;

struct Model;
/* Paged mode: main.c's loader for category `catIdx` — reads cats/<slug>.jsonl into
 * a Catalog and installs it via model_set_page(). Returns 1 on success. The model
 * (pure C) only calls through this pointer, so host tests pass a stub. */
typedef int (*PageLoader)(struct Model *m, int catIdx);

typedef struct Model {
    Catalog  *cat;               /* current page (paged) OR the whole catalog (legacy) */
    ModelCat  cats[MODEL_MAX_CATS];
    int       ncats;             /* paged: # categories from the index; legacy: +"All" at 0 */
    int       curCat;            /* current category */
    int       curItem;          /* selection within current category */
    int       topRow;           /* first visible row (scroll offset) */
    int       loadedCat;         /* paged: which curCat the page holds (-1 = none) */
    PageLoader loader;           /* paged: page loader (NULL = legacy, all items resident) */
} Model;

/* Legacy: build categories + "All" from a whole-catalog (all items resident). */
void model_build(Model *m, Catalog *cat);

/* Paged (docs/21): set up the category list from the resident index (no items yet).
 * `loader` is the page loader; the first page is loaded by the caller after. */
void model_index_init(Model *m, const CatRef *refs, int nrefs, PageLoader loader);

/* Paged: install a freshly-loaded page as the current category's items (the loader
 * calls this). Sets m->cat, fills the current category's idx[] (identity), updates
 * its count, marks it loaded, and clamps the selection. */
void model_set_page(Model *m, Catalog *page);

/* Current category / item (item may be NULL if the category is empty). */
ModelCat       *model_cur_cat(Model *m);
const CatItem  *model_cur_item(Model *m);

/* Navigation (clamp + keep selection valid). Return 1 if something changed. */
int model_move_item(Model *m, int delta);   /* up/down within category */
int model_move_cat(Model *m, int delta);    /* left/right between categories */

/* Restore a saved selection: select the category named `catName`, then the item
 * with id `itemId` within it. Robust to catalog changes — a missing category
 * leaves the cursor on "All", a missing item falls back to the first row.
 * Returns 1 only if the exact item was found. NULL/empty `catName` is a no-op. */
int model_select(Model *m, const char *catName, const char *itemId);

/* Type-ahead: select the next item in the current category whose name starts
 * with `ch` (case-insensitive), searching forward from the current selection and
 * wrapping — so repeated presses cycle. Returns 1 if a match was selected. */
int model_type_ahead(Model *m, char ch);

/* Launch hotkey: select the item anywhere in the catalog whose `hotkey` matches
 * `ch` (case-insensitive), switching to the synthesized "All" category so the
 * selection is always valid regardless of the current view. First match wins.
 * Returns 1 if found+selected (caller then launches it), 0 otherwise. */
int model_select_hotkey(Model *m, char ch);

/* True if a category name is recommendation-style (preserves dataset order). */
int model_is_list_ordered(const char *name);

#endif /* MACATRIUM_MODEL_H */
