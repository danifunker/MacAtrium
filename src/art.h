/*
 * art.h — load and draw curated PICT artwork (docs/06 Images).
 *
 * Artwork lives in /MacAtrium/images as PICT files (produced host-side by
 * `atrium pict`). A PICT *file* is a 512-byte header followed by the picture
 * data; art_load strips the header and returns a PicHandle ready for
 * DrawPicture. Depth handling is QuickDraw's: an 8-/16-bit PICT drawn on a
 * 1-bit screen is mapped down automatically.
 */
#ifndef MACATRIUM_ART_H
#define MACATRIUM_ART_H

#include <Quickdraw.h>

/* Load a PICT file at a /MacAtrium-relative path (e.g. "images/foo.pict").
 * Returns NULL if missing/too small/out of memory. Caller frees with art_dispose. */
PicHandle art_load(const char *relToRoot);
void      art_dispose(PicHandle pic);

/* Draw `pic` scaled to fit within `bounds` (aspect-preserved, centered). */
void      art_draw_fit(PicHandle pic, const Rect *bounds);

#endif /* MACATRIUM_ART_H */
