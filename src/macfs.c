/*
 * macfs.c — see macfs.h.
 *
 * Uses the classic HFS File Manager (HOpen / HGetFInfo / HCreate by
 * volume+dirID+name, and PBGetCatInfo to resolve folders) rather than the
 * System-7 FSSpec traps (FSMakeFSSpec / FSpOpenDF are trap 0xAA52, which faults
 * on base System 6 — docs/09 Milestone 4). The HFS calls work on 6.0.8 and 7.x
 * alike, so there is a single code path. An FSSpec is still used as a plain
 * carrier struct (vRefNum/parID/name); only the trap *calls* are gone.
 */
#include "macfs.h"
#include "mac_compat.h"

#include <Files.h>
#include <Gestalt.h>
#include <Memory.h>
#include <string.h>

/* "is a directory" bit of ioFlAttrib (leaner Retro68 headers may omit it). */
#ifndef ioDirMask
#define ioDirMask 0x10
#endif

static short  gBootVRef = 0;
static Boolean gHaveBoot = false;

OSErr macfs_boot_vref(short *vref)
{
    if (!gHaveBoot) {
        short v;
        long  d, sysv = 0;
        OSErr err;
        /* FindFolder is a System-7 (Folder Manager) trap — unimplemented on base
         * System 6, where it bombs ("unimplemented trap"). On 6.x use GetVol: the
         * default volume at app startup is the boot volume. */
        (void)Gestalt(gestaltSystemVersion, &sysv);
        if (sysv >= 0x0700) {
            err = FindFolder(kOnSystemDisk, kSystemFolderType, false, &v, &d);
        } else {
            err = GetVol(0L, &v);
        }
        if (err != noErr) return err;

        /* GetVol returns the default *working directory* refNum. Under MultiFinder
         * (and any time our app's default dir isn't the volume root) that is a
         * WDRefNum, not the volume's real vRefNum. HFS catalog calls (PBGetCatInfo,
         * HOpen, HGetFInfo) tolerate a WDRefNum, so the catalog and art still load —
         * but the Process Manager's launch path needs a *real* vRefNum in the
         * FSSpec, and a WDRefNum there fails with fnfErr (-43). Normalize to the
         * real vRefNum via PBHGetVInfo (ioVolIndex 0 ⇒ look the volume up by
         * ioVRefNum, returning its real refNum). FindFolder already yields a real
         * vRefNum, so this is a harmless no-op on System 7. */
        {
            HParamBlockRec hp;
            memset(&hp, 0, sizeof hp);
            hp.volumeParam.ioNamePtr  = NULL;
            hp.volumeParam.ioVRefNum  = v;
            hp.volumeParam.ioVolIndex = 0;
            if (PBHGetVInfoSync(&hp) == noErr) v = hp.volumeParam.ioVRefNum;
        }

        gBootVRef = v;
        gHaveBoot = true;
    }
    *vref = gBootVRef;
    return noErr;
}

/* Copy a C substring of length n into a Pascal string of capacity `cap` bytes
 * (clamping so the length byte + chars never exceed the destination). */
static void cstr_to_pstr(const char *s, int n, unsigned char *out, int cap)
{
    if (n > cap - 1) n = cap - 1;
    if (n < 0) n = 0;
    out[0] = (unsigned char)n;
    if (n > 0) memcpy(out + 1, s, (size_t)n);
}

/* Resolve subfolder `name` (length n) inside directory `parent` on `vref`,
 * returning its own dirID. fnfErr if it doesn't exist; fnfErr if it's a file. */
static OSErr dir_id_of(short vref, long parent, const char *name, int n, long *out)
{
    CInfoPBRec pb;
    Str255     nm;
    OSErr      err;

    cstr_to_pstr(name, n, nm, sizeof(nm));
    memset(&pb, 0, sizeof(pb));
    pb.dirInfo.ioNamePtr  = nm;
    pb.dirInfo.ioVRefNum  = vref;
    pb.dirInfo.ioDrDirID  = parent;
    pb.dirInfo.ioFDirIndex = 0;          /* look ioNamePtr up in ioDrDirID */
    err = PBGetCatInfoSync(&pb);
    if (err != noErr) return err;
    if (!(pb.dirInfo.ioFlAttrib & ioDirMask)) return fnfErr;   /* a file, not a dir */
    *out = pb.dirInfo.ioDrDirID;
    return noErr;
}

OSErr macfs_make_spec(const char *relToRoot, FSSpec *spec)
{
    short       vref;
    long        dir;
    const char *seg, *p;
    OSErr       err = macfs_boot_vref(&vref);
    if (err != noErr) return err;

    /* The tree lives at /MacAtrium on the startup volume's root. Walk every
     * path component but the last into a parent dirID; the last is the leaf
     * (which need not exist — we only build the spec). */
    dir = fsRtDirID;
    err = dir_id_of(vref, dir, "MacAtrium", 9, &dir);
    if (err != noErr) return err;

    for (seg = relToRoot; ; ) {
        for (p = seg; *p && *p != '/'; p++) {}
        if (*p == '\0') {                         /* leaf component */
            spec->vRefNum = vref;
            spec->parID   = dir;
            cstr_to_pstr(seg, (int)(p - seg), spec->name, sizeof(spec->name));
            return noErr;
        }
        err = dir_id_of(vref, dir, seg, (int)(p - seg), &dir);
        if (err != noErr) return err;             /* a parent folder is missing */
        seg = p + 1;
    }
}

OSErr macfs_open_df(const FSSpec *spec, char perm, short *refNum)
{
    /* HOpen opens the data fork by (vRefNum, dirID, name) — pre-FSSpec, works on
     * System 6. Our filenames never collide with driver names, so it's safe. */
    return HOpen(spec->vRefNum, spec->parID, (ConstStr255Param)spec->name, perm, refNum);
}

OSErr macfs_get_finfo(const FSSpec *spec, FInfo *info)
{
    return HGetFInfo(spec->vRefNum, spec->parID, (ConstStr255Param)spec->name, info);
}

OSErr macfs_create(const FSSpec *spec, OSType creator, OSType type)
{
    return HCreate(spec->vRefNum, spec->parID, (ConstStr255Param)spec->name, creator, type);
}

OSErr macfs_read_all(FSSpec *spec, char **buf, long *len)
{
    short refNum;
    long  eof;
    OSErr err;
    char *p;

    *buf = 0;
    *len = 0;

    err = macfs_open_df(spec, fsRdPerm, &refNum);
    if (err != noErr) return err;

    err = GetEOF(refNum, &eof);
    if (err != noErr) { FSClose(refNum); return err; }

    p = (char *)NewPtr(eof + 1);
    if (!p) { FSClose(refNum); return memFullErr; }

    {
        long count = eof;
        err = FSRead(refNum, &count, p);          /* eofErr means we got it all */
        if (err == eofErr) err = noErr;
        if (err != noErr) { DisposePtr(p); FSClose(refNum); return err; }
        p[count] = '\0';
        *len = count;
    }

    FSClose(refNum);
    *buf = p;
    return noErr;
}
