/*
 * bless.c — see bless.h. Uses the HFS File Manager (PBGetCatInfo / PBHGetVInfo /
 * PBHSetVInfo by vRefNum+dirID), the same 6.0.8-and-7.x-safe path as macfs.c.
 */
#include "bless.h"
#include "macfs.h"     /* macfs_boot_vref */
#include "sysctl.h"    /* sysctl_restart (ShutDwnStart) */
#include "mac_compat.h"

#include <Files.h>
#include <Memory.h>
#include <string.h>

/* "is a directory" bit of ioFlAttrib (leaner Retro68 headers may omit it). */
#ifndef ioDirMask
#define ioDirMask 0x10
#endif

/* True if folder `dirID` on `vref` holds a `System` file (type 'zsys'/'ZSYS') —
 * the discriminator the OS itself uses for a System Folder. One catalog lookup
 * by name (the System file is named "System" on every target); "System Picker"
 * and plain app folders lack it and are correctly rejected. */
static int folder_has_system(short vref, long dirID)
{
    CInfoPBRec pb;
    Str63      nm;
    OSType     t;

    BlockMoveData("\pSystem", nm, 7);          /* Pascal "System" (len 6 + 6 chars) */
    memset(&pb, 0, sizeof pb);
    pb.hFileInfo.ioNamePtr   = nm;
    pb.hFileInfo.ioVRefNum   = vref;
    pb.hFileInfo.ioDirID     = dirID;
    pb.hFileInfo.ioFDirIndex = 0;              /* look ioNamePtr up in ioDirID */
    if (PBGetCatInfoSync(&pb) != noErr) return 0;
    if (pb.hFileInfo.ioFlAttrib & ioDirMask) return 0;   /* a folder named "System" */
    t = pb.hFileInfo.ioFlFndrInfo.fdType;
    return (t == 'zsys' || t == 'ZSYS');
}

/* Read the boot volume's currently-blessed folder dir ID (ioVFndrInfo[0]). */
static long blessed_dir(short vref)
{
    HParamBlockRec hp;
    memset(&hp, 0, sizeof hp);
    hp.volumeParam.ioNamePtr  = NULL;
    hp.volumeParam.ioVRefNum  = vref;
    hp.volumeParam.ioVolIndex = 0;             /* look the volume up by ioVRefNum */
    if (PBHGetVInfoSync(&hp) != noErr) return 0;
    return hp.volumeParam.ioVFndrInfo[0];
}

int bless_enumerate(SysFolder *out, int max)
{
    short vref;
    long  blessed;
    int   n = 0;
    short i;

    if (max <= 0) return 0;
    if (macfs_boot_vref(&vref) != noErr) return 0;
    blessed = blessed_dir(vref);

    /* Walk the volume root; a subfolder holding a System file is a blessable
     * System Folder. Reset the parent dir ID each pass — PBGetCatInfo overwrites
     * ioDrDirID with the found item's dir ID for a directory. */
    for (i = 1; n < max && i < 512; i++) {
        CInfoPBRec pb;
        Str63      nm;

        nm[0] = 0;
        memset(&pb, 0, sizeof pb);
        pb.dirInfo.ioNamePtr   = nm;
        pb.dirInfo.ioVRefNum   = vref;
        pb.dirInfo.ioDrDirID   = fsRtDirID;
        pb.dirInfo.ioFDirIndex = i;
        if (PBGetCatInfoSync(&pb) != noErr) break;          /* past the last entry */
        if (!(pb.dirInfo.ioFlAttrib & ioDirMask)) continue;  /* a file, not a folder */
        if (!folder_has_system(vref, pb.dirInfo.ioDrDirID)) continue;

        out[n].dirID   = pb.dirInfo.ioDrDirID;
        BlockMoveData(nm, out[n].name, (long)nm[0] + 1);
        out[n].blessed = (pb.dirInfo.ioDrDirID == blessed);
        n++;
    }
    return n;
}

OSErr bless_set(long dirID)
{
    short          vref;
    HParamBlockRec hp;
    OSErr          err;

    err = macfs_boot_vref(&vref);
    if (err != noErr) return err;

    /* Read the whole volume record, change only the blessed folder, write it back
     * (a partial PBHSetVInfo would clobber the other Finder-info longs). */
    memset(&hp, 0, sizeof hp);
    hp.volumeParam.ioNamePtr  = NULL;
    hp.volumeParam.ioVRefNum  = vref;
    hp.volumeParam.ioVolIndex = 0;
    err = PBHGetVInfoSync(&hp);
    if (err != noErr) return err;

    hp.volumeParam.ioVFndrInfo[0] = dirID;    /* the System Folder to boot */
    hp.volumeParam.ioNamePtr      = NULL;     /* PBSetVInfo: do not rename the volume */
    err = PBSetVInfoSync(&hp);
    if (err != noErr) return err;

    return FlushVol(NULL, vref);              /* push the MDB change to disk */
}

OSErr bless_and_restart(long dirID)
{
    OSErr err = bless_set(dirID);
    if (err != noErr) return err;
    sysctl_restart();                         /* ShutDwnStart — boot the new System */
    return noErr;                             /* not reached */
}
