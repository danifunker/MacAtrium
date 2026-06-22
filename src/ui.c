/*
 * ui.c — see ui.h. Direct-to-window drawing; full redraw per change (MVP).
 */
#include "ui.h"
#include "art.h"
#include "display.h"
#include "sound.h"
#include "mac_compat.h"

#include <string.h>

#define HEADER_H 30
#define HINT_H   24
#define MARGIN   10
#define ROW_H    18
#define NARROW_W 420

static const char *kMenuItems[] = { "Launch Finder", "Restart", "Shut Down" };
#define MENU_N 3

/* ---- small helpers -------------------------------------------------------- */

static void l2s(long v, char *buf)               /* long -> C string */
{
    char tmp[16];
    int  n = 0, i = 0;
    int  neg = (v < 0);
    unsigned long u = neg ? (unsigned long)(-v) : (unsigned long)v;
    if (u == 0) tmp[n++] = '0';
    while (u) { tmp[n++] = (char)('0' + (u % 10)); u /= 10; }
    if (neg) tmp[n++] = '-';
    while (n) buf[i++] = tmp[--n];
    buf[i] = '\0';
}

static int win_w(Ui *u) { return u->win->portRect.right - u->win->portRect.left; }
static int win_h(Ui *u) { return u->win->portRect.bottom - u->win->portRect.top; }

/* keep the selected row within the visible window */
static int clamp_scroll(Ui *u, int visRows)
{
    ModelCat *c = model_cur_cat(u->m);
    if (!c || c->count == 0) { u->m->topRow = 0; return 0; }
    if (visRows < 1) visRows = 1;
    if (u->m->curItem < u->m->topRow)
        u->m->topRow = u->m->curItem;
    else if (u->m->curItem >= u->m->topRow + visRows)
        u->m->topRow = u->m->curItem - visRows + 1;
    if (u->m->topRow < 0) u->m->topRow = 0;
    return u->m->topRow;
}

/* ---- public --------------------------------------------------------------- */

void ui_init(Ui *u, Env *env, Render *r, Model *m, WindowPtr win, int safe)
{
    u->env = env; u->r = r; u->m = m; u->win = win;
    u->mode = UI_MODE_LIST;
    u->menuSel = 0;
    u->safe = safe;
    u->status[0] = '\0';
    u->previewPic = 0;
    u->listArt = 0;
    u->artFor = 0;
    u->settingsFocus = 0;
    u->setSel = 0;
    u->ndepths = display_depths(u->depths, UI_MAX_DEPTHS);
    /* Cap the offered depths at 4-bit: the colour backend is verified at 1/2/4,
     * but 8-bit+ has a colour-rendering defect (off-screen blit blanks; direct
     * DrawPicture to an 8-bit screen hangs — docs/15). Keep it safe until that's
     * fixed; the device may still boot at a higher depth. */
    {
        int i, n = 0;
        for (i = 0; i < u->ndepths; i++)
            if (u->depths[i] <= 4) u->depths[n++] = u->depths[i];
        u->ndepths = n;
    }
    u->vol = sound_available() ? sound_get_vol() : -1;
}

void ui_set_status(Ui *u, const char *msg)
{
    strncpy(u->status, msg, sizeof u->status - 1);
    u->status[sizeof u->status - 1] = '\0';
}

const char *ui_current_app(Ui *u)
{
    const CatItem *it = model_cur_item(u->m);
    return it ? it->app : 0;
}

const char *ui_current_name(Ui *u)
{
    const CatItem *it = model_cur_item(u->m);
    return it ? it->name : 0;
}

/* ---- drawing -------------------------------------------------------------- */

static Art *load_item_art(Ui *u, const char *image);   /* defined below */

#define ART_PANE_W 176          /* right-hand art pane width when wide enough */
#define ART_MIN_W  560          /* show the art pane only at this width or more */

#define GEAR_X 8                /* settings affordance: left edge */
#define GEAR_W 24               /* ...and reserved width (title starts after) */

/* The Settings affordance at the header's left — a little 3-slider icon. A frame
 * around it shows it's focused (reached by pressing Left at the first category;
 * Enter then opens the Settings panel). */
