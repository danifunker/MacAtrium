/*
 * art.c — see art.h. Loads either a raw 1-bit bitmap (drawn via CopyBits) or a
 * PICT (drawn via DrawPicture), aspect-fit and centered.
 */
#include "art.h"
#include "macfs.h"

#include <Memory.h>
#include <string.h>

/* Raw-bitmap sidecar header: "AB", u16 version, u16 w, u16 h, u16 rowBytes,
 * u16 depth — 12 bytes, then the MSB-first pixel rows. Must match build_raw1
 * in tools/atrium-tool/src/pict.rs. */
#define RAW1_HEADER_LEN 12

struct Art {
    PicHandle pic;       /* PICT path: non-NULL => DrawPicture            */
    Ptr       raw;       /* raw-bitmap file buffer: non-NULL => CopyBits  */
    short     w, h;      /* raw bitmap dimensions (pixels)                */
    short     rowBytes;  /* raw bitmap rowBytes (even, high bit clear)    */
};

static unsigned short rd16(const unsigned char *p)
{
    return (unsigned short)((p[0] << 8) | p[1]);
}

/* ---- loading -------------------------------------------------------------- */

static Art *load_raw(const char *relToRoot)
{
    FSSpec               spec;
    char                *buf;
    long                 len;
    const unsigned char *p;
    short                w, h, rb;
    Art                 *a;

    if (macfs_make_spec(relToRoot, &spec) != noErr) return 0;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return 0;
    if (len < RAW1_HEADER_LEN) { DisposePtr(buf); return 0; }

    p = (const unsigned char *)buf;
    if (p[0] != 'A' || p[1] != 'B' || rd16(p + 2) != 1) { DisposePtr(buf); return 0; }
    w  = (short)rd16(p + 4);
    h  = (short)rd16(p + 6);
    rb = (short)rd16(p + 8);                 /* p+10: depth (1), unused here */

    /* Validate: positive dims, even rowBytes with high bit clear (so QuickDraw
     * treats it as an old-style BitMap), and enough bytes for all the rows. */
    if (w <= 0 || h <= 0 || rb <= 0 || (rb & 0x8001) ||
        len < (long)RAW1_HEADER_LEN + (long)rb * (long)h) {
        DisposePtr(buf);
        return 0;
    }

    a = (Art *)NewPtr(sizeof(Art));
    if (!a) { DisposePtr(buf); return 0; }
    a->pic = 0;
    a->raw = buf;                            /* pixels start at buf + header */
    a->w = w; a->h = h; a->rowBytes = rb;
    return a;
}

static Art *load_pict(const char *relToRoot)
{
    FSSpec spec;
    long   n;
    Handle h;
    Art   *a;

    if (macfs_make_spec(relToRoot, &spec) != noErr) return 0;
    /* Read the picture data straight into the Handle, skipping the 512-byte PICT
     * file header — no full-file staging buffer, so peak memory is ~1x the cover
     * (a 318 KB PICT) instead of ~2x during the load. */
    if (macfs_read_handle(&spec, 512, &h, &n) != noErr) return 0;
    if (n <= 0) { DisposeHandle(h); return 0; }

    a = (Art *)NewPtr(sizeof(Art));
    if (!a) { DisposeHandle(h); return 0; }
    a->pic = (PicHandle)h;
    a->raw = 0;
    a->w = a->h = a->rowBytes = 0;
    return a;
}

Art *art_load(const char *relToRoot)
{
    int n = (int)strlen(relToRoot);
    if (n >= 4 && strcmp(relToRoot + n - 4, ".raw") == 0)
        return load_raw(relToRoot);
    return load_pict(relToRoot);
}

void art_dispose(Art *a)
{
    if (!a) return;
    if (a->pic) DisposeHandle((Handle)a->pic);
    if (a->raw) DisposePtr(a->raw);
    DisposePtr((Ptr)a);
}

/* ---- drawing -------------------------------------------------------------- */

/* Aspect-fit a `sw`x`sh` source into `bounds`, centered, into `dst`. Doesn't
 * upscale past the bounds. */
static void fit_rect(short sw, short sh, const Rect *bounds, Rect *dst)
{
    long bw = bounds->right - bounds->left;
    long bh = bounds->bottom - bounds->top;
    long dw = bw;
    long dh = (long)sh * bw / sw;
    if (dh > bh) { dh = bh; dw = (long)sw * bh / sh; }

    dst->left   = (short)(bounds->left + (bw - dw) / 2);
    dst->top    = (short)(bounds->top  + (bh - dh) / 2);
    dst->right  = (short)(dst->left + dw);
    dst->bottom = (short)(dst->top + dh);
}

/* The BitMap that CopyBits should draw *into* for the current port — the port's
 * own bits for a B&W GrafPort, or its PixMap for a colour port / GWorld (the
 * high bit of rowBytes marks a colour port). Mirrors how render.c blits. */
static BitMap *cur_port_bits(void)
{
    GrafPtr port;
    GetPort(&port);
    if ((unsigned short)port->portBits.rowBytes & 0x8000)
        return (BitMap *)*(((CGrafPtr)port)->portPixMap);
    return &port->portBits;
}

void art_draw_fit(Art *a, const Rect *bounds)
{
    Rect src, dst;

    if (!a) return;

    if (a->raw) {
        BitMap bm;
        SetRect(&bm.bounds, 0, 0, a->w, a->h);
        bm.rowBytes = a->rowBytes;                 /* even, high bit clear => BitMap */
        bm.baseAddr = a->raw + RAW1_HEADER_LEN;
        src = bm.bounds;
        fit_rect(a->w, a->h, bounds, &dst);

        ForeColor(blackColor);                     /* 1-bit -> black/white in a colour dst */
        BackColor(whiteColor);
        CopyBits(&bm, cur_port_bits(), &src, &dst, srcCopy, 0L);
        return;
    }

    /* PICT path */
    src = (**a->pic).picFrame;
    if (src.right <= src.left || src.bottom <= src.top) return;
    fit_rect((short)(src.right - src.left), (short)(src.bottom - src.top), bounds, &dst);
    DrawPicture(a->pic, &dst);
}
