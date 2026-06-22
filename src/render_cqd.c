/*
 * render_cqd.c — Color QuickDraw backend. Two restrained System-7-flavoured
 * palettes (docs/07): a dark default and a "Platinum"-ish light alternate. The
 * UI swaps between them at runtime (render_toggle_theme); ui.c never sees colours.
 */
#include "render_internal.h"

/* A full palette for one theme. Non-const: the classic RGBForeColor prototype
 * takes a (non-const) RGBColor*, so these can't be const. */
typedef struct {
    RGBColor bg;        /* desktop / window background */
    RGBColor panel;     /* list panel + menu fill      */
    RGBColor sel;       /* selection highlight fill     */
    RGBColor text;      /* normal / title text          */
    RGBColor dim;       /* dimmed (years, hints, desc)  */
    RGBColor selText;   /* text over the selection fill */
    RGBColor line;      /* frames + divider hlines      */
} ThemePalette;

/* Dark (default): near-black desktop, a slightly-raised charcoal panel, an azure
 * selection, off-white text, muted-grey dim, a soft rule. Greys are neutral so
 * they land on the system grey ramp (no brown tint) at indexed depths. */
static ThemePalette kDark = {
    { 0x0000, 0x0000, 0x0000 },   /* bg      black              */
    { 0x1C1C, 0x1C1C, 0x1C1C },   /* panel   #1c1c1c near-black  */
    { 0x2D2D, 0x6A6A, 0xE0E0 },   /* sel     #2d6ae0 azure       */
    { 0xECEC, 0xECEC, 0xECEC },   /* text    #ececec off-white   */
    { 0x9C9C, 0x9C9C, 0x9C9C },   /* dim     #9c9c9c             */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* selText white               */
    { 0x5555, 0x5555, 0x5555 }    /* line    #555555 grey rule   */
};

/* Light ("Platinum"): platinum desktop, white list area, system-blue selection,
 * near-black text, mid-grey dim, light rule. */
static ThemePalette kLight = {
    { 0xDCDC, 0xDCDC, 0xDCDC },   /* bg      #dcdcdc platinum   */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* panel   white              */
    { 0x2C2C, 0x6B6B, 0xDDDD },   /* sel     #2c6bdd system blue*/
    { 0x1A1A, 0x1A1A, 0x1A1A },   /* text    #1a1a1a near-black */
    { 0x6C6C, 0x6C6C, 0x6C6C },   /* dim     #6c6c6c            */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* selText white              */
    { 0xB4B4, 0xB4B4, 0xB4B4 }    /* line    #b4b4b4 light rule */
};

static ThemePalette *pal(const Render *r)
{
    return (r->theme == THEME_LIGHT) ? &kLight : &kDark;
}

void cqd_set_fill(const Render *r, int kind)
{
    ThemePalette *p = pal(r);
    PenPat(&qd.black);                 /* solid pattern; patCopy = solid fill */
    switch (kind) {
        case FILL_SEL:   RGBForeColor(&p->sel);   break;
        case FILL_PANEL: RGBForeColor(&p->panel); break;
        default:         RGBForeColor(&p->bg);    break;
    }
}

void cqd_set_ink(const Render *r, int ink)
{
    ThemePalette *p = pal(r);
    TextMode(srcOr);                   /* foreground over existing background */
    switch (ink) {
        case INK_SELECTED: RGBForeColor(&p->selText); break;
        case INK_DIM:      RGBForeColor(&p->dim);     break;
        default:           RGBForeColor(&p->text);    break;
    }
}

void cqd_set_line(const Render *r)
{
    ThemePalette *p = pal(r);
    PenPat(&qd.black);
    RGBForeColor(&p->line);
}