static void draw_settings_btn(Ui *u)
{
    Render *r = u->r;
    Rect    box, knob;
    short   x0 = GEAR_X, x1 = (short)(GEAR_X + 14);
    render_hline(r, x0, x1, 10);
    render_hline(r, x0, x1, 15);
    render_hline(r, x0, x1, 20);
    SetRect(&knob, (short)(x0 + 9), 8,  (short)(x0 + 12), 12); render_frame(r, &knob);
    SetRect(&knob, (short)(x0 + 3), 13, (short)(x0 + 6),  17); render_frame(r, &knob);
    SetRect(&knob, (short)(x0 + 8), 18, (short)(x0 + 11), 22); render_frame(r, &knob);
    if (u->settingsFocus) {
        SetRect(&box, (short)(GEAR_X - 4), 3, (short)(GEAR_X + GEAR_W - 4), (short)(HEADER_H - 4));
        render_frame(r, &box);
    }
}

static void draw_safe(Ui *u)
{
    Render *r = u->r;
    Rect full = u->win->portRect;
    int W = win_w(u), H = win_h(u);
    short cy = (short)(H / 2 - 30);

    render_fill(r, &full, FILL_BG);

    render_text_size(r, 12);
    {
        const char *t = "MacAtrium";
        short x = (short)((W - render_text_width(r, t)) / 2);
        render_text(r, x, (short)(cy - 24), t, INK_TITLE);
    }
    {
        const char *t = "No catalog found.";
        short x = (short)((W - render_text_width(r, t)) / 2);
        render_text(r, x, cy, t, INK_NORMAL);
    }
    {
        const char *t = "Expected: /MacAtrium/metadata/catalog.jsonl";
        short x = (short)((W - render_text_width(r, t)) / 2);
        render_text(r, x, (short)(cy + 22), t, INK_DIM);
    }
    {
        const char *t = "Press Esc for Restart / Shut Down.";
        short x = (short)((W - render_text_width(r, t)) / 2);
        render_text(r, x, (short)(cy + 50), t, INK_NORMAL);
    }
}

