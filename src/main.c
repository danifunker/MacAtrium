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

static WindowPtr make_window(const Env *e)
{
    Rect b = e->screen;
    b.top += e->mbarHeight;          /* sit below the menu bar (recoverable) */
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
     * forward and redraw with selection intact. */
    bring_self_front();
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
    env_probe(&gEnv);

    gWin = make_window(&gEnv);
    render_init(&gRender, &gEnv);

    loaded = load_catalog();
    model_build(&gModel, &gCat);     /* empty catalog -> just "All" with 0 items */

    ui_init(&gUi, &gEnv, &gRender, &gModel, gWin, loaded ? 0 : 1);

    bring_self_front();
    SetPort(gWin);
    ui_draw(&gUi);

    for (;;) {
        if (WaitNextEvent(everyEvent, &evt, 10L, 0L)) {
            switch (evt.what) {
                case keyDown:
                case autoKey: {
                    char c = (char)(evt.message & charCodeMask);
                    UiCommand cmd = ui_key(&gUi, c);
                    switch (cmd) {
                        case UI_LAUNCH:   do_launch(); break;
                        case UI_FINDER:
                            if (!sysctl_launch_finder()) {
                                ui_set_status(&gUi, "Finder not resident - use Restart.");
                                ui_draw(&gUi);
                            }
                            break;
                        case UI_RESTART:  sysctl_restart();  break;
                        case UI_SHUTDOWN: sysctl_shutdown(); break;
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
