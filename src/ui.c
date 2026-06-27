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
#define CATLIST_W 140          /* width of the optional categories list panel (left) */

/* Esc-menu row kinds and their labels (indexed by kind). The visible set per run
 * is built into Ui::menuRows by ui_init (Finder rows omitted on the boot shell). */
enum { MROW_SETTINGS, MROW_SHOW_FINDER, MROW_EXIT, MROW_RESTART, MROW_SHUTDOWN };
static const char *kMenuLabel[] = { "Settings", "Show Finder", "Exit to Finder", "Restart", "Shut Down" };

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
    u->ncdevs = 0; u->cdevSel = 0; u->cdevTop = 0;           /* control-panel list */
    u->bgValid = 0;                                          /* force a full first paint */
    u->lastMode = -1;
    u->catList = 0;                                          /* categories list hidden */
    {
        int i;
        for (i = 0; i < MAX_ITEMS; i++) u->rowIcon[i] = 0;   /* lazy row-icon cache */
    }
    /* Build the Esc menu for this environment. "Show Finder" / "Exit to Finder"
     * only make sense when a separate Finder process exists to front or hand back
     * to — true on System 7 (we're a Startup Item) or System 6 + MultiFinder. On
     * the System-6 boot-shell build the launcher replaced the Finder, so those
     * rows are dropped (canLaunchReturn is false: no Process Manager / Finder). */
    {
        int k = 0;
        u->menuRows[k++] = MROW_SETTINGS;
        if (env->canLaunchReturn) {
            u->menuRows[k++] = MROW_SHOW_FINDER;
            u->menuRows[k++] = MROW_EXIT;
        }
        u->menuRows[k++] = MROW_RESTART;
        u->menuRows[k++] = MROW_SHUTDOWN;
        u->nmenu = k;
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

/* Per-game max colour depth (bpp) from the catalog; 0 = no cap. */
int ui_current_maxdepth(Ui *u)
{
    const CatItem *it = model_cur_item(u->m);
    return it ? it->maxDepth : 0;
}

const CtlPanel *ui_current_cdev(Ui *u)
{
    if (u->cdevSel < 0 || u->cdevSel >= u->ncdevs) return 0;
    return &u->cdevs[u->cdevSel];
}

/* ---- drawing -------------------------------------------------------------- */

static Art *load_item_art(Ui *u, const char *image);   /* defined below */

/* Load the current selection's detail art if it isn't already loaded for that
 * item. Returns 1 if it (re)loaded, 0 if already current. Drives both the
 * deferred path (ui_idle, so a fast scroll on the direct-draw System-6 build
 * doesn't lurch on each PICT decode) and the synchronous path (draw_carousel on
 * the off-screen System-7 build, so the cover appears in the same repaint as the
 * move instead of flashing in on a second pass). */
static int ensure_art_loaded(Ui *u)
{
    const CatItem *cur = model_cur_item(u->m);
    if (cur == u->artFor) return 0;            /* already loaded for this item */
    if (u->listArt) { art_dispose(u->listArt); u->listArt = 0; }
    if (cur) {
        const char *base = art_base(u, cur);
        if (base && base[0]) u->listArt = load_item_art(u, base);
    }
    u->artFor = cur;
    return 1;
}

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

/* Invalidate the per-item art caches after a category page loads. The paged
 * catalog reuses one items array for every category (main.c), so both caches
 * alias the *previous* page's items by reused storage: `artFor` is a CatItem*
 * (now pointing at a different title at the same address) and `rowIcon[]` is
 * keyed by the reused slot index. Without this, a new selection that lands on a
 * slot the old page also used keeps the previous item's cover / tile icon.
 * Called by main's PageLoader right after model_set_page. */
void ui_page_changed(Ui *u)
{
    if (u->listArt) { art_dispose(u->listArt); u->listArt = 0; }
    u->artFor = 0;
    dispose_row_icons(u);
}

#define ART_PANE_W 176          /* right-hand art pane width when wide enough */
#define ART_MIN_W  560          /* show the art pane only at this width or more */

/* Art-pane width: the classic 176 px up to 720 px wide (640x480 unchanged), then
 * a proportional ~27% (capped at 360) on bigger screens so the art isn't a sliver
 * at 800x600 / 1024x768. The list takes the rest of the width. */
static int art_pane_w(int W)
{
    int w;
    if (W <= 720) return ART_PANE_W;
    w = W * 27 / 100;
    if (w < ART_PANE_W) w = ART_PANE_W;
    if (w > 360) w = 360;
    return w;
}

#define GEAR_X 8                /* settings affordance: left edge */
#define GEAR_W 24               /* ...and reserved width (title starts after) */

/* Mouse affordances on the browse screen (docs/07): the carousel ◀▶ arrows and
 * the Launch button below the selected icon. Sizes are fixed so hit-testing in
 * ui_click() needs no font metrics (which aren't reliable outside a draw pass). */
#define ARROW_W  26
#define ARROW_H  28
#define LAUNCH_W 84
#define LAUNCH_H 22
#define CATBAND_HALF 80         /* clickable half-width of the centred "^ cat v" */

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

/* Draw one carousel tile: the item's app icon (depth-matched, cached) centered
 * at (cx, cy) in an sz x sz box, or a framed placeholder if it has no icon. */
static void draw_tile(Ui *u, int catIdx, const CatItem *it, short cx, short cy, short sz)
{
    Rect box;
    Art *ic;
    short h = (short)(sz / 2);
    SetRect(&box, (short)(cx - h), (short)(cy - h), (short)(cx + h), (short)(cy + h));
    ic = it->icon[0] ? row_icon(u, catIdx, it) : 0;
    if (ic) art_draw_fit(ic, &box);
    else    render_frame(u->r, &box);          /* icon-less item placeholder */
}

/* The browse screen: a horizontal icon carousel (Left/Right scroll the games,
 * the selected one enlarged in the centre; Up/Down change category) over a
 * detail panel (the selected game's art + name / developer / genre / blurb).
 * Reuses the lazily-cached app icons (carousel) and the depth-matched art
 * loader (detail pane), so a move is mostly cheap icon blits. */
/* Browse-screen geometry, computed once and shared by draw_carousel() (drawing)
 * and ui_click() (hit-testing) so the drawn pixels and the clickable rects can
 * never drift apart. Holds the band rects, the centre/side tile metrics, and the
 * mouse-control rects (gear, category band, the ◀▶ arrows, the Launch button). */
typedef struct {
    int   W, H;
    short clx;                       /* content left edge (cat list or 0)        */
    short carTop, carBot, detTop, detBot;
    int   hasItems, count, center;
    short cx, iconCy, centSz, half, sideSz, sideGap;
    int   nside;
    Rect  gear, catBand, leftArrow, rightArrow, launchBtn;
    short catMidX;                   /* split: click < = prev cat, >= = next cat */
} CarLayout;

static void carousel_layout(Ui *u, CarLayout *L)
{
    int       W = win_w(u), H = win_h(u);
    ModelCat *cat = model_cur_cat(u->m);
    int       usable = H - HEADER_H - HINT_H;
    int       carH   = usable * 42 / 100;
    if (carH < 120) carH = 120;

    L->W = W; L->H = H;
    L->clx    = (short)(u->catList ? CATLIST_W : 0);
    L->carTop = HEADER_H;
    L->carBot = (short)(HEADER_H + carH);
    L->detTop = (short)(L->carBot + 1);
    L->detBot = (short)(H - HINT_H);
    L->count  = cat ? cat->count : 0;
    L->center = u->m->curItem;
    L->hasItems = (cat && cat->count > 0);

    SetRect(&L->gear, (short)(GEAR_X - 4), 3,
            (short)(GEAR_X + GEAR_W - 4), (short)(HEADER_H - 4));
    L->catMidX = (short)(W / 2);
    SetRect(&L->catBand, (short)(W / 2 - CATBAND_HALF), 2,
            (short)(W / 2 + CATBAND_HALF), (short)(HEADER_H - 2));

    SetRect(&L->leftArrow,  0, 0, 0, 0);
    SetRect(&L->rightArrow, 0, 0, 0, 0);
    SetRect(&L->launchBtn,  0, 0, 0, 0);
    L->cx = L->iconCy = L->centSz = L->half = L->sideSz = 0;
    L->sideGap = 8; L->nside = 0;

    if (L->hasItems) {
        short cx     = (short)((L->clx + W) / 2);
        short iconCy = (short)(L->carTop + (carH - 22) / 2);
        short centSz = (short)(carH - 44);
        short sideSz, half, top;
        int   nside;
        if (centSz > 76) centSz = 76;
        if (centSz < 40) centSz = 40;
        sideSz = (short)(centSz * 9 / 16);
        half   = (short)(centSz / 2);
        /* leave room for an arrow button at each edge so tiles never sit under it */
        nside  = ((W - L->clx) / 2 - half - MARGIN - ARROW_W) / (sideSz + L->sideGap);
        if (nside > 2) nside = 2;   /* a fixed 5-up carousel: centre + 2 each side */
        if (nside < 1) nside = 1;

        L->cx = cx; L->iconCy = iconCy; L->centSz = centSz;
        L->half = half; L->sideSz = sideSz; L->nside = nside;

        SetRect(&L->leftArrow,  (short)(L->clx + MARGIN), (short)(iconCy - ARROW_H / 2),
                (short)(L->clx + MARGIN + ARROW_W), (short)(iconCy + ARROW_H / 2));
        SetRect(&L->rightArrow, (short)(W - MARGIN - ARROW_W), (short)(iconCy - ARROW_H / 2),
                (short)(W - MARGIN), (short)(iconCy + ARROW_H / 2));
        top = (short)(iconCy + half + 6);
        SetRect(&L->launchBtn, (short)(cx - LAUNCH_W / 2), top,
                (short)(cx + LAUNCH_W / 2), (short)(top + LAUNCH_H));
    }
}

static void draw_carousel(Ui *u)
{
    Render        *r   = u->r;
    Model         *m   = u->m;
    ModelCat      *cat = model_cur_cat(m);
    const CatItem *cur = model_cur_item(m);
    Rect           full = u->win->portRect;
    char           num[16];
    CarLayout      L;
    int            W, H;
    short          clx, carTop, carBot, detTop, detBot;

    carousel_layout(u, &L);
    W = L.W; H = L.H; clx = L.clx;
    carTop = L.carTop; carBot = L.carBot; detTop = L.detTop; detBot = L.detBot;

    render_fill(r, &full, FILL_BG);

    /* ---- optional categories list (left panel) ---- */
    if (u->catList) {
        short sy = HEADER_H, sb = (short)(H - HINT_H);
        int   rows = (sb - sy - 4) / ROW_H;
        int   top, i;
        Rect  sr;
        SetRect(&sr, 0, sy, CATLIST_W, sb);
        render_fill(r, &sr, FILL_PANEL);
        render_frame(r, &sr);
        top = m->curCat - rows / 2;
        if (top + rows > m->ncats) top = m->ncats - rows;
        if (top < 0) top = 0;
        for (i = 0; i < rows; i++) {
            int   ci = top + i;
            short y0 = (short)(sy + 2 + i * ROW_H), base = (short)(y0 + ROW_H - 5);
            int   sel;
            char  nm[ITEM_CAT_LEN];
            if (ci >= m->ncats) break;
            sel = (ci == m->curCat);
            SetRect(&sr, 2, y0, (short)(CATLIST_W - 2), (short)(y0 + ROW_H));
            render_fill(r, &sr, sel ? FILL_SEL : FILL_PANEL);
            strncpy(nm, m->cats[ci].name, sizeof nm - 1); nm[sizeof nm - 1] = '\0';
            while (nm[0] && render_text_width(r, nm) > CATLIST_W - 14) nm[strlen(nm) - 1] = '\0';
            render_text(r, 8, base, nm, sel ? INK_SELECTED : INK_NORMAL);
        }
    }

    /* Off-screen (System 7+) path: load the selected cover *now* so it lands in
     * this same repaint as the move — no deferred second pass that flashes it in.
     * Cheap: the compositing GWorld blits once and the size-capped PICTs decode
     * fast. The direct-draw System-6 path skips this and keeps the deferred load
     * (ui_idle) so a fast scroll there never lurches on a per-move PICT decode;
     * its detail pane just stays empty until the selection settles. */
    if (u->r->useOffscreen) ensure_art_loaded(u);

    /* ---- header: gear, title, the category (with ^v), and N / M ---- */
    render_text_size(r, 12);
    draw_settings_btn(u);
    render_text(r, GEAR_X + GEAR_W, 20, "MacAtrium", INK_TITLE);
    if (cat) {
        char  cl[ITEM_CAT_LEN + 8];
        short cw;
        strcpy(cl, "^ "); strcat(cl, cat->name); strcat(cl, " v");
        cw = render_text_width(r, cl);
        render_text(r, (short)((W - cw) / 2), 20, cl, INK_NORMAL);
        {
            char line[24];
            l2s(m->curItem + 1, num); strcpy(line, num);
            strcat(line, " / ");
            l2s(cat->count, num); strcat(line, num);
            render_text(r, (short)(W - MARGIN - render_text_width(r, line)), 20, line, INK_DIM);
        }
    }
    render_hline(r, 0, (short)W, (short)(HEADER_H - 1));

    /* ---- carousel band (in the content area to the right of any cat list) ---- */
    if (!cat || cat->count == 0) {
        const char *t = "(no items in this category)";
        render_text(r, (short)(clx + (W - clx - render_text_width(r, t)) / 2),
                    (short)((carTop + carBot) / 2), t, INK_DIM);
    } else {
        int   center  = L.center;
        short cx      = L.cx;
        short iconCy  = L.iconCy;
        short centSz  = L.centSz;
        short sideSz  = L.sideSz, sideGap = L.sideGap, half = L.half;
        int   nside   = L.nside, k;

        /* side tiles, WRAPPING so the carousel always looks full and continuous:
         * the slots before the first item show the category's LAST items (and
         * vice versa), so the default view reads as a carousel you can scroll
         * either way. A `seen` guard stops a small category (fewer items than
         * slots) from drawing the same item twice. Inner slots fill first. */
        {
            char seen[MAX_CAT_ITEMS];
            int  cnt = cat->count;
            memset(seen, 0, sizeof seen);
            if (center >= 0 && center < cnt) seen[center] = 1;
            for (k = 1; k <= nside; k++) {
                short off = (short)(half + sideGap + sideSz / 2 + (k - 1) * (sideSz + sideGap));
                int   li  = ((center - k) % cnt + cnt) % cnt;
                int   ri  = (center + k) % cnt;
                if (!seen[li]) {
                    seen[li] = 1;
                    draw_tile(u, cat->idx[li], &m->cat->items[cat->idx[li]],
                              (short)(cx - off), iconCy, sideSz);
                }
                if (!seen[ri]) {
                    seen[ri] = 1;
                    draw_tile(u, cat->idx[ri], &m->cat->items[cat->idx[ri]],
                              (short)(cx + off), iconCy, sideSz);
                }
            }
        }
        /* centre tile: ALWAYS the app's own icon at the current screen depth — it
         * never swaps to the box art. The hero cover lives only in the detail pane
         * below; drawing it here too made the centre flip icon->cover when ui_idle
         * finished loading (and, on the direct-draw 6.0.8 path, painted the cover
         * twice per repaint — the "multiple passes"). A stable icon column avoids
         * both. */
        draw_tile(u, cat->idx[center], cur, cx, iconCy, centSz);
        {   /* 2px square selection box around the centred (selected) icon */
            Rect f;
            short pad = (short)(half + 5);
            SetRect(&f, (short)(cx - pad), (short)(iconCy - pad),
                    (short)(cx + pad), (short)(iconCy + pad));
            render_frame(r, &f);
            InsetRect(&f, 1, 1);
            render_frame(r, &f);
        }
        /* ◀▶ navigation arrows flanking the carousel. Both stay active because the
         * carousel wraps (there's always somewhere to scroll); dim only a 1-item
         * category, where there's nothing to move to. */
        {
            short tw;
            short ink = cat->count > 1 ? INK_NORMAL : INK_DIM;
            render_frame(r, &L.leftArrow);
            tw = render_text_width(r, "<");
            render_text(r, (short)(L.leftArrow.left + (ARROW_W - tw) / 2),
                        (short)(iconCy + 5), "<", ink);
            render_frame(r, &L.rightArrow);
            tw = render_text_width(r, ">");
            render_text(r, (short)(L.rightArrow.left + (ARROW_W - tw) / 2),
                        (short)(iconCy + 5), ">", ink);
        }
        /* Launch button directly below the selected icon */
        {
            short tw = render_text_width(r, "Launch");
            render_fill(r, &L.launchBtn, FILL_SEL);
            render_frame(r, &L.launchBtn);
            render_text(r, (short)(L.launchBtn.left + (LAUNCH_W - tw) / 2),
                        (short)(L.launchBtn.top + LAUNCH_H - 6), "Launch", INK_SELECTED);
        }
        if (cur) {
            short nw = render_text_width(r, cur->name);
            render_text(r, (short)(clx + (W - clx - nw) / 2), (short)(carBot - 6),
                        cur->name, INK_TITLE);
        }
    }
    render_hline(r, clx, (short)W, carBot);

    /* ---- detail band: art (left) + name / meta / blurb (right) ---- */
    {
        short artW = (short)((W - clx) * 2 / 5);
        short ax0, ay0, ax1, ay1;
        Rect  ar;
        if (artW > 220) artW = 220;
        ax0 = (short)(clx + MARGIN); ay0 = (short)(detTop + MARGIN);
        ax1 = (short)(ax0 + artW); ay1 = (short)(detBot - MARGIN);
        SetRect(&ar, ax0, ay0, ax1, ay1);
        render_frame(r, &ar);
        {
            /* Cover is loaded synchronously above (off-screen path) or by ui_idle
             * once the selection settles (direct-draw path). Until it has run for
             * this item the frame stays empty — no flash; "(no art)" shows only
             * after a load found nothing. */
            int loaded = (cur && cur == u->artFor);   /* cover loaded for this item */
            if (loaded && u->listArt) {
                Rect inner;
                SetRect(&inner, (short)(ax0 + 4), (short)(ay0 + 4),
                        (short)(ax1 - 4), (short)(ay1 - 4));
                art_draw_fit(u->listArt, &inner);
            } else if (loaded) {
                const char *t = "(no art)";
                render_text(r, (short)(ax0 + (artW - render_text_width(r, t)) / 2),
                            (short)((ay0 + ay1) / 2), t, INK_DIM);
            }
        }
        if (cur) {
            short tx   = (short)(ax1 + 2 * MARGIN);
            short ty   = (short)(detTop + 18);
            short maxw = (short)(W - tx - MARGIN);
            char  buf[ITEM_VENDOR_LEN + 24];

            /* title */
            render_text(r, tx, ty, cur->name, INK_TITLE);
            ty = (short)(ty + ROW_H + 3);

            /* year - developer */
            buf[0] = '\0';
            if (cur->year > 0) { l2s(cur->year, num); strcat(buf, num); }
            if (cur->vendor[0]) {
                if (buf[0]) strcat(buf, " - ");
                strncat(buf, cur->vendor, sizeof buf - strlen(buf) - 1);
            }
            if (buf[0]) { render_text(r, tx, ty, buf, INK_DIM); ty = (short)(ty + ROW_H); }

            /* genre on its own line */
            if (cur->genre[0]) {
                char g[ITEM_GENRE_LEN + 8];
                strcpy(g, "Genre: ");
                strncat(g, cur->genre, ITEM_GENRE_LEN);
                render_text(r, tx, ty, g, INK_DIM);
                ty = (short)(ty + ROW_H);
            }
            ty = (short)(ty + 6);

            /* description (a transient status line wins) */
            if (u->status[0])
                render_text(r, tx, ty, u->status, INK_NORMAL);
            else if (cur->desc[0])
                draw_wrapped(r, tx, ty, maxw, ROW_H, cur->desc, INK_NORMAL, 7);

            /* tags: the navigational categories, along the panel's last line */
            if (cur->ncats > 0) {
                char tags[ITEM_CAT_LEN * 5];
                int  ci;
                tags[0] = '\0';
                for (ci = 0; ci < cur->ncats; ci++) {
                    if (strlen(tags) + strlen(cur->cats[ci]) + 4 >= sizeof tags) break;
                    if (tags[0]) strcat(tags, " - ");
                    strcat(tags, cur->cats[ci]);
                }
                if (tags[0]) render_text(r, tx, (short)(detBot - 8), tags, INK_DIM);
            }
        }
    }

    /* ---- hint bar ---- */
    render_hline(r, 0, (short)W, (short)(H - HINT_H));
    {
        const char *hint = u->settingsFocus
            ? "Settings:   Return  open       v  back"
            : "<- ->  game    ^ v  category    Return  play    I  info   P  art   Esc  menu";
        short x = (short)((W - render_text_width(r, hint)) / 2);
        if (x < MARGIN) x = MARGIN;
        render_text(r, x, (short)(H - 7), hint, INK_DIM);
    }
}

/* The Settings panel: a list of adjustable rows (Theme / Color Depth / Volume /
 * Artwork / Startup Sound / Shutdown Sound) plus a Control Panels action row.
 * Up/Down move rows; Left/Right (and Return) change the selected row's value;
 * Return on the last row opens the Control Panels list. */
#define SET_N 8
#define SET_ROW_CTLPANELS (SET_N - 1)   /* the action row (opens the cdev list) */

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
        case 5:
            strcpy(out, "Shutdown Sound");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, u->sndShutdown ? "On" : "Off");
            break;
        case 6:
            strcpy(out, "Categories");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, u->catList ? "Shown" : "Hidden");
            break;
        default:
            strcpy(out, "Control Panels");
            while (strlen(out) < 16) strcat(out, " ");
            strcat(out, "Open >");
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
        /* Contextual bottom hint: the clip-length note on the sound rows, the
         * open hint on Control Panels, else the nav keys. */
        const char *hint;
        short x;
        if (u->setSel == 4 || u->setSel == 5)
            hint = "Sounds: a clip baked in the image, max 7 sec";
        else if (u->setSel == SET_ROW_CTLPANELS)
            hint = "Return  open the Control Panels list";
        else
            hint = "^v row   <> change   Esc back";
        x = (short)(px + (pw - render_text_width(r, hint)) / 2);
        if (x < (short)(px + 4)) x = (short)(px + 4);
        render_text(r, x, (short)(py + ph - 7), hint, INK_DIM);
    }
}

