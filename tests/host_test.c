/*
 * host_test.c — off-target unit tests for the portable core (json, catalog,
 * model). Builds and runs with plain host gcc; no Toolbox.
 *
 *   cc -I../src host_test.c ../src/json.c ../src/catalog.c ../src/model.c -o t && ./t
 */
#include "json.h"
#include "catalog.h"
#include "model.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int g_fail = 0;
static int g_total = 0;

#define CHECK(cond, msg) do {                                        \
        g_total++;                                                   \
        if (!(cond)) { g_fail++; printf("FAIL: %s\n", msg); }        \
    } while (0)

/* ---- json --------------------------------------------------------------- */

static void test_json_scalars(void)
{
    JsonObject o;
    const char *s = "{\"id\":\"abc\",\"year\":1992,\"neg\":-7,\"ok\":true,\"no\":false}";
    int r = json_parse_object(s, (long)strlen(s), &o);
    CHECK(r == 1, "json scalars parse");

    const JsonField *f = json_get(&o, "id");
    CHECK(f && f->type == JT_STR && strcmp(f->str, "abc") == 0, "json string");

    f = json_get(&o, "year");
    CHECK(f && f->type == JT_NUM && f->num == 1992, "json positive int");

    f = json_get(&o, "neg");
    CHECK(f && f->type == JT_NUM && f->num == -7, "json negative int");

    f = json_get(&o, "ok");
    CHECK(f && f->type == JT_BOOL && f->boolean == 1, "json true");

    f = json_get(&o, "no");
    CHECK(f && f->type == JT_BOOL && f->boolean == 0, "json false");
}

static void test_json_array(void)
{
    JsonObject o;
    const char *s = "{\"categories\":[\"Games\",\"Action\",\"Recommended\"]}";
    int r = json_parse_object(s, (long)strlen(s), &o);
    CHECK(r == 1, "json array parse");
    const JsonField *f = json_get(&o, "categories");
    CHECK(f && f->type == JT_ARR && f->narr == 3, "json array len");
    CHECK(f && strcmp(f->arr[0], "Games") == 0, "json array[0]");
    CHECK(f && strcmp(f->arr[2], "Recommended") == 0, "json array[2]");
}

static void test_json_escapes_and_unknown(void)
{
    JsonObject o;
    const char *s = "{\"desc\":\"a \\\"b\\\" c\\/d\",\"nested\":{\"x\":1},\"keep\":5}";
    int r = json_parse_object(s, (long)strlen(s), &o);
    CHECK(r == 1, "json escapes/nested parse");
    const JsonField *f = json_get(&o, "desc");
    CHECK(f && strcmp(f->str, "a \"b\" c/d") == 0, "json string escapes");
    /* nested object is skipped (not modelled) but must not break the rest */
    f = json_get(&o, "keep");
    CHECK(f && f->type == JT_NUM && f->num == 5, "json field after nested object");
}

static void test_json_edge(void)
{
    JsonObject o;
    CHECK(json_parse_object("{}", 2, &o) == 1 && o.nfields == 0, "json empty object");
    CHECK(json_parse_object("   ", 3, &o) == 0, "json blank -> 0");
    CHECK(json_parse_object("{\"a\":}", 6, &o) == -1, "json malformed -> -1");
    CHECK(json_parse_object("not json", 8, &o) == -1, "json garbage -> -1");
}

/* ---- catalog ------------------------------------------------------------ */

static const char *SAMPLE =
    "{\"id\":\"prince-of-persia\",\"name\":\"Prince of Persia\",\"categories\":[\"Games\",\"Action\"],\"app\":\"Apps/Prince of Persia/Prince of Persia\",\"year\":1992}\n"
    "{\"id\":\"dark-castle\",\"name\":\"Dark Castle\",\"categories\":[\"Games\",\"Action\"],\"app\":\"Apps/Dark Castle/Dark Castle\",\"year\":1986}\n"
    "{\"id\":\"lemmings\",\"name\":\"Lemmings\",\"categories\":[\"Games\",\"Puzzle\"],\"app\":\"Apps/Lemmings/Lemmings\",\"year\":1991}\n";

