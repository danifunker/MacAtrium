/*
 * catalog.h — in-memory model of metadata/catalog.jsonl (schema v2, docs/06).
 *
 * catalog_parse() is pure C (no Toolbox): it turns a whole catalog buffer into
 * an array of items and is unit-tested off-target. Reading the file from an HFS
 * volume lives in macfs.c.
 */
#ifndef MACATRIUM_CATALOG_H
#define MACATRIUM_CATALOG_H

#include "cpu.h"   /* CPU_* generations + CPU_GEN_NONE for the minCPU/maxCPU bounds */

#define MAX_ITEMS       256
/* Paged catalog (docs/21): the most items in one category PAGE. The generator
 * splits larger categories into sub-pages, so the launcher holds at most this
 * many CatItems at once — the RAM bound that fits a 4 MB Mac Plus. */
#define MAX_CAT_ITEMS   128
#define MAX_ITEM_CATS   8
#define ITEM_ID_LEN     48
#define ITEM_NAME_LEN   64
#define ITEM_PATH_LEN   192
#define ITEM_CAT_LEN    32
#define ITEM_DESC_LEN   256
#define ITEM_VENDOR_LEN 40
#define ITEM_GENRE_LEN  64
/* CD-title fields (docs/45), kept small to bound the resident page (docs/44). */
#define ITEM_CDIMG_LEN  64      /* host SD image filename, e.g. "MYST.iso"        */
#define ITEM_CDVOL_LEN  28      /* Mac HFS volume name (max 27 chars + NUL)        */
#define ITEM_CDAPP_LEN  128     /* run-from-CD app path, relative to the CD root   */

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
    char icon[ITEM_PATH_LEN];                /* list-row icon base path, "" if absent */
    char hotkey;                             /* launch hotkey char, 0 if none */
    int  maxDepth;                           /* cap screen to this bpp before launch;
                                              * 0 = no cap (launch at current depth) */
    /* Hardware requirements (docs/40): the launcher flags a title needing more than
     * this Mac (CPU tier / FPU / min depth / RAM) and confirms before launch. */
    /* CPU bounds as generation indices into the ONE table (CPU_* in cpu.h), parsed
     * from the catalog's canonical name ("68040"). Symmetric: both are a plain
     * generation, CPU_GEN_NONE = that bound is open. */
    int  minCPU;                             /* oldest CPU that runs it; flags when
                                              * gEnv.cpuGen < this */
    int  maxCPU;                             /* newest CPU it tolerates (breaks on a
                                              * FASTER Mac); flags when gEnv.cpuGen > this */
    long minOS;                              /* BCD System floor: the running System
                                              * (gEnv.sysVers) must be >= this; 0 = none */
    long maxOS;                              /* BCD System ceiling: running System must
                                              * be <= this; a title too new breaks; 0 = none */
    int  needsFPU;                           /* 1 = needs a hardware FPU (68LC040 lacks one) */
    int  minDepth;                           /* raise screen to >= this bpp before launch
                                              * (inverse of maxDepth); 0 = no floor */
    long minMem;                             /* min machine RAM in MB for a preflight
                                              * warning; 0 = none */
    /* CD-based titles (docs/45): the disc image is auto-inserted via the BlueSCSI
     * Toolbox before launch. All "" / 0 when this is not a CD title. */
    char cdImage[ITEM_CDIMG_LEN];            /* host SD image filename, "" if none */
    int  cdRequired;                         /* 1 = disc must mount to launch
                                              * (default for a CD title); 0 = optional */
    char cdVolume[ITEM_CDVOL_LEN];           /* expected mounted volume name, "" if none */
    char cdApp[ITEM_CDAPP_LEN];              /* run-from-CD app path relative to the CD
                                              * volume root; "" = app-on-HD (launch `app`,
                                              * CD mounted only as a data volume) */
} CatItem;

typedef struct {
    CatItem *items;                          /* heap array, `cap` entries (NULL if cap==0) */
    int      cap;                            /* allocated capacity (<= MAX_ITEMS) */
    int      nitems;
    int      dropped;                        /* malformed lines skipped */
} Catalog;

/* Count candidate (non-blank) lines: an upper bound on the items a buffer can
 * yield, so the caller can allocate the items array exactly once (instead of a
 * fixed CatItem[256] ≈ 390 KB allocated even for a 3-item library). Pure C. */
int catalog_count_lines(const char *buf, long len);

/* Parse a whole catalog buffer (newline-delimited; CR/LF/CRLF tolerant) into a
 * caller-provided items[cap]; fills up to `cap` items, sets *dropped to the count
 * of malformed/required-field-missing lines skipped. Returns the number loaded.
 * Pure C, allocation-free — the caller owns `items` (NewPtr on-target, malloc in
 * the host tests). */
int catalog_parse_into(const char *buf, long len, CatItem *items, int cap, int *dropped);

#endif /* MACATRIUM_CATALOG_H */
