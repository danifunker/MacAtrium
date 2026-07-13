/*
 * cdswap.c — see cdswap.h. Implements the launch-time CD insert flow (docs/45):
 *   1. fast-path: the wanted volume is already mounted → done
 *   2. unmount the CD we last inserted (avoids the "Please insert the disk" nag)
 *   3. LIST CDS → case-insensitive match of the metadata image filename
 *   4. SET NEXT CD immediately (the index is only valid against that listing)
 *   5. wait (abortable, timed) for the expected volume to mount + verify
 */
#include "cdswap.h"
#include "toolbox.h"
#include "macfs.h"

#include <Events.h>   /* TickCount */
#include <string.h>

/* The CD volume we last inserted this session (RAM), so step 2 can unmount the
 * outgoing disc even when the incoming title's metadata doesn't name it. */
static char gLastCdVol[ITEM_CDVOL_LEN];   /* "" = nothing inserted yet */
/* The image filename now in the drive, for the CD Library browser's active marker. */
static char gLastCdImage[ITEM_CDIMG_LEN];

const char *cdswap_active_image(void) { return gLastCdImage; }

void cdswap_set_active_image(const char *image)
{
    if (!image) { gLastCdImage[0] = '\0'; return; }
    strncpy(gLastCdImage, image, sizeof gLastCdImage - 1);
    gLastCdImage[sizeof gLastCdImage - 1] = '\0';
}

/* ---- session CD-image cache (docs/45) --------------------------------------- */
static short   gCdId;                 /* cached Toolbox CD-ROM SCSI id            */
static int     gCdFound;              /* 1 = a CD device answered                 */
static int     gCdScanned;            /* 1 = we've run a scan this session        */
static int     gCdN;                  /* cached entry count                       */
static TbEntry gCdCds[TB_MAX_CDS];    /* cached listing (~4.4 KB, off the stack)  */

void cdswap_scan(void)
{
    gCdScanned = 1;
    gCdN       = 0;
    gCdFound   = toolbox_probe_id(-1, &gCdId) ? 1 : 0;
    if (gCdFound)
        (void)toolbox_list_cds(gCdId, gCdCds, TB_MAX_CDS, &gCdN);
}

static void cd_scan_once(void) { if (!gCdScanned) cdswap_scan(); }

int cdswap_ready(short *id)
{
    cd_scan_once();
    if (!gCdFound) return 0;
    if (id) *id = gCdId;
    return 1;
}

const TbEntry *cdswap_cds(int *n, int *found, short *id)
{
    cd_scan_once();
    if (n)     *n = gCdN;
    if (found) *found = gCdFound;
    if (id)    *id = gCdId;
    return gCdCds;
}

int cdswap_find(const char *cdImage)
{
    int idx;
    cd_scan_once();
    if (!gCdFound || !cdImage || !cdImage[0]) return -1;
    idx = toolbox_find_cd(cdImage, gCdCds, gCdN);
    if (idx < 0) {                    /* maybe a disc was added to the folder since startup */
        cdswap_scan();
        idx = toolbox_find_cd(cdImage, gCdCds, gCdN);
    }
    return idx;
}

static void say(const CdSwapUI *ui, const char *m)
{
    if (ui && ui->message) ui->message(ui->ctx, m);
}

CdResult cdswap_ensure(const CatItem *it, const CdSwapUI *ui, short *cdVref)
{
    short   tbId;
    int     idx;
    short   vref;
    long    deadline, timeout;

    *cdVref = 0;

    /* 1. Fast-path: the wanted disc is already inserted. */
    if (it->cdVolume[0] && macfs_find_vol_by_name(it->cdVolume, &vref)) {
        *cdVref = vref;
        cdswap_set_active_image(it->cdImage);
        return CD_OK;
    }

    /* Locate the Toolbox CD from the session cache (scanned at startup, docs/45). */
    if (!cdswap_ready(&tbId))
        return CD_UNSUPPORTED;

    /* 2. Unmount the CD we last inserted, if it's still mounted and isn't the one we
     * want — classic Mac OS otherwise nags forever once the media changes under it. */
    if (gLastCdVol[0] && strcmp(gLastCdVol, it->cdVolume) != 0 &&
        macfs_find_vol_by_name(gLastCdVol, &vref)) {
        say(ui, "Ejecting the current disc...");
        if (macfs_unmount(vref) != noErr) return CD_UNMOUNT_BUSY;
        gLastCdVol[0] = '\0';
    }

    /* 3. Find the image this title needs in the cached listing (re-scans on a miss). */
    idx = cdswap_find(it->cdImage);
    if (idx < 0) return CD_NOT_FOUND;

    /* 4. Switch immediately — the index is only meaningful against that enumeration. */
    say(ui, "Inserting disc...");
    if (!toolbox_set_next_cd(tbId, idx))
        return CD_UNSUPPORTED;

    /* Without an expected volume name we can't verify or find the vRefNum, so this is
     * a best-effort switch (fine for an app-on-HD title that just reads the CD). */
    if (!it->cdVolume[0]) {
        cdswap_set_active_image(it->cdImage);
        return CD_OK;
    }

    /* 5. Wait for the volume to mount (abortable, timed), then verify by name. */
    say(ui, "Waiting for the disc to mount...");
    timeout  = (ui && ui->timeoutTicks > 0) ? ui->timeoutTicks : 900;   /* ~15 s */
    deadline = (long)TickCount() + timeout;
    for (;;) {
        if (macfs_find_vol_by_name(it->cdVolume, &vref)) {
            *cdVref = vref;
            strncpy(gLastCdVol, it->cdVolume, sizeof gLastCdVol - 1);
            gLastCdVol[sizeof gLastCdVol - 1] = '\0';
            cdswap_set_active_image(it->cdImage);
            return CD_OK;
        }
        if (ui && ui->wait_tick && !ui->wait_tick(ui->ctx))
            return CD_ABORTED;
        if ((long)TickCount() >= deadline)
            return CD_TIMEOUT;
    }
}
