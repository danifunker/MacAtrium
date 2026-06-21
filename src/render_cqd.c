/*
 * render_cqd.c — Color QuickDraw backend. A restrained System-7 "Platinum"-ish
 * default: gray desktop, white list panel, blue selection, black text (docs/07).
 * Theme colours are local for MVP; a theme module can supply them later.
 */
#include "render_internal.h"

/* Non-const: the classic RGBForeColor prototype takes a (non-const) RGBColor*. */
static RGBColor kBg    = { 0xCCCC, 0xCCCC, 0xCCCC };  /* platinum gray   */
static RGBColor kPanel = { 0xFFFF, 0xFFFF, 0xFFFF };  /* white list area */
static RGBColor kSel   = { 0x0000, 0x0000, 0xCCCC };  /* selection blue  */
static RGBColor kBlack = { 0x0000, 0x0000, 0x0000 };
static RGBColor kWhite = { 0xFFFF, 0xFFFF, 0xFFFF };
static RGBColor kDim   = { 0x7777, 0x7777, 0x7777 };  /* dimmed text     */

void cqd_set_fill(const Render *r, int kind)
{
    (void)r;
    PenPat(&qd.black);                 /* solid pattern; patCopy = solid fill */
    switch (kind) {
        case FILL_SEL:   RGBForeColor(&kSel);   break;
        case FILL_PANEL: RGBForeColor(&kPanel); break;
        default:         RGBForeColor(&kBg);    break;
    }
}

void cqd_set_ink(const Render *r, int ink)
{
    (void)r;
    TextMode(srcOr);                   /* foreground over existing background */
    switch (ink) {
        case INK_SELECTED: RGBForeColor(&kWhite); break;
        case INK_DIM:      RGBForeColor(&kDim);   break;
        default:           RGBForeColor(&kBlack); break;
    }
}

void cqd_set_line(const Render *r)
{
    (void)r;
    PenPat(&qd.black);
    RGBForeColor(&kBlack);
}
