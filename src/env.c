/*
 * env.c — see env.h. Constants confirmed in docs/11-derisk-log.md §A.
 */
#include "env.h"
#include "mac_compat.h"

#include <Gestalt.h>
#include <LowMem.h>

void env_probe(Env *e)
{
    long v;

    e->sysVers = (Gestalt(gestaltSystemVersion, &v) == noErr) ? v : 0;
    e->qdVers  = (Gestalt(gestaltQuickdrawVersion, &v) == noErr) ? v : gestaltOriginalQD;

    e->hasColorQD = (e->qdVers >= gestalt8BitQD);

    /* Resident-launch capability (the keystone guard). */
    e->canLaunchReturn = 0;
    if (Gestalt(gestaltOSAttr, &v) == noErr)
        e->canLaunchReturn = (v & (1L << gestaltLaunchCanReturn)) != 0;

    /* Shutdown Manager: present on all our targets (System 7.x); the trap is
     * implemented since the Mac II. Treat as available. */
    e->hasShutdown = 1;

    /* Screen bounds are valid on every machine via the QD globals. */
    e->screen     = qd.screenBits.bounds;
    e->mbarHeight = LMGetMBarHeight();

    /* Depth: only meaningful (and GetMainDevice only valid) under Color QD. */
    e->pixelSize = 1;
    if (e->hasColorQD) {
        GDHandle gd = GetMainDevice();
        if (gd) {
            PixMapHandle pm = (**gd).gdPMap;
            if (pm) e->pixelSize = (**pm).pixelSize;
        }
    }

    /* Backend: color only when Color QD is present AND the screen actually has
     * colour depth; otherwise the B&W path (also the graceful fallback). */
    e->useColor = (e->hasColorQD && e->pixelSize >= 4);
}
