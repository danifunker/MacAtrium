/*
 * main.c — MacAtrium launcher entry point: Toolbox init, environment probe,
 * full-screen window, catalog load (or the safe screen), and the resident event
 * loop. The shell never "quits" — exits are Launch Finder / Restart / Shut Down
 * (docs/03). MVP creator 'ATRM', type 'APPL'.
 */
#include <Quickdraw.h>
#include <Fonts.h>
#include <Windows.h>
#include <Menus.h>
#include <TextEdit.h>
#include <Dialogs.h>
#include <Events.h>
#include <Processes.h>
#include <Memory.h>
#include <AppleEvents.h>

#include "env.h"
#include "macfs.h"
#include "catalog.h"
#include "model.h"
#include "render.h"
#include "ui.h"
#include "launch.h"
#include "sysctl.h"
#include "sound.h"
#include "prefs.h"
#include "controlpanels.h"
#include "display.h"
#include "artcaps.h"
#include "bless.h"
#include "mem.h"
#include "mac_compat.h"
#include "version.h"       /* MACATRIUM_VERSION build stamp (MacAtrium Status, docs/37) */

#include <string.h>

#ifndef plainDBox
#define plainDBox 2
#endif
#ifndef movableDBoxProc
#define movableDBoxProc 5
#endif

/* MultiFinder suspend/resume (we set acceptSuspendResumeEvents in the SIZE
 * resource). Standard on full toolboxes; guard for leaner Retro68 headers. */
#ifndef osEvt
#define osEvt 15
#endif
#ifndef suspendResumeMessage
#define suspendResumeMessage 1
#endif
#ifndef resumeFlag
#define resumeFlag 1
#endif
#ifndef kHighLevelEvent
#define kHighLevelEvent 23
#endif

/* Large structures live in BSS, not on the stack. */
static Catalog   gCat;
static Model     gModel;
static Env       gEnv;
static ArtCaps   gArtCaps;         /* docs/44: art tiers this machine can show/hold */
static Render    gRender;
static Ui        gUi;
static WindowPtr gWin;
/* When a returning (concurrent) launch caps the screen depth for a game, we defer
 * putting our depth back until the game quits and we're reactivated (osEvt resume),
 * so the game keeps its capped depth the whole time it runs. 0 = nothing pending. */
static short gPendingDepthRestore = 0;
static Prefs     gPrefs;

/* Scroll-bar auto-repeat: TrackControl calls this action proc continuously while an
 * arrow or page region is held, so the selection scrolls for as long as the button
 * is down (hold-to-scroll) instead of one step per click. The thumb is tracked with
 * nil instead (a post-drag jump), so `part` here is only ever an arrow/page code.
 * Created once in main() — a no-op cast on 68k where ControlActionUPP is the ptr. */
static ControlActionUPP gScrollAction = 0;
static pascal void scroll_action(ControlHandle ctl, short part)
{
    (void)ctl;
    if (part) ui_scroll_step(&gUi, part);
}

/* WaitNextEvent is a MultiFinder/System-7 trap — absent on base System 6. We
 * use it on System 7+ and fall back to GetNextEvent + SystemTask on 6.x (which
 * still yields to MultiFinder). Set from the probed system version at startup. */
static Boolean   gHasWNE = true;

/* Fetch the next event, transparently using WaitNextEvent when present and
 * GetNextEvent (with a SystemTask yield) when it isn't. */
static Boolean next_event(EventRecord *evt)
{
    if (gHasWNE) {
        return WaitNextEvent(everyEvent, evt, 10L, 0L);
    }
    SystemTask();
    return GetNextEvent(everyEvent, evt);
}

static void toolbox_init(void)
{
    InitGraf(&qd.thePort);
    InitFonts();
    InitWindows();
    InitMenus();
    TEInit();
    InitDialogs(0L);
    InitCursor();
    FlushEvents(everyEvent, 0);
}

static void bring_self_front(void)
{
    /* The Process Manager (GetCurrentProcess/SetFrontProcess) is System 7+ (and
     * MultiFinder); on base System 6 those are unimplemented traps. We're already
     * frontmost as the launched app there, so skip it. */
    if (gHasWNE) {
        ProcessSerialNumber psn;
        if (GetCurrentProcess(&psn) == noErr)
            SetFrontProcess(&psn);
    }
}

/* ---- the System menu bar (docs/28) ---------------------------------------------
 * The launcher owns a real Toolbox menu bar (Apple / File / Edit / View / Special),
 * built programmatically — no MENU resources. Items map to the same actions the
 * keyboard drives (handle_ui_command + the ui_show_* hooks). The window sits below
 * the bar (make_window: top = mbarHeight), so the bar is always visible and we no
 * longer hide it / reclaim its strip; on return from a sub-launch (which drew its
 * own bar) we just DrawMenuBar() to repaint ours. */
enum { mApple = 128, mFile = 129, mEdit = 130, mView = 131, mSpecial = 132 };
enum { kFileLaunch = 1, kFileGetInfo = 2 };  /* then a sep, then Show Finder / Quit */
enum { kViewSettings = 5 };                  /* views are items 1..VIEW_N; 4 = sep   */
enum { kSpecialChooser = 1, kSpecialRestart = 3, kSpecialShutdown = 4 };  /* +sep at 2 */

static MenuHandle gAppleMenu, gFileMenu, gEditMenu, gViewMenu, gSpecialMenu;
/* File > Show Finder / Quit item numbers, or 0 when omitted — the System-6 boot
 * shell has no separate Finder to front or hand back to (canLaunchReturn false). */
static short gFileShowFinder = 0, gFileQuit = 0;

static void install_menus(void)
{
    gAppleMenu = NewMenu(mApple, "\p\024");        /* 0x14 = the apple glyph (Chicago) */
    AppendMenu(gAppleMenu, "\pAbout MacAtrium;(-");
    AppendResMenu(gAppleMenu, 'DRVR');             /* desk accessories from the System */
    InsertMenu(gAppleMenu, 0);

    gFileMenu = NewMenu(mFile, "\pFile");
    AppendMenu(gFileMenu, "\pLaunch/L;Get Info/I");
    if (gEnv.canLaunchReturn) {                    /* a Finder exists to hand back to */
        AppendMenu(gFileMenu, "\p(-;Show Finder;Quit/Q");
        gFileShowFinder = 4;
        gFileQuit       = 5;
    }
    InsertMenu(gFileMenu, 0);

    /* Edit: present (and standard) for desk accessories; disabled in the appliance
     * itself, which has no text editing. SystemEdit routes to an active DA. */
    gEditMenu = NewMenu(mEdit, "\pEdit");
    AppendMenu(gEditMenu, "\p(Undo/Z;(-;(Cut/X;(Copy/C;(Paste/V;(Clear");
    InsertMenu(gEditMenu, 0);

    gViewMenu = NewMenu(mView, "\pView");
    AppendMenu(gViewMenu, "\pCarousel;Icon Grid;List;(-;Settings\311");  /* \311 = ellipsis */
    InsertMenu(gViewMenu, 0);

    gSpecialMenu = NewMenu(mSpecial, "\pSpecial");
    AppendMenu(gSpecialMenu, "\pSystem Folder Chooser\311;(-;Restart;Shut Down");  /* \311 = … */
    InsertMenu(gSpecialMenu, 0);

    DrawMenuBar();
}

/* Tick the View menu's check beside the current browse view (read straight from the
 * UI). Called right before the bar is pulled down, so it's always current no matter
 * how the view last changed (menu, Settings, Tab, or the first-run chooser). */
static void sync_view_menu(void)
{
    int i;
    for (i = 0; i < VIEW_N; i++)
        CheckItem(gViewMenu, (short)(i + 1), (Boolean)(i == gUi.view));
}

/* Hand the system menu bar back to the Finder / a desk accessory regardless of our
 * hide setting: real height + reclaim the GrayRgn strip we may have ceded. Used
 * before fronting the Finder (Show Finder / Open Control Panel) and before quitting,
 * so it comes up with its menus even when we've been running with the bar hidden. */
static void restore_system_menu_bar(void)
{
    RgnHandle strip = NewRgn();
    if (strip) {
        RgnHandle gray = LMGetGrayRgn();
        Rect      mb   = gEnv.screen;
        mb.bottom = (short)(mb.top + gEnv.mbarHeight);
        RectRgn(strip, &mb);
        if (gray) DiffRgn(gray, strip, gray);
        DisposeRgn(strip);
    }
    LMSetMBarHeight(gEnv.mbarHeight);
}

/* Quit the launcher entirely and hand the machine back to the Finder (the
 * resident boot shell) — Cmd-Option-Q or File > Quit. Restore the menu bar first
 * (we may have been running with it hidden) so the Finder comes up with its menus.
 * Does not return. */
static void quit_to_finder(void)
{
    restore_system_menu_bar();
    (void)sysctl_show_finder();   /* front the Finder (best-effort) */
    ExitToShell();                /* terminate us; the Finder becomes the shell */
}

/* AppleEvent handlers. We're isHighLevelEventAware (SIZE) so AESend works for
 * the Control Panels odoc; in return the OS delivers the required core events to
 * us, which we must accept. We ignore oapp/odoc/pdoc and honour quit (the Finder
 * sends it at shutdown). */
static pascal OSErr ae_ignore(const AppleEvent *e, AppleEvent *reply, long refcon)
{
    (void)e; (void)reply; (void)refcon;
    return noErr;
}
static pascal OSErr ae_quit(const AppleEvent *e, AppleEvent *reply, long refcon)
{
    (void)e; (void)reply; (void)refcon;
    quit_to_finder();                 /* restores the bar + ExitToShell (no return) */
    return noErr;
}

static void install_ae_handlers(void)
{
    AEInstallEventHandler(kCoreEventClass, kAEOpenApplication,
                          NewAEEventHandlerUPP(ae_ignore), 0, false);
    AEInstallEventHandler(kCoreEventClass, kAEOpenDocuments,
                          NewAEEventHandlerUPP(ae_ignore), 0, false);
    AEInstallEventHandler(kCoreEventClass, kAEPrintDocuments,
                          NewAEEventHandlerUPP(ae_ignore), 0, false);
    AEInstallEventHandler(kCoreEventClass, kAEQuitApplication,
                          NewAEEventHandlerUPP(ae_quit), 0, false);
}

/* The standard document-window title bar height (the WDEF draws it in the structure
 * region, above the content portRect). */
#define kTitleBarH 19

static WindowPtr make_window(const Env *e, int hideMenu, int hideTitle)
{
    /* An immovable, full-screen window whose content sits below whatever chrome is
     * shown. With the title bar shown it's a noGrowDocProc (real WM title bar + close
     * box); hidden, it's a plainDBox (no title bar, no close box — quit via the Esc
     * menu / Cmd-Opt-Q). The content top drops the menu-bar strip and/or the title-
     * bar height as each is hidden, so the reclaimed space becomes browse area. Inset
     * 1px so the side/bottom frame shows. We never DragWindow it (immovable). */
    short   mbar   = (short)(hideMenu  ? 0 : e->mbarHeight);
    short   titleH = (short)(hideTitle ? 0 : kTitleBarH);
    short   proc   = (short)(hideTitle ? plainDBox : noGrowDocProc);
    Boolean goAway = hideTitle ? false : true;       /* plainDBox has no close box */
    Rect    b = e->screen;
    b.top    = (short)(b.top + mbar + titleH);
    b.left   = (short)(b.left + 1);
    b.right  = (short)(b.right - 1);
    b.bottom = (short)(b.bottom - 1);
    /* A colour window (CGrafPort) when Color QD is present, so the off-screen GWorld
     * blits correctly at >1-bit depths; a plain B&W window otherwise. */
    if (e->hasColorQD)
        return NewCWindow(0L, &b, "\pMacAtrium", true, proc, (WindowPtr)-1L, goAway, 0);
    return NewWindow(0L, &b, "\pMacAtrium", true, proc, (WindowPtr)-1L, goAway, 0);
}

