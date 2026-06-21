/*
 * render_internal.h — the small set of primitives that actually differ between
 * the B&W and Color backends. render.c sets ink/fill via these, then does the
 * depth-independent drawing (PaintRect / FrameRect / DrawString).
 */
#ifndef MACATRIUM_RENDER_INTERNAL_H
#define MACATRIUM_RENDER_INTERNAL_H

#include "render.h"

/* B&W backend (render_qd.c) */
void qd_set_fill(const Render *r, int kind);   /* prep fore/pen for a PaintRect */
void qd_set_ink(const Render *r, int ink);     /* prep fore/back/textmode       */
void qd_set_line(const Render *r);             /* prep for frames / hlines      */

/* Color backend (render_cqd.c) */
void cqd_set_fill(const Render *r, int kind);
void cqd_set_ink(const Render *r, int ink);
void cqd_set_line(const Render *r);

#endif /* MACATRIUM_RENDER_INTERNAL_H */
