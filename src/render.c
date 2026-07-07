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
    r->theme        = THEME_LIGHT;     /* classic black-on-white by default; 'T' toggles to dark */
    /* Era control look: default to matching the running System (docs/36 Phase 3);
     * a saved prefs/Settings choice overrides via render_set_appearance. */
    r->appearancePref = APPEAR_AUTO;
    r->appearance     = appearance_resolve(e->sysVers, e->hasAppearanceMgr, APPEAR_AUTO);
    r->look           = theme_for(r->appearance);
    r->depth        = e->pixelSize > 0 ? e->pixelSize : 1;
    /* Off-screen compositing needs Color QD *and* System 7+: on base System 6 the
     * GWorld/temp-memory path can bomb with dsMemFullErr (out of memory) at launch,
     * so 6.0.8 draws directly to the window instead (docs/09 Milestone 4). */
    r->useOffscreen = e->hasColorQD && (e->sysVers >= 0x0700);
    r->offscreen    = 0;
    {   /* Geneva — the Finder filename face; fall back to the application font (also
         * Geneva by default) if the name isn't found. */
        short fnum = 0;
        GetFNum("\pGeneva", &fnum);
        r->contentFont = fnum ? fnum : applFont;
    }
    r->textSize = 12;
}

void render_reset_for_depth(Render *r, const Env *e, int depth)
{
    if (r->offscreen) {            /* old GWorld is the wrong depth now */
        DisposeGWorld(r->offscreen);
        r->offscreen = 0;
    }
    r->depth        = depth > 0 ? depth : 1;
    r->color        = (e->hasColorQD && r->depth >= 4);   /* matches env_probe */
    r->useOffscreen = e->hasColorQD && (e->sysVers >= 0x0700);  /* 6.0.8: direct */
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

void render_set_appearance(Render *r, int pref, const Env *e)
{
    r->appearancePref = pref;
    r->appearance     = appearance_resolve(e->sysVers, e->hasAppearanceMgr, pref);
    r->look           = theme_for(r->appearance);
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
        Rect      b   = w->portRect;
        GDHandle  gd  = GetMainDevice();
        QDErr     err = (QDErr)-1;
        short     ladder[2];
        int       n = 0, i;

        /* Composite at the screen's *own* depth so deep colour art (a `.24.pict`)
         * and theme gradients render at full fidelity — no forced 8-bit quantise.
         * If memory can't hold a deep buffer, step down to 8-bit (CopyBits then
         * up-converts to the deeper screen) and finally to direct drawing. The
         * buffer is taken from temp (MultiFinder) memory first, so a deep GWorld
         * (a 24-bit 640x480 is ~1.2 MB) costs system RAM, NOT our SIZE partition;
         * only the temp-then-heap fallback would touch it. */
        short screen = (r->depth > 0) ? (short)r->depth : 1;
        ladder[n++] = screen;
        if (screen > 8) ladder[n++] = 8;          /* memory-pressure fallback depth */

        for (i = 0; i < n && (err != noErr || !r->offscreen); i++) {
            short      d    = ladder[i];
            CTabHandle ctab = 0L;
            /* Indexed depths (<=8) get the screen's colour table so our RGB theme
             * maps straight to its palette indices (one translation, clean greys);
             * direct depths (16/24/32) carry RGB per pixel and need no table. */
            if (gd && (**gd).gdPMap && d <= 8) ctab = (**(**gd).gdPMap).pmTable;
            /* Temp memory first, then the app heap. `noNewDevice`: do NOT register
             * a GDevice for this buffer — without it NewGWorld adds one to the
             * global device list, and a concurrently-launched game that scans it
             * (Prince of Persia walks GetDeviceList/GetNextDevice and prefers a
             * second device) can pick *our* buffer instead of the real screen. We
             * composite via CopyBits, so we never need a device of our own. */
            err = NewGWorld(&r->offscreen, d, &b, ctab, 0L, useTempMem | noNewDevice);
            if (err != noErr || !r->offscreen)
                err = NewGWorld(&r->offscreen, d, &b, ctab, 0L, noNewDevice);
        }
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

    TextFont(r->contentFont); /* Geneva — content face (menus/title stay Chicago) */
    TextSize(r->textSize);
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

void render_end_rect(Render *r, WindowPtr w, const Rect *dirty)
{
    if (r->useOffscreen && r->offscreen) {
        PixMapHandle pm = GetGWorldPixMap(r->offscreen);
        BitMap      *dst;
        Rect         d;
        SetGWorld(r->savePort, r->saveGD);
        SetPort(w);
        ForeColor(blackColor);
        BackColor(whiteColor);
        if ((unsigned short)((GrafPtr)w)->portBits.rowBytes & 0x8000)
            dst = (BitMap *)*(((CGrafPtr)w)->portPixMap);
        else
            dst = &((GrafPtr)w)->portBits;
        /* The GWorld bounds == the window portRect (render_begin), so a window-local
         * dirty rect maps 1:1; clip to the bounds and blit just that. */
        if (SectRect(dirty, &r->bounds, &d))
            CopyBits((BitMap *)*pm, dst, &d, &d, srcCopy, 0L);
        UnlockPixels(pm);
    }
    /* direct-to-window path: nothing to blit (drawing already hit the window) */
}

void render_end_rects(Render *r, WindowPtr w, const Rect *rects, int n)
{
    if (r->useOffscreen && r->offscreen) {
        PixMapHandle pm = GetGWorldPixMap(r->offscreen);
        BitMap      *dst;
        Rect         d;
        int          i;
        SetGWorld(r->savePort, r->saveGD);
        SetPort(w);
        ForeColor(blackColor);
        BackColor(whiteColor);
        if ((unsigned short)((GrafPtr)w)->portBits.rowBytes & 0x8000)
            dst = (BitMap *)*(((CGrafPtr)w)->portPixMap);
        else
            dst = &((GrafPtr)w)->portBits;
        for (i = 0; i < n; i++)
            if (SectRect(&rects[i], &r->bounds, &d))
                CopyBits((BitMap *)*pm, dst, &d, &d, srcCopy, 0L);
        UnlockPixels(pm);
    }
    /* direct-to-window path: nothing to blit (drawing already hit the window) */
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

void render_round_frame(Render *r, const Rect *rr)
{
    short c = r->look ? r->look->capCorner : 6;   /* per-era corner (sys7 = 6, today) */
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    PenSize(1, 1);
    if (c > 0) FrameRoundRect(rr, c, c);          /* gentle key-cap corners */
    else       FrameRect(rr);                     /* sys6: square, lined look */
}

void render_hline(Render *r, short x0, short x1, short y)
{
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    PenSize(1, 1);
    MoveTo(x0, y);
    LineTo(x1, y);
}

/* A small filled triangle centred in `box`, in the frame ink. dir: 0=left, 1=right,
 * 2=up, 3=down. Drawn (not a glyph) because the system font has no arrow keys. */
void render_arrow(Render *r, const Rect *box, int dir)
{
    PolyHandle poly;
    short cx = (short)((box->left + box->right) / 2);
    short cy = (short)((box->top + box->bottom) / 2);
    short s  = 3;
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    poly = OpenPoly();
    switch (dir) {
        case 0:  MoveTo((short)(cx - s), cy); LineTo((short)(cx + s), (short)(cy - s)); LineTo((short)(cx + s), (short)(cy + s)); break;
        case 1:  MoveTo((short)(cx + s), cy); LineTo((short)(cx - s), (short)(cy - s)); LineTo((short)(cx - s), (short)(cy + s)); break;
        case 2:  MoveTo(cx, (short)(cy - s)); LineTo((short)(cx - s), (short)(cy + s)); LineTo((short)(cx + s), (short)(cy + s)); break;
        default: MoveTo(cx, (short)(cy + s)); LineTo((short)(cx - s), (short)(cy - s)); LineTo((short)(cx + s), (short)(cy - s)); break;
    }
    ClosePoly();
    PaintPoly(poly);
    KillPoly(poly);
}

/* The "return" hook (a down-then-left arrow) centred in `box`, in the frame ink. */
void render_return(Render *r, const Rect *box)
{
    PolyHandle poly;
    short cx = (short)((box->left + box->right) / 2);
    short cy = (short)((box->top + box->bottom) / 2);
    short s  = 3;
    if (r->color) cqd_set_line(r);
    else          qd_set_line(r);
    PenSize(1, 1);
    MoveTo((short)(cx + s + 1), (short)(cy - s));   /* short down-stroke at the right */
    LineTo((short)(cx + s + 1), cy);
    LineTo((short)(cx - s), cy);                    /* left along the middle          */
    poly = OpenPoly();                              /* left-pointing arrowhead        */
    MoveTo((short)(cx - s - 2), cy);
    LineTo((short)(cx - s + 1), (short)(cy - 2));
    LineTo((short)(cx - s + 1), (short)(cy + 2));
    ClosePoly();
    PaintPoly(poly);
    KillPoly(poly);
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

void render_set_text_size(Render *r, int points)
{
    r->textSize = (short)points;
}

/* Re-assert the content font + size — used wherever the UI used to set the base
 * text size (so a section draws in Geneva at the chosen size, not a stale size). */
void render_base_text(Render *r)
{
    TextFont(r->contentFont);
    TextSize(r->textSize);
    TextFace(normal);
}

/* Chicago (the system font) at its native 12pt — for headings like the category
 * name, so they read as bold system chrome rather than Geneva body text. Callers
 * restore the body face with render_base_text afterward. */
void render_sys_text(Render *r)
{
    (void)r;
    TextFont(systemFont);   /* 0 = Chicago */
    TextSize(12);
    TextFace(normal);
}
