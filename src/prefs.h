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
