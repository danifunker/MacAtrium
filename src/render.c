/*
 * render.c — backend dispatch + depth-independent drawing. See render.h.
 */
#include "render.h"
#include "render_internal.h"

#include <Fonts.h>
#include <string.h>

void render_init(Render *r, const Env *e)
{
    r->color        = e->useColor;
    r->theme        = THEME_DARK;      /* dark by default; 'T' toggles at runtime */
    r->depth        = e->pixelSize > 0 ? e->pixelSize : 1;
    r->useOffscreen = e->hasColorQD;   /* NewGWorld needs Color QD */
    r->offscreen    = 0;
}

void render_reset_for_depth(Render *r, const Env *e, int depth)
{
    if (r->offscreen) {            /* old GWorld is the wrong depth now */
        DisposeGWorld(r->offscreen);
        r->offscreen = 0;
    }
    r->depth        = depth > 0 ? depth : 1;
    r->color        = (e->hasColorQD && r->depth >= 4);   /* matches env_probe */
    r->useOffscreen = e->hasColorQD;                      /* may rebuild next frame */
}

void render_set_theme(Render *r, int theme)
{
    r->theme = (theme == THEME_LIGHT) ? THEME_LIGHT : THEME_DARK;
}

int render_toggle_theme(Render *r)
{
    r->theme = (r->theme == THEME_DARK) ? THEME_LIGHT : THEME_DARK;
    return r->theme;
}

static void c2p(const char *s, Str255 out)
{
    int n = 0;
    while (s[n] && n < 255) { out[n + 1] = (unsigned char)s[n]; n++; }
    out[0] = (unsigned char)n;
}

void render_begin(Render *r, WindowPtr w)
{
    if (r->useOffscreen && !r->offscreen) {
        Rect  b = w->portRect;
        /* A 640x480 GWorld is ~38 KB at 1-bit but ~300 KB at 8-bit, which won't
         * fit the default app partition. Allocate from temp (MultiFinder) memory
         * first; fall back to the app heap, then to direct drawing. */
        QDErr err = NewGWorld(&r->offscreen, r->depth, &b, 0L, 0L, useTempMem);
        if (err != noErr || !r->offscreen)
            err = NewGWorld(&r->offscreen, r->depth, &b, 0L, 0L, 0);
        if (err != noErr || !r->offscreen) {
            r->useOffscreen = 0;          /* fall back to direct drawing */
            r->offscreen    = 0;
        } else {
            r->bounds = b;
        }
    }

    if (r->useOffscreen && r->offscreen) {
        GetGWorld(&r->savePort, &r->saveGD);
        SetGWorld(r->offscreen, 0L);
        LockPixels(GetGWorldPixMap(r->offscreen));
    } else {
        SetPort(w);
    }

    TextFont(systemFont);     /* Chicago */
    TextSize(12);
    TextFace(normal);
    PenNormal();
}

void render_end(Render *r, WindowPtr w)
{
    if (r->useOffscreen && r->offscreen) {
        PixMapHandle pm = GetGWorldPixMap(r->offscreen);
        BitMap      *dst;
        SetGWorld(r->savePort, r->saveGD);    /* back to the window's port */
        SetPort(w);
        ForeColor(blackColor);                /* avoid CopyBits colourising */
        BackColor(whiteColor);
        /* Colour window (CGrafPort, e.g. at 8-bit): blit into its PixMap, not
         * the overlapping old portBits. The rowBytes high bit marks a colour
         * port. (Same idiom art.c uses for the off-screen destination.) */
        if ((unsigned short)((GrafPtr)w)->portBits.rowBytes & 0x8000)
            dst = (BitMap *)*(((CGrafPtr)w)->portPixMap);
        else
            dst = &((GrafPtr)w)->portBits;
        CopyBits((BitMap *)*pm, dst, &r->bounds, &w->portRect, srcCopy, 0L);
        UnlockPixels(pm);
    }
    /* direct-to-window path: nothing to blit */
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
