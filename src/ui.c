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
#define ICON_SZ  16            /* list-row icon box (px); 32x32 art fits into it */
#define ICON_GUT 22            /* icon column width incl. the gap before the name */

static const char *kMenuItems[] = { "Show Finder", "Restart", "Shut Down" };
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

/* "1991 - Psygnosis - Puzzle, Strategy" from the item's display fields. ASCII
 * separators only — the launcher renders MacRoman, so we avoid non-ASCII glyphs.
 * `out` must hold >= ITEM_VENDOR_LEN + ITEM_GENRE_LEN + 32 bytes. */
static void build_meta(const CatItem *it, char *out)
{
    char num[16];
    out[0] = '\0';
    if (it->year > 0) { l2s(it->year, num); strcat(out, num); }
    if (it->vendor[0]) { if (out[0]) strcat(out, " - "); strcat(out, it->vendor); }
    if (it->genre[0])  { if (out[0]) strcat(out, " - "); strcat(out, it->genre); }
}

/* Word-wrap `s` into `maxw` px, drawing each line from baseline `y` stepping by
 * `dy`, up to `maxlines`. Returns the baseline after the last line drawn. */
static short draw_wrapped(Render *r, short x, short y, short maxw, short dy,
                          const char *s, int ink, int maxlines)
{
    char line[200], cand[200], word[80];
    int  ll = 0, lines = 0, i = 0;
    while (s[i] && lines < maxlines) {
        int wl = 0, cl = 0;
        while (s[i] == ' ') i++;                       /* skip run of spaces */
        while (s[i] && s[i] != ' ' && wl < (int)sizeof word - 1) word[wl++] = s[i++];
        word[wl] = '\0';
        if (wl == 0) break;
        if (ll > 0) { memcpy(cand, line, ll); cl = ll; cand[cl++] = ' '; }
        memcpy(cand + cl, word, wl); cl += wl; cand[cl] = '\0';
        if (ll > 0 && render_text_width(r, cand) > maxw) {
            line[ll] = '\0';
            render_text(r, x, y, line, ink);
            y = (short)(y + dy); lines++;
            memcpy(line, word, wl); ll = wl; line[ll] = '\0';
        } else {
            memcpy(line, cand, cl); ll = cl; line[ll] = '\0';
        }
    }
    if (ll > 0 && lines < maxlines) {
        line[ll] = '\0';
        render_text(r, x, y, line, ink);
        y = (short)(y + dy);
    }
    return y;
}

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
    u->ndepths = display_depths(u->depths, UI_MAX_DEPTHS);   /* all OS-supported depths */
    u->vol = sound_available() ? sound_get_vol() : -1;
    u->artPref = 0;                                          /* Box Art by default */
    u->sndStartup = 0;                                       /* sounds off by default */
    u->sndShutdown = 0;
    {
        int i;
        for (i = 0; i < MAX_ITEMS; i++) u->rowIcon[i] = 0;   /* lazy row-icon cache */
    }
}

/* The art base path to show for an item, honouring the Artwork setting: the
 * screenshot (`shot`) when the user picked it and one exists, else the box art
 * (`image`); each falls back to the other so a pane is never needlessly empty. */