/* Set the System menu bar to the current hide state. Hiding it isn't just
 * MBarHeight=0: the Window Manager keeps clipping that strip out of every window's
 * visible region, so a window over it can't paint there and stale menu pixels show
 * through. So we also cede the strip to the desktop (GrayRgn) when hiding and
 * reclaim it when showing — idempotent (Union/Diff), matched by a CalcVis on the
 * window so its visRgn picks up the change. (The proven full-screen recipe from
 * before the real-menu-bar redesign, now gated on the user's setting.) */
static void set_menu_bar_state(void)
{
    RgnHandle strip = NewRgn();
    if (strip) {
        RgnHandle gray = LMGetGrayRgn();
        Rect      mb   = gEnv.screen;
        mb.bottom = (short)(mb.top + gEnv.mbarHeight);
        RectRgn(strip, &mb);
        if (gray) {
            if (gUi.hideMenuBar) UnionRgn(gray, strip, gray);  /* cede strip to desktop */
            else                 DiffRgn(gray, strip, gray);   /* reclaim as menu bar  */
        }
        DisposeRgn(strip);
    }
    LMSetMBarHeight((short)(gUi.hideMenuBar ? 0 : gEnv.mbarHeight));
}

/* Re-assert the menu-bar state + repaint it after a sub-launched app drew its own
 * bar (it may have changed MBarHeight): honor the hide setting, and CalcVis so the
 * existing window owns (or releases) the reclaimed strip. */
static void show_menu_bar(void)
{
    set_menu_bar_state();
    if (gWin) CalcVis((WindowPeek)gWin);
    if (!gUi.hideMenuBar) DrawMenuBar();
}

/* Re-lay out the launcher window for the current chrome state (menu bar / title bar
 * shown or hidden). The window's procID + top edge depend on both flags, so this
 * disposes and rebuilds it. The scroll bar / push buttons live in the window port
 * (freed with it by DisposeWindow), so clear our handles for ensure_controls to
 * recreate them in the new window; drop the off-screen GWorld so it re-fits the new
 * content size; then repaint. Called at startup (to apply the saved state) and on a
 * Settings *bar toggle (UI_CHROME_DIRTY). */
static void rebuild_window(void)
{
    if (gWin) DisposeWindow(gWin);
    set_menu_bar_state();                 /* MBarHeight + GrayRgn BEFORE the new window
                                           * so its visRgn is computed for the new top */
    gWin = make_window(&gEnv, gUi.hideMenuBar, gUi.hideTitleBar);
    gUi.win = gWin;
    gUi.controlsReady = 0;                /* the controls belonged to the old port */
    gUi.scrollV = gUi.launch = gUi.quitBtn = gUi.cancelBtn = gUi.settingsBtn = 0;
    render_reset_for_depth(&gRender, &gEnv, display_current_depth());  /* GWorld re-fits */
    gUi.bgValid = 0;                      /* the fresh GWorld is blank: repaint the
                                           * whole browse screen, not just the overlay */
    CalcVis((WindowPeek)gWin);            /* claim/release the reclaimed top strip */
    SetPort(gWin);
    if (!gUi.hideMenuBar) DrawMenuBar();
    ui_draw(&gUi);
}

/* Returns 1 if a non-empty catalog loaded; 0 -> safe screen. */
static int load_catalog(void)
{
    FSSpec spec;
    char  *buf;
    long   len;
    int    cap;

    gCat.items   = 0;
    gCat.cap     = 0;
    gCat.nitems  = 0;
    gCat.dropped = 0;

    if (macfs_make_spec("metadata/catalog.jsonl", &spec) != noErr) return 0;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return 0;

    /* Size the items array to the file's line count (capped at MAX_ITEMS) and
     * allocate exactly that, so a small library no longer carries CatItem[256]
     * (~390 KB) just in case. The catalog lives for the whole session, so the
     * block is intentionally never freed. */
    cap = catalog_count_lines(buf, len);
    if (cap > MAX_ITEMS) cap = MAX_ITEMS;
    if (cap > 0) {
        gCat.items = (CatItem *)NewPtr((Size)cap * sizeof(CatItem));
        if (gCat.items) {
            gCat.cap    = cap;
            gCat.nitems = catalog_parse_into(buf, len, gCat.items, cap, &gCat.dropped);
        }
    }
    DisposePtr(buf);

    return gCat.nitems > 0;
}

/* ---- paged catalog (docs/21) -------------------------------------------------
 * A large library can't fit the 256-item single-file catalog (let alone a 4 MB
 * Mac Plus's RAM), so it ships as metadata/index.jsonl (the category list) plus
 * one cats/<slug>.jsonl page per category. Only the current page is resident;
 * navigating a category loads its page on demand (model's PageLoader). */
static CatRef gRefs[MODEL_MAX_CATS];
static int    gNrefs = 0;
static VolTable gVols;             /* multi-disk (docs/37): mounted library volumes; boot = v[0] */

/* The vRefNum for a category's source volume-table index (docs/37); the boot
 * volume when there's no table (legacy / safe screen). */
static short vol_vref(int vol)
{
    short bv;
    if (gVols.n > 0) {
        if (vol < 0 || vol >= gVols.n) vol = 0;
        return gVols.v[vol].vref;
    }
    if (macfs_boot_vref(&bv) == noErr) return bv;
    return 0;
}

/* Read every mounted library disk's metadata/index.jsonl into gRefs[], each
 * category tagged with its source volume (docs/37); boot volume first. 1 if any
 * paged catalog is present. */
static int load_index(void)
{
    int nv, k;
    gNrefs = 0;
    nv = macfs_volumes(&gVols);
    if (nv <= 0) return 0;
    for (k = 0; k < nv && gNrefs < MODEL_MAX_CATS; k++) {
        FSSpec spec;
        char  *buf;
        long   len;
        int    got, j;
        if (macfs_make_spec_on(gVols.v[k].vref, "metadata/index.jsonl", &spec) != noErr) continue;
        if (macfs_read_all(&spec, &buf, &len) != noErr) continue;
        got = catindex_parse(buf, len, gRefs + gNrefs, MODEL_MAX_CATS - gNrefs);
        DisposePtr(buf);
        for (j = 0; j < got; j++) gRefs[gNrefs + j].vol = k;   /* tag with source volume */
        gNrefs += got;
    }
    return gNrefs > 0;
}

/* A brief "Loading <name>..." notice while a category page reads + parses (the
 * page render overwrites it as soon as the load finishes). */
static void show_loading(const char *name)
{
    Rect  b  = gWin->portRect;
    short cx = (short)((b.left + b.right) / 2);
    short cy = (short)((b.top + b.bottom) / 2);
    char  msg[80];
    Rect  box;
    short w;

    strcpy(msg, "Loading ");
    strncat(msg, name, sizeof msg - strlen(msg) - 5);
    strcat(msg, "...");

    SetRect(&box, (short)(cx - 170), (short)(cy - 24), (short)(cx + 170), (short)(cy + 24));
    render_begin(&gRender, gWin);
    render_fill(&gRender, &box, FILL_PANEL);
    render_frame(&gRender, &box);
    render_text_size(&gRender, 12);
    w = render_text_width(&gRender, msg);
    render_text(&gRender, (short)(cx - w / 2), (short)(cy + 4), msg, INK_NORMAL);
    render_end(&gRender, gWin);
}

/* The model's PageLoader: read cats/<slug>.jsonl for category `catIdx` into gCat
 * (the one resident page) and install it via model_set_page. gCat.items is
 * allocated once at MAX_CAT_ITEMS and reused for every page. */
static int load_page(Model *m, int catIdx)
{
    FSSpec      spec;
    char       *buf;
    long        len;
    char        path[80];
    const char *slug = m->cats[catIdx].slug;

    show_loading(m->cats[catIdx].name);

    if (!gCat.items) {
        gCat.items = (CatItem *)NewPtr((Size)MAX_CAT_ITEMS * sizeof(CatItem));
        gCat.cap   = gCat.items ? MAX_CAT_ITEMS : 0;
    }
    gCat.nitems = 0;
    gCat.dropped = 0;
    if (!gCat.items) { model_set_page(m, &gCat); return 0; }

    strcpy(path, "metadata/cats/");
    strncat(path, slug, sizeof path - strlen(path) - 7);
    strcat(path, ".jsonl");

    if (macfs_make_spec_on(vol_vref(m->cats[catIdx].vol), path, &spec) == noErr &&
        macfs_read_all(&spec, &buf, &len) == noErr) {
        gCat.nitems = catalog_parse_into(buf, len, gCat.items, gCat.cap, &gCat.dropped);
        DisposePtr(buf);
    }
    model_set_page(m, &gCat);
    model_sort_page(m, gUi.sortMode, gUi.sortDesc);   /* keep the chosen List sort on cat change */
    /* The page's items array was just refilled; drop caches that key off it. */
    ui_page_changed(&gUi);
    return 1;
}

/* Append a signed long as decimal to a C string. */
static void append_long(char *dst, long v)
{
    char tmp[16];
    int  t = 0, k = (int)strlen(dst);
    int  neg = (v < 0);
    unsigned long u = neg ? (unsigned long)(-v) : (unsigned long)v;
    if (u == 0) tmp[t++] = '0';
    while (u) { tmp[t++] = (char)('0' + u % 10); u /= 10; }
    if (neg) tmp[t++] = '-';
    while (t) dst[k++] = tmp[--t];
    dst[k] = '\0';
}

/* Snapshot the current persisted prefs (theme / volume / selection) and write
 * them. Called whenever a persisted setting changes and before we hand control
 * away (launch / restart / shutdown), so the next boot restores them. */
static void save_prefs(void)
{
    Prefs          p;
    ModelCat      *c  = model_cur_cat(&gModel);
    const CatItem *it = model_cur_item(&gModel);

    p.theme = gRender.theme;
    p.haveTheme = 1;
    if (gUi.vol >= 0) { p.vol = gUi.vol; p.haveVol = 1; }
    else              { p.vol = 0;       p.haveVol = 0; }
    p.artPref = gUi.artPref;
    p.haveArtPref = 1;
    p.sndStartup = gUi.sndStartup;   p.haveSndStartup = 1;
    p.sndShutdown = gUi.sndShutdown; p.haveSndShutdown = 1;
    p.catList = gUi.catList;         p.haveCatList = 1;
    p.hideMenuBar = gUi.hideMenuBar; p.haveHideMenuBar = 1;
    p.hideTitleBar = gUi.hideTitleBar; p.haveHideTitleBar = 1;
    p.textSize = gRender.textSize;   p.haveTextSize = 1;
    p.gridStyle = gUi.gridStyle;     p.haveGridStyle = 1;
    p.sortMode = gUi.sortMode; p.sortDesc = gUi.sortDesc; p.haveSort = 1;
    p.listColType = gUi.listColType; p.haveListCol = 1;
    p.carousel = gUi.carousel;       p.haveCarousel = 1;
    p.view = gUi.view;               p.haveView = 1;
    p.depth = display_current_depth();  p.haveDepth = (p.depth > 0);  /* boot-depth pref */
    p.appearance = gRender.appearancePref;  p.haveAppearance = 1;     /* era-look choice */

    p.category[0] = '\0';
    p.item[0]     = '\0';
    p.haveSel     = 0;
    if (c) {
        strncpy(p.category, c->name, sizeof p.category - 1);
        p.category[sizeof p.category - 1] = '\0';
        p.haveSel = 1;
    }
    if (it) {
        strncpy(p.item, it->id, sizeof p.item - 1);
        p.item[sizeof p.item - 1] = '\0';
    }
    (void)prefs_save(&p);
}

