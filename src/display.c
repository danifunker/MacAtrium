/*
 * display.c — see display.h. Thin wrappers over the Graphics Devices Manager
 * (GetMainDevice / HasDepth / SetDepth), confirmed against SuperMario's
 * QuickDraw/GDevice.a (gdDevType bit 0: 0 = monochrome, 1 = colour).
 */
#include "display.h"
#include <Devices.h>   /* PBControlSync, CntrlParam, ParmBlkPtr */
#include <string.h>

/* gdDevType isn't in Retro68's leaner headers; it's bit 0 of gdFlags. */
#ifndef gdDevType
#define gdDevType 0
#endif

/* Classic video-driver Control selector + depth mode IDs, from Apple's Video.h
 * and JMFBDriver.a (not in Retro68's multiversal headers, so declared here).
 * cscSetDefaultMode (csCode 9) "writes the requested card default mode into slot
 * pRAM; upon restart PrimaryInit detects the change and sets the card up for
 * that mode." The driver reads the spID as a *byte* at csParam offset 0
 * (`MOVE.B csMode(A2)`). Depth modes are firstVidMode..sixthVidMode = 128..133
 * for 1/2/4/8/16/32 bpp respectively. */
#define kCscSetDefaultMode 9

static unsigned char spid_for_depth(short depth)
{
    switch (depth) {
        case 1:  return 128;  /* firstVidMode  (1 bpp)  */
        case 2:  return 129;  /* secondVidMode (2 bpp)  */
        case 4:  return 130;  /* thirdVidMode  (4 bpp)  */
        case 8:  return 131;  /* fourthVidMode (8 bpp)  */
        case 16: return 132;  /* fifthVidMode  (16 bpp) */
        case 32: return 133;  /* sixthVidMode  (32 bpp) */
        default: return 0;
    }
}

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

OSErr display_set_default_depth(short depth)
{
    GDHandle      gd   = GetMainDevice();
    unsigned char spID = spid_for_depth(depth);
    CntrlParam    pb;

    if (!gd || spID == 0) return paramErr;

    /* Control(cscSetDefaultMode): save spID as the card's boot default in slot
     * PRAM. Sets the *boot* depth only — applying it to the live screen is the
     * caller's job (display_set_depth). The driver reads the spID as a byte at
     * csParam offset 0. */
    memset(&pb, 0, sizeof pb);
    pb.ioCRefNum = (**gd).gdRefNum;
    pb.csCode    = kCscSetDefaultMode;
    ((unsigned char *)pb.csParam)[0] = spID;
    return PBControlSync((ParmBlkPtr)&pb);
}