static const char *art_base(Ui *u, const CatItem *it)
{
    if (u->artPref == 1) return it->shot[0] ? it->shot : it->image;
    return it->image[0] ? it->image : it->shot;
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

/* The small list-row icon for catalog item `catIdx`, lazily loaded and cached
 * (depth-matched like art: 1-bit ICN# .raw, or the icl8 .8.pict on colour
 * screens). NULL if the item has no icon or it won't load. */
static Art *row_icon(Ui *u, int catIdx, const CatItem *it)
{
    if (catIdx < 0 || catIdx >= MAX_ITEMS) return 0;
    if (!u->rowIcon[catIdx] && it->icon[0])
        u->rowIcon[catIdx] = load_item_art(u, it->icon);
    return u->rowIcon[catIdx];
}

/* Drop every cached row icon (e.g. after a depth change — the icl8 colour
 * variant differs from the 1-bit one). */
static void dispose_row_icons(Ui *u)
{
    int i;
    for (i = 0; i < MAX_ITEMS; i++)
        if (u->rowIcon[i]) { art_dispose(u->rowIcon[i]); u->rowIcon[i] = 0; }
}

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
    int       detailH = narrow ? 0 : 2 * ROW_H;   /* meta line + blurb */
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
            if (curIt) {
                const char *base = art_base(u, curIt);
                if (base && base[0]) u->listArt = load_item_art(u, base);
            }
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
        /* Reserve an icon gutter only when some item in this category has an
         * icon, so catalogs without icons render exactly as before. */
        int gut = 0;
        for (i = 0; i < cat->count; i++)
            if (m->cat->items[cat->idx[i]].icon[0]) { gut = ICON_GUT; break; }

        for (i = 0; i < visRows; i++) {
            int row = top + i;
            short y0, base, tx;
            int sel;
            const CatItem *it;
            if (row >= cat->count) break;
            it  = &m->cat->items[cat->idx[row]];
            sel = (row == m->curItem);
            y0  = (short)(listTop + i * ROW_H);
            base = (short)(y0 + ROW_H - 5);
            tx  = (short)(MARGIN + gut);

            SetRect(&rr, 0, y0, (short)listW, (short)(y0 + ROW_H));
            render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);

            /* small app icon in the gutter (depth-matched, lazily cached) */
            if (gut && it->icon[0]) {
                Art *ic = row_icon(u, cat->idx[row], it);
                if (ic) {
                    Rect ir;
                    SetRect(&ir, MARGIN, (short)(y0 + 1),
                            (short)(MARGIN + ICON_SZ), (short)(y0 + 1 + ICON_SZ));
                    art_draw_fit(ic, &ir);
                }
            }

            render_text(r, tx, base, it->name, sel ? INK_SELECTED : INK_NORMAL);

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

    /* ---- detail area: meta line (developer/year/genre) + blurb/status ---- */
    if (!narrow) {
        const CatItem *it = model_cur_item(m);
        short y1 = (short)(listBot + ROW_H - 5);
        short y2 = (short)(listBot + 2 * ROW_H - 5);
        if (it) {
            char meta[ITEM_VENDOR_LEN + ITEM_GENRE_LEN + 32];
            build_meta(it, meta);
            if (meta[0]) render_text(r, MARGIN, y1, meta, INK_DIM);
        }
        if (u->status[0]) {
            render_text(r, MARGIN, y2, u->status, INK_NORMAL);
        } else if (it && it->desc[0]) {
            render_text(r, MARGIN, y2, it->desc, INK_NORMAL);
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
                ? "<> cat  ^v sel  Ret launch  I info  P art  Esc menu"
                : "<- ->  category   ^ v  select   Return  launch   I  info   P  art   Esc  menu   <  settings";
        x = (short)((W - render_text_width(r, hint)) / 2);
        if (x < MARGIN) x = MARGIN;
        render_text(r, x, (short)(H - 7), hint, INK_DIM);
    }
}

/* The Settings panel: a list of adjustable rows (Theme / Color Depth / Volume /
 * Artwork / Startup Sound / Shutdown Sound). Up/Down move rows; Left/Right (and
 * Return) change the selected row's value. */
#define SET_N 6

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
            if      (u->env->pixelSize >= 32) { strcat(out, "Millions"); }
            else if (u->env->pixelSize >= 16) { strcat(out, "Thousands"); }
            else { l2s(u->env->pixelSize, num); strcat(out, num); strcat(out, "-bit"); }
            break;
        case 2:
            strcpy(out, "Volume");
            while (strlen(out) < 16) strcat(out, " ");
            if (u->vol < 0) { strcat(out, "n/a"); }
            else { l2s(u->vol, num); strcat(out, num); strcat(out, " / 7"); }
            break;
        case 3:
            strcpy(out, "Artwork");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, (u->artPref == 1) ? "Screenshot" : "Box Art");
            break;
        case 4:
            strcpy(out, "Startup Sound");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, u->sndStartup ? "On" : "Off");
            break;
        default:
            strcpy(out, "Shutdown Sound");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, u->sndShutdown ? "On" : "Off");
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
        /* On the sound rows, swap the nav hint for the clip-length note. */
        const char *hint = (u->setSel >= 4)
            ? "Sounds: a clip baked in the image, max 7 sec"
            : "^v row   <> change   Esc back";
        short x = (short)(px + (pw - render_text_width(r, hint)) / 2);
        if (x < (short)(px + 4)) x = (short)(px + 4);
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

/* The More Info card (full screen): title, year/developer, genre, the wrapped
 * description, and the box art shown large on the right. */