static void draw_list(Ui *u)
{
    Render   *r = u->r;
    Model    *m = u->m;
    int       W = win_w(u), H = win_h(u);
    int       narrow = (W < NARROW_W);
    ModelCat *cat = model_cur_cat(m);
    Rect      full = u->win->portRect;
    Rect      rr;
    char      num[16];
    int       detailH = narrow ? 0 : ROW_H;
    short     listTop = HEADER_H;
    short     listBot = (short)(H - HINT_H - detailH);
    int       showArt = (!narrow && W >= ART_MIN_W);
    int       listW   = showArt ? (W - ART_PANE_W) : W;
    int       visRows = (listBot - listTop - 2) / ROW_H;
    int       i, top;

    render_fill(r, &full, FILL_BG);

    /* lazy-load the selected item's art for the inline pane (only on change) */
    if (showArt) {
        const CatItem *curIt = model_cur_item(m);
        if (curIt != u->artFor) {
            if (u->listArt) { art_dispose(u->listArt); u->listArt = 0; }
            if (curIt && curIt->image[0]) u->listArt = load_item_art(u, curIt->image);
            u->artFor = curIt;
        }
    } else if (u->listArt) {
        art_dispose(u->listArt); u->listArt = 0; u->artFor = 0;
    }

    /* ---- header ---- */
    render_text_size(r, 12);
    draw_settings_btn(u);
    render_text(r, GEAR_X + GEAR_W, 20, "MacAtrium", INK_TITLE);
    if (cat) {
        short x = (short)(GEAR_X + GEAR_W + render_text_width(r, "MacAtrium") + 20);
        render_text(r, x, 20, cat->name, INK_NORMAL);
    }
    if (cat) {
        char line[32];
        l2s(cat->count, num);
        strcpy(line, num);
        strcat(line, (cat->count == 1) ? " item" : " items");
        render_text(r, (short)(W - MARGIN - render_text_width(r, line)), 20, line, INK_DIM);
    }
    render_hline(r, 0, (short)W, (short)(HEADER_H - 1));

    /* ---- list panel ---- */
    SetRect(&rr, 0, listTop, (short)listW, listBot);
    render_fill(r, &rr, FILL_PANEL);

    top = clamp_scroll(u, visRows);

    if (!cat || cat->count == 0) {
        render_text(r, MARGIN, (short)(listTop + 22),
                    "(no items in this category)", INK_DIM);
    } else {
        for (i = 0; i < visRows; i++) {
            int row = top + i;
            short y0, base;
            int sel;
            const CatItem *it;
            if (row >= cat->count) break;
            it  = &m->cat->items[cat->idx[row]];
            sel = (row == m->curItem);
            y0  = (short)(listTop + i * ROW_H);
            base = (short)(y0 + ROW_H - 5);

            SetRect(&rr, 0, y0, (short)listW, (short)(y0 + ROW_H));
            render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);

            render_text(r, MARGIN, base, it->name, sel ? INK_SELECTED : INK_NORMAL);

            if (!narrow && it->year > 0) {
                l2s(it->year, num);
                render_text(r, (short)(listW - MARGIN - render_text_width(r, num)),
                            base, num, sel ? INK_SELECTED : INK_DIM);
            }
        }
    }
    SetRect(&rr, 0, listTop, (short)listW, listBot);
    render_frame(r, &rr);

    /* ---- inline art pane (selected item's box art, depth-matched) ---- */
    if (showArt) {
        Rect ap, inner;
        SetRect(&ap, (short)listW, listTop, (short)W, listBot);
        render_fill(r, &ap, FILL_PANEL);
        render_frame(r, &ap);
        if (u->listArt) {
            SetRect(&inner, (short)(listW + MARGIN), (short)(listTop + MARGIN),
                    (short)(W - MARGIN), (short)(listBot - MARGIN));
            art_draw_fit(u->listArt, &inner);
        } else {
            const char *t = "(no art)";
            short tx = (short)(listW + (ART_PANE_W - render_text_width(r, t)) / 2);
            render_text(r, tx, (short)((listTop + listBot) / 2), t, INK_DIM);
        }
    }

    /* ---- detail line (selection desc / status) ---- */
    if (!narrow) {
        short dy = (short)(listBot + ROW_H - 5);
        if (u->status[0]) {
            render_text(r, MARGIN, dy, u->status, INK_NORMAL);
        } else {
            const CatItem *it = model_cur_item(m);
            if (it && it->desc[0])
                render_text(r, MARGIN, dy, it->desc, INK_DIM);
            else if (it && it->name[0])
                render_text(r, MARGIN, dy, it->name, INK_DIM);
        }
    }

    /* ---- hint bar ---- */
    render_hline(r, 0, (short)W, (short)(H - HINT_H));
    {
        const char *hint;
        short x;
        if (u->settingsFocus)
            hint = "Settings:   Return  open      ->  back";
        else
            hint = narrow
                ? "<> cat  ^v sel  Ret launch  P art  Esc menu  < settings"
                : "<- ->  category    ^ v  select    Return  launch    P  art    Esc  menu    <  settings";
        x = (short)((W - render_text_width(r, hint)) / 2);
        if (x < MARGIN) x = MARGIN;
        render_text(r, x, (short)(H - 7), hint, INK_DIM);
    }
}

/* The Settings panel: a list of adjustable rows (Theme / Color Depth / Volume).
 * Up/Down move rows; Left/Right (and Return) change the selected row's value. */
#define SET_N 3

static void set_row_text(Ui *u, int row, char *out)
{
    char num[16];
    switch (row) {
        case 0:
            strcpy(out, "Theme");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, (u->r->theme == THEME_LIGHT) ? "Light" : "Dark");
            break;
        case 1:
            strcpy(out, "Color Depth");
            while (strlen(out) < 16) strcat(out, " ");
            if (u->env->pixelSize >= 16) { strcat(out, "Thousands"); }
            else { l2s(u->env->pixelSize, num); strcat(out, num); strcat(out, "-bit"); }
            break;
        default:
            strcpy(out, "Volume");
            while (strlen(out) < 16) strcat(out, " ");
            if (u->vol < 0) { strcat(out, "n/a"); }
            else { l2s(u->vol, num); strcat(out, num); strcat(out, " / 7"); }
            break;
    }
}

