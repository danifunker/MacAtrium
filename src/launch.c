/*
 * launch.c — see launch.h. The LaunchParamBlockRec fields/flags are exactly as
 * confirmed in docs/11-derisk-log.md §A and exercised by the spike.
 */
#include "launch.h"
#include "macfs.h"
#include "mac_compat.h"

#include <Processes.h>
#include <Files.h>

LaunchResult launch_app(const char *appRel, int canReturn, OSErr *outErr)
{
    FSSpec              spec;
    FInfo               finfo;
    LaunchParamBlockRec pb;
    OSErr               err;

    *outErr = noErr;

    /* Never do the non-returning launch that would quit the shell. */
    if (!canReturn) return LAUNCH_CANT_RETURN;

    err = macfs_make_spec(appRel, &spec);
    if (err != noErr) { *outErr = err; return LAUNCH_NOT_FOUND; }

    /* Confirm the target actually exists before launching. */
    err = FSpGetFInfo(&spec, &finfo);
    if (err != noErr) { *outErr = err; return LAUNCH_NOT_FOUND; }

    pb.launchBlockID       = extendedBlock;
    pb.launchEPBLength     = extendedBlockLen;
    pb.launchFileFlags     = 0;
    pb.launchControlFlags  = launchContinue | launchNoFileFlags;
    pb.launchAppSpec       = &spec;
    pb.launchAppParameters = NULL;

    err = LaunchApplication(&pb);     /* _Launch (A9F2); returns here on quit */
    *outErr = err;

    return (err == noErr) ? LAUNCH_OK : LAUNCH_FAILED;
}
