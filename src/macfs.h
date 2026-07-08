/*
 * macfs.h — locate and read files under the on-volume /MacAtrium root.
 *
 * Paths are given relative to /MacAtrium using '/' separators (exactly as the
 * catalog stores them, e.g. "metadata/catalog.jsonl" or
 * "Apps/Prince of Persia/Prince of Persia"). We resolve them on the *startup*
 * volume's root, so the tree is relocatable (docs/06).
 */
#ifndef MACATRIUM_MACFS_H
#define MACATRIUM_MACFS_H

#include <Files.h>

/* vRefNum of the startup volume (cached after first call). */
OSErr macfs_boot_vref(short *vref);

/* Build an FSSpec for a /MacAtrium-relative path. The leaf need not exist
 * (noErr or fnfErr both yield a usable spec); other errors are real. */
OSErr macfs_make_spec(const char *relToRoot, FSSpec *spec);

/* ---- multi-disk libraries (docs/37) ------------------------------------------
 * A mounted HFS volume that carries its own self-contained /MacAtrium library.
 * The boot volume is always entry 0; additional fixed SCSI disks follow in mount
 * order. `stableId` (from metadata/volume.jsonl) is 0 until Phase 4 stamps it. */
#define VOL_MAX       6      /* max library disks aggregated at once            */
#define VOL_NAME_MAX  27     /* HFS volume-name chars (Pascal string, +len byte) */

typedef struct {
    short          vref;                    /* real vRefNum (launch-safe)        */
    unsigned char  name[VOL_NAME_MAX + 1];  /* HFS volume name (Pascal string)   */
    unsigned long  crDate;                  /* ioVCrDate — rename-proof identity */
    long           stableId;                /* metadata/volume.jsonl; 0 if none  */
} VolEntry;

typedef struct {
    VolEntry v[VOL_MAX];
    int      n;                             /* boot volume is v[0]               */
} VolTable;

/* Build an FSSpec for a /MacAtrium-relative path on a SPECIFIC volume. The
 * boot-only macfs_make_spec is a wrapper passing the startup volume's vRefNum. */
OSErr macfs_make_spec_on(short vref, const char *relToRoot, FSSpec *spec);

/* Enumerate mounted volumes carrying a /MacAtrium/metadata library into `out`
 * (boot volume first). Returns the count (0 if even the boot volume has none). */
int macfs_volumes(VolTable *out);

/* Read an entire file into a freshly malloc'd, NUL-terminated buffer.
 * Caller frees *buf. *len excludes the terminator. */
OSErr macfs_read_all(FSSpec *spec, char **buf, long *len);

/* Read a file's data fork from byte `skip` to EOF straight into a fresh
 * relocatable Handle (caller DisposeHandles it). Avoids the read-all-then-copy
 * staging buffer, halving the peak memory of loading a PICT (we skip its
 * 512-byte file header). `*len` is the bytes read. eofErr if the file is no
 * longer than `skip`. */
OSErr macfs_read_handle(const FSSpec *spec, long skip, Handle *out, long *len);

/* HFS File-Manager helpers that work on System 6.0.8 and 7.x alike (no FSSpec
 * traps): open the data fork, read Finder info, create a file — all by the
 * spec's (vRefNum, parID, name). Use these instead of FSpOpenDF / FSpGetFInfo /
 * FSpCreate so the binary runs on base System 6 (docs/09 Milestone 4). */
OSErr macfs_open_df(const FSSpec *spec, char perm, short *refNum);
OSErr macfs_get_finfo(const FSSpec *spec, FInfo *info);
OSErr macfs_create(const FSSpec *spec, OSType creator, OSType type);

#endif /* MACATRIUM_MACFS_H */
