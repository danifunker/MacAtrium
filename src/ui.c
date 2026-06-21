/*
 * ui.c — see ui.h. Direct-to-window drawing; full redraw per change (MVP).
 */
#include "ui.h"
#include "art.h"
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
    int       visRows = (listBot - listTop - 2) / ROW_H;
    int       i, top;

    render_fill(r, &full, FILL_BG);

    /* ---- header ---- */
    render_text_size(r, 12);
    render_text(r, MARGIN, 20, "MacAtrium", INK_TITLE);
    if (cat) {
        short x = (short)(MARGIN + render_text_width(r, "MacAtrium") + 20);
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
    SetRect(&rr, 0, listTop, (short)W, listBot);
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

            SetRect(&rr, 0, y0, (short)W, (short)(y0 + ROW_H));
            render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);

            render_text(r, MARGIN, base, it->name, sel ? INK_SELECTED : INK_NORMAL);

            if (!narrow && it->year > 0) {
                l2s(it->year, num);
                render_text(r, (short)(W - MARGIN - render_text_width(r, num)),
                            base, num, sel ? INK_SELECTED : INK_DIM);
            }
        }
    }
    SetRect(&rr, 0, listTop, (short)W, listBot);
    render_frame(r, &rr);

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
        const char *hint = narrow
            ? "<> cat  ^v sel  Ret launch  P art  T theme  Esc menu"
            : "<- ->  category    ^ v  select    Return  launch    P  art    T  theme    Esc  menu";
        short x = (short)((W - render_text_width(r, hint)) / 2);
        if (x < MARGIN) x = MARGIN;
        render_text(r, x, (short)(H - 7), hint, INK_DIM);
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

UiCommand ui_key(Ui *u, char ch)
{
    UiCommand cmd = UI_NONE;

    /* Theme toggle works in any mode (list / menu / safe / preview). */
    if (ch == 't' || ch == 'T') {
        render_toggle_theme(u->r);
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
        case kCharLeft:  if (!u->safe) model_move_cat(u->m, -1); break;
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
                    u->previewPic = art_load(it->image);
                    u->mode = UI_MODE_PREVIEW;   /* draws "(no artwork)" if load failed */
                }
            }
            break;
    }
    ui_draw(u);
    return cmd;
}
