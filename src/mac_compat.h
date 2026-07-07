/*
 * mac_compat.h — constants Retro68's (leaner) multiversal headers don't define,
 * with the exact values confirmed from Apple's Universal Interfaces in
 * docs/11-derisk-log.md. Plus the classic keyboard char codes the UI uses.
 *
 * Toolbox-only; not part of the host-testable core.
 */
#ifndef MACATRIUM_MAC_COMPAT_H
#define MACATRIUM_MAC_COMPAT_H

/* Process Manager (Processes.h) */
#ifndef launchNoFileFlags
#define launchNoFileFlags 0x0800   /* we resolve the FSSpec ourselves */
#endif

/* Gestalt OS-attr bits (GestaltEqu.h) — tested as (1L << bit) of gestaltOSAttr */
#ifndef gestaltLaunchCanReturn
#define gestaltLaunchCanReturn 1
#endif
#ifndef gestaltLaunchControl
#define gestaltLaunchControl 3
#endif

/* Gestalt CPU / architecture selectors + response codes (GestaltEqu.h) for the
 * OS-compatibility tier probe in env.c (docs/40). Guarded — Retro68's leaner
 * multiversal headers define only some. Values from Apple's Gestalt.h. The tier
 * is detected from the *native* CPU so it is correct even under the PowerPC 68k
 * emulator (gestaltProcessorType would report the emulated 68LC040). */
#ifndef gestaltSysArchitecture
#define gestaltSysArchitecture 'sysa'
#endif
#ifndef gestalt68k
#define gestalt68k 1
#endif
#ifndef gestaltPowerPC
#define gestaltPowerPC 2
#endif
#ifndef gestaltNativeCPUtype
#define gestaltNativeCPUtype 'cput'
#endif
#ifndef gestaltProcessorType
#define gestaltProcessorType 'proc'
#endif
/* gestaltProcessorType ('proc') responses */
#ifndef gestalt68030
#define gestalt68030 4
#endif
#ifndef gestalt68040
#define gestalt68040 5
#endif
/* gestaltNativeCPUtype ('cput') responses (68k = small ints; PPC = 0x01xx) */
#ifndef gestaltCPU68030
#define gestaltCPU68030 3
#endif
#ifndef gestaltCPU68040
#define gestaltCPU68040 4
#endif
#ifndef gestaltCPU750
#define gestaltCPU750 0x108     /* PowerPC G3 */
#endif
#ifndef gestaltCPUG4
#define gestaltCPUG4 0x10C      /* PowerPC G4 (and later sort >= this) */
#endif

/* Folders.h — FindFolder volume selector + HFS root dir id */
#ifndef kOnSystemDisk
#define kOnSystemDisk ((short)0x8000)   /* the startup disk */
#endif
#ifndef fsRtDirID
#define fsRtDirID 2L
#endif

/* Classic Mac keyboard character codes (the low byte of EventRecord.message) */
#define kCharEnter      0x03
#define kCharBackspace  0x08
#define kCharTab        0x09
#define kCharReturn     0x0D
#define kCharEscape     0x1B
#define kCharLeft       0x1C
#define kCharRight      0x1D
#define kCharUp         0x1E
#define kCharDown       0x1F
#define kCharSpace      0x20

#endif /* MACATRIUM_MAC_COMPAT_H */
