/*
 * render.h — the backend-agnostic drawing API the UI uses (docs/03, docs/07).
 * One backend is chosen at startup from env; ui.c never branches on depth.
 *
 *   render_qd.c  — classic-QuickDraw B&W backend (1-bit; selection = white-on-
 *                  black via srcCopy)
 *   render_cqd.c — Color QuickDraw backend (selection = accent fill + white text)
 *   render.c     — dispatch + depth-independent geometry/text
 */
#ifndef MACATRIUM_RENDER_H
#define MACATRIUM_RENDER_H

#include <Quickdraw.h>
#include "env.h"

/* Fill kinds (background, list panel, selection highlight). */
enum { FILL_BG = 0, FILL_PANEL, FILL_SEL };

/* Text ink roles. */
enum { INK_NORMAL = 0, INK_DIM, INK_SELECTED, INK_TITLE };

typedef struct {
    int color;      /* 1 = Color QD backend, 0 = B&W backend */
    int depth;      /* pixelSize */
} Render;

void  render_init(Render *r, const Env *e);

void  render_begin(Render *r, WindowPtr w);     /* SetPort + Chicago font */
void  render_end(Render *r, WindowPtr w);

void  render_fill(Render *r, const Rect *rr, int kind);
void  render_frame(Render *r, const Rect *rr);          /* 1px accentless frame */
void  render_hline(Render *r, short x0, short x1, short y);
void  render_text(Render *r, short x, short y, const char *s, int ink);

short render_text_width(Render *r, const char *s);
void  render_text_size(Render *r, int points);          /* 12 rows, 12+ header */

#endif /* MACATRIUM_RENDER_H */
