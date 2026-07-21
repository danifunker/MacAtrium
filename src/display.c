/*
 * display.c — see display.h. Thin wrappers over the Graphics Devices Manager
 * (GetMainDevice / HasDepth / SetDepth), confirmed against SuperMario's
 * QuickDraw/GDevice.a (gdDevType bit 0: 0 = monochrome, 1 = colour).
 */
#include "display.h"
#include <Devices.h>   /* PBControlSync, CntrlParam, ParmBlkPtr */
#include <Gestalt.h>   /* Color QuickDraw presence test (color_qd_present) */
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

/* The Graphics Devices Manager — GetMainDevice / HasDepth / SetDepth, and the slot-
 * driver Control/Status calls further down — ships with Color QuickDraw. On the 68000
 * compacts (Mac Plus, SE, Classic, Portable, PowerBook 100) Color QD is absent and
 * those traps are UNIMPLEMENTED: calling one bombs with an "unimplemented trap" system
 * error, it does NOT return NULL/paramErr. So every wrapper below early-outs when Color
 * QD is missing, BEFORE it touches a GDevice trap. That is what makes display.h's
 * "no-ops / depth 1 when Color QD is absent" contract actually hold for every caller —
 * so e.g. art_caps_probe() can measure the machine without a hasColorQD guard of its
 * own, and a future caller can't reintroduce the crash by forgetting one. Probe once
 * (Gestalt is safe on every System our images boot, 6.0.4+) and cache the result. */
static int color_qd_present(void)
{
    static int cached = -1;
    if (cached < 0) {
        long v;
        cached = (Gestalt(gestaltQuickdrawVersion, &v) == noErr && v >= gestalt8BitQD) ? 1 : 0;
    }
    return cached;
}

int display_depths(short *out, int max)
{
    GDHandle gd;
    int n = 0, i;
    if (!color_qd_present()) return 0;   /* no Color QD -> no Graphics Devices Manager */
    gd = GetMainDevice();
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
    GDHandle gd;
    if (!color_qd_present()) return 1;   /* B&W compact: the screen is 1-bit, always */
    gd = GetMainDevice();
    if (gd) {
        PixMapHandle pm = (**gd).gdPMap;
        if (pm) return (**pm).pixelSize;
    }
    return 1;
}

OSErr display_set_depth(short depth)
{
    GDHandle gd;
    short    flags;
    if (!color_qd_present()) return paramErr;   /* can't switch depth without Color QD */
    gd = GetMainDevice();
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

short display_depth_at_least(short floor)
{
    short list[NCAND];
    int   n = display_depths(list, NCAND), i;
    short best = 0;   /* smallest supported depth >= floor; 0 = floor unreachable */
    for (i = 0; i < n; i++)
        if (list[i] >= floor && (best == 0 || list[i] < best)) best = list[i];
    return best;
}

/* cscGetMode (Status csCode 2): read the depth-mode id the card is CURRENTLY
 * running (VDPageInfo.csMode, a word). That id is, by definition, a mode the card
 * can display and boot — unlike the assumed 128..133 family, which isn't guaranteed
 * to be the id a given card actually uses for that depth. 0 on failure. */
#define kCscGetMode 2
static short display_current_mode_id(GDHandle gd)
{
    CntrlParam pb;
    if (!gd) return 0;
    memset(&pb, 0, sizeof pb);
    pb.ioCRefNum = (**gd).gdRefNum;
    pb.csCode    = kCscGetMode;
    if (PBStatusSync((ParmBlkPtr)&pb) != noErr) return 0;
    return pb.csParam[0];                 /* VDPageInfo.csMode */
}

/* Persist `depth` as the card's boot default (slot PRAM), so the *system* comes up
 * at that depth from PrimaryInit. Writing slot PRAM is delicate — an id the card
 * can't cold-boot leaves the machine black until a PRAM reset — so this now REFUSES
 * to write unless the live screen is ALREADY at `depth` (the caller sets it first via
 * display_set_depth). We then persist the exact mode the card is proven to be running
 * (cscGetMode), not a hardcoded 128..133 guess. If the driver won't report its mode we
 * fall back to the standard family id, still gated on the depth being live. The driver
 * reads the id as a byte at csParam offset 0. */
OSErr display_set_default_depth(short depth)
{
    GDHandle      gd;
    short         mode;
    unsigned char spID;
    CntrlParam    pb;

    if (!color_qd_present()) return paramErr;                 /* no slot PRAM to write without Color QD */
    gd = GetMainDevice();
    if (!gd) return paramErr;
    if (display_current_depth() != depth) return paramErr;   /* only persist a live, proven depth */

    mode = display_current_mode_id(gd);                      /* the id the card actually runs */
    if (mode <= 0 || mode > 255) mode = spid_for_depth(depth);   /* fall back to the family id */
    if (mode <= 0 || mode > 255) return paramErr;
    spID = (unsigned char)mode;

    memset(&pb, 0, sizeof pb);
    pb.ioCRefNum = (**gd).gdRefNum;
    pb.csCode    = kCscSetDefaultMode;
    ((unsigned char *)pb.csParam)[0] = spID;
    return PBControlSync((ParmBlkPtr)&pb);
}
