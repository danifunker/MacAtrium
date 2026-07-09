/*
 * artcaps.c — see artcaps.h. Measure the granted partition + the card's depths and
 * derive which art tiers this machine can show and hold (docs/44 P1).
 *
 * Two axes, kept separate (docs/44):
 *   VRAM   — display_depths()/HasDepth: can the card display the tier's depth?
 *   memory — artBudget vs a conservative per-tier estimate: will it fit the partition?
 * A tier is enabled only when both pass. Screen depth and art depth are independent:
 * a deep screen on a small partition keeps the deep screen and just loads shallower
 * art, so the memory gate caps the art variant, never the display.
 *
 * art_caps_derive() below is the pure arithmetic (no Toolbox); art_caps_probe()
 * (guarded out of the host build) gathers the live numbers and calls it.
 */
#include "artcaps.h"

#include <string.h>

/* The three tiers' bit depths, and the minimum *screen* depth each needs to be
 * shown as itself: 8-bit art wants a 256-colour screen; 24-bit ("millions") wants
 * a direct/truecolor screen (>=16 bpp — QuickDraw stores 24-bit colour in 32-bit
 * pixels, so there is no 24 bpp screen mode to test for). 1-bit shows anywhere. */
static const short kArtModeDepth[ART_MODE_COUNT]     = { 1, 8, 24 };
static const short kArtModeMinScreen[ART_MODE_COUNT] = { 1, 8, 16 };

/* Build-side art bound (tools/atrium-tool config.rs DEFAULT_ART_BOUND is 720x720).
 * The launcher isn't told each build's max_art_size, so mirror the default as the
 * peak-estimate basis. peakArtBytes uses the *uncompressed* pixmap at this box as a
 * deliberately conservative upper bound; the real resident PICT is PackBits-packed
 * and smaller, and P2's per-resource on-disk size check is the authoritative gate. */
#define ARTCAPS_ART_DIM  720L

/* Reserves carved out of free partition before art (docs/44 budget model). The
 * catalog page + row-icon cache aren't resident yet when we probe (main() calls us
 * before the catalog/UI load), so reserve for them explicitly. */
#define ARTCAPS_CATALOG_RESERVE  (150L * 1024)   /* resident metadata page (docs/21) */
#define ARTCAPS_ROWICON_RESERVE  ( 48L * 1024)   /* per-row icon cache in the browse view */
#define ARTCAPS_TEMP_SCARCE      ( 64L * 1024)   /* TempFreeMem below this ⇒ "no temp memory" */

/* Conservative resident bytes for a `depth`-bit art variant at the build art bound:
 * the uncompressed pixmap (an upper bound on the packed PICT the loader holds). */
static long peak_art_bytes(short depth)
{
    long rowBytes = (ARTCAPS_ART_DIM * (long)depth + 7) / 8;
    return rowBytes * ARTCAPS_ART_DIM;
}

void art_caps_derive(ArtCaps *out, const ArtCapsInput *in)
{
    long gworldBytes, gworldReserve, reserves, rowBytes;
    int  m;

    memset(out, 0, sizeof *out);
    out->grantedPartition = in->grantedPartition;
    out->partitionFree    = in->partitionFree;
    out->maxBlock         = in->maxBlock;
    out->tempFree         = in->tempFree;
    out->maxCardDepth     = (in->maxCardDepth >= 1) ? in->maxCardDepth : 1;

    /* Bare System 6 has no temp memory, so the off-screen GWorld (render.c) falls
     * INTO the partition instead of system temp. Reserve its footprint — computed
     * from the actual screen, so a 1-bit compact reserves kilobytes, not megabytes
     * — only when temp memory is scarce. */
    rowBytes    = (in->screenW * (in->screenDepth > 0 ? in->screenDepth : 1) + 7) / 8;
    gworldBytes = rowBytes * in->screenH;
    gworldReserve = (in->tempFree < ARTCAPS_TEMP_SCARCE) ? gworldBytes : 0;

    reserves = ARTCAPS_CATALOG_RESERVE + ARTCAPS_ROWICON_RESERVE + gworldReserve;
    out->artBudget = in->partitionFree - reserves;
    if (out->artBudget < 0) out->artBudget = 0;

    out->maxAffordableDepth = 1;                 /* 1-bit is the always-available floor */
    out->defaultMode        = 1;
    for (m = 0; m < ART_MODE_COUNT; m++) {
        out->peakArtBytes[m] = peak_art_bytes(kArtModeDepth[m]);
        out->displayable[m]  = (out->maxCardDepth >= kArtModeMinScreen[m]);
        out->affordable[m]   = (out->artBudget >= out->peakArtBytes[m]);
        out->enabled[m]      = (out->displayable[m] && out->affordable[m]);
        if (out->affordable[m] && kArtModeDepth[m] > out->maxAffordableDepth)
            out->maxAffordableDepth = kArtModeDepth[m];
        if (out->enabled[m] && kArtModeDepth[m] > out->defaultMode)
            out->defaultMode = kArtModeDepth[m];
    }
}

/* ---- live measurement (Toolbox; excluded from the host build) --------------- */
#ifndef ARTCAPS_HOST_TEST

#include "display.h"
#include <Memory.h>
#include <Processes.h>

/* Granted partition + free bytes via the Process Manager (System 7+). Mirrors
 * mem.c's proc_mem. Returns 0 (leaving *out untouched) when unavailable. */
static int proc_partition(long *size, long *freeb)
{
    ProcessSerialNumber psn;
    ProcessInfoRec      info;
    if (GetCurrentProcess(&psn) != noErr) return 0;
    memset(&info, 0, sizeof info);
    info.processInfoLength = (long)sizeof info;   /* name/appSpec left nil */
    if (GetProcessInformation(&psn, &info) != noErr) return 0;
    *size  = (long)info.processSize;
    *freeb = (long)info.processFreeMem;
    return 1;
}

/* Extent of the application heap zone (the "partition" when the Process Manager
 * isn't there, i.e. System 6). 0 if the zone looks bogus. */
static long heap_zone_extent(void)
{
    THz z = ApplicationZone();
    if (z && z->bkLim > (Ptr)z) return (long)(z->bkLim - (Ptr)z);
    return 0;
}

void art_caps_probe(ArtCaps *out, const Env *e)
{
    ArtCapsInput in;
    short        depths[6];
    int          nDepths, i;

    memset(&in, 0, sizeof in);

    in.maxBlock = MaxBlock();
    in.tempFree = (e->sysVers >= 0x0700) ? TempFreeMem() : 0;   /* trap is Sys7-era */

    if (e->sysVers >= 0x0700 && proc_partition(&in.grantedPartition, &in.partitionFree)) {
        /* processSize / processFreeMem: the real MultiFinder partition. */
    } else {
        in.grantedPartition = heap_zone_extent();               /* Sys6: the app heap */
        in.partitionFree    = FreeMem();
        if (in.grantedPartition < in.partitionFree)
            in.grantedPartition = in.partitionFree;             /* keep granted >= free */
    }

    /* VRAM: deepest depth the card can display (1 for B&W / no Color QD). */
    nDepths = display_depths(depths, 6);
    in.maxCardDepth = 1;
    for (i = 0; i < nDepths; i++)
        if (depths[i] > in.maxCardDepth) in.maxCardDepth = depths[i];

    in.screenW     = (long)(e->screen.right - e->screen.left);
    in.screenH     = (long)(e->screen.bottom - e->screen.top);
    in.screenDepth = (short)((e->pixelSize > 0) ? e->pixelSize : 1);

    art_caps_derive(out, &in);
}

#endif /* !ARTCAPS_HOST_TEST */
