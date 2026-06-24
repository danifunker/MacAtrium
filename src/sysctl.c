/*
 * sysctl.c — see sysctl.h.
 */
#include "sysctl.h"

#include <Processes.h>
#include <Gestalt.h>

void sysctl_restart(void)  { ShutDwnStart(); }
void sysctl_shutdown(void) { ShutDwnPower(); }

int sysctl_show_finder(void)
{
    ProcessSerialNumber psn;
    ProcessInfoRec      info;
    Str31               nameBuf;
    long                sysv = 0;

    /* The Process Manager (GetNextProcess/SetFrontProcess) is System 7+/
     * MultiFinder — unimplemented on base System 6. Report "no Finder" there so
     * the caller offers Restart instead of bombing on an unimplemented trap. */
    (void)Gestalt(gestaltSystemVersion, &sysv);
    if (sysv < 0x0700) return 0;

    psn.highLongOfPSN = 0;
    psn.lowLongOfPSN  = kNoProcess;
    while (GetNextProcess(&psn) == noErr) {
        info.processInfoLength = sizeof(info);
        info.processName       = nameBuf;
        info.processAppSpec     = 0;
        if (GetProcessInformation(&psn, &info) == noErr &&
            info.processSignature == 'MACS') {
            SetFrontProcess(&psn);
            return 1;
        }
    }
    return 0;   /* no resident Finder — caller should offer Restart */
}
