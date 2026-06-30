/*
 * prefs.c — see prefs.h. A tiny key=value text file in the Preferences folder:
 *
 *     theme=dark|light
 *     volume=0..7
 *     category=<name>
 *     item=<id>
 *
 * Lines are CR/LF/CRLF tolerant; the value is the rest of the line (so category
 * names with spaces survive). Unknown keys are ignored. We read the whole file
 * via macfs_read_all() and write it back wholesale (the file is tiny).
 */
#include "prefs.h"
#include "macfs.h"        /* macfs_read_all */
#include "mac_compat.h"

#include <Files.h>        /* FindFolder, FSp* — multiversal header */
#include <Gestalt.h>      /* gestaltSystemVersion — System-6 path selection */
#include <Memory.h>
#include <string.h>

/* These live in <Folders.h>/<Script.h> on full toolboxes; Retro68 folds them
 * into the multiversal header, but guard in case a leaner build omits them. */
#ifndef kPreferencesFolderType
#define kPreferencesFolderType 'pref'
#endif
#ifndef kCreateFolder
#define kCreateFolder 1
#endif
#ifndef kDontCreateFolder
#define kDontCreateFolder 0
#endif
#ifndef smSystemScript
#define smSystemScript 0
#endif

#define PREFS_NAME    "\pMacAtrium Prefs"
#define PREFS_CREATOR 'ATRM'
#define PREFS_TYPE    'pref'

/* Locate the prefs file's FSSpec under the (boot) Preferences folder. With
 * `create`, FindFolder makes the Preferences folder if it's missing. A missing
 * file yields fnfErr with a still-usable spec (for the create-then-write path). */
static OSErr prefs_spec(Boolean create, FSSpec *spec, short *vrefOut)
{
    short vref;
    long  dirID;
    long sysv = 0;
    (void)Gestalt(gestaltSystemVersion, &sysv);
    /* System 6 has no Preferences folder and no FindFolder (System-7 Folder
     * Manager trap). Store the prefs file inside /MacAtrium instead (next to the
     * metadata), resolved by macfs — no FindFolder, works on 6.0.8. */
    if (sysv < 0x0700) {
        OSErr err = macfs_make_spec("MacAtrium Prefs", spec);
        if (err != noErr) return err;
        if (vrefOut) *vrefOut = spec->vRefNum;
        return noErr;
    }
    OSErr err = FindFolder(kOnSystemDisk, kPreferencesFolderType,
                           create ? kCreateFolder : kDontCreateFolder,
                           &vref, &dirID);
    if (err != noErr) return err;
    if (vrefOut) *vrefOut = vref;
    /* Build the spec directly (FSMakeFSSpec is a System-7 trap that faults on
     * 6.0.8): the Preferences folder's dirID is the parent, PREFS_NAME the leaf. */
    spec->vRefNum = vref;
    spec->parID   = dirID;
    BlockMoveData(PREFS_NAME, spec->name, (long)((const unsigned char *)PREFS_NAME)[0] + 1);
    return noErr;
}

static int parse_int(const char *s)
{
    int v = 0, neg = 0;
    if (*s == '-') { neg = 1; s++; }
    while (*s >= '0' && *s <= '9') { v = v * 10 + (*s - '0'); s++; }
    return neg ? -v : v;
}

