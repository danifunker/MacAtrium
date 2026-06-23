/*
 * sound.c — see sound.h. The Sound Manager's SysBeepVolume uses a 0..0x0100
 * level (0x0100 = full); we present the classic 0..7 scale and map between them.
 * GetSysBeepVolume is also our availability probe.
 */
#include "sound.h"
#include "macfs.h"     /* macfs_make_spec */

#include <Sound.h>     /* shims to Multiverse: Get/SetSysBeepVolume, SysBeep */
#include <Resources.h> /* FSpOpenResFile, Get1Resource, DetachResource        */
#include <Files.h>

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

void sound_apply_vol(int v)
{
    long lvl;
    check();
    if (!gAvail) return;
    if (v < 0) v = 0;
    if (v > SOUND_VOL_MAX) v = SOUND_VOL_MAX;
    lvl = ((long)v * FULL) / SOUND_VOL_MAX;
    (void)SetSysBeepVolume(lvl);
}

void sound_set_vol(int v)
{
    sound_apply_vol(v);
    if (gAvail) SysBeep(1);                         /* feedback at the new level */
}

/* One persistent channel for async (startup) playback; the OS frees it at exit.
 * Synchronous (shutdown) playback passes a NULL channel and lets SndPlay manage
 * its own. */
static SndChannelPtr gChan = 0;

void sound_play_file(const char *relToRoot, int async)
{
    FSSpec spec;
    short  refNum;
    Handle h;

    check();
    if (!gAvail) return;                            /* no Sound Manager */
    if (macfs_make_spec(relToRoot, &spec) != noErr) return;

    refNum = FSpOpenResFile(&spec, fsRdPerm);
    if (refNum == -1) return;                       /* no such sound file */

    h = Get1Resource('snd ', 128);
    if (h) {
        DetachResource(h);                          /* survive CloseResFile below */
        HLock(h);
        if (async) {
            if (!gChan)
                if (SndNewChannel(&gChan, sampledSynth, 0, 0L) != noErr) gChan = 0;
            /* Keep the handle locked for the channel's lifetime — a one-shot
             * boot chime, so the small retained block is acceptable. */
            if (gChan) (void)SndPlay(gChan, h, true);
            else { DisposeHandle(h); }              /* couldn't get a channel */
        } else {
            (void)SndPlay(0, h, false);             /* blocks until done */
            DisposeHandle(h);
        }
    }
    CloseResFile(refNum);
}
