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

void env_os_name(long v, char *out)
{
    int         major = (int)((v >> 8) & 0xFF);   /* BCD high byte: 6/7/8/9 */
    int         minor = (int)((v >> 4) & 0x0F);
    int         bug   = (int)(v & 0x0F);
    char       *p = out;
    const char *s = "System ";

    while (*s) *p++ = *s++;
    if (v <= 0) { s = "(unknown)"; while (*s) *p++ = *s++; *p = '\0'; return; }
    if (major >= 10) *p++ = (char)('0' + major / 10);
    *p++ = (char)('0' + major % 10);
    *p++ = '.';
    *p++ = (char)('0' + minor);
    if (bug) { *p++ = '.'; *p++ = (char)('0' + bug); }
    *p = '\0';
}
