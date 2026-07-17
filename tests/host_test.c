/*
 * host_test.c — off-target unit tests for the portable core (json, catalog,
 * model). Builds and runs with plain host gcc; no Toolbox.
 *
 *   cc -I../src host_test.c ../src/json.c ../src/catalog.c ../src/model.c -o t && ./t
 */
#include "json.h"
#include "catalog.h"
#include "compat.h"    /* pure per-title compatibility check (docs/40) */
#include "model.h"
#include "artcaps.h"   /* pure half only, via -DARTCAPS_HOST_TEST (docs/44) */
#include "toolbox.h"   /* pure half only, via -DTOOLBOX_HOST_TEST (docs/45) */

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

static void test_catalog_cd_fields(void)
{
    const char *s =
        /* run-from-CD title: cdApp set; cdRequired defaults to 1 for a CD title */
        "{\"id\":\"myst\",\"name\":\"Myst\",\"categories\":[\"Games\"],\"app\":\"x\","
        "\"cdImage\":\"MYST.iso\",\"cdVolume\":\"Myst\",\"cdApp\":\"Myst/Myst\"}\n"
        /* app-on-HD CD title with the disc explicitly optional */
        "{\"id\":\"foo\",\"name\":\"Foo\",\"categories\":[\"Games\"],\"app\":\"Apps/Foo/Foo\","
        "\"cdImage\":\"FOO.iso\",\"cdRequired\":false}\n"
        /* a normal (non-CD) title: all CD fields empty, cdRequired 0 */
        "{\"id\":\"bar\",\"name\":\"Bar\",\"categories\":[\"Games\"],\"app\":\"Apps/Bar/Bar\"}\n";
    Catalog c;
    int n = catalog_parse(s, (long)strlen(s), &c);
    CHECK(n == 3, "catalog CD-fields items parse");

    CHECK(strcmp(c.items[0].cdImage, "MYST.iso") == 0, "catalog cdImage");
    CHECK(strcmp(c.items[0].cdVolume, "Myst") == 0, "catalog cdVolume");
    CHECK(strcmp(c.items[0].cdApp, "Myst/Myst") == 0, "catalog cdApp (run-from-CD)");
    CHECK(c.items[0].cdRequired == 1, "catalog cdRequired defaults to 1 for a CD title");

    CHECK(strcmp(c.items[1].cdImage, "FOO.iso") == 0, "catalog cdImage (app-on-HD)");
    CHECK(c.items[1].cdApp[0] == '\0', "catalog empty cdApp -> app-on-HD");
    CHECK(c.items[1].cdRequired == 0, "catalog explicit cdRequired:false honored");

    CHECK(c.items[2].cdImage[0] == '\0', "catalog non-CD title has empty cdImage");
    CHECK(c.items[2].cdRequired == 0, "catalog non-CD title cdRequired 0");
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

/* Multi-disk (docs/37): a saved selection whose category is gone (its disk was
 * removed) falls back to Recommended, by name — not to an arbitrary first category. */
static void test_model_recommended_fallback(void)
{
    CatRef refs[3];
    Model  m;
    memset(refs, 0, sizeof refs);
    /* Recommended deliberately NOT at index 0, to prove the fallback finds it by name. */
    strcpy(refs[0].name, "Action");      strcpy(refs[0].slug, "action");      refs[0].count = 2; refs[0].vol = 0;
    strcpy(refs[1].name, "Recommended"); strcpy(refs[1].slug, "recommended"); refs[1].count = 2; refs[1].vol = 0; refs[1].listOrdered = 1;
    strcpy(refs[2].name, "Arcade");      strcpy(refs[2].slug, "arcade");      refs[2].count = 2; refs[2].vol = 1;
    model_index_init(&m, refs, 3, stub_loader);
    stub_loader(&m, 0);   /* first page loaded at boot */

    model_select(&m, "Arcade", "");   /* present -> selects it (curCat 2, on disk 1) */
    CHECK(m.curCat == 2, "restore selects the saved category when present");

    model_select(&m, "Arcade Classics", "");   /* gone (disk 1 removed) -> Recommended */
    CHECK(m.curCat == 1, "missing category falls back to Recommended, not category 0 (docs/37)");
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

/* ---- artcaps (docs/44 P1: art-capability gating) ------------------------ */

#define KB(n) ((long)(n) * 1024)

static void test_artcaps(void)
{
    ArtCaps c, a, b;
    ArtCapsInput in;
    long gworld;

    /* Peak estimates are fixed by the build art bound (720x720): 1-bit ~63 KB,
     * 8-bit ~506 KB, 24-bit ~1519 KB. */

    /* Compact: System 6, 1-bit screen, small partition, no temp memory. Only the
     * 1-bit tier can be shown, and only it fits. */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(512); in.partitionFree = KB(400); in.maxBlock = KB(380);
    in.tempFree = 0; in.maxCardDepth = 1;
    in.screenW = 512; in.screenH = 342; in.screenDepth = 1;
    art_caps_derive(&c, &in);
    CHECK(c.displayable[ART_MODE_1BIT] && !c.displayable[ART_MODE_8BIT] &&
          !c.displayable[ART_MODE_24BIT], "compact: only 1-bit displayable");
    CHECK(c.enabled[ART_MODE_1BIT] && !c.enabled[ART_MODE_8BIT] &&
          !c.enabled[ART_MODE_24BIT], "compact: only 1-bit enabled");
    CHECK(c.maxAffordableDepth == 1 && c.defaultMode == 1, "compact: default 1-bit");

    /* 8-bit card, ample partition: 24-bit art is affordable but NOT displayable, so
     * it stays disabled and the default caps at 8-bit — screen and art are separate
     * axes (docs/44). maxAffordableDepth tracks memory (24), defaultMode tracks the
     * enabled set (8). */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(2048); in.partitionFree = KB(1900); in.maxBlock = KB(1800);
    in.tempFree = KB(2048); in.maxCardDepth = 8;
    in.screenW = 640; in.screenH = 480; in.screenDepth = 8;
    art_caps_derive(&c, &in);
    CHECK(c.affordable[ART_MODE_24BIT], "8-bit card: 24-bit art is affordable");
    CHECK(!c.displayable[ART_MODE_24BIT], "8-bit card: 24-bit art not displayable");
    CHECK(!c.enabled[ART_MODE_24BIT], "8-bit card: 24-bit art disabled by VRAM gate");
    CHECK(c.enabled[ART_MODE_8BIT] && c.defaultMode == 8, "8-bit card: default 8-bit");
    CHECK(c.maxAffordableDepth == 24, "8-bit card: maxAffordable follows memory, not VRAM");

    /* Truecolor card, SMALL partition: the deep screen stays, but 24-bit art can't
     * fit, so art degrades to 8-bit (docs/44 risk #3). */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(1024); in.partitionFree = KB(1000); in.maxBlock = KB(950);
    in.tempFree = KB(4096); in.maxCardDepth = 32;
    in.screenW = 832; in.screenH = 624; in.screenDepth = 32;
    art_caps_derive(&c, &in);
    CHECK(c.displayable[ART_MODE_24BIT], "small part: 24-bit displayable on truecolor card");
    CHECK(!c.affordable[ART_MODE_24BIT], "small part: 24-bit art unaffordable");
    CHECK(c.enabled[ART_MODE_8BIT] && !c.enabled[ART_MODE_24BIT], "small part: art caps at 8-bit");
    CHECK(c.maxAffordableDepth == 8 && c.defaultMode == 8, "small part: default 8-bit");

    /* Quadra: truecolor card, big partition — every tier shows and fits. */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(8192); in.partitionFree = KB(7000); in.maxBlock = KB(6000);
    in.tempFree = KB(8192); in.maxCardDepth = 32;
    in.screenW = 832; in.screenH = 624; in.screenDepth = 32;
    art_caps_derive(&c, &in);
    CHECK(c.enabled[ART_MODE_1BIT] && c.enabled[ART_MODE_8BIT] && c.enabled[ART_MODE_24BIT],
          "quadra: all tiers enabled");
    CHECK(c.maxAffordableDepth == 24 && c.defaultMode == 24, "quadra: default 24-bit");

    /* The no-temp GWorld reserve: with temp memory scarce the off-screen buffer is
     * charged to the partition, so the art budget drops by exactly the screen's
     * GWorld size versus the temp-ample case. */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(4096); in.partitionFree = KB(3000); in.maxBlock = KB(2800);
    in.maxCardDepth = 8; in.screenW = 640; in.screenH = 480; in.screenDepth = 8;
    gworld = 640L * 480L;                       /* 8-bit rowBytes*height = 300 KB */
    in.tempFree = 0;        art_caps_derive(&a, &in);
    in.tempFree = KB(4096); art_caps_derive(&b, &in);
    CHECK(b.artBudget - a.artBudget == gworld, "gworld reserve subtracts only when temp scarce");

    /* Starved heap: budget floors at zero (never negative) and the 1-bit fallback
     * floor always holds. */
    memset(&in, 0, sizeof in);
    in.grantedPartition = KB(384); in.partitionFree = KB(64); in.maxBlock = KB(50);
    in.tempFree = 0; in.maxCardDepth = 1; in.screenW = 512; in.screenH = 342; in.screenDepth = 1;
    art_caps_derive(&c, &in);
    CHECK(c.artBudget >= 0, "budget never negative");
    CHECK(c.maxAffordableDepth >= 1 && c.defaultMode >= 1, "1-bit floor always holds");
}

#undef KB

/* ---- toolbox (docs/45: BlueSCSI Toolbox CD entry parse + name match) ----- */

/* Build a 40-byte LIST entry: index, file/dir, name (NUL-padded), 32-bit size at
 * offset 36 (byte 35 left 0, matching snow/MiSTer's 4-byte size write). */
static void mk_entry(unsigned char *e, int index, int isFile, const char *name,
                     unsigned long size)
{
    memset(e, 0, TB_ENTRY_SIZE);
    e[0] = (unsigned char)index;
    e[1] = isFile ? 0x01 : 0x00;
    strncpy((char *)&e[TB_NAME_OFF], name, TB_NAME_MAX);   /* [2..34) */
    e[36] = (unsigned char)((size >> 24) & 0xFF);
    e[37] = (unsigned char)((size >> 16) & 0xFF);
    e[38] = (unsigned char)((size >>  8) & 0xFF);
    e[39] = (unsigned char)( size        & 0xFF);
}

static void test_toolbox_entry_parse(void)
{
    unsigned char e[TB_ENTRY_SIZE];
    TbEntry t;

    mk_entry(e, 3, 1, "Myst.iso", 681984UL);
    toolbox_parse_cd_entry(e, &t);
    CHECK(t.index == 3, "tb entry index");
    CHECK(t.isDir == 0, "tb entry is a file");
    CHECK(strcmp(t.name, "Myst.iso") == 0, "tb entry name");
    CHECK(t.size == 681984UL, "tb entry size (4-byte at 36)");

    /* directory entry: type byte 0x00 */
    mk_entry(e, 0, 0, "Games", 0);
    toolbox_parse_cd_entry(e, &t);
    CHECK(t.isDir == 1, "tb entry is a directory");

    /* a full 32-char name has no NUL in-field; parse must read all 32 */
    mk_entry(e, 1, 1, "0123456789012345678901234567890X", 0);   /* 32 chars */
    toolbox_parse_cd_entry(e, &t);
    CHECK(strlen(t.name) == 32, "tb 32-char name reads full field");
    CHECK(t.name[31] == 'X', "tb 32-char name last byte");

    /* firmware 5-byte size: byte 35 (bits 32..39) non-zero -> clamp (>4 GB) */
    memset(e, 0, sizeof e);
    e[1] = 0x01; strcpy((char *)&e[2], "Big.iso");
    e[35] = 0x01;                       /* size >= 4 GB */
    toolbox_parse_cd_entry(e, &t);
    CHECK(t.size == 0xFFFFFFFFUL, "tb >4GB size clamps");
}

static void test_toolbox_name_match(void)
{
    CHECK(toolbox_name_eq("MYST.ISO", "myst.iso") == 1, "tb name case-insensitive");
    CHECK(toolbox_name_eq("Myst.iso", "Myst.iso") == 1, "tb name exact");
    CHECK(toolbox_name_eq("Myst.iso", "Myst2.iso") == 0, "tb name differ");
    CHECK(toolbox_name_eq("Myst", "Myst.iso") == 0, "tb name prefix is not a match");
    CHECK(toolbox_name_eq("Myst.iso", "Myst") == 0, "tb name longer is not a match");
    CHECK(toolbox_name_eq("", "") == 1, "tb empty names equal");
}

static void test_toolbox_find(void)
{
    unsigned char raw[3][TB_ENTRY_SIZE];
    TbEntry ents[3];
    int i;
    mk_entry(raw[0], 0, 0, "Discs",     0);       /* a directory named like nothing */
    mk_entry(raw[1], 1, 1, "SPECTRE.iso", 1000);
    mk_entry(raw[2], 2, 1, "Myst.iso",  681984UL);
    for (i = 0; i < 3; i++) toolbox_parse_cd_entry(raw[i], &ents[i]);

    CHECK(toolbox_find_cd("myst.iso", ents, 3) == 2, "tb find by name (case-insensitive) -> index 2");
    CHECK(toolbox_find_cd("spectre.iso", ents, 3) == 1, "tb find -> index 1");
    CHECK(toolbox_find_cd("nope.iso", ents, 3) == -1, "tb find miss -> -1");
    CHECK(toolbox_find_cd("", ents, 3) == -1, "tb find empty name -> -1");

    /* a directory whose name matches must be skipped (CD images are files) */
    mk_entry(raw[0], 0, 0, "Myst.iso", 0);        /* same name, but a directory */
    toolbox_parse_cd_entry(raw[0], &ents[0]);
    CHECK(toolbox_find_cd("Myst.iso", ents, 3) == 2, "tb find skips a same-named directory");
}

static void test_toolbox_fuzzy(void)
{
    unsigned char raw[3][TB_ENTRY_SIZE];
    TbEntry ents[3];
    int i;
    /* 38 chars: LIST CDS clips the on-disk name to the 32-byte MacRoman field. */
    const char *full = "Prince of Persia Deluxe Collection.iso";

    mk_entry(raw[0], 0, 1, full, 1000);
    mk_entry(raw[1], 1, 1, "Myst.iso", 2000);
    mk_entry(raw[2], 2, 1, "Spectre.iso", 3000);
    for (i = 0; i < 3; i++) toolbox_parse_cd_entry(raw[i], &ents[i]);

    CHECK(strlen(ents[0].name) == 32, "fuzzy: a long host name arrives clipped to 32");
    CHECK(toolbox_find_cd(full, ents, 3) == 0, "fuzzy: full catalog name matches its clipped entry");
    CHECK(toolbox_find_cd("myst.iso", ents, 3) == 1, "fuzzy: exact still matches (case-insensitive)");
    CHECK(toolbox_find_cd("Prince", ents, 3) == -1, "fuzzy: a short name does not prefix-match a clip");
    CHECK(toolbox_find_cd("Spect", ents, 3) == -1, "fuzzy: prefix of a short (unclipped) name is not a match");
}

static void test_toolbox_cdb(void)
{
    unsigned char cdb[TB_CDB_LEN];
    int i, allzero;

    toolbox_cdb_list_cds(cdb);
    CHECK(cdb[0] == 0xD7, "tb CDB LIST_CDS opcode");
    allzero = 1; for (i = 1; i < TB_CDB_LEN; i++) if (cdb[i]) allzero = 0;
    CHECK(allzero, "tb CDB LIST_CDS body zero");

    toolbox_cdb_set_next_cd(cdb, 5);
    CHECK(cdb[0] == 0xD8 && cdb[1] == 5, "tb CDB SET_NEXT_CD opcode + index");

    toolbox_cdb_device_info(cdb, TB_SUB_LIST_DEVICES);
    CHECK(cdb[0] == 0xD9 && cdb[1] == 0x00, "tb CDB DEVICE_INFO list-devices subcmd");

    {   /* MODE SENSE(6) page 0x31 — the canonical Toolbox detection CDB (6 bytes) */
        unsigned char m[6];
        toolbox_cdb_mode_sense_p31(m);
        CHECK(m[0] == 0x1A && m[2] == 0x31 && m[4] > 0, "tb CDB MODE SENSE(6) page 0x31");
    }
}

static void test_toolbox_magic(void)
{
    unsigned char page[56];

    /* The page-0x31 payload as snow/BlueSCSI place it, after the mode header. */
    memset(page, 0, sizeof page);
    memcpy(page + 14, "BlueSCSI is the BEST STOLEN FROM BLUESCSI", 41);
    CHECK(toolbox_has_magic(page, (int)sizeof page), "tb magic detected in page 0x31");

    memset(page, 0, sizeof page);
    CHECK(!toolbox_has_magic(page, (int)sizeof page), "tb no magic in a blank page");

    /* An ordinary disk's mode data must not false-positive as a Toolbox device. */
    memset(page, 0, sizeof page);
    memcpy(page, "SEAGATE ST225N mode parameters here!!", 37);
    CHECK(!toolbox_has_magic(page, (int)sizeof page), "tb no magic in ordinary mode data");
}

/* ---- compat (docs/40) --------------------------------------------------- */

static void test_compat(void)
{
    CatItem it;
    Env     e;
    char    out[COMPAT_REASON_LEN];

    /* Marathon 2 (needs a 68040 + FPU + 8 MB) on a Mac LC (68020/tier 0, no FPU,
     * 4 MB, 8-bit): under-spec, and the reason names each shortfall. */
    memset(&e, 0, sizeof e);
    e.tier = 0; e.hasFPU = 0; e.ramKB = 4096; e.maxScreenDepth = 8; e.hasColorQD = 1;
    memset(&it, 0, sizeof it);
    it.minCPU = 2; it.needsFPU = 1; it.minMem = 8; it.minDepth = 8; it.maxDepth = 8;  /* minMem = MB */
    CHECK(compat_reason(&it, &e, out) == 1, "compat: M2 under-spec on an LC");
    CHECK(strstr(out, "68040") != 0, "compat: names the 68040");
    CHECK(strstr(out, "FPU") != 0, "compat: names the FPU");
    CHECK(strstr(out, "8 MB") != 0, "compat: names the RAM");

    /* Same title on a Quadra (68040/tier 2, FPU, 16 MB) — adequate, empty reason. */
    e.tier = 2; e.hasFPU = 1; e.ramKB = 16384;
    CHECK(compat_reason(&it, &e, out) == 0, "compat: M2 fine on a Quadra");
    CHECK(out[0] == '\0', "compat: empty reason when adequate");

    /* A 256-colour title on a 1-bit B&W Mac: min depth unreachable → flagged. */
    memset(&it, 0, sizeof it); it.minDepth = 8;
    memset(&e, 0, sizeof e); e.tier = 0; e.hasColorQD = 0; e.maxScreenDepth = 1; e.ramKB = 4096;
    CHECK(compat_reason(&it, &e, out) == 1, "compat: 256-colour title on a B&W Mac");
    CHECK(strstr(out, "colour display") != 0, "compat: names the colour display");
    /* Reachable min depth (8-bit screen) is NOT flagged (launcher raises instead). */
    e.hasColorQD = 1; e.maxScreenDepth = 8;
    CHECK(compat_reason(&it, &e, out) == 0, "compat: reachable min depth not flagged");

    /* maxCPU: a title that tolerates <= 68030 (maxCPU = tier 1 + 1 = 2) breaks on a
     * 68040 (tier 2) but is fine on a 68030 (tier 1). */
    memset(&it, 0, sizeof it); it.maxCPU = 2;
    memset(&e, 0, sizeof e); e.hasColorQD = 1; e.maxScreenDepth = 8; e.ramKB = 8192;
    e.tier = 2;
    CHECK(compat_reason(&it, &e, out) == 1, "compat: too-fast title flagged on a 68040");
    CHECK(strstr(out, "crash") != 0, "compat: maxCPU warns of a crash");
    e.tier = 1;
    CHECK(compat_reason(&it, &e, out) == 0, "compat: too-fast title fine on a 68030");

    /* OS range: a title made for System 7.1 only (minOS = maxOS = 0x0710). The check
     * is against the RUNNING System (sysVers): too old → needs a newer System; too
     * new → may not run; in range → fine. */
    memset(&it, 0, sizeof it); it.minOS = 0x0710; it.maxOS = 0x0710;
    memset(&e, 0, sizeof e); e.tier = 2; e.hasColorQD = 1; e.maxScreenDepth = 8; e.ramKB = 8192;
    e.sysVers = 0x0608;   /* booted System 6.0.8 — too old */
    CHECK(compat_reason(&it, &e, out) == 1, "compat: title needs a newer System");
    CHECK(strstr(out, "System 7.1") != 0, "compat: names the needed System");
    e.sysVers = 0x0761;   /* booted System 7.6.1 — too new */
    CHECK(compat_reason(&it, &e, out) == 1, "compat: title too new for this System");
    CHECK(strstr(out, "not run") != 0, "compat: maxOS warns it may not run");
    e.sysVers = 0x0710;   /* booted System 7.1 — in range */
    CHECK(compat_reason(&it, &e, out) == 0, "compat: in-range System is fine");
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
    test_catalog_cd_fields();
    test_compat();
    test_catindex();
    test_model_paged();
    test_model_categories();
    test_model_sort();
    test_model_list_ordered();
    test_model_nav();
    test_model_type_ahead();
    test_model_select();
    test_model_recommended_fallback();
    test_artcaps();
    test_toolbox_entry_parse();
    test_toolbox_name_match();
    test_toolbox_find();
    test_toolbox_fuzzy();
    test_toolbox_cdb();
    test_toolbox_magic();

    printf("\n%d/%d checks passed\n", g_total - g_fail, g_total);
    return g_fail ? 1 : 0;
}
