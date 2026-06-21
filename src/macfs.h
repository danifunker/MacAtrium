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

#endif /* MACATRIUM_MACFS_H */