/* The Control Panels list (reached from Settings -> Control Panels): a scrollable
 * panel of the System's `cdev` files; Return opens the selected one via the
 * Finder, Esc returns to Settings. */
static void draw_ctlpanels(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    int     pw = 320;
    int     rows = 9;                       /* visible list rows */
    int     ph = rows * (ROW_H + 2) + 56;
    short   px = (short)((W - pw) / 2);
    short   py = (short)((H - ph) / 2);
    Rect    panel, rr;
    int     i;

    SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
    render_fill(r, &panel, FILL_PANEL);
    render_frame(r, &panel);

    render_text_size(r, 12);
    render_text(r, (short)(px + MARGIN), (short)(py + 22), "Control Panels", INK_TITLE);
    render_hline(r, px, (short)(px + pw), (short)(py + 30));

    /* keep the selection in view */
    if (u->cdevSel < u->cdevTop) u->cdevTop = u->cdevSel;
    else if (u->cdevSel >= u->cdevTop + rows) u->cdevTop = u->cdevSel - rows + 1;
    if (u->cdevTop < 0) u->cdevTop = 0;

    if (u->ncdevs == 0) {
        render_text(r, (short)(px + MARGIN), (short)(py + 52),
                    "(none found)", INK_DIM);
    } else {
        for (i = 0; i < rows; i++) {
            int   idx = u->cdevTop + i;
            short y0  = (short)(py + 38 + i * (ROW_H + 2));
            short base = (short)(y0 + ROW_H - 5);
            int   sel = (idx == u->cdevSel);
            char  nm[64];
            const unsigned char *p;
            int   k;
            if (idx >= u->ncdevs) break;
            p = u->cdevs[idx].name;
            for (k = 0; k < p[0] && k < 63; k++) nm[k] = (char)p[k + 1];
            nm[k] = '\0';
            SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
            render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);
            render_text(r, (short)(px + MARGIN), base, nm, sel ? INK_SELECTED : INK_NORMAL);
        }
    }

    render_hline(r, px, (short)(px + pw), (short)(py + ph - 22));
    {
        const char *hint = "^v select   Return  open   Esc  back";
        short x = (short)(px + (pw - render_text_width(r, hint)) / 2);
        render_text(r, x, (short)(py + ph - 7), hint, INK_DIM);
    }
}

