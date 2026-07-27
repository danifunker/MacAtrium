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

/* Build an FSSpec for a path relative to a volume's ROOT (no /MacAtrium descent) —
 * for run-from-CD apps whose path is relative to the mounted CD volume (docs/45). */
OSErr macfs_make_spec_root(short vref, const char *relToVolRoot, FSSpec *spec);

/* Find a mounted volume by HFS name (case-insensitive, as HFS matches). Returns 1
 * and writes *vref on a match, 0 otherwise. Used to detect a CD volume (docs/45). */
int   macfs_find_vol_by_name(const char *name, short *vref);

/* Unmount a mounted volume (PBUnmountVol). Returns the OSErr — fBsyErr (-47) when
 * files are open on it. 6.0.8-safe. Used to swap the Toolbox CD (docs/45). */
OSErr macfs_unmount(short vref);

/* Finder "Put Away" for a removable volume: EJECT the media through its driver
 * (PBEject), then unmount the offline shell (PBUnmountVol). The eject is the
 * load-bearing half for CD swapping: the AppleCD driver only polls for insertion
 * while it believes the drive is EMPTY, and PBUnmountVol alone never tells the
 * drive — so a Toolbox SET NEXT CD after a bare unmount is invisible forever
 * (2026-07-27, MiSTer HW). Returns the PBEject OSErr (fBsyErr when files are
 * open); the follow-up unmount is best-effort. */
OSErr macfs_eject_unmount(short vref);

/* Eject the physical CD DRIVE even when no volume is mounted from it — an audio
 * CD, or a disc HFS never mounted, leaves macfs_find_cd_vol empty-handed but the
 * AppleCD driver just as asleep, so a Toolbox swap over it is invisible without
 * this (2026-07-27, MiSTer HW). PBEject by drive number (cached from the last
 * mounted CD volume, else resolved by walking the drive queue for the ".AppleCD"
 * driver), with the driver's eject control (csCode 7) as the second chance. On a
 * truly empty drive it's a harmless no-op/error — callers treat it as advisory. */
OSErr macfs_eject_cd_drive(void);

/* Find a mounted CD-ROM volume (hardware-locked / write-protected media). Returns
 * 1 and writes *vref on the first match, 0 if none is mounted. Lets the CD Library
 * drop the outgoing disc before a Toolbox swap so Mac OS doesn't nag (docs/45). */
int macfs_find_cd_vol(short *vref);

/* Like macfs_find_cd_vol, but also copy the mounted CD-ROM volume's HFS name into
 * `name` (a C string, NUL-terminated, up to `cap` bytes) — the CD Library reverse-
 * maps it to the host image so the "(in drive)" marker names the disc actually in
 * the drive, even after a reboot (docs/45, cdidx.h). `name` may be NULL (then this
 * is exactly macfs_find_cd_vol). */
int macfs_find_cd_vol_named(short *vref, char *name, int cap);

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

/* Write side, for moving files to and from the SD card (docs/46). `macfs_open_rf`
 * opens the resource fork as a byte STREAM (HOpenRF, not FSpOpenResFile — we copy
 * bytes, we do not read a resource map); `macfs_set_finfo` restores type/creator,
 * without which a correctly copied application is an unopenable document to the
 * Finder; `macfs_mkdir` creates a /MacAtrium-relative folder (already-exists is
 * success). All System 6-safe. */
OSErr macfs_open_rf(const FSSpec *spec, char perm, short *refNum);
OSErr macfs_set_finfo(const FSSpec *spec, const FInfo *info);
OSErr macfs_mkdir(const char *relToRoot);

/* Commit a volume's buffered catalog + data. Closing a fork is NOT enough: classic
 * Mac OS holds writes until a clean unmount, so a copied file can vanish on a reset
 * while every call reported success. Call after finishing a write (docs/46). */
OSErr macfs_flush_vol(short vref);

#endif /* MACATRIUM_MACFS_H */
