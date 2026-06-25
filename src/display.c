/*
 * display.c — see display.h. Thin wrappers over the Graphics Devices Manager
 * (GetMainDevice / HasDepth / SetDepth), confirmed against SuperMario's
 * QuickDraw/GDevice.a (gdDevType bit 0: 0 = monochrome, 1 = colour).
 */
#include "display.h"

/* gdDevType isn't in Retro68's leaner headers; it's bit 0 of gdFlags. */
#ifndef gdDevType
#define gdDevType 0
#endif

static const short kCandidates[] = { 1, 2, 4, 8, 16, 32 };
#define NCAND ((int)(sizeof kCandidates / sizeof kCandidates[0]))

int display_depths(short *out, int max)
{
    GDHandle gd = GetMainDevice();
    int n = 0, i;
    if (!gd) return 0;
    for (i = 0; i < NCAND; i++) {
        /* whichFlags = 0 -> match any mode at this depth (colour or mono). */
        if (HasDepth(gd, kCandidates[i], 0, 0) != 0) {
            if (n < max) out[n] = kCandidates[i];
            n++;
        }
    }
    return n;
}

short display_current_depth(void)
{
    GDHandle gd = GetMainDevice();
    if (gd) {
        PixMapHandle pm = (**gd).gdPMap;
        if (pm) return (**pm).pixelSize;
    }
    return 1;
}

OSErr display_set_depth(short depth)
{
    GDHandle gd = GetMainDevice();
    short    flags;
    if (!gd) return paramErr;
    /* Pick a colour mode for >1 bpp, monochrome for 1 bpp. */
    flags = (depth > 1) ? (1 << gdDevType) : 0;
    return SetDepth(gd, depth, 1 << gdDevType, flags);
}

short display_depth_at_most(short cap)
{
    short list[NCAND];
    int   n = display_depths(list, NCAND), i;
    short best = 0;
    for (i = 0; i < n; i++)
        if (list[i] <= cap && list[i] > best) best = list[i];
    return best;
}