static void draw_settings(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    int     pw = 320;
    int     ph = SET_N * (ROW_H + 6) + 56;
    short   px = (short)((W - pw) / 2);
    short   py = (short)((H - ph) / 2);
    Rect    panel, rr;
    int     i;

    SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
    render_fill(r, &panel, FILL_PANEL);
    render_frame(r, &panel);

    render_text_size(r, 12);
    render_text(r, (short)(px + MARGIN), (short)(py + 22), "Settings", INK_TITLE);
    render_hline(r, px, (short)(px + pw), (short)(py + 30));

    for (i = 0; i < SET_N; i++) {
        short y0   = (short)(py + 40 + i * (ROW_H + 6));
        short base = (short)(y0 + ROW_H - 5);
        int   sel  = (i == u->setSel);
        char  line[48];
        SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
        render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);
        set_row_text(u, i, line);
        render_text(r, (short)(px + MARGIN), base, line, sel ? INK_SELECTED : INK_NORMAL);
    }

    render_hline(r, px, (short)(px + pw), (short)(py + ph - 22));
    {
        const char *hint = "^v row   <> change   Esc back";
        short x = (short)(px + (pw - render_text_width(r, hint)) / 2);
        render_text(r, x, (short)(py + ph - 7), hint, INK_DIM);
    }
}

static void draw_menu(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    int     pw = 240;
    int     ph = MENU_N * (ROW_H + 4) + 44;
    short   px = (short)((W - pw) / 2);
    short   py = (short)((H - ph) / 2);
    Rect    panel, rr;
    int     i;

    SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
    render_fill(r, &panel, FILL_PANEL);
    render_frame(r, &panel);

    render_text(r, (short)(px + MARGIN), (short)(py + 22), "MacAtrium  Menu", INK_TITLE);
    render_hline(r, px, (short)(px + pw), (short)(py + 30));

    for (i = 0; i < MENU_N; i++) {
        short y0   = (short)(py + 36 + i * (ROW_H + 4));
        short base = (short)(y0 + ROW_H - 5);
        int   sel  = (i == u->menuSel);
        SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
        render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);
        render_text(r, (short)(px + MARGIN), base, kMenuItems[i],
                    sel ? INK_SELECTED : INK_NORMAL);
    }
}

static void draw_preview(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    Rect    full = u->win->portRect;
    Rect    art;
    const CatItem *it = model_cur_item(u->m);

    render_fill(r, &full, FILL_BG);

    /* title */
    render_text_size(r, 12);
    if (it)
        render_text(r, MARGIN, 20, it->name, INK_TITLE);
    render_hline(r, 0, (short)W, (short)(HEADER_H - 1));

    /* art area between header and hint bar */
    SetRect(&art, MARGIN, (short)(HEADER_H + MARGIN),
            (short)(W - MARGIN), (short)(H - HINT_H - MARGIN));
    if (u->previewPic)
        art_draw_fit(u->previewPic, &art);
    else
        render_text(r, MARGIN, (short)(H / 2), "(no artwork)", INK_DIM);

    render_hline(r, 0, (short)W, (short)(H - HINT_H));
    {
        const char *hint = "any key  back";
        short x = (short)((W - render_text_width(r, hint)) / 2);
        render_text(r, x, (short)(H - 7), hint, INK_DIM);
    }
}

void ui_draw(Ui *u)
{
    render_begin(u->r, u->win);
    if (u->mode == UI_MODE_PREVIEW) {
        draw_preview(u);
    } else {
        if (u->safe) {
            draw_safe(u);
        } else {
            draw_list(u);
        }
        if (u->mode == UI_MODE_MENU)
            draw_menu(u);
        else if (u->mode == UI_MODE_SETTINGS)
            draw_settings(u);
    }
    render_end(u->r, u->win);
}

/* ---- input ---------------------------------------------------------------- */

static UiCommand menu_select(Ui *u)
{
    u->mode = UI_MODE_LIST;
    switch (u->menuSel) {
        case 0: return UI_FINDER;
        case 1: return UI_RESTART;
        case 2: return UI_SHUTDOWN;
    }
    return UI_NONE;
}

