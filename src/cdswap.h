/*
 * cdswap.h — orchestrate "insert the right CD before launch" for a CD title
 * (docs/45): probe the BlueSCSI Toolbox, switch to the image the title needs, wait
 * for its volume to mount, verify it. Uses toolbox.c (SCSI transport) + macfs.c
 * (volume scan / unmount). Target-only (Toolbox + File Manager); the caller supplies
 * UI hooks so this stays free of window/event code.
 */
#ifndef MACATRIUM_CDSWAP_H
#define MACATRIUM_CDSWAP_H

#include "catalog.h"
#include "toolbox.h"   /* TbEntry, for the cached CD listing */

typedef enum {
    CD_OK = 0,        /* the required volume is mounted (and verified by name)  */
    CD_UNSUPPORTED,   /* no Toolbox CD device answered (feature inactive)       */
    CD_NOT_FOUND,     /* the image isn't in the host's CD folder                */
    CD_UNMOUNT_BUSY,  /* the current CD volume has open files — can't switch     */
    CD_TIMEOUT,       /* the new volume didn't mount within the timeout          */
    CD_ABORTED        /* the user cancelled the wait                             */
} CdResult;

/* UI hooks + tunables, so cdswap_ensure needs no window/event/Prefs knowledge. */
typedef struct {
    void (*message)(void *ctx, const char *msg);  /* show a transient status line    */
    int  (*wait_tick)(void *ctx);                 /* pump one event; 0 = user abort   */
    void  *ctx;
    long   timeoutTicks;                          /* mount-wait cap (TickCount units) */
    int    pinId;                                 /* forced Toolbox SCSI id, or -1    */
} CdSwapUI;

/* Ensure the disc for `it` (it->cdImage) is inserted. On CD_OK, *cdVref receives the
 * mounted CD volume's vRefNum (needed for a run-from-CD launch). Non-OK results carry
 * the reason; the caller decides whether to still launch (per it->cdRequired). */
CdResult cdswap_ensure(const CatItem *it, const CdSwapUI *ui, short *cdVref);

/* The image filename of the CD currently inserted this session ("" if none) — for
 * the CD Library browser's "active" marker. */
const char *cdswap_active_image(void);

/* Record an image as the active disc (the CD Library browser sets this after a
 * manual insert). */
void        cdswap_set_active_image(const char *image);

/* ---- session CD-image cache (docs/45) ------------------------------------------
 * Scan the host CD listing once (probe + LIST CDS) and keep it in RAM, so title
 * launches don't re-walk the SCSI bus each time. The scan is LAZY — it runs on the
 * first cache use (a CD-title launch or the CD Library open), NOT at startup, since
 * probing the SCSI bus for a Toolbox CD is slow (per-id selection timeouts) and
 * would tax boot on every machine, including those with no CD device. Call
 * cdswap_scan() directly only to force a refresh (e.g. on opening the CD Library);
 * it's safe when no CD device answers. */
void cdswap_scan(void);

/* 1 (and *id when non-NULL) if a Toolbox CD-ROM was found this session, else 0.
 * Scans on first use if cdswap_scan() hasn't run yet. */
int  cdswap_ready(short *id);

/* The cached listing for the CD Library browser: entries, with *n = count, *found =
 * a CD device answered, *id = its SCSI id. Any out-param may be NULL. Scans on first
 * use. The returned pointer is owned by cdswap (valid until the next scan). */
const TbEntry *cdswap_cds(int *n, int *found, short *id);

/* Find `cdImage` in the cached listing (fuzzy, via toolbox_find_cd): returns the
 * SET NEXT CD index, or -1 if absent / no CD device. Re-scans once on a miss, in
 * case a disc appeared in the folder since startup. */
int  cdswap_find(const char *cdImage);

#endif /* MACATRIUM_CDSWAP_H */
