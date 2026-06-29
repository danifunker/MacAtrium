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
#include "mem.h"
#include "mac_compat.h"

#include <string.h>

#ifndef plainDBox
#define plainDBox 2
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
static Render    gRender;
static Ui        gUi;
static WindowPtr gWin;
/* When a returning (concurrent) launch caps the screen depth for a game, we defer
 * putting our depth back until the game quits and we're reactivated (osEvt resume),
 * so the game keeps its capped depth the whole time it runs. 0 = nothing pending. */
static short gPendingDepthRestore = 0;
static Prefs     gPrefs;

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

/* True full-screen: drop the menu bar to 0 height AND cede its strip back to the
 * desktop (GrayRgn), so a full-screen window's visible region actually owns the
 * top — otherwise the Window Manager keeps clipping that strip and our header
 * never paints there. Called at startup and again after a sub-launch (the child
 * restores the bar). Restored for Launch Finder. */
static void hide_menu_bar(void)
{
    RgnHandle mbRgn;
    LMSetMBarHeight(0);
    mbRgn = NewRgn();
    if (mbRgn) {
        RgnHandle gray = LMGetGrayRgn();
        Rect mb = gEnv.screen;
        mb.bottom = (short)(mb.top + gEnv.mbarHeight);
        if (gray) {
            RectRgn(mbRgn, &mb);
            UnionRgn(gray, mbRgn, gray);
        }
        DisposeRgn(mbRgn);
    }
}

/* Give the system menu bar back its original height — for Show Finder and Quit,
 * so the Finder has its menus. (We leave the reclaimed GrayRgn strip alone; the
 * Finder redraws its own bar over it on activation.) */
static void restore_menu_bar(void)
{
    LMSetMBarHeight(gEnv.mbarHeight);
}

/* Quit the launcher entirely and hand the machine back to the Finder (the
 * resident boot shell) — Cmd-Option-Q. Restores the menu bar first so the Finder
 * comes up with its menus. Does not return. */