static void do_launch(void)
{
    const char  *app  = ui_current_app(&gUi);
    const char  *name = ui_current_name(&gUi);
    OSErr        lerr  = noErr;
    LaunchResult lr;
    char         msg[96];
    short        savedDepth = 0;      /* >0 → restore this depth after launch */
    int          returns;

    if (!app) return;

    /* The returning, working-directory-setting extended _Launch identifies the app
     * by FSSpec (launchAppSpec) — a System-7 Process Manager feature. MultiFinder
     * 6.x reports gestaltLaunchCanReturn too, but its _Launch predates FSSpec and
     * rejects the System-7 block with fnfErr (-43). So only take the returning
     * extended path on System 7+; on System 6 (bare OR MultiFinder) use the classic
     * Segment-Loader launch (which works under MultiFinder, and there the Process
     * Manager still sets the launched app's working directory). */
    returns = gEnv.canLaunchReturn && (gEnv.sysVers >= 0x0700);

    /* A non-returning launch won't run us again until the System relaunches us, so
     * hand the game the environment it expects — reset the cursor. (The menu bar is
     * already at full height; the game draws its own.) */
    if (!returns) {
        InitCursor();
    }

    /* Per-game launch depth (catalog `maxDepth`, in the overrides DB) is a CAP:
     * it only ever LOWERS the screen, for titles that bomb above a certain depth
     * (Dark Castle needs 1-bit, maxDepth 1). We NEVER auto-raise — a game whose cap
     * is at or above the current depth runs at the current depth, untouched (so
     * Prince of Persia, maxDepth 8, just runs at whatever colour depth the screen
     * is on; it is not forced anywhere). Raising the depth for a game that wants
     * more is the user's call (prompt/Settings), not automatic. maxDepth 0 = no
     * cap. On a returning launch we restore below; on the bare appliance the
     * relaunched MacAtrium comes up at its own default depth. */
    {
        int   maxd = ui_current_maxdepth(&gUi);
        short cur  = display_current_depth();
        if (maxd > 0 && gEnv.hasColorQD && cur > (short)maxd) {
            short target = display_depth_at_most((short)maxd);   /* highest ≤ cap */
            if (target > 0 && target < cur && display_set_depth(target) == noErr) {
                savedDepth = cur;                      /* restore this on the app's quit */
                /* LIVE depth only — a per-game cap is temporary and must NOT touch the
                 * boot default in slot PRAM (that's Settings-only). We put the live
                 * depth back when the game quits (osEvt resume / below). */
                /* Re-fit our backend to the new depth, but DON'T repaint: the game is
                 * about to take over the screen, so a "setting up the display" redraw
                 * here is just an extra flash on top of the depth switch itself. (We
                 * restore + repaint cleanly when the game quits — osEvt resume.) */
                render_reset_for_depth(&gRender, &gEnv, target);
            }
        }
    }

    lr = launch_app(vol_vref(gModel.cats[gModel.curCat].vol), app, returns, &lerr);

    /* A returning launch uses launchContinue: LaunchApplication returns IMMEDIATELY
     * and the game runs concurrently as the new front process. We must NOT touch the
     * screen here — restoring the capped depth or re-fronting now would yank both out
     * from under the just-started game (Beyond Dark Castle, capped to 1-bit, would
     * see the depth snap back to colour and refuse to run). Defer the depth restore
     * to when the game quits and we're reactivated (osEvt resume) and just yield:
     * a suspend event follows and the osEvt handler hides our window behind the game. */
    if (returns && lr == LAUNCH_OK) {
        gPendingDepthRestore = savedDepth;   /* 0 if we didn't cap the depth */
        ui_set_status(&gUi, "");
        return;
    }

    /* Otherwise we're staying resident: a returning launch that FAILED, or a
     * non-returning (System 6) launch that only returns on failure. Put the capped
     * depth back, repaint our menu bar (the child drew its own), re-front and report. */
    if (savedDepth > 0) {
        (void)display_set_depth(savedDepth);           /* live only — never the boot default */
        render_reset_for_depth(&gRender, &gEnv, savedDepth);
    }
    bring_self_front();
    show_menu_bar();                  /* the child drew its own bar; repaint ours */
    SelectWindow(gWin);
    SetPort(gWin);

    switch (lr) {
        case LAUNCH_OK:
            ui_set_status(&gUi, "");
            break;
        case LAUNCH_CANT_RETURN:
            ui_set_status(&gUi, "Resident launch unavailable on this system.");
            break;
        case LAUNCH_NOT_FOUND:
            strcpy(msg, "Not found: ");
            strncat(msg, name ? name : app, sizeof msg - strlen(msg) - 1);
            ui_set_status(&gUi, msg);
            break;
        default:
            strcpy(msg, "Launch failed (err ");
            append_long(msg, lerr);
            strcat(msg, ")");
            ui_set_status(&gUi, msg);
            break;
    }
    ui_draw(&gUi);
}

/* Dispatch a UiCommand from ui_key() / ui_click(). The side-effecting actions
 * (launch / Finder / power / persist) live here so the UI layer stays draw+state
 * only; both the keyboard and the mouse paths funnel through it. */
/* ---- the real Settings window (docs/33 §2.2) --------------------------------
 * Replaces the old in-GWorld overlay panel with a real movable modal dialog of
 * live Toolbox controls: checkboxes for the binary settings, < / > push-button
 * steppers for the multi-value ones (depth / volume / view / …), a Control Panels
 * action button, and a default Done button. It runs its own modal loop so it stays
 * out of the main async state machine; the arrow keys move a focus ring and
 * Space/Return activate the focused control, so it is standard-looking yet fully
 * keyboard / gamepad drivable. Checkboxes + push buttons are original-Mac controls
 * (System 6+); popup menus (System 7-only) are deliberately avoided so the 6.0.8
 * build keeps a working Settings screen. ui.c owns the actual setting logic
 * (ui_setting_*), so this is just chrome + input. */
#define SD_CW   360            /* content width                          */
#define SD_LM   18             /* left / right content margin            */
#define SD_RH   22             /* row pitch                              */
#define SD_ROW0 16             /* first row's top (content-local)        */
#define SD_MAXROWS 24

typedef enum { SD_DONE = 0, SD_OPEN_CDEVS } SettingsResult;

/* per-row controls + the Done / page buttons, shared by the modal loop's handlers */
static ControlHandle gSdChk[SD_MAXROWS];   /* CHECK row's checkbox        */
static ControlHandle gSdDec[SD_MAXROWS];   /* STEPPER row's <  button     */
static ControlHandle gSdInc[SD_MAXROWS];   /* STEPPER row's  > button     */
static ControlHandle gSdAct[SD_MAXROWS];   /* ACTION row's button         */
static ControlHandle gSdDone;
static ControlHandle gSdPage;              /* "More…"/"Back" page toggle (paged only) */
static short         gSdDoneTop;
static int           gSdRows;
/* Paging: a tall dialog (all rows) won't fit a short screen (9" 512x342: only
 * ~290px of content under the menu bar + the dialog's title bar). When it doesn't
 * fit, the rows split across pages and a "More…"/"Back" button flips between them.
 * gSdRpp = rows per page (>= gSdRows => one page, the 640x480 default — no paging).
 * sd_row_top wraps a row index into its slot, so all the geometry helpers below are
 * paging-aware for free. */
static int           gSdRpp = SD_MAXROWS;
static int           gSdPaged;

static void c2p255(const char *s, Str255 out)
{
    int n = 0;
    while (s[n] && n < 255) { out[n + 1] = (unsigned char)s[n]; n++; }
    out[0] = (unsigned char)n;
}

static short sd_row_top(int i) { return (short)(SD_ROW0 + (i % gSdRpp) * SD_RH); }
static void  sd_row_frame(int i, Rect *r)
{
    short t = sd_row_top(i);
    SetRect(r, SD_LM - 3, (short)(t - 1), (short)(SD_CW - SD_LM + 3), (short)(t + 19));
}
static void  sd_dec_rect(int i, Rect *r)
{ short t = sd_row_top(i); SetRect(r, (short)(SD_CW-SD_LM-46), t, (short)(SD_CW-SD_LM-24), (short)(t+18)); }
static void  sd_inc_rect(int i, Rect *r)
{ short t = sd_row_top(i); SetRect(r, (short)(SD_CW-SD_LM-22), t, (short)(SD_CW-SD_LM),    (short)(t+18)); }
static void  sd_done_rect(Rect *r)
{ SetRect(r, (short)(SD_CW-SD_LM-78), gSdDoneTop, (short)(SD_CW-SD_LM), (short)(gSdDoneTop+20)); }
static void  sd_page_rect(Rect *r)
{ SetRect(r, SD_LM, gSdDoneTop, (short)(SD_LM + 84), (short)(gSdDoneTop + 20)); }

/* Which page a row is on, and a page's focusable-item count / focus->item map.
 * A focus index walks: the page's visible rows, then (if paged) the page button,
 * then Done. sd_focus_item returns a row index (>=0), -1 = page button, -2 = Done. */
static int sd_page_lastrow(int page) { int hi = (page+1)*gSdRpp; return hi < gSdRows ? hi : gSdRows; }
static int sd_page_nfocus(int page)
{ return (sd_page_lastrow(page) - page*gSdRpp) + (gSdPaged ? 1 : 0) + 1; }
static int sd_focus_item(int focus, int page)
{
    int lo = page * gSdRpp, vis = sd_page_lastrow(page) - lo;
    if (focus < vis)              return lo + focus;
    if (gSdPaged && focus == vis) return -1;          /* page button */
    return -2;                                          /* Done       */
}

/* Show only the current page's row controls (off-page controls share slots, so
 * they're hidden); a full redraw follows to repaint cleanly. */
static void sd_show_page(WindowPtr dlg, int page)
{
    int i, k;
    (void)dlg;
    for (i = 0; i < gSdRows; i++) {
        ControlHandle cs[4]; int on = (i / gSdRpp) == page;
        cs[0]=gSdChk[i]; cs[1]=gSdDec[i]; cs[2]=gSdInc[i]; cs[3]=gSdAct[i];
        for (k = 0; k < 4; k++) if (cs[k]) { if (on) ShowControl(cs[k]); else HideControl(cs[k]); }
    }
}

/* Draw one stepper row's label (left) + value (right of label, left of the < button),
 * erasing the value band first so a wider value can shrink cleanly. */
static void sd_draw_stepper_text(int i)
{
    char val[24];
    const char *lab = ui_setting_label(i);
    short top = sd_row_top(i), base = (short)(top + 14);
    short vright = (short)(SD_CW - SD_LM - 52);   /* value's right edge (left of <) */
    short vw;
    Rect  band;
    TextFont(0); TextSize(12);
    MoveTo(SD_LM, base); DrawText((Ptr)lab, 0, (short)strlen(lab));
    SetRect(&band, (short)(SD_LM + 128), top, (short)(SD_CW - SD_LM - 50), (short)(top + 18));
    EraseRect(&band);
    ui_setting_value(&gUi, i, val);
    vw = TextWidth(val, 0, (short)strlen(val));
    MoveTo((short)(vright - vw), base); DrawText(val, 0, (short)strlen(val));
}

/* The bold ring the Window/Control Manager doesn't draw around a default button. */
static void sd_default_ring(void)
{
    Rect r; sd_done_rect(&r); InsetRect(&r, -4, -4);
    PenSize(3, 3); FrameRoundRect(&r, 16, 16); PenSize(1, 1);
}

