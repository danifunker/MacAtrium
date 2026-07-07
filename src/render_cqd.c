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
    RGBColor tile;      /* At Ease tile button face (raised grey) */
    RGBColor bevelHi;   /* Platinum (sys8) frame bevel: highlight (bottom/right) */
    RGBColor bevelLo;   /* Platinum (sys8) frame bevel: shadow (top/left)        */
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
    { 0x5555, 0x5555, 0x5555 },   /* line    #555555 grey rule   */
    { 0x4444, 0x4444, 0x4444 },   /* tile    #444444 raised face  */
    { 0x5A5A, 0x5A5A, 0x5A5A },   /* bevelHi #5a5a5a (2-tone, reserved) */
    { 0x7070, 0x7070, 0x7070 }    /* bevelLo #707070 soft Platinum frame */
};

/* Light (authentic System 7): white window interiors, black-on-white chrome, a
 * PALE low-saturation highlight (the System 7 Highlight colour — NOT a saturated
 * accent), black 1px rules. selText is BLACK (dark text reads on the pale tint).
 * This is the classic look; the Platinum/blue version read as a modern app. */
static ThemePalette kLight = {
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* bg      white (window interior)      */
    { 0xEAEA, 0xEAEA, 0xEAEA },   /* panel   #eaeaea — overlays stand out */
    { 0xC6C6, 0xCFCF, 0xEFEF },   /* sel     #c6cfef pale blue-grey tint  */
    { 0x0000, 0x0000, 0x0000 },   /* text    black                        */
    { 0x7878, 0x7878, 0x7878 },   /* dim     #787878 mid grey             */
    { 0x0000, 0x0000, 0x0000 },   /* selText black (dark on the pale tint)*/
    { 0x0000, 0x0000, 0x0000 },   /* line    black 1px rules + frames      */
    { 0xCCCC, 0xCCCC, 0xCCCC },   /* tile    #cccccc platinum button face  */
    { 0xFFFF, 0xFFFF, 0xFFFF },   /* bevelHi white highlight               */
    { 0x8888, 0x8888, 0x8888 }    /* bevelLo #888888 shadow                 */
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
        /* sys6 selection is a flat inversion (bar in the ink colour, no colour
         * accent); sys7/8 use the tinted highlight. */
        case FILL_SEL:   RGBForeColor(r->look->selInvert ? &p->text : &p->sel); break;
        case FILL_PANEL: RGBForeColor(&p->panel); break;
        case FILL_TILE:  RGBForeColor(&p->tile);  break;
        default:         RGBForeColor(&p->bg);    break;
    }
}

void cqd_set_ink(const Render *r, int ink)
{
    ThemePalette *p = pal(r);
    TextMode(srcOr);                   /* foreground over existing background */
    switch (ink) {
        /* Over a sys6 inverted bar the text is the paper colour (white-on-dark);
         * over the sys7/8 tint it's the palette's selText. */
        case INK_SELECTED: RGBForeColor(r->look->selInvert ? &p->bg : &p->selText); break;
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

void cqd_set_line_shade(const Render *r, int hi)
{
    ThemePalette *p = pal(r);
    PenPat(&qd.black);
    RGBForeColor(hi ? &p->bevelHi : &p->bevelLo);
}
