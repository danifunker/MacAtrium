/*
 * env.h — startup environment probe (docs/03, docs/08). Everything OS-version-
 * or depth-specific is detected once here and passed down; no other module
 * re-probes Gestalt.
 */
#ifndef MACATRIUM_ENV_H
#define MACATRIUM_ENV_H

#ifdef MACATRIUM_HOST_TEST
/* Host unit tests (tests/) have no Toolbox: supply the only Toolbox type the Env
 * struct uses (Rect) so the pure compat.c can be exercised off-target. The real
 * 68k build takes the Quickdraw definition. */
typedef struct { short top, left, bottom, right; } Rect;
#else
#include <Quickdraw.h>
#endif

#include "cpu.h"   /* CPU_* generations — env_probe fills `cpuGen` from this table */

/* CPU→OS compatibility tiers (docs/40 / data/os-tiers.json). The highest System
 * a Mac can boot is a function of its CPU/ROM generation, not the model; the tier
 * is DERIVED from the probed `cpuGen` (68000+68020 share a tier — same OS ceiling)
 * and carries the OS range. Per-title CPU requirements use `cpuGen`, not this. */
enum {
    TIER_68K_EARLY = 0,   /* 68000 / 68020 → max System 7.5.5 */
    TIER_68030,           /* 68030         → max System 7.6.1 */
    TIER_68040,           /* 68040 / LC040 → max Mac OS 8.1   */
    TIER_PPC_OLDWORLD,    /* 601/603/604   → max Mac OS 9.1   */
    TIER_PPC_NEWWORLD     /* G3 / G4       → max Mac OS 9.2.2 */
};

typedef struct {
    long  sysVers;          /* gestaltSystemVersion (BCD, e.g. 0x0755)   */
    long  qdVers;           /* gestaltQuickdrawVersion                   */
    int   hasColorQD;       /* Color QuickDraw present                   */
    int   pixelSize;        /* current main-device depth in bits         */
    int   useColor;         /* chosen render backend: 1 = color, 0 = B&W */
    int   canLaunchReturn;  /* gestaltLaunchCanReturn — resident launch  */
    int   hasShutdown;      /* Shutdown Manager available                */
    int   hasAppearanceMgr; /* Appearance Manager present (Platinum on 8+) */
    int   cpuGen;           /* CPU generation (CPU_* in cpu.h) — the table the
                             * per-title minCPU/maxCPU facets also index    */
    int   tier;             /* CPU→OS tier (TIER_*), derived from cpuGen  */
    long  maxOSbcd;         /* highest bootable System for this Mac (BCD) */
    long  minOSbcd;         /* lowest bootable System for this Mac (BCD): the CPU
                             * tier floor, refined up by the per-model table    */
    /* Hardware facets for the per-title compatibility gate (docs/40). */
    int   hasFPU;           /* hardware FPU present (gestaltFPUType != NoFPU) */
    long  ramKB;            /* physical machine RAM in KB (gestaltPhysicalRAMSize) */
    long  machineID;        /* gestaltMachineType (box/model ID); 0 if unknown */
    int   maxScreenDepth;   /* deepest bpp the main screen supports (1 if B&W); the
                             * per-title minDepth reachability test reads this      */
    Rect  screen;           /* full main-screen bounds (global coords)   */
    short mbarHeight;       /* menu-bar height                           */
} Env;

void env_probe(Env *e);

/* Format a Gestalt gestaltSystemVersion value (e.g. 0x0755) as a human string
 * like "System 7.5.5" into `out` (>= 24 bytes); a trailing ".0" bugfix is dropped,
 * and v <= 0 yields "System (unknown)". */
void env_os_name(long sysVers, char *out);

/* Format just the version digits of a gestaltSystemVersion (e.g. "7.1.1") into
 * `out` (>= 12 bytes); "?" when unknown. For "MacOS Version: X"-style labels. */
void env_os_version(long sysVers, char *out);

#endif /* MACATRIUM_ENV_H */
