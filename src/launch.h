/*
 * launch.h — the resident sub-launch (docs/03, docs/08). Resolves a catalog
 * `app` path (relative to /MacAtrium) to an FSSpec and launches it with
 * launchContinue so control RETURNS to us when the child quits. This is the
 * exact mechanism proved in spikes/launch-return.
 */
#ifndef MACATRIUM_LAUNCH_H
#define MACATRIUM_LAUNCH_H

#include <MacTypes.h>

typedef enum {
    LAUNCH_OK = 0,        /* launched and returned */
    LAUNCH_CANT_RETURN,   /* gestaltLaunchCanReturn is false — refused */
    LAUNCH_NOT_FOUND,     /* app path didn't resolve / doesn't exist */
    LAUNCH_FAILED         /* LaunchApplication returned an error */
} LaunchResult;

/* Launch the app at /MacAtrium-relative `appRel` on volume `vref` (docs/37 — the
 * item's source disk). canReturn comes from env (gestaltLaunchCanReturn); the raw
 * OSErr (if any) is written to *outErr. */
LaunchResult launch_app(short vref, const char *appRel, int canReturn, OSErr *outErr);

#endif /* MACATRIUM_LAUNCH_H */