static void sd_focus_rect(int focus, int page, Rect *r)
{
    int it = sd_focus_item(focus, page);
    if (it >= 0)       sd_row_frame(it, r);
    else if (it == -1) { sd_page_rect(r); InsetRect(r, -4, -4); }    /* page button */
    else               { sd_done_rect(r); InsetRect(r, -7, -7); }    /* Done: outside its ring */
}
/* Draw (on) or erase (off, paint white) the focus frame around a focusable item. */
static void sd_focus_frame(int focus, int page, int on)
{
    Rect r; sd_focus_rect(focus, page, &r);
    PenPat(on ? &qd.black : &qd.white);
    FrameRect(&r);
    PenPat(&qd.black);
}

/* Full content redraw (initial paint + every updateEvt + page flip): the shown
 * controls, the current page's stepper text, the hint line (single page only — a
 * paged dialog has no room), the Done ring, and the focus frame. */
static void sd_draw_content(WindowPtr dlg, int focus, int page)
{
    Rect content = dlg->portRect;
    int  i, lo = page * gSdRpp, hi = sd_page_lastrow(page);
    SetPort(dlg);
    EraseRect(&content);
    DrawControls(dlg);
    for (i = lo; i < hi; i++)
        if (ui_setting_kind(i) == SETTING_STEPPER) sd_draw_stepper_text(i);
    if (!gSdPaged) {
        const char *h = "Arrows move \xC9 Space/Return change \xC9 Esc closes";  /* 0xC9 = … */
        short tw;
        TextFont(0); TextSize(12);
        tw = TextWidth((Ptr)h, 0, (short)strlen(h));
        MoveTo((short)((SD_CW - tw) / 2), (short)(sd_row_top(gSdRows - 1) + 18 + 16));
        DrawText((Ptr)h, 0, (short)strlen(h));
    }
    sd_default_ring();
    sd_focus_frame(focus, page, 1);
}

static void sd_set_focus(int *focus, int newFocus, int page)
{
    if (newFocus == *focus) return;
    sd_focus_frame(*focus, page, 0);
    *focus = newFocus;
    sd_focus_frame(*focus, page, 1);
}

/* Flip to the next page (wraps): show its controls, relabel the page button, reset
 * focus to the top, and repaint. */
static void sd_flip_page(WindowPtr dlg, int *page, int *focus, int npages)
{
    *page = (*page + 1) % npages;
    sd_show_page(dlg, *page);
    SetControlTitle(gSdPage, (*page < npages - 1) ? "\pMore\xC9" : "\pBack");
    *focus = 0;
    sd_draw_content(dlg, *focus, *page);
}

/* Apply a checkbox toggle / stepper step (dir +1 toggles a checkbox) and reflect it:
 * update the control's value or value text, and capture a deferred chrome change. */
static void sd_apply(WindowPtr dlg, int row, int dir, int *chromeChanged)
{
    SetPort(dlg);
    ui_setting_step(&gUi, row, dir);
    if (gUi.chromeDirty) { gUi.chromeDirty = 0; *chromeChanged = 1; }
    if (ui_setting_kind(row) == SETTING_CHECK)
        SetControlValue(gSdChk[row], (short)ui_setting_checked(&gUi, row));
    else
        sd_draw_stepper_text(row);   /* a depth change also posts an updateEvt -> full redraw */
}

static SettingsResult run_settings_dialog(int *chromeChanged)
{
    WindowPtr      dlg;
    Rect           bounds, sb = qd.screenBits.bounds;
    int            nrows = ui_setting_count(), i, focus = 0, running = 1;
    int            npages = 1, curPage = 0;
    short          CH, availH;
    SettingsResult res = SD_DONE;

    *chromeChanged = 0;
    if (nrows > SD_MAXROWS) nrows = SD_MAXROWS;
    gSdRows = nrows;

    /* Paging: how many rows fit between the menu bar (20) and the dialog's own title
     * bar (~22) with a margin? Single page when they all fit (the 640x480 case);
     * otherwise split into balanced pages reachable with the More…/Back button. */
    availH = (short)((sb.bottom - sb.top) - 20 - 22 - 12);
    {
        int rppMax = (availH - SD_ROW0 - 50) / SD_RH;     /* single-page chrome: hint + Done */
        if (rppMax < 1) rppMax = 1;
        if (nrows <= rppMax) { gSdPaged = 0; gSdRpp = nrows; npages = 1; }
        else {
            gSdPaged = 1;
            rppMax = (availH - SD_ROW0 - 32) / SD_RH;      /* paged chrome: page btn + Done row */
            if (rppMax < 1) rppMax = 1;
            npages = (nrows + rppMax - 1) / rppMax;
            gSdRpp = (nrows + npages - 1) / npages;        /* balance the pages */
        }
    }
    {
        int   pageRows = (nrows < gSdRpp) ? nrows : gSdRpp;
        short rowsBot  = (short)(SD_ROW0 + pageRows * SD_RH);
        gSdDoneTop = (short)(rowsBot + (gSdPaged ? 8 : 24));
        CH = (short)(gSdDoneTop + 20 + (gSdPaged ? 12 : 14));
    }
    {
        short L = (short)(sb.left + ((sb.right - sb.left) - SD_CW) / 2);
        short T = (short)(sb.top  + ((sb.bottom - sb.top) - CH) / 2);
        if (T < (short)(sb.top + 44)) T = (short)(sb.top + 44);
        SetRect(&bounds, L, T, (short)(L + SD_CW), (short)(T + CH));
    }
    dlg = NewWindow(0L, &bounds, "\pSettings", true, movableDBoxProc, (WindowPtr)-1L, false, 0L);
    if (!dlg) return SD_DONE;
    SetPort(dlg);
    TextFont(0); TextSize(12);

    for (i = 0; i < nrows; i++) {
        int     kind = ui_setting_kind(i);
        short   top  = sd_row_top(i);
        Boolean vis  = (Boolean)((i / gSdRpp) == 0);    /* only page 0 visible initially */
        Str255  t;
        gSdChk[i] = gSdDec[i] = gSdInc[i] = gSdAct[i] = 0;
        if (kind == SETTING_CHECK) {
            Rect r; SetRect(&r, SD_LM, top, (short)(SD_CW - SD_LM), (short)(top + 18));
            c2p255(ui_setting_label(i), t);
            gSdChk[i] = NewControl(dlg, &r, t, vis, (short)ui_setting_checked(&gUi, i), 0, 1, checkBoxProc, 0L);
        } else if (kind == SETTING_STEPPER) {
            Rect rd, ri; sd_dec_rect(i, &rd); sd_inc_rect(i, &ri);
            gSdDec[i] = NewControl(dlg, &rd, "\p<", vis, 1, 0, 1, pushButProc, 0L);
            gSdInc[i] = NewControl(dlg, &ri, "\p>", vis, 1, 0, 1, pushButProc, 0L);
        } else {
            Rect r; SetRect(&r, SD_LM, top, (short)(SD_LM + 150), (short)(top + 18));
            c2p255(ui_setting_label(i), t);
            gSdAct[i] = NewControl(dlg, &r, t, vis, 1, 0, 1, pushButProc, 0L);
        }
    }
    { Rect r; sd_done_rect(&r); gSdDone = NewControl(dlg, &r, "\pDone", true, 1, 0, 1, pushButProc, 0L); }
    gSdPage = 0;
    if (gSdPaged) {
        Rect r; sd_page_rect(&r);
        gSdPage = NewControl(dlg, &r, "\pMore\xC9", true, 1, 0, 1, pushButProc, 0L);
    }

    sd_draw_content(dlg, focus, curPage);
    ValidRect(&dlg->portRect);                       /* we drew it; swallow the show updateEvt */

    while (running) {
        EventRecord evt;
        int nfocus = sd_page_nfocus(curPage);
        if (!next_event(&evt)) continue;
        switch (evt.what) {
            case updateEvt: {
                WindowPtr w = (WindowPtr)evt.message;
                if (w == dlg) { BeginUpdate(w); sd_draw_content(dlg, focus, curPage); EndUpdate(w); }
                else { BeginUpdate(w); SetPort(w); ui_draw(&gUi); EndUpdate(w); SetPort(dlg); }
                break;
            }
            case mouseDown: {
                WindowPtr w; short part = FindWindow(evt.where, &w);
                if (part == inDrag && w == dlg) {
                    DragWindow(w, evt.where, &sb); SetPort(dlg);
                } else if (part == inContent && w == dlg) {
                    Point p = evt.where; ControlHandle ctl; short cp;
                    SetPort(dlg); GlobalToLocal(&p);
                    cp = FindControl(p, dlg, &ctl);
                    if (cp && ctl && TrackControl(ctl, p, (ControlActionUPP)0)) {
                        int row = -1;
                        if (ctl == gSdDone) { running = 0; break; }
                        if (gSdPaged && ctl == gSdPage) { sd_flip_page(dlg, &curPage, &focus, npages); break; }
                        for (i = 0; i < nrows; i++)               /* hidden (off-page) controls aren't hit */
                            if (ctl==gSdChk[i] || ctl==gSdDec[i] || ctl==gSdInc[i] || ctl==gSdAct[i]) { row = i; break; }
                        if (row >= 0) {
                            int kind = ui_setting_kind(row);
                            if (kind == SETTING_ACTION) { res = SD_OPEN_CDEVS; running = 0; break; }
                            if (ctl == gSdChk[row]) sd_apply(dlg, row, +1, chromeChanged);
                            else if (ctl == gSdDec[row]) sd_apply(dlg, row, -1, chromeChanged);
                            else if (ctl == gSdInc[row]) sd_apply(dlg, row, +1, chromeChanged);
                            sd_set_focus(&focus, row - curPage * gSdRpp, curPage);
                        }
                    }
                } else if (w != dlg) {
                    SysBeep(1);                       /* modal: clicks elsewhere just beep */
                }
                break;
            }
            case keyDown:
            case autoKey: {
                char c = (char)(evt.message & charCodeMask);
                int  it;
                if (evt.modifiers & cmdKey) { if (c == '.') running = 0; break; }
                switch (c) {
                    case kCharEscape: running = 0; break;
                    case kCharUp:     sd_set_focus(&focus, (focus - 1 + nfocus) % nfocus, curPage); break;
                    case '\t':
                    case kCharDown:   sd_set_focus(&focus, (focus + 1) % nfocus, curPage); break;
                    case kCharLeft:
                        it = sd_focus_item(focus, curPage);
                        if (it >= 0 && ui_setting_kind(it) == SETTING_STEPPER) sd_apply(dlg, it, -1, chromeChanged);
                        break;
                    case kCharRight:
                        it = sd_focus_item(focus, curPage);
                        if (it >= 0 && ui_setting_kind(it) == SETTING_STEPPER) sd_apply(dlg, it, +1, chromeChanged);
                        break;
                    case ' ':
                    case kCharReturn:
                    case kCharEnter:
                        it = sd_focus_item(focus, curPage);
                        if (it == -2) running = 0;                                     /* Done */
                        else if (it == -1) sd_flip_page(dlg, &curPage, &focus, npages); /* page button */
                        else if (ui_setting_kind(it) == SETTING_ACTION) { res = SD_OPEN_CDEVS; running = 0; }
                        else sd_apply(dlg, it, +1, chromeChanged);
                        break;
                }
                break;
            }
        }
    }
    DisposeWindow(dlg);                               /* frees the dialog's controls too */
    return res;
}

/* ---- the Quick-Launch menu (docs/33; the ESC menu, now a real window) ---------
 * A movable modal window of standard push buttons (one per menu action) with the
 * same focus-ring keyboard model as the Settings dialog: ↑/↓ move the highlight,
 * Space/Return activate, Esc cancels, the mouse clicks a button. Returns the chosen
 * UiCommand (UI_NONE if cancelled) for main to dispatch. */
