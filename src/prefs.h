/*
 * prefs.h — persist a small set of launcher preferences (theme, alert volume,
 * and the last selection) to a "MacAtrium Prefs" file in the System's
 * Preferences folder, so they survive a reboot (docs/15: theme/volume reset to
 * defaults otherwise).
 *
 * Guest disk writes work (docs/13 §6); the file round-trips on real hardware /
 * an interactive emulator. The headless harness can read a *pre-seeded* file but
 * doesn't sync writes back to the .hda, so the full cross-boot round-trip needs a
 * non-headless check.
 */
#ifndef MACATRIUM_PREFS_H
#define MACATRIUM_PREFS_H

#include <Files.h>        /* OSErr */
#include "catalog.h"      /* ITEM_CAT_LEN, ITEM_ID_LEN */

typedef struct {
    int  theme;                    /* 0 = dark, 1 = light (mirrors render.h)   */
    int  haveTheme;                /* 1 if `theme` was loaded                  */
    int  vol;                      /* 0..SOUND_VOL_MAX alert volume            */
    int  haveVol;                  /* 1 if `vol` was loaded                    */
    int  artPref;                  /* 0 = Box Art, 1 = Screenshot              */
    int  haveArtPref;              /* 1 if `artPref` was loaded                */
    int  sndStartup;               /* 1 = play the startup sound on launch     */
    int  haveSndStartup;           /* 1 if `sndStartup` was loaded             */
    int  sndShutdown;              /* 1 = play the shutdown sound on Shut Down  */
    int  haveSndShutdown;          /* 1 if `sndShutdown` was loaded            */
    int  catList;                  /* 1 = show the categories list panel        */
    int  haveCatList;              /* 1 if `catList` was loaded                 */
    int  carousel;                 /* carousel icon count (odd 3..25)           */
    int  haveCarousel;             /* 1 if `carousel` was loaded                */
    int  view;                     /* browse view (VIEW_CAROUSEL/ICON/LIST)     */
    int  haveView;                 /* 1 if set; 0 = first run (show the chooser) */
    int  depth;                    /* saved colour depth in bits (0 = unset)    */
    int  haveDepth;                /* 1 if `depth` was loaded                   */
    char category[ITEM_CAT_LEN];   /* last category name ("" if none)          */
    char item[ITEM_ID_LEN];        /* last item id ("" if none)                */
    int  haveSel;                  /* 1 if a category was loaded               */
} Prefs;

/* Read the prefs file; every field defaults to "unset" if it's missing or
 * unreadable (never fails — a missing file is normal on first boot). */
void  prefs_load(Prefs *p);

/* Write the prefs file (creating the file / the Preferences folder as needed)
 * and flush the volume. Returns the first OSErr encountered, noErr on success. */
OSErr prefs_save(const Prefs *p);

#endif /* MACATRIUM_PREFS_H */