static void draw_menu(Ui *u)
{
    Render *r = u->r;
    int     W = win_w(u), H = win_h(u);
    int     pw = 240;
    int     ph = u->nmenu * (ROW_H + 4) + 44;
    short   px = (short)((W - pw) / 2);
    short   py = (short)((H - ph) / 2);
    Rect    panel, rr;
    int     i;

    SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
    render_fill(r, &panel, FILL_PANEL);
    render_frame(r, &panel);

    render_text(r, (short)(px + MARGIN), (short)(py + 22), "MacAtrium  Menu", INK_TITLE);
    render_hline(r, px, (short)(px + pw), (short)(py + 30));

    for (i = 0; i < u->nmenu; i++) {
        short y0   = (short)(py + 36 + i * (ROW_H + 4));
        short base = (short)(y0 + ROW_H - 5);
        int   sel  = (i == u->menuSel);
        SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
        render_fill(r, &rr, sel ? FILL_SEL : FILL_PANEL);
        render_text(r, (short)(px + MARGIN), base, kMenuLabel[u->menuRows[i]],
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
    /* Keep the colour backend matched to the live screen depth on *every* repaint,
     * including ones that bypass ui_idle's poll — e.g. the updateEvt the system
     * posts when the screen depth changes (our startup 1-bit->8-bit bump). Without
     * this, that first repaint paints with the stale (B&W) backend and the screen
     * stays black until the user happens to press a key. Lightweight: only re-fits
     * when the depth actually differs; ui_idle still does the full re-fit (art and
     * icon caches are depth-specific). */
    {
        short now = display_current_depth();
        if (u->r->depth != now) render_reset_for_depth(u->r, u->env, now);
    }

    render_begin(u->r, u->win);
    /* On a mode change, force one carousel repaint only when leaving a view that
     * drew *over* the carousel (an overlay panel or a full-screen card) so that
     * panel gets erased. Opening an overlay from the clean LIST/SAFE carousel
     * needs none: the carousel is already in the GWorld, so the panel just
     * composites on top — no needless re-decode of every icon + the cover. */
    if (u->mode != u->lastMode) {
        if (u->lastMode != UI_MODE_LIST)   /* LIST = clean carousel or safe screen */
            u->bgValid = 0;
        u->lastMode = u->mode;
    }
    if (u->mode == UI_MODE_PREVIEW) {
        draw_preview(u);
        u->bgValid = 0;                 /* full-screen view -> carousel not in GWorld */
    } else if (u->mode == UI_MODE_INFO) {
        draw_info(u);
        u->bgValid = 0;
    } else {
        /* The menu / settings / control-panels panels are overlays. When only the
         * overlay's selection changed, the carousel behind it is already in the
         * GWorld, so skip repainting it — that's what made menu Up/Down lurch. */
        int overlay = (u->mode == UI_MODE_MENU || u->mode == UI_MODE_SETTINGS ||
                       u->mode == UI_MODE_CTLPANELS);
        if (!overlay || !u->bgValid) {
            if (u->safe) draw_safe(u);
            else         draw_carousel(u);
            u->bgValid = 1;
        }
        if (u->mode == UI_MODE_MENU)
            draw_menu(u);
        else if (u->mode == UI_MODE_SETTINGS)
            draw_settings(u);
        else if (u->mode == UI_MODE_CTLPANELS)
            draw_ctlpanels(u);
    }
    render_end(u->r, u->win);
}

int ui_idle(Ui *u)
{
    /* Pick up a screen-depth change made *outside* MacAtrium — the Monitors
     * control panel, or the emulator's own video setting — so colour engages
     * (or B&W falls back) without a trip through our Settings. Our own Settings
     * path goes through apply_depth(); this covers everything else. On a machine
     * with no Color QD (or only 1-bit available) display_current_depth() stays
     * at 1, so this is a no-op. Checked every idle tick (GetMainDevice is cheap). */
    {
        short now = display_current_depth();
        if (now != u->env->pixelSize) {
            u->env->pixelSize = now;
            u->env->useColor  = (u->env->hasColorQD && now >= 4);
            render_reset_for_depth(u->r, u->env, now);
            if (u->listArt) { art_dispose(u->listArt); u->listArt = 0; }
            u->artFor = 0;                       /* art variant is depth-specific  */
            dispose_row_icons(u);                /* icl8/ICN# variant too          */
            u->bgValid = 0;                      /* whole screen must repaint      */
            return 1;
        }
    }

    if (u->safe || u->mode != UI_MODE_LIST) return 0;  /* only the carousel loads art */
    return ensure_art_loaded(u);                       /* deferred load (System 6) */
}

/* ---- input ---------------------------------------------------------------- */

static UiCommand menu_select(Ui *u)
{
    switch (u->menuRows[u->menuSel]) {
        case MROW_SETTINGS:    u->mode = UI_MODE_SETTINGS; u->setSel = 0; return UI_NONE;
        case MROW_SHOW_FINDER: u->mode = UI_MODE_LIST; return UI_SHOW_FINDER;
        case MROW_EXIT:        u->mode = UI_MODE_LIST; return UI_QUIT;
        case MROW_RESTART:     u->mode = UI_MODE_LIST; return UI_RESTART;
        case MROW_SHUTDOWN:    u->mode = UI_MODE_LIST; return UI_SHUTDOWN;
    }
    u->mode = UI_MODE_LIST;
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
    /* Persist it as the boot default in slot PRAM so the system comes up here next
     * time (the launcher then just matches it). main also saves the depth to prefs. */
    (void)display_set_default_depth(u->env->pixelSize);
    u->env->useColor  = (u->env->hasColorQD && u->env->pixelSize >= 4);
    render_reset_for_depth(u->r, u->env, u->env->pixelSize);
    if (u->listArt) {                              /* art variant is depth-specific */
        art_dispose(u->listArt); u->listArt = 0; u->artFor = 0;
    }
    dispose_row_icons(u);                          /* icl8/ICN# variant is depth-specific */
    u->bgValid = 0;                                /* whole screen must repaint */
}

/* Change the selected Settings row's value by `dir` (-1 / +1). Returns 1 if a
 * *persisted* preference (theme / volume / colour depth) changed, so main can
 * save prefs. Colour depth now persists via slot PRAM (apply_depth →
 * display_set_default_depth) plus the prefs `depth` key (docs/15). */
static int settings_adjust(Ui *u, int dir)
{
    switch (u->setSel) {
        case 0:                                    /* Theme (persisted) */
            render_toggle_theme(u->r);
            u->bgValid = 0;                         /* carousel colours changed */
            return 1;
        case 1: {                                  /* Color Depth (persisted) */
            int i, cur = 0;
            for (i = 0; i < u->ndepths; i++)
                if (u->depths[i] == u->env->pixelSize) cur = i;
            cur += dir;
            if (cur < 0) cur = 0;
            if (cur >= u->ndepths) cur = u->ndepths - 1;
            if (u->ndepths > 0 && u->depths[cur] != u->env->pixelSize) {
                apply_depth(u, u->depths[cur]);
                return 1;                           /* persist the new boot depth */
            }
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
            u->artFor = 0;                          /* force the detail pane to reload */
            u->bgValid = 0;                         /* carousel art changes */
            return 1;                               /* persisted */
        case 4:                                    /* Startup Sound On/Off (persisted) */
            u->sndStartup = u->sndStartup ? 0 : 1;
            if (u->sndStartup) sound_play_file("sounds/startup", 1);   /* preview */
            return 1;
        case 5:                                    /* Shutdown Sound On/Off (persisted) */
            u->sndShutdown = u->sndShutdown ? 0 : 1;
            if (u->sndShutdown) sound_play_file("sounds/shutdown", 1); /* preview */
            return 1;
        default:                                   /* Categories list Show/Hide (persisted) */
            u->catList = u->catList ? 0 : 1;
            u->bgValid = 0;                         /* browse layout changes */
            return 1;
    }
}

UiCommand ui_key(Ui *u, char ch)
{
    UiCommand cmd = UI_NONE;

    /* Theme toggle works in any mode (list / menu / safe / preview). */
    if (ch == 't' || ch == 'T') {
        render_toggle_theme(u->r);
        u->bgValid = 0;                             /* colours changed everywhere */
        ui_draw(u);
        return UI_PREFS_DIRTY;                      /* theme changed -> persist */
    }

    /* Settings panel: rows adjusted with arrows, Esc returns to the list. */
    if (u->mode == UI_MODE_SETTINGS) {
        int dirty = 0;
        /* The Control Panels row is an action: Return/Right opens the cdev list
         * (enumerated fresh each time). */
        if (u->setSel == SET_ROW_CTLPANELS &&
            (ch == kCharReturn || ch == kCharEnter || ch == kCharRight)) {
            u->ncdevs  = ctlpanels_list(u->cdevs, CTLPANEL_MAX);
            u->cdevSel = 0;
            u->cdevTop = 0;
            u->mode = UI_MODE_CTLPANELS;
            ui_draw(u);
            return UI_NONE;
        }
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

    /* Control Panels list: navigate, Return opens via the Finder, Esc back. */
    if (u->mode == UI_MODE_CTLPANELS) {
        switch (ch) {
            case kCharUp:    if (u->cdevSel > 0) u->cdevSel--; break;
            case kCharDown:  if (u->cdevSel < u->ncdevs - 1) u->cdevSel++; break;
            case kCharReturn:
            case kCharEnter:
                if (u->ncdevs > 0) { ui_draw(u); return UI_OPEN_CDEV; }
                break;
            case kCharEscape: u->mode = UI_MODE_SETTINGS; break;
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
            case kCharDown:  if (u->menuSel < u->nmenu - 1) u->menuSel++; break;
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
        /* Carousel nav: Left/Right scroll games, Up/Down change category. */
        case kCharLeft:  if (!u->safe) model_move_item(u->m, -1); break;
        case kCharRight: if (!u->safe) model_move_item(u->m, +1); break;
        case kCharUp:
            /* Up past the first category focuses the Settings gear. */
            if (!u->safe && !model_move_cat(u->m, -1)) u->settingsFocus = 1;
            break;
        case kCharDown:  if (!u->safe) model_move_cat(u->m, +1); break;
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
    /* Launching hands off to the game and tears down the launcher window, so the
     * pre-launch repaint was a wasted full-screen blit the user saw flash by. */
    if (cmd != UI_LAUNCH) ui_draw(u);
    return cmd;
}

/* Mouse equivalent of ui_key: hit-test a window-local click (main does the
 * GlobalToLocal) and return the same UiCommand the keyboard path would, redrawing
 * first so the click gives immediate feedback. Mirrors ui_key's per-mode shape.
 * Browse screen: the gear opens the menu hub; the ◀▶ arrows and a side tile move
 * the selection; the centred "^ cat v" band changes category (left half = prev,
 * right = next); the Launch button launches. Modal panels hit-test their rows and
 * dismiss on a click outside the panel (the universal mouse "Back"). */
UiCommand ui_click(Ui *u, Point pt)
{
    /* Safe (no-catalog) screen: a click opens the Esc menu so Restart / Shut Down
     * stay reachable with the mouse. */
    if (u->safe) {
        u->mode = UI_MODE_MENU; u->menuSel = 0;
        ui_draw(u);
        return UI_NONE;
    }

    if (u->mode == UI_MODE_MENU) {
        int   W = win_w(u), H = win_h(u);
        int   pw = 240, ph = u->nmenu * (ROW_H + 4) + 44;
        short px = (short)((W - pw) / 2), py = (short)((H - ph) / 2);
        Rect  panel;
        int   i;
        SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
        if (!PtInRect(pt, &panel)) { u->mode = UI_MODE_LIST; ui_draw(u); return UI_NONE; }
        for (i = 0; i < u->nmenu; i++) {
            short y0 = (short)(py + 36 + i * (ROW_H + 4));
            Rect  rr;
            SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
            if (PtInRect(pt, &rr)) {
                UiCommand c;
                u->menuSel = i;
                c = menu_select(u);
                ui_draw(u);
                return c;
            }
        }
        ui_draw(u);
        return UI_NONE;
    }

    if (u->mode == UI_MODE_SETTINGS) {
        int   W = win_w(u), H = win_h(u);
        int   pw = 320, ph = SET_N * (ROW_H + 6) + 56;
        short px = (short)((W - pw) / 2), py = (short)((H - ph) / 2);
        Rect  panel;
        int   i, dirty = 0;
        SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
        if (!PtInRect(pt, &panel)) { u->mode = UI_MODE_LIST; ui_draw(u); return UI_NONE; }
        for (i = 0; i < SET_N; i++) {
            short y0 = (short)(py + 40 + i * (ROW_H + 6));
            Rect  rr;
            SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
            if (PtInRect(pt, &rr)) {
                if (i == SET_ROW_CTLPANELS) {        /* action row: open the cdev list */
                    u->ncdevs  = ctlpanels_list(u->cdevs, CTLPANEL_MAX);
                    u->cdevSel = 0; u->cdevTop = 0;
                    u->mode = UI_MODE_CTLPANELS;
                } else if (i == u->setSel) {         /* re-click the selected row adjusts */
                    dirty = settings_adjust(u, +1);
                } else {                             /* first click just selects */
                    u->setSel = i;
                }
                break;
            }
        }
        ui_draw(u);
        return dirty ? UI_PREFS_DIRTY : UI_NONE;
    }

    if (u->mode == UI_MODE_CTLPANELS) {
        int   W = win_w(u), H = win_h(u);
        int   pw = 320, rows = 9, ph = rows * (ROW_H + 2) + 56;
        short px = (short)((W - pw) / 2), py = (short)((H - ph) / 2);
        Rect  panel;
        int   i;
        SetRect(&panel, px, py, (short)(px + pw), (short)(py + ph));
        if (!PtInRect(pt, &panel)) { u->mode = UI_MODE_SETTINGS; ui_draw(u); return UI_NONE; }
        for (i = 0; i < rows; i++) {
            int   idx = u->cdevTop + i;
            short y0  = (short)(py + 38 + i * (ROW_H + 2));
            Rect  rr;
            if (idx >= u->ncdevs) break;
            SetRect(&rr, (short)(px + 4), y0, (short)(px + pw - 4), (short)(y0 + ROW_H));
            if (PtInRect(pt, &rr)) {
                if (idx == u->cdevSel) { ui_draw(u); return UI_OPEN_CDEV; }  /* re-click opens */
                u->cdevSel = idx;
                break;
            }
        }
        ui_draw(u);
        return UI_NONE;
    }

    /* Full-screen info / preview cards: any click returns to the list. */
    if (u->mode == UI_MODE_INFO || u->mode == UI_MODE_PREVIEW) {
        if (u->previewPic) { art_dispose(u->previewPic); u->previewPic = 0; }
        u->mode = UI_MODE_LIST;
        ui_draw(u);
        return UI_NONE;
    }

    /* ---- browse screen (UI_MODE_LIST) ---- */
    {
        CarLayout L;
        carousel_layout(u, &L);
        u->settingsFocus = 0;                        /* a click takes focus off the gear */

        if (PtInRect(pt, &L.gear)) {                 /* gear -> the menu hub */
            u->mode = UI_MODE_MENU; u->menuSel = 0;
            ui_draw(u);
            return UI_NONE;
        }
        if (PtInRect(pt, &L.catBand)) {              /* "^ cat v": left half prev, right next */
            model_move_cat(u->m, pt.h < L.catMidX ? -1 : +1);
            ui_draw(u);
            return UI_NONE;
        }
        if (L.hasItems) {
            if (PtInRect(pt, &L.launchBtn) && model_cur_item(u->m)) {
                u->status[0] = '\0';
                ui_draw(u);
                return UI_LAUNCH;
            }
            if (PtInRect(pt, &L.leftArrow))  { model_move_item(u->m, -1); ui_draw(u); return UI_NONE; }
            if (PtInRect(pt, &L.rightArrow)) { model_move_item(u->m, +1); ui_draw(u); return UI_NONE; }
            {   /* a visible side tile -> jump the selection to it */
                int k;
                for (k = 1; k <= L.nside; k++) {
                    short off = (short)(L.half + L.sideGap + L.sideSz / 2 +
                                        (k - 1) * (L.sideSz + L.sideGap));
                    short h = (short)(L.sideSz / 2);
                    Rect  lt, rt;
                    SetRect(&lt, (short)(L.cx - off - h), (short)(L.iconCy - h),
                            (short)(L.cx - off + h), (short)(L.iconCy + h));
                    SetRect(&rt, (short)(L.cx + off - h), (short)(L.iconCy - h),
                            (short)(L.cx + off + h), (short)(L.iconCy + h));
                    if (L.center - k >= 0 && PtInRect(pt, &lt)) {
                        model_move_item(u->m, -k); ui_draw(u); return UI_NONE;
                    }
                    if (L.center + k < L.count && PtInRect(pt, &rt)) {
                        model_move_item(u->m, +k); ui_draw(u); return UI_NONE;
                    }
                }
            }
        }
        ui_draw(u);
        return UI_NONE;
    }
}
