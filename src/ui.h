/*
 * ui.h — list/menu UI: layout computed from the window rect, keyboard nav, and
 * the Esc menu (docs/07). Side-effecting actions (launch / power / Finder) are
 * returned to main as commands so the UI stays drawing+state only.
 */
#ifndef MACATRIUM_UI_H
#define MACATRIUM_UI_H

#include <Windows.h>
#include "env.h"
#include "render.h"
#include "model.h"
#include "art.h"
#include "controlpanels.h"

typedef enum {
    UI_NONE = 0,
    UI_LAUNCH,       /* launch the current item            */
    UI_SHOW_FINDER,  /* bring the resident Finder to front */
    UI_RESTART,
    UI_SHUTDOWN,
    UI_OPEN_CDEV,    /* open the selected control panel via the Finder */
    UI_QUIT,         /* quit the launcher entirely, back to the Finder        */
    UI_PREFS_DIRTY   /* a persisted setting (theme/volume) changed; main saves */
} UiCommand;

enum { UI_MODE_LIST = 0, UI_MODE_MENU, UI_MODE_PREVIEW, UI_MODE_SETTINGS,
       UI_MODE_INFO, UI_MODE_CTLPANELS, UI_MODE_SETUP, UI_MODE_ABOUT };

/* Browse view modes — the first-run chooser (UI_MODE_SETUP) and the View menu
 * pick one; `Ui::view` holds the current. All views drive the same Model. */
enum { VIEW_CAROUSEL = 0, VIEW_ICON, VIEW_LIST, VIEW_N };

#define UI_MAX_DEPTHS 6

typedef struct {
    Env       *env;
    Render    *r;
    Model     *m;
    WindowPtr  win;
    int        mode;
    int        menuSel;
    int        menuRows[5];   /* visible Esc-menu rows (MROW_*), built per-environment */
    int        nmenu;         /* count in menuRows[] */
    int        safe;          /* 1 = "no catalog" recoverable screen */
    char       status[96];    /* transient line (e.g. launch error)  */
    Art       *previewPic;    /* loaded art while in UI_MODE_PREVIEW  */
    Art       *listArt;       /* selected item's art for the inline pane */
    const CatItem *artFor;    /* item listArt was loaded for (NULL = none) */
    int        settingsFocus; /* 1 = gear focused on the list screen (Left)   */
    int        setSel;        /* selected row in the Settings panel           */
    short      depths[UI_MAX_DEPTHS]; /* screen depths the device supports     */
    int        ndepths;       /* count in depths[]                            */
    int        vol;           /* speaker volume 0..SOUND_VOL_MAX (-1 = n/a)   */
    int        artPref;       /* 0 = Box Art, 1 = Screenshot (the `shot` field) */
    int        view;          /* current browse view (VIEW_CAROUSEL/ICON/LIST)  */
    int        listFocus;     /* List view: 0 = categories pane, 1 = items pane  */
    int        setupSel;      /* selected row on the first-run UI_MODE_SETUP screen */
    int        carousel;      /* carousel icons shown: odd 3..25, capped by fit */
    int        sndStartup;    /* 1 = play the startup sound on launch          */
    int        sndShutdown;   /* 1 = play the shutdown sound on Shut Down      */
    Art       *rowIcon[MAX_ITEMS]; /* lazily-loaded list-row icons, by catalog idx */
    CtlPanel   cdevs[CTLPANEL_MAX]; /* control panels (UI_MODE_CTLPANELS)         */
    int        ncdevs;        /* count enumerated                              */
    int        cdevSel;       /* selected control panel                        */
    int        cdevTop;       /* first visible row (scroll)                    */
    int        bgValid;       /* 1 = the GWorld already holds the carousel, so a
                              * menu/settings overlay can redraw without repainting
                              * the whole screen behind it (fast modal nav)      */
    int        lastMode;      /* mode at the previous draw; a change forces one
                              * full repaint so a switched/closed overlay clears  */
    int        catList;       /* 1 = show the categories list panel on the browse
                              * screen (toggled in Settings)                      */
    int        overlayDrawn;  /* 1 = the menu/settings panel is fully drawn, so a
                              * selection move repaints only the changed rows     */
    int        lastSel;       /* overlay row last drawn selected (-1 = none)      */
} Ui;

void      ui_init(Ui *u, Env *env, Render *r, Model *m, WindowPtr win, int safe);
void      ui_draw(Ui *u);
void      ui_draw_art(Ui *u);   /* targeted: repaint only the detail cover area */
UiCommand ui_key(Ui *u, char ch);
UiCommand ui_click(Ui *u, Point pt);   /* mouse: hit-test a window-local click */

/* ui_idle() return: what (if anything) the idle tick needs repainted —
 * NONE, just the cover box (ui_draw_art), or the whole screen (ui_draw). */
enum { UI_IDLE_NONE = 0, UI_IDLE_ART = 1, UI_IDLE_FULL = 2 };

/* Idle work: lazily load the selected item's detail art (deferred so scrolling
 * stays cheap), or pick up an external screen-depth change. Returns a UI_IDLE_*
 * code telling the caller what to repaint. Call from the event loop when
 * WaitNextEvent reports no event. */
int       ui_idle(Ui *u);
void      ui_set_status(Ui *u, const char *msg);

/* Menu-bar entry points (main.c owns the real System menu bar and calls these so
 * the UI layer stays draw + state only). Each sets the relevant mode/state and
 * repaints; the View switch + Settings changes are persisted by main (save_prefs). */
void      ui_show_about(Ui *u);              /* Apple > About MacAtrium     */
void      ui_show_settings(Ui *u);           /* View  > Settings…           */
void      ui_show_info(Ui *u);               /* File  > Get Info            */
void      ui_set_view(Ui *u, int view);      /* View  > Carousel/Icon/List  */

/* Drop the per-item art caches (detail cover + tile icons) after a category
 * page loads — the paged catalog reuses its items array, so the caches would
 * otherwise show the previous page's art. Call right after model_set_page. */
void      ui_page_changed(Ui *u);

/* The current item's app path (for main to launch); NULL if none. */
const char *ui_current_app(Ui *u);
const char *ui_current_name(Ui *u);
int         ui_current_maxdepth(Ui *u);

/* The selected control panel (for main to open via the Finder); NULL if none. */
const CtlPanel *ui_current_cdev(Ui *u);

#endif /* MACATRIUM_UI_H */
