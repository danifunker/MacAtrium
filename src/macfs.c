/*
 * macfs.c — see macfs.h.
 */
#include "macfs.h"
#include "mac_compat.h"

/* FindFolder / kSystemFolderType live in the (single) multiversal header, which
 * any Toolbox shim pulls in; Retro68 has no separate <Folders.h>. */
#include <Files.h>
#include <Gestalt.h>
#include <Memory.h>
#include <string.h>

static short  gBootVRef = 0;
static Boolean gHaveBoot = false;

OSErr macfs_boot_vref(short *vref)
{
    if (!gHaveBoot) {
        short v;
        long  d;
        OSErr err = FindFolder(kOnSystemDisk, kSystemFolderType, false, &v, &d);
        if (err != noErr) return err;
        gBootVRef = v;
        gHaveBoot = true;
    }
    *vref = gBootVRef;
    return noErr;
}

/* Build "\p:MacAtrium:<rel with '/'->':'>" into a Str255. */
static void build_colon_path(const char *rel, Str255 out)
{
    static const char prefix[] = ":MacAtrium:";
    int n = 0;
    int i;

    for (i = 0; prefix[i] && n < 255; i++)
        out[++n] = prefix[i];

    for (i = 0; rel[i] && n < 255; i++)
        out[++n] = (rel[i] == '/') ? ':' : (unsigned char)rel[i];

    out[0] = (unsigned char)n;
}

OSErr macfs_make_spec(const char *relToRoot, FSSpec *spec)
{
    short vref;
    Str255 path;
    OSErr err = macfs_boot_vref(&vref);
    if (err != noErr) return err;

    build_colon_path(relToRoot, path);
    err = FSMakeFSSpec(vref, fsRtDirID, path, spec);
    if (err == noErr || err == fnfErr) return noErr;
    return err;
}

OSErr macfs_read_all(FSSpec *spec, char **buf, long *len)
{
    short refNum;
    long  eof;
    OSErr err;
    char *p;

    *buf = 0;
    *len = 0;

    err = FSpOpenDF(spec, fsRdPerm, &refNum);
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
