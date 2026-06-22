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

/* Themes. Dark is the default (docs/07); both backends honour it — Color QD via
 * two palettes, B&W via a straight black/white inversion. */
enum { THEME_DARK = 0, THEME_LIGHT };

typedef struct {
    int       color;       /* 1 = Color QD backend, 0 = B&W backend */
    int       theme;       /* THEME_DARK (default) / THEME_LIGHT */
    int       depth;       /* pixelSize */
    /* Off-screen compositing: draw a whole frame into a GWorld, then blit it to
     * the window in one CopyBits so there's no on-screen erase/repaint flicker
     * (docs/03). Enabled when Color QD is present (NewGWorld available); falls
     * back to direct-to-window drawing otherwise. */
    int       useOffscreen;
    GWorldPtr offscreen;
    Rect      bounds;
    CGrafPtr  savePort;    /* GetGWorld/SetGWorld use CGrafPtr */
    GDHandle  saveGD;
} Render;

void  render_init(Render *r, const Env *e);

/* Re-select the backend after the user changes screen depth at runtime: drops
 * the off-screen GWorld (rebuilt next frame at `depth`) and chooses colour vs
 * B&W to match. `e` supplies hasColorQD. */
void  render_reset_for_depth(Render *r, const Env *e, int depth);

void  render_set_theme(Render *r, int theme);   /* THEME_DARK / THEME_LIGHT */
int   render_toggle_theme(Render *r);           /* flip; returns new theme */

void  render_begin(Render *r, WindowPtr w);     /* SetPort + Chicago font */
void  render_end(Render *r, WindowPtr w);

void  render_fill(Render *r, const Rect *rr, int kind);
void  render_frame(Render *r, const Rect *rr);          /* 1px accentless frame */
void  render_hline(Render *r, short x0, short x1, short y);
void  render_text(Render *r, short x, short y, const char *s, int ink);

short render_text_width(Render *r, const char *s);
void  render_text_size(Render *r, int points);          /* 12 rows, 12+ header */

#endif /* MACATRIUM_RENDER_H */
