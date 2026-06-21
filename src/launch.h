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

/* appRel is the catalog `app` field, e.g. "Apps/Prince of Persia/Prince of
 * Persia". canReturn comes from env (gestaltLaunchCanReturn). On return the
 * raw OSErr (if any) is written to *outErr. */
LaunchResult launch_app(const char *appRel, int canReturn, OSErr *outErr);

#endif /* MACATRIUM_LAUNCH_H */
