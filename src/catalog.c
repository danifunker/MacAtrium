/*
 * catalog.c — pure parse of the catalog JSONL into CatItem records.
 */
#include "catalog.h"
#include "json.h"

#include <string.h>

static void copy_field(char *dst, int cap, const char *src)
{
    strncpy(dst, src, cap - 1);
    dst[cap - 1] = '\0';
}

/* Map one parsed JSON object to an item. Returns 1 if it has the required
 * fields (id, name, app, categories[>=1]); 0 if it should be dropped. */
static int item_from_object(const JsonObject *o, CatItem *it)
{
    const JsonField *f;

    memset(it, 0, sizeof *it);

    f = json_get(o, "id");
    if (!f || f->type != JT_STR || f->str[0] == '\0') return 0;
    copy_field(it->id, sizeof it->id, f->str);

    f = json_get(o, "name");
    if (!f || f->type != JT_STR || f->str[0] == '\0') return 0;
    copy_field(it->name, sizeof it->name, f->str);

    f = json_get(o, "app");
    if (!f || f->type != JT_STR || f->str[0] == '\0') return 0;
    copy_field(it->app, sizeof it->app, f->str);

    f = json_get(o, "categories");
    if (!f || f->type != JT_ARR || f->narr < 1) return 0;
    {
        int i;
        it->ncats = 0;
        for (i = 0; i < f->narr && it->ncats < MAX_ITEM_CATS; i++) {
            if (f->arr[i][0] == '\0') continue;
            copy_field(it->cats[it->ncats], ITEM_CAT_LEN, f->arr[i]);
            it->ncats++;
        }
        if (it->ncats < 1) return 0;
    }

    /* optional fields */
    f = json_get(o, "year");
    if (f && f->type == JT_NUM) it->year = f->num;

    f = json_get(o, "vendor");
    if (f && f->type == JT_STR) copy_field(it->vendor, sizeof it->vendor, f->str);

    f = json_get(o, "genre");
    if (f && f->type == JT_STR) copy_field(it->genre, sizeof it->genre, f->str);

    f = json_get(o, "type");
    if (f && f->type == JT_STR) copy_field(it->type, sizeof it->type, f->str);

    f = json_get(o, "creator");
    if (f && f->type == JT_STR) copy_field(it->creator, sizeof it->creator, f->str);

    f = json_get(o, "desc");
    if (f && f->type == JT_STR) copy_field(it->desc, sizeof it->desc, f->str);

    f = json_get(o, "image");
    if (f && f->type == JT_STR) copy_field(it->image, sizeof it->image, f->str);

    f = json_get(o, "shot");
    if (f && f->type == JT_STR) copy_field(it->shot, sizeof it->shot, f->str);

    return 1;
}

int catalog_parse(const char *buf, long len, Catalog *cat)
{
    long i = 0;

    cat->nitems  = 0;
    cat->dropped = 0;

    while (i < len) {
        long start = i;
        /* find end of line (CR, LF, or CRLF) */
        while (i < len && buf[i] != '\n' && buf[i] != '\r') i++;
        long lineLen = i - start;
        /* swallow the line terminator (handles CRLF as one) */
        if (i < len && buf[i] == '\r') {
            i++;
            if (i < len && buf[i] == '\n') i++;
        } else if (i < len && buf[i] == '\n') {
            i++;
        }

        if (lineLen <= 0) continue;            /* blank line */

        JsonObject obj;
        int r = json_parse_object(buf + start, lineLen, &obj);
        if (r <= 0) {
            if (r < 0) cat->dropped++;         /* malformed (not just blank) */
            continue;
        }

        if (cat->nitems >= MAX_ITEMS) break;

        CatItem it;
        if (item_from_object(&obj, &it)) {
            cat->items[cat->nitems++] = it;
        } else {
            cat->dropped++;
        }
    }

    return cat->nitems;
}