#define QL_CW    240
#define QL_LM    24
#define QL_BTNH  24
#define QL_PITCH 30
#define QL_TOP   16
#define QL_MAXITEMS 8
#define QL_HDR   24                  /* header band (the "MacOS Version" line) above the buttons */

static char gQlHeader[48];           /* "MacOS Version: X", set before each modal loop */

static void ql_btn_rect(int i, Rect *r)
{ short y = (short)(QL_TOP + QL_HDR + i * QL_PITCH); SetRect(r, QL_LM, y, (short)(QL_CW - QL_LM), (short)(y + QL_BTNH)); }

static void ql_focus_frame(int i, int on)
{
    Rect r; ql_btn_rect(i, &r); InsetRect(&r, -4, -4);
    PenPat(on ? &qd.black : &qd.white);
    FrameRect(&r);
    PenPat(&qd.black);
}

static void ql_draw(WindowPtr dlg, int focus)
{
    Rect content = dlg->portRect;
    SetPort(dlg);
    EraseRect(&content);
    if (gQlHeader[0]) {                                  /* the "MacOS Version: X" header band */
        Str255 h; short w;
        TextFont(0); TextSize(12);
        c2p255(gQlHeader, h);
        w = StringWidth(h);
        MoveTo((short)((content.right - content.left - w) / 2), (short)(QL_TOP + 6));
        DrawString(h);
    }
    DrawControls(dlg);
    ql_focus_frame(focus, 1);
}

static void ql_set_focus(int *focus, int nf)
{
    if (nf == *focus) return;
    ql_focus_frame(*focus, 0);
    *focus = nf;
    ql_focus_frame(*focus, 1);
}

static UiCommand run_quicklaunch_menu(void)
{
    WindowPtr     dlg;
    Rect          bounds, sb = qd.screenBits.bounds;
    int           n = ui_menu_count(&gUi), i, focus = 0, running = 1;
    short         CH;
    UiCommand     result = UI_NONE;
    ControlHandle btn[QL_MAXITEMS];

    if (n < 1) return UI_NONE;
    if (n > QL_MAXITEMS) n = QL_MAXITEMS;
    strcpy(gQlHeader, "MacOS Version: "); env_os_version(gEnv.sysVers, gQlHeader + 15);
    CH = (short)(QL_TOP + QL_HDR + n * QL_PITCH + 14);
    {
        short L = (short)(sb.left + ((sb.right - sb.left) - QL_CW) / 2);
        short T = (short)(sb.top  + ((sb.bottom - sb.top) - CH) / 2);
        if (T < (short)(sb.top + 44)) T = (short)(sb.top + 44);
        SetRect(&bounds, L, T, (short)(L + QL_CW), (short)(T + CH));
    }
    dlg = NewWindow(0L, &bounds, "\pQuick-Launch Menu", true, movableDBoxProc, (WindowPtr)-1L, false, 0L);
    if (!dlg) return UI_NONE;
    SetPort(dlg);
    TextFont(0); TextSize(12);
    for (i = 0; i < n; i++) {
        Rect r; Str255 t;
        ql_btn_rect(i, &r);
        c2p255(ui_menu_label(&gUi, i), t);
        btn[i] = NewControl(dlg, &r, t, true, 0, 0, 0, pushButProc, 0L);
    }
    ql_draw(dlg, focus);
    ValidRect(&dlg->portRect);

    while (running) {
        EventRecord evt;
        if (!next_event(&evt)) continue;
        switch (evt.what) {
            case updateEvt: {
                WindowPtr w = (WindowPtr)evt.message;
                if (w == dlg) { BeginUpdate(w); ql_draw(dlg, focus); EndUpdate(w); }
                else { BeginUpdate(w); SetPort(w); ui_draw(&gUi); EndUpdate(w); SetPort(dlg); }
                break;
            }
            case mouseDown: {
                WindowPtr w; short part = FindWindow(evt.where, &w);
                if (part == inDrag && w == dlg) { DragWindow(w, evt.where, &sb); SetPort(dlg); }
                else if (part == inContent && w == dlg) {
                    Point p = evt.where; ControlHandle ctl; short cp;
                    SetPort(dlg); GlobalToLocal(&p);
                    cp = FindControl(p, dlg, &ctl);
                    if (cp && ctl && TrackControl(ctl, p, (ControlActionUPP)0))
                        for (i = 0; i < n; i++)
                            if (ctl == btn[i]) { result = ui_menu_command(&gUi, i); running = 0; break; }
                } else if (w != dlg) {
                    SysBeep(1);
                }
                break;
            }
            case keyDown:
            case autoKey: {
                char c = (char)(evt.message & charCodeMask);
                if (evt.modifiers & cmdKey) { if (c == '.') running = 0; break; }
                switch (c) {
                    case kCharEscape: running = 0; break;
                    case kCharUp:     ql_set_focus(&focus, (focus - 1 + n) % n); break;
                    case '\t':
                    case kCharDown:   ql_set_focus(&focus, (focus + 1) % n); break;
                    case ' ':
                    case kCharReturn:
                    case kCharEnter:  result = ui_menu_command(&gUi, focus); running = 0; break;
                }
                break;
            }
        }
    }
    DisposeWindow(dlg);
    return result;
}

/* ---- the System Folder Chooser (docs/36/37 Phase 2) ---------------------------
 * A movable modal window of standard push buttons — one per blessable System Folder
 * (the current one bulleted) plus Cancel — reusing the Quick-Launch menu's built-in
 * widgets + focus-ring model. Choosing a folder blesses it (bless_set), then asks
 * whether to shut down now — the launcher never triggers an in-core reboot; the swap
 * takes effect on the next power-on. Returns 1 if the user chose Shut Down. */
static void osc_label(const SysFolder *s, Str255 out)
{
    int  n = 0, i;
    char v[16];
    if (s->blessed) { out[++n] = 0xA5; out[++n] = ' '; }        /* 0xA5 = • (MacRoman) */
    for (i = 1; i <= s->name[0] && n < 200; i++) out[++n] = s->name[i];
    if (s->version > 0) {                                       /* + the real System version */
        env_os_version(s->version, v);
        out[++n] = ' '; out[++n] = ' '; out[++n] = '(';
        for (i = 0; v[i] && n < 250; i++) out[++n] = v[i];
        out[++n] = ')';
    }
    out[0] = (unsigned char)n;
}

#define OSC_STATUS_H 34   /* band below the buttons for one status / warning line */

/* Compatibility gating (docs/40). A candidate System Folder is bootable on THIS
 * Mac iff its System version is within [envelope floor 6.0.4, this CPU tier's
 * ceiling `gEnv.maxOSbcd`]. The running System, an unreadable version, and a
 * failed tier probe are always allowed, so we never falsely grey a folder. */
static int osc_bootable(const SysFolder *s)
{
    if (s->blessed) return 1;                 /* the running System obviously boots */
    if (s->version <= 0) return 1;            /* version unreadable: don't disable  */
    if (gEnv.maxOSbcd <= 0) return 1;         /* tier probe failed: don't disable   */
    return (s->version >= 0x0604 && s->version <= gEnv.maxOSbcd);
}

/* One-line status for the focused item: why a folder is greyed (too new for this
 * Mac), or that a swap there would land in the Finder (docs/40 #3). Empty when
 * there's nothing to flag — Cancel, or a folder MacAtrium already runs under. */
static void osc_status_text(int i, const SysFolder *sys, int nsys, char *out)
{
    out[0] = '\0';
    if (i < 0 || i >= nsys) return;                       /* Cancel / out of range */
    if (!osc_bootable(&sys[i])) {
        char v[12];
        env_os_version(gEnv.maxOSbcd, v);
        strcpy(out, "Won't boot on this Mac - max System ");
        strcat(out, v);
    } else if (!sys[i].blessed && !sys[i].macatriumReady) {
        strcpy(out, "MacAtrium not installed - boots to Finder");
    }
}

static void osc_status_rect(WindowPtr dlg, int nbtn, Rect *r)
{
    short y = (short)(QL_TOP + QL_HDR + nbtn * QL_PITCH + 2);
    SetRect(r, 8, y, (short)(dlg->portRect.right - dlg->portRect.left - 8),
            (short)(y + OSC_STATUS_H - 4));
}

/* Redraw just the status band for the focused item (called after focus moves, so
 * the buttons/frames aren't disturbed). */
static void osc_draw_status(WindowPtr dlg, int focus, const SysFolder *sys, int nsys, int nbtn)
{
    Rect   r;
    char   msg[64];
    Str255 p;
    osc_status_rect(dlg, nbtn, &r);
    EraseRect(&r);
    osc_status_text(focus, sys, nsys, msg);
    if (msg[0]) {
        TextFont(0); TextSize(9);
        c2p255(msg, p);
        MoveTo(r.left, (short)(r.top + 12));
        DrawString(p);
        TextSize(12);                                     /* restore for header/buttons */
    }
}

/* Draw the post-bless shut-down confirmation: message + buttons + default ring. */
static void osc_sd_draw(WindowPtr dlg, const unsigned char *name, ControlHandle sd)
{
    Rect r;
    SetPort(dlg);
    EraseRect(&dlg->portRect);
    TextFont(0); TextSize(12);
    TextFace(bold);
    { const char *s = "Startup System Folder set to:"; MoveTo(20, 26); DrawText((Ptr)s, 0, (short)strlen(s)); }
    TextFace(normal);
    MoveTo(38, 46); DrawString(name);
    { const char *s = "Shut down now, then switch the computer"; MoveTo(20, 74); DrawText((Ptr)s, 0, (short)strlen(s)); }
    { const char *s = "back on to start up from it.";            MoveTo(20, 90); DrawText((Ptr)s, 0, (short)strlen(s)); }
    DrawControls(dlg);
    r = (**sd).contrlRect; InsetRect(&r, -4, -4);
    PenSize(3, 3); FrameRoundRect(&r, 16, 16); PenSize(1, 1);   /* the default-button ring */
}

/* After a bless swap the machine must restart to boot the new System — but the
 * launcher never triggers an in-core reboot. The emulator CPU cores can't restart
 * in core (you power-cycle through the emulator/OSD); on real hardware a clean
 * shutdown + power-on is equally safe. So ask: shut down now (the caller runs the
 * normal Shut Down path — flush + ShutDwnPower), or Later, leaving the swap to take
 * effect on the next manual restart. Returns 1 for Shut Down, 0 for Later. */