/* Test convenience mirroring main.c's load_catalog: count lines, allocate the
 * items array (malloc here; the launcher uses NewPtr), then parse into it. The
 * short-lived test process intentionally leaks the small arrays. */
static int catalog_parse(const char *buf, long len, Catalog *cat)
{
    int cap = catalog_count_lines(buf, len);
    if (cap > MAX_ITEMS) cap = MAX_ITEMS;
    cat->items   = cap > 0 ? (CatItem *)malloc((size_t)cap * sizeof(CatItem)) : 0;
    cat->cap     = cat->items ? cap : 0;
    cat->dropped = 0;
    cat->nitems  = catalog_parse_into(buf, len, cat->items, cat->cap, &cat->dropped);
    return cat->nitems;
}

static void test_catalog_basic(void)
{
    Catalog c;
    int n = catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    CHECK(n == 3, "catalog 3 items");
    CHECK(strcmp(c.items[0].name, "Prince of Persia") == 0, "catalog item name");
    CHECK(strcmp(c.items[0].app, "Apps/Prince of Persia/Prince of Persia") == 0, "catalog app path");
    CHECK(c.items[0].ncats == 2, "catalog PoP has 2 cats");
    CHECK(c.items[0].year == 1992, "catalog year");
}

static void test_catalog_line_endings(void)
{
    /* same data with CR and CRLF terminators, plus a blank line */
    const char *crlf =
        "{\"id\":\"a\",\"name\":\"A\",\"categories\":[\"X\"],\"app\":\"p\"}\r\n"
        "\r\n"
        "{\"id\":\"b\",\"name\":\"B\",\"categories\":[\"X\"],\"app\":\"p\"}\r";
    Catalog c;
    int n = catalog_parse(crlf, (long)strlen(crlf), &c);
    CHECK(n == 2, "catalog CRLF/CR tolerated, blank skipped");
}

static void test_catalog_drops_bad(void)
{
    const char *mixed =
        "{\"id\":\"good\",\"name\":\"G\",\"categories\":[\"X\"],\"app\":\"p\"}\n"
        "{\"id\":\"nocats\",\"name\":\"N\",\"app\":\"p\"}\n"        /* missing categories */
        "{\"id\":\"noname\",\"categories\":[\"X\"],\"app\":\"p\"}\n" /* missing name */
        "garbage line here\n"
        "{\"id\":\"good2\",\"name\":\"G2\",\"categories\":[\"X\"],\"app\":\"p\"}\n";
    Catalog c;
    int n = catalog_parse(mixed, (long)strlen(mixed), &c);
    CHECK(n == 2, "catalog keeps 2 good items");
    CHECK(c.dropped == 3, "catalog drops 3 bad lines");
}

static void test_catalog_optional_fields(void)
{
    const char *s =
        "{\"id\":\"lem\",\"name\":\"Lemmings\",\"categories\":[\"Games\",\"Puzzle\"],"
        "\"app\":\"a\",\"year\":1991,\"vendor\":\"Psygnosis\","
        "\"genre\":\"Puzzle, Strategy\",\"desc\":\"Guide them to the exit.\","
        "\"image\":\"images/lem\",\"shot\":\"images/lem.shot\"}\n";
    Catalog c;
    int n = catalog_parse(s, (long)strlen(s), &c);
    CHECK(n == 1, "catalog optional-fields item parses");
    CHECK(strcmp(c.items[0].vendor, "Psygnosis") == 0, "catalog vendor field");
    CHECK(strcmp(c.items[0].genre, "Puzzle, Strategy") == 0, "catalog genre field");
    CHECK(strcmp(c.items[0].desc, "Guide them to the exit.") == 0, "catalog desc field");
    CHECK(strcmp(c.items[0].image, "images/lem") == 0, "catalog image field");
    CHECK(strcmp(c.items[0].shot, "images/lem.shot") == 0, "catalog shot field");
    CHECK(c.items[0].year == 1991, "catalog year field");
}

