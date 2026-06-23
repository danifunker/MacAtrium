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
#include "mac_compat.h"

#include <string.h>

#ifndef plainDBox
#define plainDBox 2
#endif

/* Large structures live in BSS, not on the stack. */
static Catalog   gCat;
static Model     gModel;
static Env       gEnv;
static Render    gRender;
static Ui        gUi;
static WindowPtr gWin;
static Prefs     gPrefs;

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
    ProcessSerialNumber psn;
    if (GetCurrentProcess(&psn) == noErr)
        SetFrontProcess(&psn);
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

    if (!app) return;

    lr = launch_app(app, gEnv.canLaunchReturn, &lerr);

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
    prefs_load(&gPrefs);              /* saved theme / volume / selection */
    hide_menu_bar();                  /* true full-screen (restored for Launch
                                         Finder; gEnv keeps the original height) */

    gWin = make_window(&gEnv);
    CalcVis((WindowPeek)gWin);         /* claim the reclaimed top strip */
    render_init(&gRender, &gEnv);
    if (gPrefs.haveTheme) render_set_theme(&gRender, gPrefs.theme);
    if (gPrefs.haveVol && sound_available()) sound_apply_vol(gPrefs.vol);  /* no boot beep */

    loaded = load_catalog();
    model_build(&gModel, &gCat);     /* empty catalog -> just "All" with 0 items */
    if (gPrefs.haveSel) model_select(&gModel, gPrefs.category, gPrefs.item);

    ui_init(&gUi, &gEnv, &gRender, &gModel, gWin, loaded ? 0 : 1);
    if (gPrefs.haveArtPref) gUi.artPref = gPrefs.artPref;   /* restore Artwork choice */
    if (gPrefs.haveSndStartup)  gUi.sndStartup  = gPrefs.sndStartup;   /* restore sound prefs */
    if (gPrefs.haveSndShutdown) gUi.sndShutdown = gPrefs.sndShutdown;

    bring_self_front();
    SetPort(gWin);
    ui_draw(&gUi);

    /* Startup chime (async so it overlaps the UI coming up); off by default,
     * a no-op if no sound was baked into the image. */
    if (gUi.sndStartup) sound_play_file("sounds/startup", 1);

    for (;;) {
        if (WaitNextEvent(everyEvent, &evt, 10L, 0L)) {
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
            }
        }
    }
    return 0;
}