static int osc_confirm_shutdown(const unsigned char *name)
{
    WindowPtr     dlg;
    Rect          bounds, r, sb = qd.screenBits.bounds;
    ControlHandle sd, later;
    int           running = 1, result = 0;
    const short   CW = 344, CHh = 128;
    short         L = (short)(sb.left + ((sb.right - sb.left) - CW) / 2);
    short         T = (short)(sb.top  + ((sb.bottom - sb.top) - CHh) / 2);

    if (T < (short)(sb.top + 44)) T = (short)(sb.top + 44);
    SetRect(&bounds, L, T, (short)(L + CW), (short)(T + CHh));
    dlg = NewWindow(0L, &bounds, "\p", true, movableDBoxProc, (WindowPtr)-1L, false, 0L);
    if (!dlg) return 0;
    SetPort(dlg);

    SetRect(&r, (short)(CW - 20 - 96),      (short)(CHh - 32), (short)(CW - 20),      (short)(CHh - 12));
    sd    = NewControl(dlg, &r, "\pShut Down", true, 0, 0, 0, pushButProc, 0L);
    SetRect(&r, (short)(CW - 20 - 96 - 84), (short)(CHh - 32), (short)(CW - 20 - 96 - 16), (short)(CHh - 12));
    later = NewControl(dlg, &r, "\pLater", true, 0, 0, 0, pushButProc, 0L);
    (void)later;

    osc_sd_draw(dlg, name, sd);
    ValidRect(&dlg->portRect);

    /* Discard any keystrokes still queued from the chooser selection (a held/repeated
     * Return that blessed the folder must NOT pass through to this dialog's default
     * and confirm a shutdown the user never saw). The confirm waits for a fresh key. */
    FlushEvents(keyDownMask | autoKeyMask, 0);

    while (running) {
        EventRecord evt;
        if (!next_event(&evt)) continue;
        switch (evt.what) {
            case updateEvt: {
                WindowPtr w = (WindowPtr)evt.message;
                if (w == dlg) { BeginUpdate(w); osc_sd_draw(dlg, name, sd); EndUpdate(w); }
                else { BeginUpdate(w); SetPort(w); ui_draw(&gUi); EndUpdate(w); SetPort(dlg); }
                break;
            }
            case mouseDown: {
                WindowPtr w; short part = FindWindow(evt.where, &w);
                if (part == inDrag && w == dlg) { DragWindow(w, evt.where, &sb); SetPort(dlg); }
                else if (part == inContent && w == dlg) {
                    Point pt = evt.where; ControlHandle ctl; short cp;
                    SetPort(dlg); GlobalToLocal(&pt);
                    cp = FindControl(pt, dlg, &ctl);
                    if (cp && ctl && TrackControl(ctl, pt, (ControlActionUPP)0)) { result = (ctl == sd); running = 0; }
                } else if (w != dlg) { SysBeep(1); }   /* modal: block clicks elsewhere */
                break;
            }
            case keyDown:
            case autoKey: {
                char c = (char)(evt.message & charCodeMask);
                if ((evt.modifiers & cmdKey) && c == '.')     { result = 0; running = 0; }
                else if (c == kCharReturn || c == kCharEnter) { result = 1; running = 0; }   /* default: Shut Down */
                else if (c == kCharEscape)                    { result = 0; running = 0; }   /* Later */
                break;
            }
        }
    }
    DisposeWindow(dlg);
    return result;
}

static int run_os_chooser(void)
{
    SysFolder     sys[BLESS_MAX_SYS];
    WindowPtr     dlg;
    Rect          bounds, sb = qd.screenBits.bounds;
    int           nsys = bless_enumerate(sys, BLESS_MAX_SYS, gEnv.sysVers);
    int           n, i, focus = 0, running = 1, didBless = 0;
    short         CH;
    ControlHandle btn[QL_MAXITEMS];
    Str63         chosenName;

    if (nsys < 1) { SysBeep(1); return 0; }                     /* nothing blessable */
    if (nsys > QL_MAXITEMS - 1) nsys = QL_MAXITEMS - 1;         /* leave a slot for Cancel */
    n = nsys + 1;
    for (i = 0; i < nsys; i++) if (sys[i].blessed) focus = i;   /* start on the current OS */

    strcpy(gQlHeader, "MacOS Version: "); env_os_version(gEnv.sysVers, gQlHeader + 15);
    CH = (short)(QL_TOP + QL_HDR + n * QL_PITCH + OSC_STATUS_H);
    {
        short L = (short)(sb.left + ((sb.right - sb.left) - QL_CW) / 2);
        short T = (short)(sb.top  + ((sb.bottom - sb.top) - CH) / 2);
        if (T < (short)(sb.top + 44)) T = (short)(sb.top + 44);
        SetRect(&bounds, L, T, (short)(L + QL_CW), (short)(T + CH));
    }
    dlg = NewWindow(0L, &bounds, "\pSystem Folder Chooser", true, movableDBoxProc, (WindowPtr)-1L, false, 0L);
    if (!dlg) return 0;
    SetPort(dlg);
    TextFont(0); TextSize(12);
    for (i = 0; i < n; i++) {
        Rect r; Str255 t;
        ql_btn_rect(i, &r);
        if (i < nsys) osc_label(&sys[i], t);
        else BlockMoveData("\pCancel", t, 7);
        btn[i] = NewControl(dlg, &r, t, true, 0, 0, 0, pushButProc, 0L);
    }
    for (i = 0; i < nsys; i++)                          /* grey out un-bootable Systems */
        if (!osc_bootable(&sys[i])) HiliteControl(btn[i], 255);
    ql_draw(dlg, focus);
    osc_draw_status(dlg, focus, sys, nsys, n);
    ValidRect(&dlg->portRect);

    while (running) {
        EventRecord evt;
        if (!next_event(&evt)) continue;
        switch (evt.what) {
            case updateEvt: {
                WindowPtr w = (WindowPtr)evt.message;
                if (w == dlg) { BeginUpdate(w); ql_draw(dlg, focus); osc_draw_status(dlg, focus, sys, nsys, n); EndUpdate(w); }
                else { BeginUpdate(w); SetPort(w); ui_draw(&gUi); EndUpdate(w); SetPort(dlg); }
                break;
            }
            case mouseDown: {
                WindowPtr w; short part = FindWindow(evt.where, &w);
                if (part == inDrag && w == dlg) { DragWindow(w, evt.where, &sb); SetPort(dlg); }
                else if (part == inContent && w == dlg) {
                    Point p = evt.where; ControlHandle ctl; short cp;
                    SetPort(dlg); GlobalToLocal(&p);
                    cp = FindControl(p, dlg, &ctl);
                    if (cp && ctl && TrackControl(ctl, p, (ControlActionUPP)0))
                        for (i = 0; i < n; i++)
                            if (ctl == btn[i]) {
                                if (i >= nsys) running = 0;                              /* Cancel */
                                else if (!osc_bootable(&sys[i])) SysBeep(1);             /* greyed: can't boot */
                                else if (bless_set(sys[i].dirID) != noErr) SysBeep(1);   /* bless failed: stay */
                                else { BlockMoveData(sys[i].name, chosenName, (long)sys[i].name[0] + 1);
                                       didBless = 1; running = 0; }                       /* then prompt shutdown */
                                break;
                            }
                } else if (w != dlg) { SysBeep(1); }
                break;
            }
            case keyDown:
            case autoKey: {
                char c = (char)(evt.message & charCodeMask);
                if (evt.modifiers & cmdKey) { if (c == '.') running = 0; break; }
                switch (c) {
                    case kCharEscape: running = 0; break;
                    case kCharUp:     ql_set_focus(&focus, (focus - 1 + n) % n);
                                      osc_draw_status(dlg, focus, sys, nsys, n); break;
                    case '\t':
                    case kCharDown:   ql_set_focus(&focus, (focus + 1) % n);
                                      osc_draw_status(dlg, focus, sys, nsys, n); break;
                    case ' ':
                    case kCharReturn:
                    case kCharEnter:
                        if (focus >= nsys) running = 0;                          /* Cancel */
                        else if (!osc_bootable(&sys[focus])) SysBeep(1);         /* greyed: can't boot */
                        else if (bless_set(sys[focus].dirID) != noErr) SysBeep(1);   /* bless failed: stay */
                        else { BlockMoveData(sys[focus].name, chosenName, (long)sys[focus].name[0] + 1);
                               didBless = 1; running = 0; }                           /* then prompt shutdown */
                        break;
                }
                break;
            }
        }
    }
    DisposeWindow(dlg);
    /* 0 = no change (Cancel); 1 = blessed + Shut Down now; 2 = blessed + Later. */
    if (didBless) return osc_confirm_shutdown(chosenName) ? 1 : 2;
    return 0;
}

/* ---- MacAtrium Status (docs/37) ------------------------------------------------
 * A read-only movable modal: the environment (OS / depth / build) and every mounted
 * library disk with what it contributes — the legend for the browse view's [N] disk
 * tokens. Done / Esc / Return closes it. */
#define ST_CW 380
#define ST_LM 20

static void st_line(short x, short y, const char *s)
{
    MoveTo(x, y);
    DrawText((Ptr)s, 0, (short)strlen(s));
}

/* Append a byte count rounded to KB, with a trailing 'K' (docs/44 readout). */
static void st_appendK(char *dst, long bytes)
{
    append_long(dst, (bytes + 512) / 1024);
    strcat(dst, "K");
}

/* Vertical space st_draw's "Memory & art" section adds (bold header + four
 * readout lines, minus 2px reclaimed by tightening the Screen line); folded into
 * the Status window height CH. */
#define ST_MEM_SECTION 84

/* Categories + titles contributed by the disk at volume-table index `vol`. */
static void st_disk_counts(int vol, int *ncat, long *ntitle)
{
    int i;
    *ncat = 0; *ntitle = 0;
    for (i = 0; i < gModel.ncats; i++)
        if (gModel.cats[i].vol == vol) { (*ncat)++; *ntitle += gModel.cats[i].count; }
}

static void st_draw(WindowPtr dlg)
{
    char  line[96];
    short y = 24;
    int   i;

    SetPort(dlg);
    EraseRect(&dlg->portRect);
    TextFont(0); TextSize(12);

    TextFace(bold); st_line(ST_LM, y, "MacAtrium Status"); TextFace(normal); y = (short)(y + 22);

    env_os_name(gEnv.sysVers, line);
    st_line(ST_LM, y, line); y = (short)(y + 16);

    strcpy(line, "Build "); strcat(line, MACATRIUM_VERSION);
    st_line(ST_LM, y, line); y = (short)(y + 16);

    strcpy(line, "Screen: "); append_long(line, display_current_depth()); strcat(line, "-bit");
    st_line(ST_LM, y, line); y = (short)(y + 20);

    /* docs/44 P1: the runtime art-capability set — granted partition, the art
     * budget we carve from it, and which of the 1/8/24-bit tiers this machine can
     * show (VRAM) AND hold (memory). Measurement only; nothing here changes what
     * art is drawn. Read these at two partition sizes to fill docs/44's table. */
    TextFace(bold); st_line(ST_LM, y, "Memory & art (docs/44)"); TextFace(normal); y = (short)(y + 16);

    strcpy(line, "Partition ");   st_appendK(line, gArtCaps.grantedPartition);
    strcat(line, "  free ");      st_appendK(line, gArtCaps.partitionFree);
    strcat(line, "  blk ");       st_appendK(line, gArtCaps.maxBlock);
    st_line(ST_LM, y, line); y = (short)(y + 16);

    strcpy(line, "Art budget ");  st_appendK(line, gArtCaps.artBudget);
    strcat(line, "   (tmp ");     st_appendK(line, gArtCaps.tempFree);
    strcat(line, ")");
    st_line(ST_LM, y, line); y = (short)(y + 16);

    strcpy(line, "Tiers 1/8/24 ");
    strcat(line, gArtCaps.enabled[ART_MODE_1BIT]  ? "on"  : "off"); strcat(line, "/");
    strcat(line, gArtCaps.enabled[ART_MODE_8BIT]  ? "on"  : "off"); strcat(line, "/");
    strcat(line, gArtCaps.enabled[ART_MODE_24BIT] ? "on"  : "off");
    strcat(line, "   max ");      append_long(line, gArtCaps.maxAffordableDepth); strcat(line, "-bit");
    st_line(ST_LM, y, line); y = (short)(y + 16);

    strcpy(line, "Peak 8/24 ");   st_appendK(line, gArtCaps.peakArtBytes[ART_MODE_8BIT]);
    strcat(line, "/");            st_appendK(line, gArtCaps.peakArtBytes[ART_MODE_24BIT]);
    strcat(line, "  default ");   append_long(line, gArtCaps.defaultMode); strcat(line, "-bit");
    st_line(ST_LM, y, line); y = (short)(y + 22);

    TextFace(bold); st_line(ST_LM, y, "Library disks"); TextFace(normal); y = (short)(y + 18);

    for (i = 0; i < gVols.n; i++) {
        int  ncat, off, k, ln;
        long ntitle;
        st_disk_counts(i, &ncat, &ntitle);
        strcpy(line, "Disk "); append_long(line, i); strcat(line, "  ");
        ln  = gVols.v[i].name[0];                        /* Pascal length byte */
        off = (int)strlen(line);
        for (k = 0; k < ln && off < 78; k++) line[off++] = (char)gVols.v[i].name[1 + k];
        line[off] = '\0';
        st_line(ST_LM, y, line); y = (short)(y + 14);
        strcpy(line, "  "); append_long(line, ncat); strcat(line, " categories, ");
        append_long(line, ntitle); strcat(line, " titles");
        st_line((short)(ST_LM + 12), y, line); y = (short)(y + 18);
    }
    if (gVols.n == 0) st_line(ST_LM, y, "No library disks found.");

    DrawControls(dlg);
}

