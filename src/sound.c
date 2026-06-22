/*
 * sound.c — see sound.h. The Sound Manager's SysBeepVolume uses a 0..0x0100
 * level (0x0100 = full); we present the classic 0..7 scale and map between them.
 * GetSysBeepVolume is also our availability probe.
 */
#include "sound.h"

#include <Sound.h>     /* shims to Multiverse: Get/SetSysBeepVolume, SysBeep */

#define FULL  0x0100L  /* SysBeepVolume full-scale level */

static int gChecked = 0;
static int gAvail   = 0;

static void check(void)
{
    if (!gChecked) {
        long lvl;
        gAvail   = (GetSysBeepVolume(&lvl) == noErr);
        gChecked = 1;
    }
}

int sound_available(void)
{
    check();
    return gAvail;
}

int sound_get_vol(void)
{
    long lvl = 0;
    check();
    if (!gAvail) return 0;
    if (GetSysBeepVolume(&lvl) != noErr) return 0;
    lvl &= 0xFFFF;                                  /* level lives in the low word */
    return (int)((lvl * SOUND_VOL_MAX + FULL / 2) / FULL);
}

void sound_set_vol(int v)
{
    long lvl;
    check();
    if (!gAvail) return;
    if (v < 0) v = 0;
    if (v > SOUND_VOL_MAX) v = SOUND_VOL_MAX;
    lvl = ((long)v * FULL) / SOUND_VOL_MAX;
    (void)SetSysBeepVolume(lvl);
    SysBeep(1);                                     /* feedback at the new level */
}
