/*
 * env.h — startup environment probe (docs/03, docs/08). Everything OS-version-
 * or depth-specific is detected once here and passed down; no other module
 * re-probes Gestalt.
 */
#ifndef MACATRIUM_ENV_H
#define MACATRIUM_ENV_H

#include <Quickdraw.h>

typedef struct {
    long  sysVers;          /* gestaltSystemVersion (BCD, e.g. 0x0755)   */
    long  qdVers;           /* gestaltQuickdrawVersion                   */
    int   hasColorQD;       /* Color QuickDraw present                   */
    int   pixelSize;        /* current main-device depth in bits         */
    int   useColor;         /* chosen render backend: 1 = color, 0 = B&W */
    int   canLaunchReturn;  /* gestaltLaunchCanReturn — resident launch  */
    int   hasShutdown;      /* Shutdown Manager available                */
    Rect  screen;           /* full main-screen bounds (global coords)   */
    short mbarHeight;       /* menu-bar height                           */
} Env;

void env_probe(Env *e);

/* Format a Gestalt gestaltSystemVersion value (e.g. 0x0755) as a human string
 * like "System 7.5.5" into `out` (>= 24 bytes); a trailing ".0" bugfix is dropped,
 * and v <= 0 yields "System (unknown)". */
void env_os_name(long sysVers, char *out);

/* Format just the version digits of a gestaltSystemVersion (e.g. "7.1.1") into
 * `out` (>= 12 bytes); "?" when unknown. For "MacOS Version: X"-style labels. */
void env_os_version(long sysVers, char *out);

#endif /* MACATRIUM_ENV_H */
