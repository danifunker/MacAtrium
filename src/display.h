/*
 * display.h — query and set the main screen's pixel depth (Color QuickDraw).
 *
 * Lets the launcher offer a "Color Depth" setting: enumerate what the main
 * graphics device supports (HasDepth), read the current depth (gdPMap), and
 * switch it (SetDepth). All no-ops / depth 1 when Color QD is absent.
 */
#ifndef MACATRIUM_DISPLAY_H
#define MACATRIUM_DISPLAY_H

#include <Quickdraw.h>

/* Fill `out` (ascending) with the depths the main device supports, from the
 * candidate set {1,2,4,8,16,32}. Returns the count (0 if no Color QD). */
int   display_depths(short *out, int max);

/* Current main-device depth in bits (1 if unknown / no Color QD). */
short display_current_depth(void);

/* Set the main device to `depth` bits (colour for >1, mono for 1).
 * Returns noErr on success. */
OSErr display_set_depth(short depth);

#endif /* MACATRIUM_DISPLAY_H */