/* ---- model -------------------------------------------------------------- */

static int cat_index(Model *m, const char *name)
{
    int i;
    for (i = 0; i < m->ncats; i++)
        if (strcmp(m->cats[i].name, name) == 0) return i;
    return -1;
}

static void test_model_categories(void)
{
    Catalog c; Model m;
    catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    model_build(&m, &c);

    CHECK(strcmp(m.cats[0].name, "All") == 0, "model All first");
    CHECK(m.cats[0].count == 3, "model All has all items");

    int gi = cat_index(&m, "Games");
    int ai = cat_index(&m, "Action");
    int pi = cat_index(&m, "Puzzle");
    CHECK(gi > 0 && ai > 0 && pi > 0, "model has Games/Action/Puzzle");
    CHECK(m.cats[gi].count == 3, "Games has 3");
    CHECK(m.cats[ai].count == 2, "Action has 2 (PoP, Dark Castle)");
    CHECK(m.cats[pi].count == 1, "Puzzle has 1 (Lemmings)");

    /* many-to-many: PoP appears in both Games and Action */
    int j, inGames = 0, inAction = 0;
    for (j = 0; j < m.cats[gi].count; j++)
        if (strcmp(c.items[m.cats[gi].idx[j]].id, "prince-of-persia") == 0) inGames = 1;
    for (j = 0; j < m.cats[ai].count; j++)
        if (strcmp(c.items[m.cats[ai].idx[j]].id, "prince-of-persia") == 0) inAction = 1;
    CHECK(inGames && inAction, "PoP in both Games and Action (many-to-many)");
}

static void test_model_sort(void)
{
    Catalog c; Model m;
    catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    model_build(&m, &c);

    /* "Games" alphabetical: Dark Castle, Lemmings, Prince of Persia */
    int gi = cat_index(&m, "Games");
    CHECK(strcmp(c.items[m.cats[gi].idx[0]].name, "Dark Castle") == 0, "Games sorted [0]");
    CHECK(strcmp(c.items[m.cats[gi].idx[1]].name, "Lemmings") == 0, "Games sorted [1]");
    CHECK(strcmp(c.items[m.cats[gi].idx[2]].name, "Prince of Persia") == 0, "Games sorted [2]");
}

static void test_model_list_ordered(void)
{
    /* Recommended must preserve dataset order (Zed before Alpha). */
    const char *rec =
        "{\"id\":\"z\",\"name\":\"Zed\",\"categories\":[\"Recommended\"],\"app\":\"p\"}\n"
        "{\"id\":\"a\",\"name\":\"Alpha\",\"categories\":[\"Recommended\"],\"app\":\"p\"}\n";
    Catalog c; Model m;
    catalog_parse(rec, (long)strlen(rec), &c);
    model_build(&m, &c);
    int ri = cat_index(&m, "Recommended");
    CHECK(ri > 0, "Recommended exists");
    CHECK(m.cats[ri].listOrdered == 1, "Recommended is list-ordered");
    CHECK(strcmp(c.items[m.cats[ri].idx[0]].name, "Zed") == 0, "Recommended keeps dataset order [0]=Zed");
    CHECK(strcmp(c.items[m.cats[ri].idx[1]].name, "Alpha") == 0, "Recommended keeps dataset order [1]=Alpha");
}

static void test_model_nav(void)
{
    Catalog c; Model m;
    catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    model_build(&m, &c);

    m.curCat = 0; m.curItem = 0;   /* cat 0 = "All", 3 items */
    CHECK(model_move_item(&m, 1) == 1 && m.curItem == 1, "nav right");
    CHECK(model_move_item(&m, -1) == 1 && m.curItem == 0, "nav left");
    CHECK(model_move_item(&m, -1) == 1 && m.curItem == 2, "nav left WRAPS to last");
    CHECK(model_move_item(&m, 1) == 1 && m.curItem == 0, "nav right WRAPS to first");
    CHECK(model_move_item(&m, 3) == 0, "a full-loop delta is a no-op");

    /* per-category position memory: leaving + returning restores the cursor */
    m.curCat = 0; m.curItem = 2;
    int before = m.curCat;
    CHECK(model_move_cat(&m, 1) == 1 && m.curCat == before + 1, "cat right");
    CHECK(m.curItem == 0, "a fresh category starts at its saved position (0)");
    m.curItem = 1;
    CHECK(model_move_cat(&m, -1) == 1 && m.curCat == before, "cat left back");
    CHECK(m.curItem == 2, "returning to a category restores its saved cursor");
}

