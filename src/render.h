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
#include "theme.h"

/* Fill kinds (background, list panel, selection highlight, At Ease tile face). */
enum { FILL_BG = 0, FILL_PANEL, FILL_SEL, FILL_TILE };

/* Text ink roles. */
enum { INK_NORMAL = 0, INK_DIM, INK_SELECTED, INK_TITLE };

/* Themes. Dark is the default (docs/07); both backends honour it — Color QD via
 * two palettes, B&W via a straight black/white inversion. */
enum { THEME_DARK = 0, THEME_LIGHT };

typedef struct {
    int          color;    /* 1 = Color QD backend, 0 = B&W backend */
    int          theme;    /* THEME_DARK (default) / THEME_LIGHT */
    int          appearancePref; /* user choice: APPEAR_AUTO or a forced APPEAR_SYS* */
    int          appearance; /* resolved era look: APPEAR_SYS6/7/8 (docs/36) */
    const Theme *look;     /* trait table for `appearance` (never NULL)       */
    int       depth;       /* pixelSize */
    short     contentFont; /* Geneva — the Finder content/filename face; all of the
                            * launcher's own text uses it (the menu bar + WM title bar
                            * stay Chicago, drawn by the System). */
    short     textSize;    /* content point size (9 / 10 / 12); Settings picks it and
                            * the layout reflows (ROW_H derives from it). */
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

/* Set the era control look from a prefs/Settings choice (`pref` = APPEAR_AUTO or a
 * forced APPEAR_SYS*). Stores the choice and re-resolves the concrete look against
 * this machine (`e` supplies sysVers + Appearance-Manager presence). */
void  render_set_appearance(Render *r, int pref, const Env *e);

void  render_begin(Render *r, WindowPtr w);     /* SetPort + Chicago font */
void  render_end(Render *r, WindowPtr w);
/* Like render_end but blits only `dirty` (window-local) — the Mac update model
 * (copy just the changed region) for incremental redraws. No-op on the direct
 * path (drawing already hit the window). */
void  render_end_rect(Render *r, WindowPtr w, const Rect *dirty);
/* Blit several disjoint dirty rects in one lock/unlock cycle — the multi-region
 * Mac update model. Use instead of calling render_end_rect repeatedly (that
 * over-unlocks the off-screen pixmap, reading it after it can move). */
void  render_end_rects(Render *r, WindowPtr w, const Rect *rects, int n);

void  render_fill(Render *r, const Rect *rr, int kind);
void  render_frame(Render *r, const Rect *rr);          /* 1px accentless frame */
void  render_round_frame(Render *r, const Rect *rr);    /* 1px rounded frame (key-cap) */
void  render_hline(Render *r, short x0, short x1, short y);

/* Small drawn key glyphs for the key-cap hints — Chicago has no arrow / return /
 * escape glyphs (only Apple/command at 0x11-0x14), so we draw them in the ink. */
void  render_arrow(Render *r, const Rect *box, int dir);  /* filled triangle: 0=L 1=R 2=U 3=D */
void  render_return(Render *r, const Rect *box);          /* the return hook glyph */
void  render_text(Render *r, short x, short y, const char *s, int ink);

short render_text_width(Render *r, const char *s);
void  render_text_size(Render *r, int points);          /* explicit one-off size */
void  render_set_text_size(Render *r, int points);      /* set the content size (S/M/L) */
void  render_base_text(Render *r);                      /* re-assert content font + size */
void  render_sys_text(Render *r);                       /* Chicago (system font) for headings */

#endif /* MACATRIUM_RENDER_H */
