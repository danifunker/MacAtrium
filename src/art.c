/*
 * art.c — see art.h. Lazy PICT load (data fork minus the 512-byte file header)
 * + aspect-fit DrawPicture.
 */
#include "art.h"
#include "macfs.h"

#include <Memory.h>

PicHandle art_load(const char *relToRoot)
{
    FSSpec spec;
    char  *buf;
    long   len, n;
    Handle h;

    if (macfs_make_spec(relToRoot, &spec) != noErr) return 0;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return 0;

    if (len <= 512) { DisposePtr(buf); return 0; }   /* header only / empty */

    n = len - 512;
    h = NewHandle(n);
    if (!h) { DisposePtr(buf); return 0; }

    BlockMoveData(buf + 512, *h, n);                 /* picture data after header */
    DisposePtr(buf);
    return (PicHandle)h;
}

void art_dispose(PicHandle pic)
{
    if (pic) DisposeHandle((Handle)pic);
}

void art_draw_fit(PicHandle pic, const Rect *bounds)
{
    Rect  src, dst;
    long  sw, sh, bw, bh, dw, dh;

    if (!pic) return;

    src = (**pic).picFrame;
    sw = src.right - src.left;
    sh = src.bottom - src.top;
    if (sw <= 0 || sh <= 0) return;

    bw = bounds->right - bounds->left;
    bh = bounds->bottom - bounds->top;

    /* scale to fit, preserve aspect (don't upscale past 2x for tiny art) */
    dw = bw;
    dh = (sh * bw) / sw;
    if (dh > bh) { dh = bh; dw = (sw * bh) / sh; }

    dst.left   = (short)(bounds->left + (bw - dw) / 2);
    dst.top    = (short)(bounds->top  + (bh - dh) / 2);
    dst.right  = (short)(dst.left + dw);
    dst.bottom = (short)(dst.top + dh);

    DrawPicture(pic, &dst);
}
