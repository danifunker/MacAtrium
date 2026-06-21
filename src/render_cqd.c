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

/* Dark (default): charcoal desktop, slightly-lighter panel so it reads as a
 * raised list, a bright blue selection, near-white text, a visible grey rule. */
static ThemePalette kDark = {
    { 0x1C1C, 0x1C1C, 0x1C1C },   /* bg      #1c1c1c */
    { 0x2C2C, 0x2C2C, 0x2C2C },   /* panel   #2c2c2c */
    { 0x3333, 0x6666, 0xEEEE },   /* sel     bright blue */
    { 0xECEC, 0xECEC, 0xECEC },   /* text    #ececec */
    { 0x9999, 0x9999, 0x9999 },   /* dim     #999999 */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* selText white   */
    { 0x5555, 0x5555, 0x5555 }    /* line    #555555 */
};

/* Light ("Platinum"): grey desktop, white list area, classic blue selection,
 * black text, mid-grey dim, black rules. */
static ThemePalette kLight = {
    { 0xCCCC, 0xCCCC, 0xCCCC },   /* bg      platinum grey */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* panel   white         */
    { 0x0000, 0x0000, 0xCCCC },   /* sel     selection blue */
    { 0x0000, 0x0000, 0x0000 },   /* text    black         */
    { 0x7777, 0x7777, 0x7777 },   /* dim     grey          */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* selText white         */
    { 0x0000, 0x0000, 0x0000 }    /* line    black         */
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
