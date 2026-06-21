/*
 * model.c — see model.h.
 */
#include "model.h"

#include <string.h>

/* case-insensitive ASCII compare (bytes > 127 compared raw — fine for MacRoman
 * ordering within the MVP). */
static int ci_cmp(const char *a, const char *b)
{
    for (;;) {
        unsigned char ca = (unsigned char)*a++;
        unsigned char cb = (unsigned char)*b++;
        if (ca >= 'A' && ca <= 'Z') ca += 32;
        if (cb >= 'A' && cb <= 'Z') cb += 32;
        if (ca != cb) return (int)ca - (int)cb;
        if (ca == '\0') return 0;
    }
}

int model_is_list_ordered(const char *name)
{
    return ci_cmp(name, "Recommended") == 0 ||
           ci_cmp(name, "Staff Picks") == 0 ||
           ci_cmp(name, "Featured")    == 0;
}

/* Find a category slot by name, or create it. Index 0 is reserved for "All". */
static ModelCat *find_or_add_cat(Model *m, const char *name)
{
    int i;
    for (i = 1; i < m->ncats; i++)
        if (ci_cmp(m->cats[i].name, name) == 0)
            return &m->cats[i];

    if (m->ncats >= MODEL_MAX_CATS) return 0;

    ModelCat *c = &m->cats[m->ncats++];
    memset(c, 0, sizeof *c);
    strncpy(c->name, name, ITEM_CAT_LEN - 1);
    c->name[ITEM_CAT_LEN - 1] = '\0';
    c->listOrdered = model_is_list_ordered(name);
    return c;
}

/* Insertion sort a category's item indices alphabetically by item name. */
static void sort_cat(Model *m, ModelCat *c)
{
    int i, j;
    if (c->listOrdered) return;
    for (i = 1; i < c->count; i++) {
        int key = c->idx[i];
        const char *kn = m->cat->items[key].name;
        for (j = i - 1; j >= 0 &&
                        ci_cmp(m->cat->items[c->idx[j]].name, kn) > 0; j--)
            c->idx[j + 1] = c->idx[j];
        c->idx[j + 1] = key;
    }
}

/* Sort the category list (slots 1..n) alphabetically; "All" stays at 0. */
static void sort_cat_list(Model *m)
{
    int i, j;
    for (i = 2; i < m->ncats; i++) {
        ModelCat key = m->cats[i];
        for (j = i - 1; j >= 1 && ci_cmp(m->cats[j].name, key.name) > 0; j--)
            m->cats[j + 1] = m->cats[j];
        m->cats[j + 1] = key;
    }
}

void model_build(Model *m, Catalog *cat)
{
    int i, k;

    m->cat     = cat;
    m->ncats   = 0;
    m->curCat  = 0;
    m->curItem = 0;
    m->topRow  = 0;

    /* slot 0 = synthesized "All" */
    {
        ModelCat *all = &m->cats[m->ncats++];
        memset(all, 0, sizeof *all);
        strncpy(all->name, "All", ITEM_CAT_LEN - 1);
        all->listOrdered = 0;
    }

    /* assign items to "All" (dataset order) and to each named category */
    for (i = 0; i < cat->nitems; i++) {
        ModelCat *all = &m->cats[0];
        if (all->count < MAX_ITEMS) all->idx[all->count++] = i;

        for (k = 0; k < cat->items[i].ncats; k++) {
            ModelCat *c = find_or_add_cat(m, cat->items[i].cats[k]);
            if (c && c->count < MAX_ITEMS) c->idx[c->count++] = i;
        }
    }

    /* order the category list (alphabetical, "All" pinned first) */
    sort_cat_list(m);

    /* order items within each category */
    for (i = 0; i < m->ncats; i++)
        sort_cat(m, &m->cats[i]);
}

ModelCat *model_cur_cat(Model *m)
{
    if (m->curCat < 0 || m->curCat >= m->ncats) return 0;
    return &m->cats[m->curCat];
}

const CatItem *model_cur_item(Model *m)
{
    ModelCat *c = model_cur_cat(m);
    if (!c || c->count == 0) return 0;
    if (m->curItem < 0 || m->curItem >= c->count) return 0;
    return &m->cat->items[c->idx[m->curItem]];
}

int model_move_item(Model *m, int delta)
{
    ModelCat *c = model_cur_cat(m);
    if (!c || c->count == 0) return 0;
    int ni = m->curItem + delta;
    if (ni < 0) ni = 0;
    if (ni >= c->count) ni = c->count - 1;
    if (ni == m->curItem) return 0;
    m->curItem = ni;
    return 1;
}

int model_move_cat(Model *m, int delta)
{
    if (m->ncats == 0) return 0;
    int nc = m->curCat + delta;
    if (nc < 0) nc = 0;
    if (nc >= m->ncats) nc = m->ncats - 1;
    if (nc == m->curCat) return 0;
    m->curCat  = nc;
    m->curItem = 0;
    m->topRow  = 0;
    return 1;
}