/* Build "<base>.<depth>.<ext>" into buf (depth is 1/4/8/16). */
static void art_variant_path(char *buf, const char *base, int depth, const char *ext)
{
    int n;
    strcpy(buf, base);
    n = (int)strlen(buf);
    buf[n++] = '.';
    if (depth >= 10) { buf[n++] = (char)('0' + depth / 10); buf[n++] = (char)('0' + depth % 10); }
    else             { buf[n++] = (char)('0' + depth); }
    buf[n++] = '.';
    strcpy(buf + n, ext);
}

/* Load the depth variant for `base`. 1-bit art ships as a raw CopyBits bitmap
 * (.raw) — preferred over a .pict because DrawPicture faults Snow on some valid
 * 1-bit art (docs/14); fall back to a .pict variant if the .raw is absent. */
static Art *load_variant(const char *base, int depth)
{
    char buf[208];
    Art *p;
    if (depth == 1) {
        art_variant_path(buf, base, 1, "raw");
        p = art_load(buf);
        if (p) return p;
    }
    art_variant_path(buf, base, depth, "pict");
    return art_load(buf);
}

/* Resolve an item's `image` to an Art. An explicit ".pict"/".raw" path loads
 * directly; otherwise it's a base path and we pick the depth variant matching
 * the screen (e.g. "images/foo" -> "images/foo.1.raw" on a 1-bit screen),
 * falling back to shallower depths then "<base>.pict"/".raw". This keeps a
 * 1-bit screen from ever drawing a colour PICT (docs/06 depth variants). */
static Art *load_item_art(Ui *u, const char *image)
{
    int n = (int)strlen(image);
    char buf[208];
    Art *p;
    int cand[4], nc = 0, i, depth;

    if ((n >= 5 && strcmp(image + n - 5, ".pict") == 0) ||
        (n >= 4 && strcmp(image + n - 4, ".raw") == 0))
        return art_load(image);

    depth = u->env->pixelSize;
    if      (depth <= 1) { cand[nc++] = 1; }
    else if (depth <= 4) { cand[nc++] = 4;  cand[nc++] = 1; }
    else if (depth <= 8) { cand[nc++] = 8;  cand[nc++] = 4;  cand[nc++] = 1; }
    else                 { cand[nc++] = 16; cand[nc++] = 8;  cand[nc++] = 1; }

    for (i = 0; i < nc; i++) {
        p = load_variant(image, cand[i]);
        if (p) return p;
    }
    strcpy(buf, image);
    strcat(buf, ".pict");
    p = art_load(buf);
    if (p) return p;
    strcpy(buf, image);
    strcat(buf, ".raw");
    p = art_load(buf);
    if (p) return p;
    /* last resort: the app's own Finder icon, baked as a raw bitmap (docs/14). */
    strcpy(buf, image);
    strcat(buf, ".icon.raw");
    return art_load(buf);
}

/* Switch the screen to `depth` bits and re-fit our rendering to it. */
static void apply_depth(Ui *u, short depth)
{
    if (display_set_depth(depth) != noErr) return;
    u->env->pixelSize = display_current_depth();   /* re-read what we actually got */
    u->env->useColor  = (u->env->hasColorQD && u->env->pixelSize >= 4);
    render_reset_for_depth(u->r, u->env, u->env->pixelSize);
    if (u->listArt) {                              /* art variant is depth-specific */
        art_dispose(u->listArt); u->listArt = 0; u->artFor = 0;
    }
}

/* Change the selected Settings row's value by `dir` (-1 / +1). */
static void settings_adjust(Ui *u, int dir)
{
    switch (u->setSel) {
        case 0:                                    /* Theme */
            render_toggle_theme(u->r);
            break;
        case 1: {                                  /* Color Depth */
            int i, cur = 0;
            for (i = 0; i < u->ndepths; i++)
                if (u->depths[i] == u->env->pixelSize) cur = i;
            cur += dir;
            if (cur < 0) cur = 0;
            if (cur >= u->ndepths) cur = u->ndepths - 1;
            if (u->ndepths > 0 && u->depths[cur] != u->env->pixelSize)
                apply_depth(u, u->depths[cur]);
            break;
        }
        default:                                   /* Volume */
            if (u->vol >= 0) {
                u->vol += dir;
                if (u->vol < 0) u->vol = 0;
                if (u->vol > SOUND_VOL_MAX) u->vol = SOUND_VOL_MAX;
                sound_set_vol(u->vol);
            }
            break;
    }
}

