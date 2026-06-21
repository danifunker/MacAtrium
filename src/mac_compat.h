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