void prefs_load(Prefs *p)
{
    FSSpec spec;
    char  *buf = 0;
    long   len = 0, i;
    char   line[ITEM_PATH_LEN];

    /* defaults: everything unset */
    p->theme = 0; p->haveTheme = 0;
    p->vol   = 0; p->haveVol   = 0;
    p->artPref = 0; p->haveArtPref = 0;
    p->sndStartup = 0; p->haveSndStartup = 0;
    p->sndShutdown = 0; p->haveSndShutdown = 0;
    p->catList = 0; p->haveCatList = 0;
    p->hideMenuBar = 0; p->haveHideMenuBar = 0;
    p->hideTitleBar = 0; p->haveHideTitleBar = 0;
    p->textSize = 0; p->haveTextSize = 0;
    p->gridStyle = 0; p->haveGridStyle = 0;
    p->sortMode = 0; p->sortDesc = 0; p->haveSort = 0;
    p->listColType = 0; p->haveListCol = 0;
    p->carousel = 7; p->haveCarousel = 0;
    p->view = 0; p->haveView = 0;
    p->depth = 0; p->haveDepth = 0;
    p->category[0] = '\0';
    p->item[0]     = '\0';
    p->haveSel = 0;

    if (prefs_spec(false, &spec, 0) != noErr) return;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return;

    for (i = 0; i < len; ) {
        int   n = 0;
        char *eq;
        const char *key, *val;

        while (i < len && buf[i] != '\r' && buf[i] != '\n') {
            if (n < (int)sizeof line - 1) line[n++] = buf[i];
            i++;
        }
        line[n] = '\0';
        while (i < len && (buf[i] == '\r' || buf[i] == '\n')) i++;  /* eat EOL */

        eq = strchr(line, '=');
        if (!eq) continue;
        *eq = '\0';
        key = line;
        val = eq + 1;

        if (strcmp(key, "theme") == 0) {
            p->theme = (strcmp(val, "light") == 0) ? 1 : 0;
            p->haveTheme = 1;
        } else if (strcmp(key, "volume") == 0) {
            int v = parse_int(val);
            if (v < 0) v = 0;
            if (v > 7) v = 7;                    /* SOUND_VOL_MAX scale */
            p->vol = v;
            p->haveVol = 1;
        } else if (strcmp(key, "artwork") == 0) {
            p->artPref = (strcmp(val, "screenshot") == 0) ? 1 : 0;
            p->haveArtPref = 1;
        } else if (strcmp(key, "startupsound") == 0) {
            p->sndStartup = (strcmp(val, "on") == 0) ? 1 : 0;
            p->haveSndStartup = 1;
        } else if (strcmp(key, "shutdownsound") == 0) {
            p->sndShutdown = (strcmp(val, "on") == 0) ? 1 : 0;
            p->haveSndShutdown = 1;
        } else if (strcmp(key, "categorieslist") == 0) {
            p->catList = (strcmp(val, "on") == 0) ? 1 : 0;
            p->haveCatList = 1;
        } else if (strcmp(key, "menubar") == 0) {
            p->hideMenuBar = (strcmp(val, "hidden") == 0) ? 1 : 0;
            p->haveHideMenuBar = 1;
        } else if (strcmp(key, "titlebar") == 0) {
            p->hideTitleBar = (strcmp(val, "hidden") == 0) ? 1 : 0;
            p->haveHideTitleBar = 1;
        } else if (strcmp(key, "textsize") == 0) {
            int v = parse_int(val);
            if (v >= 9 && v <= 12) { p->textSize = v; p->haveTextSize = 1; }
        } else if (strcmp(key, "gridstyle") == 0) {
            p->gridStyle = (strcmp(val, "tiles") == 0) ? 1 : 0;
            p->haveGridStyle = 1;
        } else if (strcmp(key, "sortmode") == 0) {
            int v = parse_int(val);
            if (v >= 0 && v <= 3) { p->sortMode = v; p->haveSort = 1; }
        } else if (strcmp(key, "sortdesc") == 0) {
            p->sortDesc = (parse_int(val) != 0) ? 1 : 0;
        } else if (strcmp(key, "listcol") == 0) {
            int v = parse_int(val);
            if (v >= 60 && v <= 260) { p->listColType = v; p->haveListCol = 1; }
        } else if (strcmp(key, "carousel") == 0) {
            int v = parse_int(val);
            if (v >= 3) {
                if (v > 25) v = 25;
                if (v % 2 == 0) v++;        /* odd icon counts only */
                p->carousel = v;
                p->haveCarousel = 1;
            }
        } else if (strcmp(key, "view") == 0) {
            int v = parse_int(val);
            if (v < 0) v = 0;
            if (v > 2) v = 2;               /* VIEW_CAROUSEL/ICON/LIST */
            p->view = v; p->haveView = 1;
        } else if (strcmp(key, "depth") == 0) {
            int v = parse_int(val);
            if (v > 0) { p->depth = v; p->haveDepth = 1; }
        } else if (strcmp(key, "category") == 0) {
            strncpy(p->category, val, sizeof p->category - 1);
            p->category[sizeof p->category - 1] = '\0';
            if (p->category[0]) p->haveSel = 1;
        } else if (strcmp(key, "item") == 0) {
            strncpy(p->item, val, sizeof p->item - 1);
            p->item[sizeof p->item - 1] = '\0';
        }
    }

    DisposePtr(buf);
}

static void append_str(char *dst, int *n, int cap, const char *s)
{
    while (*s && *n < cap - 1) dst[(*n)++] = *s++;
}