static void run_status_dialog(void)
{
    WindowPtr     dlg;
    Rect          bounds, r, sb = qd.screenBits.bounds;
    int           running = 1;
    short         CH;
    ControlHandle done;
    int           nrows = gVols.n > 0 ? gVols.n : 1;

    CH = (short)(120 + ST_MEM_SECTION + nrows * 32 + 44);
    {
        short L = (short)(sb.left + ((sb.right - sb.left) - ST_CW) / 2);
        short T = (short)(sb.top  + ((sb.bottom - sb.top) - CH) / 2);
        if (T < (short)(sb.top + 44)) T = (short)(sb.top + 44);
        SetRect(&bounds, L, T, (short)(L + ST_CW), (short)(T + CH));
    }
    dlg = NewWindow(0L, &bounds, "\pMacAtrium Status", true, movableDBoxProc, (WindowPtr)-1L, false, 0L);
    if (!dlg) return;
    SetPort(dlg);
    SetRect(&r, (short)(ST_CW - ST_LM - 78), (short)(CH - 30), (short)(ST_CW - ST_LM), (short)(CH - 10));
    done = NewControl(dlg, &r, "\pDone", true, 1, 0, 1, pushButProc, 0L);
    (void)done;

    st_draw(dlg);
    ValidRect(&dlg->portRect);

    while (running) {
        EventRecord evt;
        if (!next_event(&evt)) continue;
        switch (evt.what) {
            case updateEvt: {
                WindowPtr w = (WindowPtr)evt.message;
                if (w == dlg) { BeginUpdate(w); st_draw(dlg); EndUpdate(w); }
                else { BeginUpdate(w); SetPort(w); ui_draw(&gUi); EndUpdate(w); SetPort(dlg); }
                break;
            }
            case mouseDown: {
                WindowPtr w; short part = FindWindow(evt.where, &w);
                if (part == inDrag && w == dlg) { DragWindow(w, evt.where, &sb); SetPort(dlg); }
                else if (part == inContent && w == dlg) {
                    Point p = evt.where; ControlHandle ctl; short cp;
                    SetPort(dlg); GlobalToLocal(&p);
                    cp = FindControl(p, dlg, &ctl);
                    if (cp && ctl && TrackControl(ctl, p, (ControlActionUPP)0)) running = 0;   /* Done */
                } else if (w != dlg) { SysBeep(1); }
                break;
            }
            case keyDown:
            case autoKey: {
                char c = (char)(evt.message & charCodeMask);
                if ((evt.modifiers & cmdKey) && c == '.') running = 0;
                else if (c == kCharEscape || c == kCharReturn || c == kCharEnter) running = 0;
                break;
            }
        }
    }
    DisposeWindow(dlg);
}

static void handle_ui_command(UiCommand cmd)
{
    switch (cmd) {
        case UI_LAUNCH:   do_launch(); save_prefs(); break;
        case UI_SHOW_FINDER:
            /* Front the Finder; it draws its own menu bar (we get suspended and, on
             * resume, repaint ours). If no Finder is resident we stayed front. */
            restore_system_menu_bar();    /* hand it our bar (we may have hidden it) */
            if (!sysctl_show_finder()) {
                ui_set_status(&gUi, "Finder not resident - use Restart.");
                show_menu_bar();
                ui_draw(&gUi);
            }
            break;
        case UI_OPEN_CDEV: {
            /* Open the chosen control panel via the Finder: send the odoc and front
             * the Finder so the cdev is visible (it draws its own bar; on resume we
             * repaint ours). */
            const CtlPanel *cp = ui_current_cdev(&gUi);
            if (cp) {
                OSErr oe;
                oe = ctlpanels_open(cp);
                if (oe == noErr) {
                    restore_system_menu_bar();   /* the Finder shows the cdev with menus */
                    (void)sysctl_show_finder();
                } else {
                    char m[48];
                    gUi.mode = UI_MODE_LIST;   /* so the status shows */
                    strcpy(m, "Open control panel failed (err ");
                    append_long(m, oe);
                    strcat(m, ")");
                    ui_set_status(&gUi, m);
                    show_menu_bar();
                    ui_draw(&gUi);
                }
            }
            break;
        }
        case UI_QUIT:     save_prefs(); quit_to_finder();  break;  /* does not return */
        case UI_RESTART:  save_prefs(); sysctl_restart();  break;
        case UI_SHUTDOWN:
            save_prefs();
            /* Shutdown chime — synchronous so it finishes before the machine
             * powers off. No-op if none is baked. */
            if (gUi.sndShutdown) sound_play_file("sounds/shutdown", 0);
            sysctl_shutdown();
            break;
        case UI_PREFS_DIRTY: save_prefs(); break;
        case UI_CHROME_DIRTY:                 /* a menu-bar / title-bar toggle */
            rebuild_window();                 /* re-lay out the window + bar, repaint */
            save_prefs();
            break;
        case UI_OPEN_SETTINGS: {              /* the real Settings window (run modally) */
            int            chromeChanged = 0;
            SettingsResult sr = run_settings_dialog(&chromeChanged);
            gUi.mode = UI_MODE_LIST;
            if (chromeChanged) {
                rebuild_window();             /* a menu/title-bar toggle: re-lay out + repaint */
            } else {
                SetPort(gWin);
                ui_reblit(&gUi);              /* re-blit the browse (or re-render if a setting changed it) */
                ValidRect(&gWin->portRect);   /* swallow the dispose-exposed updateEvt */
            }
            save_prefs();
            if (sr == SD_OPEN_CDEVS) {        /* the Control Panels action button */
                gUi.ncdevs = ctlpanels_list(gUi.cdevs, CTLPANEL_MAX);
                gUi.cdevSel = 0; gUi.cdevTop = 0;
                gUi.mode = UI_MODE_CTLPANELS;
                ui_draw(&gUi);
            }
            break;
        }
        case UI_OPEN_MENU: {                  /* the real Quick-Launch menu window */
            UiCommand mc = run_quicklaunch_menu();
            gUi.mode = UI_MODE_LIST;
            SetPort(gWin);
            if (mc == UI_OPEN_SETTINGS) {
                /* Going straight into another modal window: don't re-blit the whole
                 * browse now (that full-screen flash is what the user sees "redraw
                 * before the Settings appear"). The Settings window opens on top; its
                 * update handler repaints just the sliver the closed menu exposed. */
                handle_ui_command(mc);
            } else {
                ui_reblit(&gUi);              /* the menu window never touched the buffer */
                ValidRect(&gWin->portRect);
                if (mc != UI_NONE) handle_ui_command(mc);   /* dispatch the chosen action */
            }
            break;
        }
        case UI_OPEN_CHOOSER: {              /* the System Folder Chooser (bless, then offer shutdown) */
            int r = run_os_chooser();
            if (r == 1) {                    /* blessed + Shut Down: proper shutdown, no return */
                handle_ui_command(UI_SHUTDOWN);
                break;
            }
            gUi.mode = UI_MODE_LIST;
            SetPort(gWin);
            if (r == 2) {                    /* blessed + Later: note the swap applies on next boot */
                ui_set_status(&gUi, "Startup System changed - applies on next boot.");
                ui_draw(&gUi);
            } else {
                ui_reblit(&gUi);             /* Cancel: the chooser never touched the buffer */
            }
            ValidRect(&gWin->portRect);
            break;
        }
        case UI_SHOW_STATUS: {               /* the MacAtrium Status screen (docs/37) */
            run_status_dialog();
            gUi.mode = UI_MODE_LIST;
            SetPort(gWin);
            ui_reblit(&gUi);                 /* the status window never touched the buffer */
            ValidRect(&gWin->portRect);
            break;
        }
        default: break;
    }
}

/* Dispatch a MenuSelect / MenuKey result (HiWord = menu id, LoWord = item number).
 * Most items funnel into the same handlers the keyboard uses; the View + the
 * Apple/Edit items poke the UI / Toolbox directly. HiliteMenu(0) unhighlights the
 * pulled-down title afterward. */
static void do_menu(long mr)
{
    short menu = HiWord(mr);
    short item = LoWord(mr);
    if (menu == 0) return;             /* click/key released off any menu */
    switch (menu) {
        case mApple:
            if (item == 1) {
                ui_show_about(&gUi);
            } else {                   /* a desk accessory (item 2 is the separator) */
                Str255 nm;
                GetMenuItemText(gAppleMenu, item, nm);
                (void)OpenDeskAcc(nm);
            }
            break;
        case mFile:
            if (item == kFileLaunch)             { do_launch(); save_prefs(); }
            else if (item == kFileGetInfo)       ui_show_info(&gUi);
            else if (gFileShowFinder && item == gFileShowFinder)
                                                 handle_ui_command(UI_SHOW_FINDER);
            else if (gFileQuit && item == gFileQuit)
                                                 ui_confirm_quit(&gUi);   /* ask first */
            break;
        case mEdit:
            (void)SystemEdit((short)(item - 1));   /* route to an active DA, if any */
            break;
        case mView:
            if (item >= 1 && item <= VIEW_N)     { ui_set_view(&gUi, item - 1); save_prefs(); }
            else if (item == kViewSettings)      handle_ui_command(UI_OPEN_SETTINGS);
            break;
        case mSpecial:
            if (item == kSpecialChooser)         handle_ui_command(UI_OPEN_CHOOSER);
            else if (item == kSpecialRestart)    handle_ui_command(UI_RESTART);
            else if (item == kSpecialShutdown)   handle_ui_command(UI_SHUTDOWN);
            break;
    }
    HiliteMenu(0);
}

