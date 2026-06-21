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

#endif /* MACATRIUM_ENV_H */
