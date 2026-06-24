/*
 * controlpanels.c — see controlpanels.h.
 */
#include "controlpanels.h"
#include "mac_compat.h"

#include <Files.h>     /* FindFolder + CInfoPBRec live in the multiversal header */
#include <AppleEvents.h>
#include <Gestalt.h>
#include <Errors.h>
#include <string.h>

/* Control Panels here use FindFolder + an odoc AppleEvent, both System 7+. On
 * base System 6 they'd be unimplemented traps, so the feature is disabled. */
static int ctlpanels_available(void)
{
    long sysv = 0;
    (void)Gestalt(gestaltSystemVersion, &sysv);
    return sysv >= 0x0700;
}

#ifndef kControlPanelFolderType
#define kControlPanelFolderType 'ctrl'
#endif
#ifndef kDontCreateFolder
#define kDontCreateFolder 0
#endif
#ifndef ioDirMask
#define ioDirMask 0x10              /* ioFlAttrib bit 4: 1 = directory */
#endif

/* Case-insensitive Pascal-string compare for the name sort. */
static int pstr_cmp(const unsigned char *a, const unsigned char *b)
{
    int n = (a[0] < b[0]) ? a[0] : b[0], i;
    for (i = 1; i <= n; i++) {
        unsigned char ca = a[i], cb = b[i];
        if (ca >= 'A' && ca <= 'Z') ca += 32;
        if (cb >= 'A' && cb <= 'Z') cb += 32;
        if (ca != cb) return (int)ca - (int)cb;
    }
    return (int)a[0] - (int)b[0];
}

int ctlpanels_list(CtlPanel *out, int max)
{
    short       vref;
    long        dirID;
    int         n = 0, i, j;
    CInfoPBRec  pb;
    Str63       name;

    if (!ctlpanels_available()) return 0;   /* System 6: no FindFolder/odoc */

    if (FindFolder(kOnSystemDisk, kControlPanelFolderType, kDontCreateFolder,
                   &vref, &dirID) != noErr)
        return 0;

    for (i = 1; n < max; i++) {
        memset(&pb, 0, sizeof pb);
        pb.hFileInfo.ioNamePtr   = name;
        pb.hFileInfo.ioVRefNum   = vref;
        pb.hFileInfo.ioDirID     = dirID;        /* reset each call — PB rewrites it */
        pb.hFileInfo.ioFDirIndex = i;
        if (PBGetCatInfoSync(&pb) != noErr) break;       /* past the last entry */

        if (pb.hFileInfo.ioFlAttrib & ioDirMask) continue;   /* a sub-folder */
        if (pb.hFileInfo.ioFlFndrInfo.fdType != 'cdev') continue;

        BlockMoveData(name, out[n].name, name[0] + 1);
        out[n].vref  = vref;
        out[n].parID = dirID;
        n++;
    }

    /* insertion sort by name (the Finder shows them alphabetically) */
    for (i = 1; i < n; i++) {
        CtlPanel key = out[i];
        for (j = i - 1; j >= 0 && pstr_cmp(out[j].name, key.name) > 0; j--)
            out[j + 1] = out[j];
        out[j + 1] = key;
    }
    return n;
}

OSErr ctlpanels_open(const CtlPanel *cp)
{
    OSType        sig = 'MACS';                  /* the Finder */
    AEAddressDesc target;
    AppleEvent    ae, reply;
    AEDescList    docs;
    FSSpec        spec;
    OSErr         err;

    if (!ctlpanels_available()) return paramErr;  /* System 6: no AppleEvents */

    /* Rebuild the cdev's FSSpec from the recorded location. */
    spec.vRefNum = cp->vref;
    spec.parID   = cp->parID;
    BlockMoveData(cp->name, spec.name, cp->name[0] + 1);

    err = AECreateDesc(typeApplSignature, &sig, sizeof sig, &target);
    if (err != noErr) return err;

    err = AECreateAppleEvent(kCoreEventClass, kAEOpenDocuments, &target,
                             kAutoGenerateReturnID, kAnyTransactionID, &ae);
    if (err == noErr) {
        err = AECreateList(0L, 0, false, &docs);
        if (err == noErr) {
            err = AEPutPtr(&docs, 0, typeFSS, &spec, sizeof spec);
            if (err == noErr)
                err = AEPutParamDesc(&ae, keyDirectObject, &docs);
            AEDisposeDesc(&docs);
        }
        if (err == noErr)
            err = AESend(&ae, &reply, kAENoReply + kAECanInteract,
                         kAENormalPriority, kAEDefaultTimeout, 0L, 0L);
        AEDisposeDesc(&ae);
    }
    AEDisposeDesc(&target);
    return err;
}
