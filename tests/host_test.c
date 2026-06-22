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

    m.curCat = 0; m.curItem = 0;
    CHECK(model_move_item(&m, 1) == 1 && m.curItem == 1, "nav down");
    CHECK(model_move_item(&m, -5) == 1 && m.curItem == 0, "nav up clamps to 0");
    CHECK(model_move_item(&m, 100) == 1 && m.curItem == 2, "nav down clamps to last");
    CHECK(model_move_item(&m, 100) == 0, "nav at end is no-op");

    int before = m.curCat;
    CHECK(model_move_cat(&m, 1) == 1 && m.curCat == before + 1, "cat right");
    CHECK(m.curItem == 0, "cat switch resets selection");
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

int main(void)
{
    test_json_scalars();
    test_json_array();
    test_json_escapes_and_unknown();
    test_json_edge();
    test_catalog_basic();
    test_catalog_line_endings();
    test_catalog_drops_bad();
    test_model_categories();
    test_model_sort();
    test_model_list_ordered();
    test_model_nav();
    test_model_type_ahead();

    printf("\n%d/%d checks passed\n", g_total - g_fail, g_total);
    return g_fail ? 1 : 0;
}