static void draw_info(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    Rect    full = u->win->portRect;
    const CatItem *it = model_cur_item(u->m);
    short   artW = (short)(W * 2 / 5);            /* right ~40% for the art */
    short   textR = (short)(W - artW - 2 * MARGIN);
    short   x = MARGIN, y;

    render_fill(r, &full, FILL_BG);
    render_text_size(r, 12);

    if (!it) {
        render_text(r, MARGIN, (short)(H / 2), "(no item selected)", INK_DIM);
        render_hline(r, 0, (short)W, (short)(H - HINT_H));
        return;
    }

    render_text(r, x, 24, it->name, INK_TITLE);
    render_hline(r, 0, (short)W, (short)(HEADER_H - 1));

    y = (short)(HEADER_H + 16);
    {
        char num[16], meta[ITEM_VENDOR_LEN + 24];
        meta[0] = '\0';
        if (it->year > 0) { l2s(it->year, num); strcat(meta, num); }
        if (it->vendor[0]) { if (meta[0]) strcat(meta, " - "); strcat(meta, it->vendor); }
        if (meta[0]) { render_text(r, x, y, meta, INK_NORMAL); y = (short)(y + ROW_H); }
    }
    if (it->genre[0]) {
        char g[ITEM_GENRE_LEN + 8];
        strcpy(g, "Genre: ");
        strncat(g, it->genre, ITEM_GENRE_LEN);
        render_text(r, x, y, g, INK_DIM);
        y = (short)(y + ROW_H);
    }
    y = (short)(y + 8);
    if (it->desc[0]) {
        y = draw_wrapped(r, x, y, textR, ROW_H, it->desc, INK_NORMAL, 10);
    }

    if (u->previewPic) {
        Rect ar;
        SetRect(&ar, (short)(W - artW - MARGIN), (short)(HEADER_H + MARGIN),
                (short)(W - MARGIN), (short)(H - HINT_H - MARGIN));
        art_draw_fit(u->previewPic, &ar);
    } else {
        const char *base = art_base(u, it);
        if (!base || !base[0])
            render_text(r, (short)(W - artW), (short)(H / 2), "(no art)", INK_DIM);
    }

    render_hline(r, 0, (short)W, (short)(H - HINT_H));
    {
        const char *hint = "Return  launch        any other key  back";
        short hx = (short)((W - render_text_width(r, hint)) / 2);
        render_text(r, hx, (short)(H - 7), hint, INK_DIM);
    }
}