static void test_model_type_ahead(void)
{
    Catalog c; Model m;
    catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    model_build(&m, &c);

    /* "All" sorts alphabetically: Dark Castle(0), Lemmings(1), Prince of Persia(2) */
    m.curCat = 0; m.curItem = 0;
    CHECK(model_type_ahead(&m, 'l') == 1 && m.curItem == 1, "type-ahead l -> Lemmings");
    CHECK(model_type_ahead(&m, 'P') == 1 && m.curItem == 2, "type-ahead P (case-insensitive) -> Prince");
    CHECK(model_type_ahead(&m, 'd') == 1 && m.curItem == 0, "type-ahead d wraps -> Dark Castle");
    CHECK(model_type_ahead(&m, 'z') == 0 && m.curItem == 0, "type-ahead no match -> no-op");
}

static void test_model_select(void)
{
    Catalog c; Model m;
    int ai;
    catalog_parse(SAMPLE, (long)strlen(SAMPLE), &c);
    model_build(&m, &c);
    ai = cat_index(&m, "Action");   /* Action sorts: Dark Castle(0), Prince(1) */

    /* exact restore by category name + item id */
    CHECK(model_select(&m, "Action", "prince-of-persia") == 1, "select exact returns 1");
    CHECK(m.curCat == ai, "select set category to Action");
    CHECK(strcmp(c.items[m.cats[m.curCat].idx[m.curItem]].id, "prince-of-persia") == 0,
          "select landed on Prince of Persia");

    /* category match is case-insensitive (mirrors build-time naming) */
    CHECK(model_select(&m, "action", "dark-castle") == 1, "select case-insensitive category");
    CHECK(strcmp(c.items[m.cats[m.curCat].idx[m.curItem]].id, "dark-castle") == 0,
          "select landed on Dark Castle");

    /* missing item -> keep category, fall back to first row, return 0 */
    CHECK(model_select(&m, "Action", "lemmings") == 0, "select missing item returns 0");
    CHECK(m.curCat == ai && m.curItem == 0, "select missing item -> first row");

    /* missing category -> cursor stays put (default All), return 0 */
    m.curCat = 0; m.curItem = 2;
    CHECK(model_select(&m, "Nope", "x") == 0, "select missing category returns 0");
    CHECK(m.curCat == 0 && m.curItem == 0, "select missing category -> All, first row");

    /* empty / null args are no-ops */
    CHECK(model_select(&m, "", "x") == 0, "select empty category no-op");
    CHECK(model_select(&m, 0, 0) == 0, "select null no-op");
}

/* A stub page loader (no Toolbox): builds a 2-item page for any category. */
static CatItem g_page_items[8];
static Catalog g_page;
static int     g_load_calls;
static int stub_loader(Model *m, int catIdx)
{
    (void)catIdx;
    g_load_calls++;
    memset(g_page_items, 0, sizeof g_page_items);
    strcpy(g_page_items[0].id, "a"); strcpy(g_page_items[0].name, "Aaa");
    g_page_items[0].ncats = 1; strcpy(g_page_items[0].cats[0], "x");
    strcpy(g_page_items[1].id, "b"); strcpy(g_page_items[1].name, "Bbb");
    g_page_items[1].ncats = 1; strcpy(g_page_items[1].cats[0], "x");
    g_page.items = g_page_items; g_page.cap = 8; g_page.nitems = 2; g_page.dropped = 0;
    model_set_page(m, &g_page);
    return 1;
}

