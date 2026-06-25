/*
 * launch.c — see launch.h. The LaunchParamBlockRec fields/flags are exactly as
 * confirmed in docs/11-derisk-log.md §A and exercised by the spike.
 */
#include "launch.h"
#include "macfs.h"
#include "mac_compat.h"

#include <Processes.h>
#include <Files.h>   /* ResolveAliasFile comes from the multiversal header here */
#include <Gestalt.h>
#include <string.h>

/* Alias Manager presence bit (System 7+); on 6.0.8 the Alias Manager is absent
 * and ResolveAliasFile would be an unimplemented trap. */
#ifndef gestaltAliasMgrAttr
#define gestaltAliasMgrAttr 'alis'
#endif

LaunchResult launch_app(const char *appRel, int canReturn, OSErr *outErr)
{
    FSSpec              spec;
    FInfo               finfo;
    LaunchParamBlockRec pb;
    OSErr               err;

    *outErr = noErr;

    err = macfs_make_spec(appRel, &spec);
    if (err != noErr) { *outErr = err; return LAUNCH_NOT_FOUND; }

    /* If the catalog points at an *alias* file, resolve it to the real app so
     * moved/aliased targets still launch (docs/08). A regular file resolves to
     * itself (wasAliased=false, spec unchanged), so the proven direct path is
     * untouched; on any error we keep the original spec. The Alias Manager is
     * System-7 only, so skip this on 6.0.8 (catalog paths there are direct). */
    {
        long aliasAttr;
        if (Gestalt(gestaltAliasMgrAttr, &aliasAttr) == noErr) {
            FSSpec  resolved = spec;
            Boolean isFolder, wasAliased;
            if (ResolveAliasFile(&resolved, true, &isFolder, &wasAliased) == noErr && !isFolder)
                spec = resolved;
        }
    }

    /* Confirm the target actually exists before launching. */
    err = macfs_get_finfo(&spec, &finfo);
    if (err != noErr) { *outErr = err; return LAUNCH_NOT_FOUND; }

    if (canReturn) {
        /* A Process Manager is present (System 7, or System 6 + MultiFinder), so
         * _Launch understands the extended parameter block. launchContinue makes
         * it return control to us when the child quits — selection intact, no
         * relaunch. The proven 7.x path (docs/11-derisk-log.md §A). Zero the whole
         * block: the size/reserved fields must not be stack garbage. */
        memset(&pb, 0, sizeof pb);
        pb.launchBlockID       = extendedBlock;
        pb.launchEPBLength     = extendedBlockLen;
        pb.launchFileFlags     = 0;
        pb.launchControlFlags  = launchContinue | launchNoFileFlags;
        pb.launchAppSpec       = &spec;
        pb.launchAppParameters = NULL;

        err = LaunchApplication(&pb);     /* returns here on the child's quit */
        *outErr = err;
        return (err == noErr) ? LAUNCH_OK : LAUNCH_FAILED;
    }

    /* Bare System 6 (no Process Manager). _Launch ignores the extended block and
     * reads the *original* Segment-Loader format: a 4-byte pointer to the app's
     * file name at offset 0, then a flags word. (Confirmed empirically: feeding a
     * zeroed extended block here makes the trap read offset 0 as the name pointer
     * — NULL — and bomb with a garbage "application is busy or damaged" name.) The
     * classic launch resolves the name against the *default directory*, so point
     * that at the app's parent first. Non-returning: the game replaces us, and on
     * quit the System relaunches the boot shell — the file named "Finder", i.e.
     * MacAtrium (installed FNDR/MACS) — so we come straight back. */
    {
        struct OldLaunch { StringPtr pfName; short param; long pad[2]; } olb;
        WDPBRec wd;
        memset(&wd, 0, sizeof wd);
        wd.ioNamePtr = NULL;
        wd.ioVRefNum = spec.vRefNum;
        wd.ioWDDirID = spec.parID;
        (void)PBHSetVolSync(&wd);            /* default dir = app's parent */

        olb.pfName = (StringPtr)spec.name;   /* FSSpec.name is a Pascal Str63 */
        olb.param  = 0;                      /* 0 = launch (replace), don't sublaunch */
        olb.pad[0] = 0; olb.pad[1] = 0;
        err = LaunchApplication((LaunchParamBlockRec *)&olb);
        /* Only reached if the launch failed (otherwise control transferred away). */
        *outErr = err;
        return LAUNCH_FAILED;
    }
}
