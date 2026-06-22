/*
 * sysctl.c — see sysctl.h.
 */
#include "sysctl.h"

#include <Processes.h>

void sysctl_restart(void)  { ShutDwnStart(); }
void sysctl_shutdown(void) { ShutDwnPower(); }

int sysctl_show_finder(void)
{
    ProcessSerialNumber psn;
    ProcessInfoRec      info;
    Str31               nameBuf;

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