static void test_model_paged(void)
{
    CatRef refs[3];
    Model  m;
    int    before;
    memset(refs, 0, sizeof refs);
    strcpy(refs[0].name, "Recommended"); strcpy(refs[0].slug, "recommended"); refs[0].count = 18; refs[0].listOrdered = 1;
    strcpy(refs[1].name, "Action");      strcpy(refs[1].slug, "action");      refs[1].count = 128;
    strcpy(refs[2].name, "Puzzle");      strcpy(refs[2].slug, "puzzle");      refs[2].count = 50;
    refs[0].vol = 0; refs[1].vol = 1; refs[2].vol = 1;   /* boot + one data disk (docs/37) */

    g_load_calls = 0;
    model_index_init(&m, refs, 3, stub_loader);
    CHECK(m.ncats == 3, "paged index_init sets ncats from the index");
    CHECK(m.cats[0].vol == 0 && m.cats[2].vol == 1, "index_init carries the source volume tag (docs/37)");
    CHECK(strcmp(model_cur_cat(&m)->name, "Recommended") == 0, "paged lands on Recommended (default)");
    CHECK(m.cats[1].count == 128, "index count before a page loads");
    CHECK(m.loadedCat == -1, "no page loaded at index init");

    stub_loader(&m, 0);   /* caller loads the first page at boot */
    CHECK(m.loadedCat == 0 && model_cur_cat(&m)->count == 2, "first page loaded; count = page nitems");
    CHECK(strcmp(model_cur_item(&m)->id, "a") == 0, "current item comes from the page");

    before = g_load_calls;
    CHECK(model_move_cat(&m, 1) == 1, "move to the next category");
    CHECK(m.curCat == 1 && m.loadedCat == 1, "moving category loaded its page via the loader");
    CHECK(g_load_calls == before + 1, "loader fired exactly once on the move");
    CHECK(model_move_item(&m, 1) == 1 && strcmp(model_cur_item(&m)->id, "b") == 0, "move item within the page");
}

static void test_catindex(void)
{
    /* The paged catalog index (docs/21): one {name,slug,count,ordered} per line. */
    const char *idx =
        "{\"name\":\"Recommended\",\"slug\":\"recommended\",\"count\":18,\"ordered\":true}\n"
        "{\"name\":\"Action & Arcade\",\"slug\":\"action-arcade\",\"count\":128,\"ordered\":false}\n"
        "\n"
        "{\"name\":\"Action & Arcade (2)\",\"slug\":\"action-arcade-2\",\"count\":72,\"ordered\":false}\n";
    CatRef refs[MODEL_MAX_CATS];
    int n = catindex_parse(idx, (long)strlen(idx), refs, MODEL_MAX_CATS);
    CHECK(n == 3, "catindex parses 3 pages (blank line skipped)");
    CHECK(strcmp(refs[0].name, "Recommended") == 0, "catindex name");
    CHECK(strcmp(refs[0].slug, "recommended") == 0, "catindex slug");
    CHECK(refs[0].count == 18, "catindex count");
    CHECK(refs[0].listOrdered == 1, "catindex Recommended is ordered");
    CHECK(refs[1].count == 128 && refs[1].listOrdered == 0, "catindex Action page 1");
    CHECK(strcmp(refs[2].slug, "action-arcade-2") == 0, "catindex sub-page slug");
    CHECK(catindex_parse(idx, (long)strlen(idx), refs, 1) == 1, "catindex honours cap");
}

int main(void)
{
    test_json_scalars();
    test_json_array();
    test_json_escapes_and_unknown();
    test_json_edge();
    test_catalog_basic();
    test_catalog_line_endings();
    test_catalog_drops_bad();
    test_catalog_optional_fields();
    test_catindex();
    test_model_paged();
    test_model_categories();
    test_model_sort();
    test_model_list_ordered();
    test_model_nav();
    test_model_type_ahead();
    test_model_select();

    printf("\n%d/%d checks passed\n", g_total - g_fail, g_total);
    return g_fail ? 1 : 0;
}
