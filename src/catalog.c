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

    f = json_get(o, "icon");
    if (f && f->type == JT_STR) copy_field(it->icon, sizeof it->icon, f->str);

    f = json_get(o, "hotkey");
    if (f && f->type == JT_STR && f->str[0]) it->hotkey = f->str[0];

    f = json_get(o, "maxDepth");
    if (f && f->type == JT_NUM) it->maxDepth = (int)f->num;

    /* CD-based titles (docs/45). cdRequired defaults to 1 for a CD title (the disc
     * is usually essential); an explicit cdRequired overrides. */
    f = json_get(o, "cdImage");
    if (f && f->type == JT_STR) copy_field(it->cdImage, sizeof it->cdImage, f->str);

    it->cdRequired = it->cdImage[0] ? 1 : 0;
    f = json_get(o, "cdRequired");
    if (f && f->type == JT_BOOL)      it->cdRequired = f->boolean;
    else if (f && f->type == JT_NUM)  it->cdRequired = (f->num != 0);

    f = json_get(o, "cdVolume");
    if (f && f->type == JT_STR) copy_field(it->cdVolume, sizeof it->cdVolume, f->str);

    f = json_get(o, "cdApp");
    if (f && f->type == JT_STR) copy_field(it->cdApp, sizeof it->cdApp, f->str);

    return 1;
}

/* Advance `*i` past one line in buf[0..len); return that line's length (the
 * bytes before its CR/LF/CRLF terminator). Shared by count + parse so both walk
 * lines identically. */
static long next_line(const char *buf, long len, long *i)
{
    long start = *i;
    while (*i < len && buf[*i] != '\n' && buf[*i] != '\r') (*i)++;
    long lineLen = *i - start;
    if (*i < len && buf[*i] == '\r') {         /* swallow CR, CRLF as one */
        (*i)++;
        if (*i < len && buf[*i] == '\n') (*i)++;
    } else if (*i < len && buf[*i] == '\n') {
        (*i)++;
    }
    return lineLen;
}

int catalog_count_lines(const char *buf, long len)
{
    long i = 0;
    int  n = 0;
    while (i < len) {
        if (next_line(buf, len, &i) > 0) n++;
    }
    return n;
}

int catalog_parse_into(const char *buf, long len, CatItem *items, int cap, int *dropped)
{
    long i = 0;
    int  nitems = 0;
    int  drop = 0;

    while (i < len) {
        long start = i;
        long lineLen = next_line(buf, len, &i);
        if (lineLen <= 0) continue;            /* blank line */

        /* NOT on the stack: a JsonObject is ~26 KB (24 fields x ~1 KB each: str[256]
         * + arr[16][48]). On the small 68k app stack that fits at boot (shallow call
         * stack) but overflows into the heap — System Error 28 — when the SAME parse
         * runs from the deep runtime category-change path (event loop -> ui_key -> nav
         * -> model_move_cat -> load_page -> here). json_parse_object re-inits it every
         * call (out->nfields = 0) and there's no reentrancy, so one static instance is
         * both correct and ~26 KB lighter on the stack. */
        static JsonObject obj;
        int r = json_parse_object(buf + start, lineLen, &obj);
        if (r <= 0) {
            if (r < 0) drop++;                 /* malformed (not just blank) */
            continue;
        }

        if (nitems >= cap) break;

        CatItem it;
        if (item_from_object(&obj, &it)) {
            items[nitems++] = it;
        } else {
            drop++;
        }
    }

    if (dropped) *dropped = drop;
    return nitems;
}
