/*
 * env.c — see env.h. Constants confirmed against Apple's Universal Interfaces.
 */
#include "env.h"
#include "mac_compat.h"
#include "display.h"

#include <Gestalt.h>
#include <LowMem.h>

/* Per-MODEL OS-floor refinement (docs/40). Some 68020/68030 Macs need a System
 * NEWER than their CPU tier's floor — a Color Classic / LC III / Mac IIvx boots
 * 7.1, not 6.0.x — so the tier floor alone under-greys the chooser for them.
 * Every machine (by gestaltMachineType) whose minimum System exceeds its tier
 * floor is listed here; env_probe raises minOSbcd to it. Baked from
 * data/models.jsonl (68K models with minSystem >= 7.1, collapsed per gestaltID to
 * the most permissive floor — board-family IDs are shared). The 68040 rows are
 * redundant with the tier floor (also 7.1) but harmless. Regenerate when
 * models.jsonl changes; New-World Macs report a generic id and stay on the tier. */
typedef struct { short id; short bcd; } ModelMinOS;
static const ModelMinOS kModelMinOS[] = {
    {  27, 0x0710 }, {  29, 0x0710 }, {  30, 0x0710 }, {  32, 0x0710 }, {  33, 0x0710 },
    {  34, 0x0710 }, {  35, 0x0710 }, {  36, 0x0710 }, {  38, 0x0710 }, {  44, 0x0710 },
    {  45, 0x0710 }, {  48, 0x0710 }, {  49, 0x0710 }, {  50, 0x0710 }, {  52, 0x0710 },
    {  53, 0x0710 }, {  56, 0x0710 }, {  60, 0x0710 }, {  62, 0x0710 }, {  71, 0x0710 },
    {  72, 0x0711 }, {  77, 0x0710 }, {  78, 0x0710 }, {  80, 0x0710 }, {  83, 0x0710 },
    {  84, 0x0710 }, {  85, 0x0711 }, {  89, 0x0710 }, {  92, 0x0710 }, {  94, 0x0710 },
    {  98, 0x0710 }, {  99, 0x0710 }, { 102, 0x0710 }, { 103, 0x0710 }, { 115, 0x0711 },
};

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

    /* Deepest bpp the main screen supports — the per-title minDepth reachability
     * test (compat.c) reads this: a title needing 8-bit is flagged on a Mac that
     * tops out at 1-bit. 1 when there is no Color QD. */
    e->maxScreenDepth = 1;
    if (e->hasColorQD) {
        short list[8];
        int   nd = display_depths(list, 8), k;
        for (k = 0; k < nd; k++) if (list[k] > e->maxScreenDepth) e->maxScreenDepth = list[k];
    }

    /* CPU→OS-compatibility tier (docs/40). OS support clusters by CPU/ROM, not by
     * model; detect the tier from the *native* CPU (correct even under the PPC 68k
     * emulator, where gestaltProcessorType reports the emulated 68LC040) and derive
     * the highest System this Mac can boot. The 5-row ceiling table is baked from
     * data/os-tiers.json (`maxBcd` per tier) — keep the two in sync. */
    {
        static const long kTierMaxBcd[] = { 0x0755, 0x0761, 0x0810, 0x0910, 0x0922 };
        /* Per-tier OS FLOOR (min bootable System), baked from data/os-tiers.json
         * `minOS`: 68000/020 and 030 reach back to the 6.0.4 envelope floor, but a
         * 68040 needs >= 7.1 and PowerPC >= 7.1.2 / 8.1 — so the chooser can grey a
         * System too OLD for this Mac, not only too new. Refined per-model below. */
        static const long kTierMinBcd[] = { 0x0604, 0x0604, 0x0710, 0x0712, 0x0810 };
        /* CPU generation (cpu.h) -> OS tier. 68000 and 68020 collapse to one tier
         * (both top out at 7.5.5) — that lumping is right for the OS chooser but
         * wrong for per-title CPU gating, which is why the finer generation is kept
         * in e->cpuGen and the tier is derived from it rather than probed directly. */
        static const int kGenToTier[CPU_GEN_COUNT] = {
            TIER_68K_EARLY,                                     /* CPU_GEN_NONE (unused) */
            TIER_68K_EARLY, TIER_68K_EARLY,                     /* 68000, 68020 */
            TIER_68030, TIER_68040,                             /* 68030, 68040 */
            TIER_PPC_OLDWORLD, TIER_PPC_OLDWORLD, TIER_PPC_OLDWORLD,   /* 601, 603, 604 */
            TIER_PPC_NEWWORLD, TIER_PPC_NEWWORLD                /* G3, G4 */
        };
        long arch = 0, cpu = 0;
        e->cpuGen = CPU_68000;                     /* conservative default */
        if (Gestalt(gestaltSysArchitecture, &arch) == noErr && arch == gestaltPowerPC) {
            Gestalt(gestaltNativeCPUtype, &cpu);   /* real PPC chip, even under emulation */
            if      (cpu >= gestaltCPUG4)        e->cpuGen = CPU_PPC_G4;
            else if (cpu == gestaltCPU750)       e->cpuGen = CPU_PPC_G3;
            else if (cpu == gestaltCPU604 || cpu == gestaltCPU604e ||
                     cpu == gestaltCPU604ev)     e->cpuGen = CPU_PPC_604;
            else if (cpu == gestaltCPU603 || cpu == gestaltCPU603e ||
                     cpu == gestaltCPU603ev)     e->cpuGen = CPU_PPC_603;
            else                                 e->cpuGen = CPU_PPC_601;
        } else {                                   /* 68k: prefer 'cput', fall back to 'proc' */
            if (Gestalt(gestaltNativeCPUtype, &cpu) == noErr) {   /* cput: 020=2, 030=3, 040=4 */
                e->cpuGen = (cpu == gestaltCPU68040) ? CPU_68040 :
                            (cpu == gestaltCPU68030) ? CPU_68030 :
                            (cpu == gestaltCPU68020) ? CPU_68020 : CPU_68000;
            } else {
                Gestalt(gestaltProcessorType, &cpu);              /* proc: 020=3, 030=4, 040=5 */
                e->cpuGen = (cpu == gestalt68040) ? CPU_68040 :
                            (cpu == gestalt68030) ? CPU_68030 :
                            (cpu == gestalt68020) ? CPU_68020 : CPU_68000;
            }
        }
        e->tier     = kGenToTier[e->cpuGen];
        e->maxOSbcd = kTierMaxBcd[e->tier];
        e->minOSbcd = kTierMinBcd[e->tier];
    }

    /* Hardware facets for the per-title compatibility gate (docs/40): a game
     * needing more than this Mac (an FPU, more RAM) is flagged before launch. The
     * machine (box) ID refines the OS floor per-model in main.c. */
    e->hasFPU = 0;
    if (Gestalt(gestaltFPUType, &v) == noErr) e->hasFPU = (v != gestaltNoFPU);

    e->ramKB = 0;
    if (Gestalt(gestaltPhysicalRAMSize, &v) == noErr) e->ramKB = v / 1024L;
    else if (Gestalt(gestaltLogicalRAMSize, &v) == noErr) e->ramKB = v / 1024L;

    e->machineID = 0;
    (void)Gestalt(gestaltMachineType, &e->machineID);

    /* Refine the OS floor for models that need a System newer than their tier's
     * floor (a 68030 Color Classic needs 7.1, not 6.0.x). Shared board IDs were
     * collapsed to the most permissive floor, so this never over-greys. */
    if (e->machineID > 0) {
        unsigned i;
        for (i = 0; i < sizeof kModelMinOS / sizeof kModelMinOS[0]; i++)
            if (kModelMinOS[i].id == e->machineID) {
                if ((long)kModelMinOS[i].bcd > e->minOSbcd) e->minOSbcd = kModelMinOS[i].bcd;
                break;
            }
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