UiCommand ui_key(Ui *u, char ch)
{
    UiCommand cmd = UI_NONE;

    /* Theme toggle works in any mode (list / menu / safe / preview). */
    if (ch == 't' || ch == 'T') {
        render_toggle_theme(u->r);
        ui_draw(u);
        return UI_NONE;
    }

    /* Settings panel: rows adjusted with arrows, Esc returns to the list. */
    if (u->mode == UI_MODE_SETTINGS) {
        switch (ch) {
            case kCharUp:    if (u->setSel > 0) u->setSel--; break;
            case kCharDown:  if (u->setSel < SET_N - 1) u->setSel++; break;
            case kCharLeft:  settings_adjust(u, -1); break;
            case kCharRight: settings_adjust(u, +1); break;
            case kCharReturn:
            case kCharEnter: settings_adjust(u, +1); break;
            case kCharEscape: u->mode = UI_MODE_LIST; break;   /* gear stays focused */
        }
        ui_draw(u);
        return UI_NONE;
    }

    /* Gear focused on the list screen (reached by Left at the first category). */
    if (u->mode == UI_MODE_LIST && u->settingsFocus) {
        switch (ch) {
            case kCharReturn:
            case kCharEnter:  u->mode = UI_MODE_SETTINGS; u->setSel = 0; break;
            case kCharEscape: u->settingsFocus = 0; u->mode = UI_MODE_MENU; u->menuSel = 0; break;
            default:          u->settingsFocus = 0; break;   /* any other key unfocuses */
        }
        ui_draw(u);
        return UI_NONE;
    }

    /* Preview mode: any other key returns to the list. */
    if (u->mode == UI_MODE_PREVIEW) {
        art_dispose(u->previewPic);
        u->previewPic = 0;
        u->mode = UI_MODE_LIST;
        ui_draw(u);
        return UI_NONE;
    }

    if (u->mode == UI_MODE_MENU) {
        switch (ch) {
            case kCharUp:    if (u->menuSel > 0) u->menuSel--; break;
            case kCharDown:  if (u->menuSel < MENU_N - 1) u->menuSel++; break;
            case kCharReturn:
            case kCharEnter: cmd = menu_select(u); break;
            case kCharEscape: u->mode = UI_MODE_LIST; break;
        }
        ui_draw(u);
        return cmd;
    }

    /* list / safe mode */
    switch (ch) {
        case kCharEscape:
            u->mode = UI_MODE_MENU;
            u->menuSel = 0;
            break;
        case kCharUp:    if (!u->safe) model_move_item(u->m, -1); break;
        case kCharDown:  if (!u->safe) model_move_item(u->m, +1); break;
        case kCharLeft:
            /* Left past the first category focuses the Settings gear. */
            if (!u->safe && !model_move_cat(u->m, -1)) u->settingsFocus = 1;
            break;
        case kCharRight: if (!u->safe) model_move_cat(u->m, +1); break;
        case kCharReturn:
        case kCharEnter:
            if (!u->safe && model_cur_item(u->m)) {
                u->status[0] = '\0';
                cmd = UI_LAUNCH;
            }
            break;
        case 'p':
        case 'P':
            if (!u->safe) {
                const CatItem *it = model_cur_item(u->m);
                if (it && it->image[0]) {
                    u->previewPic = load_item_art(u, it->image);
                    u->mode = UI_MODE_PREVIEW;   /* draws "(no artwork)" if load failed */
                }
            }
            break;
        default:
            /* Type-ahead: any other printable key jumps to the next matching
             * item. ('t'/'p' stay reserved for theme/preview above.) */
            if (!u->safe &&
                ((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
                 (ch >= '0' && ch <= '9')))
                model_type_ahead(u->m, ch);
            break;
    }
    ui_draw(u);
    return cmd;
}