static void quit_to_finder(void)
{
    restore_menu_bar();
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

static WindowPtr make_window(const Env *e)
{
    Rect b = e->screen;              /* full screen — the menu bar is hidden    */
                                     /* (LMSetMBarHeight 0), so we own y = 0..top */
    /* A colour window (CGrafPort) when Color QD is present, so the off-screen
     * GWorld blits correctly at >1-bit depths (and the user can switch depth at
     * runtime); a plain B&W window otherwise. */
    if (e->hasColorQD)
        return NewCWindow(0L, &b, "\p", true, plainDBox, (WindowPtr)-1L, false, 0);
    return NewWindow(0L, &b, "\p", true, plainDBox, (WindowPtr)-1L, false, 0);
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

/* Read metadata/index.jsonl into gRefs[]; 1 if a paged catalog is present. */
static int load_index(void)
{
    FSSpec spec;
    char  *buf;
    long   len;
    gNrefs = 0;
    if (macfs_make_spec("metadata/index.jsonl", &spec) != noErr) return 0;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return 0;
    gNrefs = catindex_parse(buf, len, gRefs, MODEL_MAX_CATS);
    DisposePtr(buf);
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

    if (macfs_make_spec(path, &spec) == noErr &&
        macfs_read_all(&spec, &buf, &len) == noErr) {
        gCat.nitems = catalog_parse_into(buf, len, gCat.items, gCat.cap, &gCat.dropped);
        DisposePtr(buf);
    }
    model_set_page(m, &gCat);
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
    p.carousel = gUi.carousel;       p.haveCarousel = 1;
    p.depth = display_current_depth();  p.haveDepth = (p.depth > 0);  /* boot-depth pref */

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

/* A brief centred notice shown after a per-game screen-depth switch and before the
 * app launches, so it stays on screen while the depth change + app load take their
 * moment (and the screen flashes). Caller must have re-fit the backend to the new
 * depth first. */
static void show_switch_message(void)
{
    Rect        b = gWin->portRect;
    short       cx = (short)((b.left + b.right) / 2);
    short       cy = (short)((b.top + b.bottom) / 2);
    const char *msg = "Setting up the display - one moment...";
    Rect        box;
    short       w;

    SetRect(&box, (short)(cx - 190), (short)(cy - 24), (short)(cx + 190), (short)(cy + 24));
    render_begin(&gRender, gWin);
    render_fill(&gRender, &box, FILL_PANEL);
    render_frame(&gRender, &box);
    render_text_size(&gRender, 12);
    w = render_text_width(&gRender, msg);
    render_text(&gRender, (short)(cx - w / 2), (short)(cy + 4), msg, INK_NORMAL);
    render_end(&gRender, gWin);
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
     * hand the game the environment it expects — give the menu bar its height back
     * (we hid it) and reset the cursor. (A returning launch restores below.) */
    if (!returns) {
        restore_menu_bar();
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
                /* Commit to slot PRAM too, even though this is a temporary per-game
                 * cap: a bare SetDepth doesn't always stick, and the appliance reads
                 * its boot depth from PRAM when the game quits and relaunches us. */
                (void)display_set_default_depth(target);
                /* Re-fit our backend to the new depth and post the notice AT that
                 * depth, so it stays on screen while the app loads (a bare draw
                 * before SetDepth would be wiped by the mode switch instantly). */
                render_reset_for_depth(&gRender, &gEnv, target);
                show_switch_message();
            }
        }
    }

    lr = launch_app(app, returns, &lerr);

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
     * depth back, re-hide the menu bar on the appliance, re-front and report. */
    if (savedDepth > 0) {
        (void)display_set_depth(savedDepth);
        (void)display_set_default_depth(savedDepth);   /* keep PRAM in step (docs/15) */
        render_reset_for_depth(&gRender, &gEnv, savedDepth);
    }
    if (!returns) {
        hide_menu_bar();
        CalcVis((WindowPeek)gWin);
    }
    bring_self_front();
    hide_menu_bar();                  /* child restored the bar; re-hide + reclaim */
    CalcVis((WindowPeek)gWin);
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
static void handle_ui_command(UiCommand cmd)
{
    switch (cmd) {
        case UI_LAUNCH:   do_launch(); save_prefs(); break;
        case UI_SHOW_FINDER:
            restore_menu_bar();    /* the Finder needs its menus */
            if (!sysctl_show_finder()) {
                ui_set_status(&gUi, "Finder not resident - use Restart.");
                hide_menu_bar();   /* stayed put -> full-screen again */
                CalcVis((WindowPeek)gWin);
                ui_draw(&gUi);
            }
            break;
        case UI_OPEN_CDEV: {
            /* Open the chosen control panel via the Finder: give the menu bar
             * back, send the odoc, and front the Finder so the cdev is visible.
             * On resume (osEvt) we re-hide the bar. */
            const CtlPanel *cp = ui_current_cdev(&gUi);
            if (cp) {
                OSErr oe;
                restore_menu_bar();
                oe = ctlpanels_open(cp);
                if (oe == noErr) {
                    (void)sysctl_show_finder();
                } else {
                    char m[48];
                    gUi.mode = UI_MODE_LIST;   /* so the status shows */
                    strcpy(m, "Open control panel failed (err ");
                    append_long(m, oe);
                    strcat(m, ")");
                    ui_set_status(&gUi, m);
                    hide_menu_bar();
                    CalcVis((WindowPeek)gWin);
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
        default: break;
    }
}

int main(void)
{
    EventRecord evt;
    int loaded;

    toolbox_init();
    env_probe(&gEnv);                 /* match whatever depth the OS is set to;
                                         saves the original menu-bar height       */
    gHasWNE = (gEnv.sysVers >= 0x0700);  /* WaitNextEvent + AppleEvent Mgr: System 7+ */
    /* The AppleEvent Manager is System 7+; AEInstallEventHandler doesn't exist on
     * base 6.0.8, so only install the handlers (and accept high-level events) there. */
    if (gHasWNE) install_ae_handlers();
    prefs_load(&gPrefs);              /* saved theme / volume / selection */
    hide_menu_bar();                  /* true full-screen (restored for Launch
                                         Finder; gEnv keeps the original height) */

    gWin = make_window(&gEnv);
    CalcVis((WindowPeek)gWin);         /* claim the reclaimed top strip */
    render_init(&gRender, &gEnv);

    /* Colour depth persists in the video card's slot PRAM (display_set_default_depth
     * → cscSetDefaultMode), so the *system* boots at the chosen depth and we just
     * match it. Two cases:
     *   • saved choice (prefs `depth`): re-assert it as the boot default (self-heals
     *     if PRAM was zapped) and apply it to the live screen if it isn't already;
     *   • first boot / no saved choice: bootstrap to the best colour depth ≤ 8-bit
     *     and make THAT the boot default, so the next boot comes up in colour from
     *     the system — out-of-box colour without forcing it every boot.
     * The chosen depth is recorded by save_prefs (current depth) on the next save,
     * which makes the bootstrap one-time: a user who picks 1-bit then stays 1-bit.
     * display_depth_at_most() clamps a saved depth to what this card supports. */
    if (gEnv.hasColorQD) {
        short want = (gPrefs.haveDepth && gPrefs.depth > 0)
                     ? display_depth_at_most((short)gPrefs.depth)
                     : display_depth_at_most(8);
        if (want >= 1) {
            (void)display_set_default_depth(want);             /* persist as the boot default */
            if (want != display_current_depth() && display_set_depth(want) == noErr) {
                gEnv.pixelSize = want;                          /* so art/UI pick the colour variant */
                gEnv.useColor  = (gEnv.hasColorQD && want >= 4);
                render_reset_for_depth(&gRender, &gEnv, want);
            }
        }
    }

    if (gPrefs.haveTheme) render_set_theme(&gRender, gPrefs.theme);
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
    if (gPrefs.haveArtPref) gUi.artPref = gPrefs.artPref;   /* restore Artwork choice */
    if (gPrefs.haveSndStartup)  gUi.sndStartup  = gPrefs.sndStartup;   /* restore sound prefs */
    if (gPrefs.haveSndShutdown) gUi.sndShutdown = gPrefs.sndShutdown;
    if (gPrefs.haveCatList)     gUi.catList     = gPrefs.catList;      /* restore cat-list view */
    if (gPrefs.haveCarousel)    gUi.carousel    = gPrefs.carousel;    /* restore carousel size */

    bring_self_front();
    SetPort(gWin);
    ui_draw(&gUi);

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
                    /* Cmd-Option-Q quits the launcher back to the Finder. Match the
                     * virtual key CODE (Q = 0x0C), not the char: Option mangles the
                     * character (Option-Q yields the "oe" ligature, not 'q'). */
                    short keyCode = (short)((evt.message >> 8) & 0xFF);
                    if ((evt.modifiers & cmdKey) && (evt.modifiers & optionKey) &&
                        keyCode == 0x0C) {
                        quit_to_finder();          /* does not return */
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
                    /* The launcher is a single full-screen window, so a content
                     * click maps straight to a UI hit-test. Convert to window-local
                     * and let ui_click() return the command to dispatch. */
                    WindowPtr w;
                    short part = FindWindow(evt.where, &w);
                    if (part == inContent && w == gWin) {
                        Point p = evt.where;
                        SetPort(gWin);
                        GlobalToLocal(&p);
                        handle_ui_command(ui_click(&gUi, p));
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
                     * fronted the Finder for Show Finder or to open a control
                     * panel) hand the menu bar back AND hide our full-screen
                     * window so the Finder/control panel is actually visible. On
                     * resume, show + repaint full-screen and re-hide the bar. */
                    if (((evt.message >> 24) & 0xFFL) == suspendResumeMessage) {
                        if (evt.message & resumeFlag) {
                            ShowWindow(gWin);
                            SelectWindow(gWin);
                            hide_menu_bar();
                            CalcVis((WindowPeek)gWin);
                            SetPort(gWin);
                            /* A depth-capped game just quit: put our depth back now
                             * that we're front again (ui_draw re-fits the backend to
                             * the new depth). */
                            if (gPendingDepthRestore > 0) {
                                (void)display_set_depth(gPendingDepthRestore);
                                (void)display_set_default_depth(gPendingDepthRestore);
                                gPendingDepthRestore = 0;
                            }
                            ui_draw(&gUi);
                        } else {
                            restore_menu_bar();
                            HideWindow(gWin);
                        }
                    }
                    break;
            }
        }
    }
    return 0;
}
