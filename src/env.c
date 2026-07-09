/*
 * env.c — see env.h. Constants confirmed against Apple's Universal Interfaces.
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

    /* Appearance Manager: built into Mac OS 8+, optional 7.x extension. Gates the
     * true-Platinum sys8 control look (docs/36 Phase 3). */
    e->hasAppearanceMgr = 0;
    if (Gestalt(gestaltAppearanceAttr, &v) == noErr)
        e->hasAppearanceMgr = (v & (1L << gestaltAppearanceExists)) != 0;

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

    /* CPU→OS-compatibility tier (docs/40). OS support clusters by CPU/ROM, not by
     * model; detect the tier from the *native* CPU (correct even under the PPC 68k
     * emulator, where gestaltProcessorType reports the emulated 68LC040) and derive
     * the highest System this Mac can boot. The 5-row ceiling table is baked from
     * data/os-tiers.json (`maxBcd` per tier) — keep the two in sync. */
    {
        static const long kTierMaxBcd[] = { 0x0755, 0x0761, 0x0810, 0x0910, 0x0922 };
        long arch = 0, cpu = 0;
        if (Gestalt(gestaltSysArchitecture, &arch) == noErr && arch == gestaltPowerPC) {
            Gestalt(gestaltNativeCPUtype, &cpu);   /* real PPC chip, even under emulation */
            e->tier = (cpu == gestaltCPU750 || cpu >= gestaltCPUG4)
                      ? TIER_PPC_NEWWORLD : TIER_PPC_OLDWORLD;
        } else {                                   /* 68k: prefer 'cput', fall back to 'proc' */
            int is030, is040;
            if (Gestalt(gestaltNativeCPUtype, &cpu) == noErr) {   /* cput: 030=3, 040=4 */
                is030 = (cpu == gestaltCPU68030); is040 = (cpu == gestaltCPU68040);
            } else {
                Gestalt(gestaltProcessorType, &cpu);              /* proc: 030=4, 040=5 */
                is030 = (cpu == gestalt68030);    is040 = (cpu == gestalt68040);
            }
            e->tier = is040 ? TIER_68040 : is030 ? TIER_68030 : TIER_68K_EARLY;
        }
        e->maxOSbcd = kTierMaxBcd[e->tier];
    }
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

void env_os_version(long v, char *out)
{
    int   major = (int)((v >> 8) & 0xFF);
    int   minor = (int)((v >> 4) & 0x0F);
    int   bug   = (int)(v & 0x0F);
    char *p = out;

    if (v <= 0) { *p++ = '?'; *p = '\0'; return; }
    if (major >= 10) *p++ = (char)('0' + major / 10);
    *p++ = (char)('0' + major % 10);
    *p++ = '.';
    *p++ = (char)('0' + minor);
    if (bug) { *p++ = '.'; *p++ = (char)('0' + bug); }
    *p = '\0';
}