int main(void)
{
    EventRecord evt;
    int loaded;

    toolbox_init();
    gScrollAction = NewControlActionUPP(scroll_action);  /* hold-to-scroll auto-repeat */
    env_probe(&gEnv);                 /* match whatever depth the OS is set to;
                                         saves the original menu-bar height       */
    gHasWNE = (gEnv.sysVers >= 0x0700);  /* WaitNextEvent + AppleEvent Mgr: System 7+ */
    /* The AppleEvent Manager is System 7+; AEInstallEventHandler doesn't exist on
     * base 6.0.8, so only install the handlers (and accept high-level events) there. */
    if (gHasWNE) install_ae_handlers();
    prefs_load(&gPrefs);              /* saved theme / volume / selection */

    gWin = make_window(&gEnv, 0, 0);   /* full screen below the bars; rebuild_window
                                        * re-lays out for the saved hide state below */
    render_init(&gRender, &gEnv);
    install_menus();                   /* real System menu bar (docs/28) */

    /* Match the LIVE screen to the saved/bootstrap depth — but NEVER write slot PRAM
     * here. Slot PRAM (the *boot* default) is only ever changed from the Settings
     * "Color Depth" stepper (ui.c apply_depth); a stray boot-default write is what left
     * some machines needing a PRAM reset. So we just raise the live depth each boot:
     *   • saved choice (prefs `depth`): re-apply it live if it isn't already;
     *   • first boot / no saved choice: come up at the deepest depth the DISPLAY CARD
     *     can show — display_depth_at_most(32) scans HasDepth for the card's top mode:
     *     32 bpp truecolor on an 8•24 card, 8-bit on an 8-bit card, 1-bit on a compact.
     *     Screen depth tracks the display SYSTEM, not the art budget — a deep screen on
     *     a small partition keeps the deep screen and just loads shallower art (docs/44:
     *     screen and art are separate axes; the budget cap belongs on the art variant,
     *     which is P2's job). Live only — the system still cold-boots at whatever PRAM
     *     says until the user picks a depth in Settings, which persists it. */
    if (gEnv.hasColorQD) {
        short want = (gPrefs.haveDepth && gPrefs.depth > 0)
                     ? display_depth_at_most((short)gPrefs.depth)
                     : display_depth_at_most(32);   /* first boot: the deepest the card can display */
        if (want >= 1 && want != display_current_depth() && display_set_depth(want) == noErr) {
            short got = display_current_depth();               /* what we actually got */
            gEnv.pixelSize = got;                              /* so art/UI pick the colour variant */
            gEnv.useColor  = (gEnv.hasColorQD && got >= 4);
            render_reset_for_depth(&gRender, &gEnv, got);
        }
    }

    /* docs/44 P1: with env + the live screen depth settled, measure the granted
     * partition and the card's depths → the art-capability set (MacAtrium Status
     * reports it; P2's budget-aware loader is the first consumer). Pure measurement. */
    art_caps_probe(&gArtCaps, &gEnv);

    if (gPrefs.haveTheme) render_set_theme(&gRender, gPrefs.theme);
    if (gPrefs.haveAppearance)             /* saved era-look override beats the OS default */
        render_set_appearance(&gRender, gPrefs.appearance, &gEnv);
    if (gPrefs.haveVol && sound_available()) sound_apply_vol(gPrefs.vol);  /* no boot beep */

    if (load_index()) {
        /* Paged catalog (docs/21): set the category list from the index, install
         * the page loader, and pull in the default category's first page. */
        model_index_init(&gModel, gRefs, gNrefs, load_page);
        load_page(&gModel, 0);
        loaded = 1;
    } else {
        /* Legacy single-file catalog (all items resident). */
        loaded = load_catalog();
        model_build(&gModel, &gCat);   /* empty catalog -> just "All" with 0 items */
    }
    if (gPrefs.haveSel) model_select(&gModel, gPrefs.category, gPrefs.item);

    ui_init(&gUi, &gEnv, &gRender, &gModel, gWin, loaded ? 0 : 1);
    gUi.vols = &gVols;   /* multi-disk (docs/37): art/launch resolve per source volume */
    gUi.caps = &gArtCaps;   /* docs/44 P2: the art loader caps each cover at the affordable tier */
    if (gPrefs.haveArtPref) gUi.artPref = gPrefs.artPref;   /* restore Artwork choice */
    if (gPrefs.haveSndStartup)  gUi.sndStartup  = gPrefs.sndStartup;   /* restore sound prefs */
    if (gPrefs.haveSndShutdown) gUi.sndShutdown = gPrefs.sndShutdown;
    if (gPrefs.haveCatList)     gUi.catList     = gPrefs.catList;      /* restore cat-list view */
    if (gPrefs.haveHideMenuBar)  gUi.hideMenuBar  = gPrefs.hideMenuBar;  /* restore chrome */
    if (gPrefs.haveHideTitleBar) gUi.hideTitleBar = gPrefs.hideTitleBar;
    if (gPrefs.haveTextSize)     ui_set_text_size(&gUi, gPrefs.textSize);  /* restore Text Size */
    if (gPrefs.haveGridStyle)    gUi.gridStyle    = gPrefs.gridStyle;      /* restore Grid Style */
    if (gPrefs.haveSort) { gUi.sortMode = gPrefs.sortMode; gUi.sortDesc = gPrefs.sortDesc;
                           model_sort_page(&gModel, gUi.sortMode, gUi.sortDesc); }  /* sort the loaded page */
    if (gPrefs.haveListCol)      gUi.listColType  = gPrefs.listColType;     /* restore column width */
    if (gPrefs.haveCarousel)    gUi.carousel    = gPrefs.carousel;    /* restore carousel size */
    if (gPrefs.haveView)        gUi.view        = gPrefs.view;        /* restore browse view */
    else if (loaded)            gUi.mode        = UI_MODE_SETUP;      /* first run: ask how to browse */

    bring_self_front();
    if (gUi.hideMenuBar || gUi.hideTitleBar) {
        rebuild_window();          /* apply the saved hide state (re-lays out + repaints) */
    } else {
        SetPort(gWin);
        ui_draw(&gUi);
    }
    /* We just painted the first frame ourselves. Validate the whole window so the
     * updateEvt the OS queued — for the new window's content AND the boot 1-bit->8-bit
     * depth bump (display_set_depth above invalidates every window over the changed
     * screen) — finds an empty region and doesn't drive a redundant second full
     * repaint. That double-paint was the "refreshes when the app first loads". A
     * genuine later exposure still InvalRects + repaints normally. */
    SetPort(gWin);
    ValidRect(&gWin->portRect);

    /* Startup chime (async so it overlaps the UI coming up); off by default,
     * a no-op if no sound was baked into the image. */
    if (gUi.sndStartup) sound_play_file("sounds/startup", 1);

    for (;;) {
        mem_debug_tick(gWin);   /* dev-only memory overlay; no-op without MEM_DEBUG */
        if (!next_event(&evt)) {
            /* Idle: load the settled selection's detail art (deferred so a fast
             * scroll never blocks on decoding a colour PICT). Repaint only the
             * cover box for an art load — repainting the whole screen there was the
             * "double refresh" on flip; a depth change still needs a full redraw. */
            {
                int rc = ui_idle(&gUi);
                if (rc == UI_IDLE_FULL)     ui_draw(&gUi);
                else if (rc == UI_IDLE_ART) ui_draw_art(&gUi);
            }
            continue;
        }
        {
            switch (evt.what) {
                case keyDown:
                case autoKey: {
                    char c = (char)(evt.message & charCodeMask);
                    short keyCode = (short)((evt.message >> 8) & 0xFF);
                    /* Tab cycles the browse view — a discrete action, so ignore the
                     * auto-repeat (a held Tab used to spin through every view). */
                    if (c == '\t' && evt.what == autoKey) break;
                    if (evt.modifiers & cmdKey) {
                        /* Cmd-Option-Q quits the launcher back to the Finder. Match
                         * the virtual key CODE (Q = 0x0C), not the char: Option
                         * mangles it (Option-Q yields the "oe" ligature). */
                        if ((evt.modifiers & optionKey) && keyCode == 0x0C) {
                            ui_confirm_quit(&gUi); /* ask before quitting */
                            break;
                        }
                        /* Other Cmd-combos: a menu shortcut if MenuKey matches one,
                         * otherwise a UI Cmd-shortcut (theme / box art / per-item
                         * launch hotkey — plain keys are reserved for the filter). */
                        {
                            long mr = MenuKey(c);
                            if (HiWord(mr)) do_menu(mr);
                            else            handle_ui_command(ui_key_cmd(&gUi, c));
                        }
                        break;
                    }
                    handle_ui_command(ui_key(&gUi, c));
                    break;
                }
                case updateEvt: {
                    WindowPtr w = (WindowPtr)evt.message;
                    BeginUpdate(w);
                    ui_draw(&gUi);
                    EndUpdate(w);
                    break;
                }
                case mouseDown: {
                    /* The launcher is a single window below the menu bar. A menu-bar
                     * click pulls a menu down (MenuSelect); a content click maps to a
                     * UI hit-test (window-local); a click in a desk-accessory window
                     * goes to that DA (SystemClick). */
                    WindowPtr w;
                    short part = FindWindow(evt.where, &w);
                    if (part == inMenuBar) {
                        sync_view_menu();          /* checkmark the live view first */
                        do_menu(MenuSelect(evt.where));
                    } else if (part == inContent && w == gWin) {
                        Point         p = evt.where;
                        ControlHandle ctl;
                        short         cp;
                        SetPort(gWin);
                        GlobalToLocal(&p);
                        /* A real Control-Manager control under the click takes it
                         * (hidden controls are skipped by FindControl); otherwise the
                         * click is a normal UI hit-test. */
                        cp = FindControl(p, gWin, &ctl);
                        if (cp && ctl == gUi.launch) {
                            if (TrackControl(ctl, p, (ControlActionUPP)0) == inButton)
                                handle_ui_command(UI_LAUNCH);
                        } else if (cp && ctl == gUi.settingsBtn) {
                            if (TrackControl(ctl, p, (ControlActionUPP)0) == inButton)
                                handle_ui_command(UI_OPEN_MENU);   /* the Quick-Launch menu */
                        } else if (cp && ctl == gUi.quitBtn) {
                            if (TrackControl(ctl, p, (ControlActionUPP)0) == inButton)
                                handle_ui_command(UI_QUIT);   /* confirmed quit */
                        } else if (cp && ctl == gUi.cancelBtn) {
                            if (TrackControl(ctl, p, (ControlActionUPP)0) == inButton) {
                                gUi.mode = UI_MODE_LIST; ui_draw(&gUi);   /* cancel */
                            }
                        } else if (cp && ctl == gUi.scrollV) {
                            if (cp == inThumb) {
                                if (TrackControl(ctl, p, (ControlActionUPP)0))
                                    ui_scroll_to(&gUi, GetControlValue(ctl));
                            } else {
                                /* Arrow / page region: the action proc steps the
                                 * selection for as long as the part is held (a quick
                                 * click still fires it once), so no post-track step. */
                                TrackControl(ctl, p, gScrollAction);
                            }
                        } else {
                            handle_ui_command(ui_click(&gUi, p));
                        }
                    } else if (part == inGoAway && w == gWin) {
                        /* The title bar's close box asks before quitting. */
                        if (TrackGoAway(w, evt.where))
                            ui_confirm_quit(&gUi);
                    } else if (part == inDrag && w == gWin) {
                        /* The window is immovable, so a click on its title bar opens
                         * the menu hub instead — a way to reach the menu from the
                         * title bar, especially when the System menu bar is hidden. */
                        gUi.mode = UI_MODE_MENU; gUi.menuSel = 0;
                        ui_draw(&gUi);
                    } else if (part == inSysWindow) {
                        SystemClick(&evt, w);      /* a desk accessory's window */
                    } else {
                        SelectWindow(gWin);
                    }
                    break;
                }
                case kHighLevelEvent:
                    if (gHasWNE)                 /* AppleEvent Manager is System 7+ */
                        AEProcessAppleEvent(&evt);   /* dispatch to the handlers above */
                    break;
                case osEvt:
                    /* Suspend/resume from the app switcher. On suspend (e.g. we
                     * fronted the Finder for Show Finder or to open a control panel)
                     * hide our window so the Finder/control panel is visible. On
                     * resume, show + repaint, and redraw our menu bar (the front app
                     * drew its own over ours). */
                    if (((evt.message >> 24) & 0xFFL) == suspendResumeMessage) {
                        if (evt.message & resumeFlag) {
                            ShowWindow(gWin);
                            SelectWindow(gWin);
                            show_menu_bar();      /* honor the hide-menu-bar setting */
                            SetPort(gWin);
                            /* A depth-capped game just quit: put our LIVE depth back now
                             * that we're front again (ui_draw re-fits the backend). Never
                             * the boot default — slot PRAM is Settings-only. */
                            if (gPendingDepthRestore > 0) {
                                (void)display_set_depth(gPendingDepthRestore);
                                gPendingDepthRestore = 0;
                            }
                            ui_draw(&gUi);
                        } else {
                            HideWindow(gWin);
                        }
                    }
                    break;
            }
        }
    }
    return 0;
}
