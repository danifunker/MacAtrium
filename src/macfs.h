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
