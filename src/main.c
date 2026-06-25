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

    gCat.nitems = 0;

    if (macfs_make_spec("metadata/catalog.jsonl", &spec) != noErr) return 0;
    if (macfs_read_all(&spec, &buf, &len) != noErr) return 0;

    catalog_parse(buf, len, &gCat);
    DisposePtr(buf);

    return gCat.nitems > 0;
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

    if (!app) return;

    /* On the bare System-6 appliance the launch is non-returning: we won't run
     * again until the System relaunches us as the shell, so hand the game the
     * environment it expects — give the menu bar its height back (we hid it) and
     * reset the cursor. (On System 7 / MultiFinder the launch returns and the
     * post-launch block below restores everything instead.) */
    if (!gEnv.canLaunchReturn) {
        restore_menu_bar();
        InitCursor();
    }

    /* Per-game launch depth (catalog `maxDepth`, maintained in the overrides DB):
     * run this title at the highest supported depth ≤ maxDepth, SETTING the screen
     * there before launch (raises a low boot depth OR lowers a deep one). Some
     * titles need an exact depth — Dark Castle needs 1-bit (maxDepth 1); Prince of
     * Persia only does colour at 8-bit and B&W otherwise, never 16/24-bit
     * (maxDepth 8). No value (0) launches at the current depth (the default). On a
     * returning launch (System 7) we restore below; on the bare System-6 appliance
     * the relaunched MacAtrium restores its own depth at startup. */
    {
        int maxd = ui_current_maxdepth(&gUi);
        if (maxd > 0 && gEnv.hasColorQD) {
            short cur    = display_current_depth();
            short target = display_depth_at_most((short)maxd);
            if (target > 0 && target != cur && display_set_depth(target) == noErr) {
                savedDepth = cur;                      /* restore this on the app's quit */
                /* Re-fit our backend to the new depth and post the notice AT that
                 * depth, so it stays on screen while the app loads (a bare draw
                 * before SetDepth would be wiped by the mode switch instantly). */
                render_reset_for_depth(&gRender, &gEnv, target);
                show_switch_message();
            }
        }
    }

    lr = launch_app(app, gEnv.canLaunchReturn, &lerr);

    /* Reached on a returning launch (System 7) or a FAILED non-returning one.
     * Restore the capped depth and, on the appliance, re-hide the menu bar. */
    if (savedDepth > 0) (void)display_set_depth(savedDepth);
    if (!gEnv.canLaunchReturn) {
        hide_menu_bar();
        CalcVis((WindowPeek)gWin);
    }

    /* We are back: the child quit (launchContinue honoured). Pull ourselves
     * forward and redraw with selection intact. The child drew its own menu bar,
     * so re-hide ours; our full-screen redraw below paints over the old bar. */
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

    /* Come up in colour when the hardware can do it. Some setups (notably 6.0.8)
     * boot the screen in 1-bit even though the card supports 8-bit. Switch the
     * screen here — SetDepth at startup *does* take effect — but deliberately
     * DON'T update gEnv.pixelSize: that leaves a discrepancy the first ui_idle()
     * poll detects, which re-fits the backend to colour AND forces the repaint
     * (without it the screen keeps showing the stale B&W frame). Prefer 8-bit
     * (the carousel/art sweet spot); the user can still drop to 1-bit in Settings,
     * and launching a System-6 game drops to 1-bit for the game. */
    if (gEnv.hasColorQD && gEnv.pixelSize < 4) {
        short depths[8];
        int   n = display_depths(depths, 8), i, best = 0;
        for (i = 0; i < n; i++)
            if (depths[i] >= 4 && depths[i] <= 8 && depths[i] > best) best = depths[i];
        if (best >= 4) (void)display_set_depth((short)best);
    }

    if (gPrefs.haveTheme) render_set_theme(&gRender, gPrefs.theme);
    if (gPrefs.haveVol && sound_available()) sound_apply_vol(gPrefs.vol);  /* no boot beep */

    loaded = load_catalog();
    model_build(&gModel, &gCat);     /* empty catalog -> just "All" with 0 items */
    if (gPrefs.haveSel) model_select(&gModel, gPrefs.category, gPrefs.item);

    ui_init(&gUi, &gEnv, &gRender, &gModel, gWin, loaded ? 0 : 1);
    if (gPrefs.haveArtPref) gUi.artPref = gPrefs.artPref;   /* restore Artwork choice */
    if (gPrefs.haveSndStartup)  gUi.sndStartup  = gPrefs.sndStartup;   /* restore sound prefs */
    if (gPrefs.haveSndShutdown) gUi.sndShutdown = gPrefs.sndShutdown;
    if (gPrefs.haveCatList)     gUi.catList     = gPrefs.catList;      /* restore cat-list view */

    bring_self_front();
    SetPort(gWin);
    ui_draw(&gUi);

    /* Startup chime (async so it overlaps the UI coming up); off by default,
     * a no-op if no sound was baked into the image. */
    if (gUi.sndStartup) sound_play_file("sounds/startup", 1);

    for (;;) {
        if (!next_event(&evt)) {
            /* Idle: load the settled selection's detail art (deferred so a fast
             * scroll never blocks on decoding a colour PICT), then repaint. */
            if (ui_idle(&gUi)) ui_draw(&gUi);
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
                    UiCommand cmd = ui_key(&gUi, c);
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
                            /* Open the chosen control panel via the Finder: give
                             * the menu bar back, send the odoc, and front the
                             * Finder so the cdev is visible. On resume (osEvt) we
                             * re-hide the bar. */
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
                        case UI_RESTART:  save_prefs(); sysctl_restart();  break;
                        case UI_SHUTDOWN:
                            save_prefs();
                            /* Shutdown chime — synchronous so it finishes before
                             * the machine powers off. No-op if none is baked. */
                            if (gUi.sndShutdown) sound_play_file("sounds/shutdown", 0);
                            sysctl_shutdown();
                            break;
                        case UI_PREFS_DIRTY: save_prefs(); break;
                        default: break;
                    }
                    break;
                }
                case updateEvt: {
                    WindowPtr w = (WindowPtr)evt.message;
                    BeginUpdate(w);
                    ui_draw(&gUi);
                    EndUpdate(w);
                    break;
                }
                case mouseDown:
                    SelectWindow(gWin);
                    break;
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