void ui_draw(Ui *u)
{
    render_begin(u->r, u->win);
    if (u->mode == UI_MODE_PREVIEW) {
        draw_preview(u);
    } else if (u->mode == UI_MODE_INFO) {
        draw_info(u);
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
        case 0: return UI_SHOW_FINDER;
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
    int cand[5], nc = 0, i, depth;

    if ((n >= 5 && strcmp(image + n - 5, ".pict") == 0) ||
        (n >= 4 && strcmp(image + n - 4, ".raw") == 0))
        return art_load(image);

    /* Pick the best available variant for the screen depth: the native depth
     * first, then *higher* depths (QuickDraw down-converts a deeper PICT to the
     * screen — docs/15, verified), then *lower* colour depths, and the 1-bit raw
     * last. So a single deep master (e.g. `<id>.24.pict`) covers every colour
     * screen, while a 1-bit screen still prefers the ordered-dither `<id>.1.raw`.
     * (Encoder depths: 1/4/8 indexed, 16/24 direct.) */
    depth = u->env->pixelSize;
    if      (depth <= 1)  { cand[nc++]=1;  cand[nc++]=8;  cand[nc++]=16; cand[nc++]=24; cand[nc++]=4; }
    else if (depth <= 4)  { cand[nc++]=4;  cand[nc++]=8;  cand[nc++]=16; cand[nc++]=24; cand[nc++]=1; }
    else if (depth <= 8)  { cand[nc++]=8;  cand[nc++]=16; cand[nc++]=24; cand[nc++]=4;  cand[nc++]=1; }
    else if (depth <= 16) { cand[nc++]=16; cand[nc++]=24; cand[nc++]=8;  cand[nc++]=4;  cand[nc++]=1; }
    else                  { cand[nc++]=24; cand[nc++]=16; cand[nc++]=8;  cand[nc++]=4;  cand[nc++]=1; }

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
    dispose_row_icons(u);                          /* icl8/ICN# variant is depth-specific */
}

/* Change the selected Settings row's value by `dir` (-1 / +1). Returns 1 if a
 * *persisted* preference (theme / volume) changed, so main can save prefs.
 * Color Depth is deliberately not persisted (docs/16: startup matches the OS
 * depth), so it returns 0. */
static int settings_adjust(Ui *u, int dir)
{
    switch (u->setSel) {
        case 0:                                    /* Theme (persisted) */
            render_toggle_theme(u->r);
            return 1;
        case 1: {                                  /* Color Depth (not persisted) */
            int i, cur = 0;
            for (i = 0; i < u->ndepths; i++)
                if (u->depths[i] == u->env->pixelSize) cur = i;
            cur += dir;
            if (cur < 0) cur = 0;
            if (cur >= u->ndepths) cur = u->ndepths - 1;
            if (u->ndepths > 0 && u->depths[cur] != u->env->pixelSize)
                apply_depth(u, u->depths[cur]);
            return 0;
        }
        case 2:                                    /* Volume (persisted) */
            if (u->vol >= 0) {
                int old = u->vol;
                u->vol += dir;
                if (u->vol < 0) u->vol = 0;
                if (u->vol > SOUND_VOL_MAX) u->vol = SOUND_VOL_MAX;
                sound_set_vol(u->vol);
                return (u->vol != old);
            }
            return 0;
        case 3:                                    /* Artwork: Box Art / Screenshot */
            u->artPref = u->artPref ? 0 : 1;
            if (u->listArt) { art_dispose(u->listArt); u->listArt = 0; }
            u->artFor = 0;                          /* force the inline pane to reload */
            return 1;                               /* persisted */
        case 4:                                    /* Startup Sound On/Off (persisted) */
            u->sndStartup = u->sndStartup ? 0 : 1;
            if (u->sndStartup) sound_play_file("sounds/startup", 1);   /* preview */
            return 1;
        default:                                   /* Shutdown Sound On/Off (persisted) */
            u->sndShutdown = u->sndShutdown ? 0 : 1;
            if (u->sndShutdown) sound_play_file("sounds/shutdown", 1); /* preview */
            return 1;
    }
}

UiCommand ui_key(Ui *u, char ch)
{
    UiCommand cmd = UI_NONE;

    /* Theme toggle works in any mode (list / menu / safe / preview). */
    if (ch == 't' || ch == 'T') {
        render_toggle_theme(u->r);
        ui_draw(u);
        return UI_PREFS_DIRTY;                      /* theme changed -> persist */
    }

    /* Settings panel: rows adjusted with arrows, Esc returns to the list. */
    if (u->mode == UI_MODE_SETTINGS) {
        int dirty = 0;
        switch (ch) {
            case kCharUp:    if (u->setSel > 0) u->setSel--; break;
            case kCharDown:  if (u->setSel < SET_N - 1) u->setSel++; break;
            case kCharLeft:  dirty = settings_adjust(u, -1); break;
            case kCharRight: dirty = settings_adjust(u, +1); break;
            case kCharReturn:
            case kCharEnter: dirty = settings_adjust(u, +1); break;
            case kCharEscape: u->mode = UI_MODE_LIST; break;   /* gear stays focused */
        }
        ui_draw(u);
        return dirty ? UI_PREFS_DIRTY : UI_NONE;
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

    /* More Info: Return launches the title; any other key returns to the list. */
    if (u->mode == UI_MODE_INFO) {
        art_dispose(u->previewPic);
        u->previewPic = 0;
        u->mode = UI_MODE_LIST;
        if ((ch == kCharReturn || ch == kCharEnter) && model_cur_item(u->m)) {
            u->status[0] = '\0';
            ui_draw(u);
            return UI_LAUNCH;
        }
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

    /* Per-item launch hotkey: a printable key mapped to a title launches it
     * immediately (doubles as a gamepad button via MiSTer's button->key map).
     * Checked before navigation/type-ahead; restricted to printable keys so it
     * never shadows the arrows / Return / Esc. (Note 't' is consumed by the
     * global theme toggle above, so it can't be a hotkey.) */
    if (!u->safe && (unsigned char)ch >= 0x20 && model_select_hotkey(u->m, ch)) {
        u->status[0] = '\0';
        ui_draw(u);
        return UI_LAUNCH;
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
                const char *base = it ? art_base(u, it) : 0;
                if (base && base[0]) {
                    u->previewPic = load_item_art(u, base);
                    u->mode = UI_MODE_PREVIEW;   /* draws "(no artwork)" if load failed */
                }
            }
            break;
        case 'i':
        case 'I':
            /* More Info card for the selection (loads its art, like preview). */
            if (!u->safe) {
                const CatItem *it = model_cur_item(u->m);
                if (it) {
                    const char *base = art_base(u, it);
                    if (base && base[0]) u->previewPic = load_item_art(u, base);
                    u->mode = UI_MODE_INFO;
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
