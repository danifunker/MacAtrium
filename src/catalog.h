/*
 * catalog.h — in-memory model of metadata/catalog.jsonl (schema v2, docs/06).
 *
 * catalog_parse() is pure C (no Toolbox): it turns a whole catalog buffer into
 * an array of items and is unit-tested off-target. Reading the file from an HFS
 * volume lives in macfs.c.
 */
#ifndef MACATRIUM_CATALOG_H
#define MACATRIUM_CATALOG_H

#define MAX_ITEMS       256
#define MAX_ITEM_CATS   8
#define ITEM_ID_LEN     48
#define ITEM_NAME_LEN   64
#define ITEM_PATH_LEN   192
#define ITEM_CAT_LEN    32
#define ITEM_DESC_LEN   128
#define ITEM_VENDOR_LEN 40
#define ITEM_GENRE_LEN  64

typedef struct {
    char id[ITEM_ID_LEN];
    char name[ITEM_NAME_LEN];
    char app[ITEM_PATH_LEN];                 /* path relative to /MacAtrium */
    char cats[MAX_ITEM_CATS][ITEM_CAT_LEN];
    int  ncats;
    long year;                               /* 0 if absent */
    char vendor[ITEM_VENDOR_LEN];            /* developer/publisher, "" if absent */
    char genre[ITEM_GENRE_LEN];              /* genres joined for display, "" if absent */
    char type[8];                            /* OSType text, "" if absent */
    char creator[8];
    char desc[ITEM_DESC_LEN];
    char image[ITEM_PATH_LEN];               /* box-art base path, "" if absent */
    char shot[ITEM_PATH_LEN];                /* screenshot base path, "" if absent */
} CatItem;

typedef struct {
    CatItem items[MAX_ITEMS];
    int     nitems;
    int     dropped;                         /* malformed lines skipped */
} Catalog;

/* Parse a whole catalog buffer (newline-delimited; CR/LF/CRLF tolerant).
 * A required-field-missing or malformed line is skipped (counted in .dropped).
 * Returns the number of items loaded. */
int catalog_parse(const char *buf, long len, Catalog *cat);

#endif /* MACATRIUM_CATALOG_H */
