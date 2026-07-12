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

#endif /* MACATRIUM_CDSWAP_H */
