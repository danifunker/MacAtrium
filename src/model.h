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

typedef struct {
    char name[ITEM_CAT_LEN];
    int  idx[MAX_ITEMS];          /* indices into Catalog.items */
    int  count;
    int  listOrdered;             /* 1 = keep dataset order, don't sort */
} ModelCat;

typedef struct {
    Catalog  *cat;
    ModelCat  cats[MODEL_MAX_CATS];
    int       ncats;              /* includes "All" at index 0 */
    int       curCat;            /* current category */
    int       curItem;          /* selection within current category */
    int       topRow;           /* first visible row (scroll offset) */
} Model;

/* Build categories + "All" from the catalog and apply the ordering rule. */
void model_build(Model *m, Catalog *cat);

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

/* True if a category name is recommendation-style (preserves dataset order). */
int model_is_list_ordered(const char *name);

#endif /* MACATRIUM_MODEL_H */
