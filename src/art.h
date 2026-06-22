/*
 * art.h — load and draw curated artwork (docs/06 Images, docs/14).
 *
 * Artwork lives in /MacAtrium/images, produced host-side by `atrium`. Two
 * on-disk forms, picked by extension:
 *
 *   *.raw   — a raw 1-bit bitmap ([u16 w][u16 h][u16 rowBytes][u16 depth] + a
 *             6-byte "AB"+version preamble, MSB-first rows). Drawn with
 *             **CopyBits** straight into the current port — the same blit the
 *             renderer uses every frame. This bypasses the PICT opcode
 *             interpreter, which faults Snow on some valid 1-bit art (docs/14).
 *   *.pict  — a PICT file (512-byte header + picture data), drawn with
 *             DrawPicture. Used for colour depths, where DrawPicture is fine.
 *
 * An 8-/16-bit PICT drawn on a 1-bit screen is depth-mapped by QuickDraw, but
 * the launcher prefers a depth-matched variant (see ui.c load_item_art).
 */
#ifndef MACATRIUM_ART_H
#define MACATRIUM_ART_H

#include <Quickdraw.h>

/* Opaque loaded artwork: a PICT or a raw 1-bit bitmap. */
typedef struct Art Art;

/* Load artwork at a /MacAtrium-relative path. A ".raw" path loads the raw
 * bitmap sidecar; anything else is treated as a PICT file. Returns NULL if
 * missing/too small/malformed/out of memory. Caller frees with art_dispose. */
Art *art_load(const char *relToRoot);
void art_dispose(Art *a);

/* Draw `a` scaled to fit within `bounds` (aspect-preserved, centered). */
void art_draw_fit(Art *a, const Rect *bounds);

#endif /* MACATRIUM_ART_H */
