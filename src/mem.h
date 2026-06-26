/*
 * mem.h — MEM_DEBUG memory probe (dev-only; build with cmake -DMEM_DEBUG=ON).
 *
 * Tracks the worst-case (low-water) memory the launcher reaches and paints a
 * two-line overlay top-left, so a headless Snow run can read the numbers straight
 * off a captured frame (no disk-write-persistence dependency). Used to size the
 * per-config `SIZE` partition (builds/RESUME.md). When MEM_DEBUG is undefined the
 * tick compiles to nothing, so call sites stay clean.
 */
#ifndef MACATRIUM_MEM_H
#define MACATRIUM_MEM_H

#include <Windows.h>

#ifdef MEM_DEBUG
/* Sample memory, update the session low-water marks, and paint the overlay to
 * `w`. Call once per event-loop turn (cheap). */
void mem_debug_tick(WindowPtr w);
#else
#define mem_debug_tick(w) ((void)0)
#endif

#endif /* MACATRIUM_MEM_H */
