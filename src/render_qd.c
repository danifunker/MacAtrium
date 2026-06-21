/*
 * render_qd.c — classic QuickDraw B&W backend. Selection is white-on-black via
 * srcCopy text (robust in 1-bit, unlike srcOr). See render_internal.h.
 */
#include "render_internal.h"

void qd_set_fill(const Render *r, int kind)
{
    (void)r;
    PenPat(&qd.black);                 /* solid; patCopy fills with foreColor */
    if (kind == FILL_SEL) ForeColor(blackColor);
    else                  ForeColor(whiteColor);
}

void qd_set_ink(const Render *r, int ink)
{
    (void)r;
    TextMode(srcCopy);                 /* paints glyph cell fg + bg            */
    if (ink == INK_SELECTED) {
        ForeColor(whiteColor);
        BackColor(blackColor);
    } else {
        ForeColor(blackColor);
        BackColor(whiteColor);
    }
}

void qd_set_line(const Render *r)
{
    (void)r;
    PenPat(&qd.black);
    ForeColor(blackColor);
    BackColor(whiteColor);
}
