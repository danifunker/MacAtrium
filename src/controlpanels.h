/*
 * controlpanels.h — enumerate the System's Control Panels and open one via the
 * Finder (docs/18 "Settings -> Control Panels").
 *
 * We list the boot Control Panels folder (FindFolder + PBGetCatInfo) for `cdev`
 * files and open the chosen one by sending the resident Finder an `odoc`
 * AppleEvent with the cdev's FSSpec — exactly what a double-click does — so the
 * Finder brings up the control panel. The launcher itself never hosts cdevs.
 */
#ifndef MACATRIUM_CONTROLPANELS_H
#define MACATRIUM_CONTROLPANELS_H

#include <Files.h>     /* Str63, FSSpec, OSErr */

#define CTLPANEL_MAX 48        /* plenty for a stock System Folder */

typedef struct {
    Str63 name;                /* Pascal name (display + FSSpec)         */
    short vref;                /* Control Panels folder volume ref       */
    long  parID;              /* ...and directory id                    */
} CtlPanel;

/* Fill `out` (up to `max`) with the `cdev` control panels in the boot Control
 * Panels folder, sorted by name. Returns the count (0 if none / unavailable). */
int   ctlpanels_list(CtlPanel *out, int max);

/* Ask the resident Finder ('MACS') to open the control panel via an `odoc`
 * AppleEvent. Returns noErr on send (the Finder then opens it), or an error if
 * AppleEvents/the Finder aren't available. */
OSErr ctlpanels_open(const CtlPanel *cp);

#endif /* MACATRIUM_CONTROLPANELS_H */
