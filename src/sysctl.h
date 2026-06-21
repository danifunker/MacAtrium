/*
 * sysctl.h — power + escape-hatch actions for the Esc menu (docs/08).
 */
#ifndef MACATRIUM_SYSCTL_H
#define MACATRIUM_SYSCTL_H

void sysctl_restart(void);    /* ShutDwnStart — restart the machine    */
void sysctl_shutdown(void);   /* ShutDwnPower — power off              */

/* Bring the resident Finder (creator 'MACS') to the front. Returns 1 if found
 * and switched, 0 otherwise (caller should then offer Restart). */
int  sysctl_launch_finder(void);

#endif /* MACATRIUM_SYSCTL_H */
