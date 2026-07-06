/*
 * bless.h — enumerate the boot volume's System Folders and switch which one the
 * ROM boots (docs/36/37 Phase 2, the OS chooser).
 *
 * A "blessable" folder is one holding a `System` file (type 'zsys'/'ZSYS'). The
 * blessed folder is the volume's `ioVFndrInfo[0]` (its dir ID) — the field
 * `rb-cli bless` writes, validated offline: re-blessing HD20SC from System 7.1.2
 * (CNID 27) to System 6.0.8 (CNID 18) makes the ROM boot System 6.
 */
#ifndef MACATRIUM_BLESS_H
#define MACATRIUM_BLESS_H

#include <Files.h>

#define BLESS_MAX_SYS 16

typedef struct {
    long  dirID;      /* the System Folder's dir ID (the bless target) */
    Str63 name;       /* folder name (Pascal string) for display */
    int   blessed;    /* 1 = currently the blessed (bootable) folder */
} SysFolder;

/* Enumerate blessable System Folders on the boot volume's root into out[max],
 * flagging the currently-blessed one. Returns the count (0 on error). */
int bless_enumerate(SysFolder *out, int max);

/* Set the boot volume's blessed System Folder to `dirID` and flush to disk.
 * (PBHSetVInfo ioVFndrInfo[0] — the drFndrInfo blessed-folder dir ID.) */
OSErr bless_set(long dirID);

/* Bless `dirID`, then restart into it. Returns only if the bless step failed. */
OSErr bless_and_restart(long dirID);

#endif /* MACATRIUM_BLESS_H */
