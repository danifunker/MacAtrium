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
       UI_MODE_INFO, UI_MODE_CTLPANELS };

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
} Ui;

void      ui_init(Ui *u, Env *env, Render *r, Model *m, WindowPtr win, int safe);
void      ui_draw(Ui *u);
UiCommand ui_key(Ui *u, char ch);
UiCommand ui_click(Ui *u, Point pt);   /* mouse: hit-test a window-local click */

/* Idle work: lazily load the selected item's detail art (deferred so scrolling
 * stays cheap). Returns 1 if it loaded something and the caller should redraw.
 * Call from the event loop when WaitNextEvent reports no event. */
int       ui_idle(Ui *u);
void      ui_set_status(Ui *u, const char *msg);

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
