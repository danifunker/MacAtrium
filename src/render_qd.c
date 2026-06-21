/*
 * render_qd.c — classic QuickDraw B&W backend. In 1-bit there are no shades, so
 * "dark mode" is a straight black/white inversion: dark = black paper / white
 * ink (selection flips to a white bar with black text); light = the reverse.
 * Selection text uses srcCopy (robust in 1-bit, unlike srcOr). See render.h.
 */
#include "render_internal.h"

void qd_set_fill(const Render *r, int kind)
{
    int dark = (r->theme != THEME_LIGHT);
    PenPat(&qd.black);                 /* solid; patCopy fills with foreColor */
    if (kind == FILL_SEL)
        ForeColor(dark ? whiteColor : blackColor);   /* highlight bar */
    else
        ForeColor(dark ? blackColor : whiteColor);    /* paper (bg/panel) */
}

void qd_set_ink(const Render *r, int ink)
{
    int dark = (r->theme != THEME_LIGHT);
    TextMode(srcCopy);                 /* paints glyph cell fg + bg            */
    if (ink == INK_SELECTED) {         /* ink over the highlight bar */
        ForeColor(dark ? blackColor : whiteColor);
        BackColor(dark ? whiteColor : blackColor);
    } else {                           /* ink over the paper */
        ForeColor(dark ? whiteColor : blackColor);
        BackColor(dark ? blackColor : whiteColor);
    }
}

void qd_set_line(const Render *r)
{
    int dark = (r->theme != THEME_LIGHT);
    PenPat(&qd.black);
    ForeColor(dark ? whiteColor : blackColor);   /* rule contrasts with paper */
    BackColor(dark ? blackColor : whiteColor);
}