static void append_int(char *dst, int *n, int cap, int v)
{
    char tmp[12];
    int  t = 0, neg = (v < 0);
    unsigned u = neg ? (unsigned)(-v) : (unsigned)v;
    if (u == 0) tmp[t++] = '0';
    while (u) { tmp[t++] = (char)('0' + u % 10); u /= 10; }
    if (neg && *n < cap - 1) dst[(*n)++] = '-';
    while (t && *n < cap - 1) dst[(*n)++] = tmp[--t];
}

OSErr prefs_save(const Prefs *p)
{
    FSSpec spec;
    short  vref = 0, refNum;
    OSErr  err, first = noErr;
    char   body[320];
    int    n = 0;
    long   count;

    err = prefs_spec(true, &spec, &vref);
    if (err != noErr && err != fnfErr) return err;

    err = macfs_create(&spec, PREFS_CREATOR, PREFS_TYPE);   /* HCreate (6.0.8-safe) */
    if (err != noErr && err != dupFNErr) return err;   /* dupFNErr = already there */

    if (p->haveTheme) {
        append_str(body, &n, sizeof body, "theme=");
        append_str(body, &n, sizeof body, p->theme ? "light" : "dark");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveVol) {
        append_str(body, &n, sizeof body, "volume=");
        append_int(body, &n, sizeof body, p->vol);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveArtPref) {
        append_str(body, &n, sizeof body, "artwork=");
        append_str(body, &n, sizeof body, p->artPref ? "screenshot" : "box");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveSndStartup) {
        append_str(body, &n, sizeof body, "startupsound=");
        append_str(body, &n, sizeof body, p->sndStartup ? "on" : "off");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveSndShutdown) {
        append_str(body, &n, sizeof body, "shutdownsound=");
        append_str(body, &n, sizeof body, p->sndShutdown ? "on" : "off");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveCatList) {
        append_str(body, &n, sizeof body, "categorieslist=");
        append_str(body, &n, sizeof body, p->catList ? "on" : "off");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveHideMenuBar) {
        append_str(body, &n, sizeof body, "menubar=");
        append_str(body, &n, sizeof body, p->hideMenuBar ? "hidden" : "shown");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveHideTitleBar) {
        append_str(body, &n, sizeof body, "titlebar=");
        append_str(body, &n, sizeof body, p->hideTitleBar ? "hidden" : "shown");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveTextSize) {
        append_str(body, &n, sizeof body, "textsize=");
        append_int(body, &n, sizeof body, p->textSize);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveGridStyle) {
        append_str(body, &n, sizeof body, "gridstyle=");
        append_str(body, &n, sizeof body, p->gridStyle ? "tiles" : "finder");
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveSort) {
        append_str(body, &n, sizeof body, "sortmode=");
        append_int(body, &n, sizeof body, p->sortMode);
        append_str(body, &n, sizeof body, "\r");
        append_str(body, &n, sizeof body, "sortdesc=");
        append_int(body, &n, sizeof body, p->sortDesc);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveListCol) {
        append_str(body, &n, sizeof body, "listcol=");
        append_int(body, &n, sizeof body, p->listColType);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveCarousel) {
        append_str(body, &n, sizeof body, "carousel=");
        append_int(body, &n, sizeof body, p->carousel);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveView) {
        append_str(body, &n, sizeof body, "view=");
        append_int(body, &n, sizeof body, p->view);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveDepth) {
        append_str(body, &n, sizeof body, "depth=");
        append_int(body, &n, sizeof body, p->depth);
        append_str(body, &n, sizeof body, "\r");
    }
    if (p->haveSel && p->category[0]) {
        append_str(body, &n, sizeof body, "category=");
        append_str(body, &n, sizeof body, p->category);
        append_str(body, &n, sizeof body, "\r");
        if (p->item[0]) {
            append_str(body, &n, sizeof body, "item=");
            append_str(body, &n, sizeof body, p->item);
            append_str(body, &n, sizeof body, "\r");
        }
    }

    err = macfs_open_df(&spec, fsWrPerm, &refNum);   /* HOpen (6.0.8-safe) */
    if (err != noErr) return err;

    err = SetEOF(refNum, 0);                           /* drop any old content */
    if (err != noErr) first = err;
    count = n;
    err = FSWrite(refNum, &count, body);
    if (err != noErr && first == noErr) first = err;
    err = FSClose(refNum);
    if (err != noErr && first == noErr) first = err;

    err = FlushVol(0, vref);                            /* push it to disk */
    if (err != noErr && first == noErr) first = err;

    return first;
}
