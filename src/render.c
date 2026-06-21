/*
 * render.c — backend dispatch + depth-independent drawing. See render.h.
 */
#include "render.h"
#include "render_internal.h"

#include <Fonts.h>
#include <string.h>

void render_init(Render *r, const Env *e)
{
    r->color = e->useColor;
    r->depth = e->pixelSize;
}

static void c2p(const char *s, Str255 out)
{
    int n = 0;
    while (s[n] && n < 255) { out[n + 1] = (unsigned char)s[n]; n++; }
    out[0] = (unsigned char)n;
}

void render_begin(Render *r, WindowPtr w)
{
    (void)r;
    SetPort(w);
    TextFont(systemFont);     /* Chicago */
    TextSize(12);
    TextFace(normal);
    PenNormal();
}

void render_end(Render *r, WindowPtr w)
{
    (void)r;
    (void)w;
    /* Direct-to-window drawing for MVP; off-screen GWorld compositing is a
     * later polish (docs/03 "Rendering strategy"). */
}

void render_fill(Render *r, const Rect *rr, int kind)
{
    if (r->color) cqd_set_fill(r, kind);
    else          qd_set_fill(r, kind);
    PaintRect(rr);
}

void render_frame(Render *r, const Rect *rr)
{
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    PenSize(1, 1);
    FrameRect(rr);
}

void render_hline(Render *r, short x0, short x1, short y)
{
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    PenSize(1, 1);
    MoveTo(x0, y);
    LineTo(x1, y);
}

void render_text(Render *r, short x, short y, const char *s, int ink)
{
    Str255 p;
    if (r->color) cqd_set_ink(r, ink);
    else          qd_set_ink(r, ink);
    c2p(s, p);
    MoveTo(x, y);
    DrawString(p);
}

short render_text_width(Render *r, const char *s)
{
    Str255 p;
    (void)r;
    c2p(s, p);
    return StringWidth(p);
}

void render_text_size(Render *r, int points)
{
    (void)r;
    TextSize(points);
}
