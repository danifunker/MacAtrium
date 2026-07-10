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
#include "macfs.h"
#include "art.h"
#include "artcaps.h"
#include "controlpanels.h"

typedef enum {
    UI_NONE = 0,
    UI_LAUNCH,       /* launch the current item            */
    UI_SHOW_FINDER,  /* bring the resident Finder to front */
    UI_RESTART,
    UI_SHUTDOWN,
    UI_OPEN_CDEV,    /* open the selected control panel via the Finder */
    UI_QUIT,         /* quit the launcher entirely, back to the Finder        */
    UI_PREFS_DIRTY,  /* a persisted setting (theme/volume) changed; main saves */
    UI_CHROME_DIRTY, /* menu-bar / title-bar visibility changed; main re-lays
                      * out the window + menu bar (rebuild_window) AND saves    */
    UI_OPEN_SETTINGS,/* open the real Settings window (main.c run_settings_dialog)  */
    UI_OPEN_MENU,    /* open the real Quick-Launch menu window (run_quicklaunch_menu) */
    UI_OPEN_CHOOSER, /* open the System Folder Chooser (main.c run_os_chooser)        */
    UI_SHOW_STATUS   /* open the MacAtrium Status screen (main.c run_status_dialog)   */
} UiCommand;

enum { UI_MODE_LIST = 0, UI_MODE_MENU /* unused: now a real window */, UI_MODE_PREVIEW,
       UI_MODE_SETTINGS /* unused: now a real window */,
       UI_MODE_INFO, UI_MODE_CTLPANELS, UI_MODE_SETUP, UI_MODE_ABOUT,
       UI_MODE_QUITCONFIRM };

/* Browse view modes — the first-run chooser (UI_MODE_SETUP) and the View menu
 * pick one; `Ui::view` holds the current. All views drive the same Model. */
enum { VIEW_CAROUSEL = 0, VIEW_ICON, VIEW_LIST, VIEW_N };

#define UI_MAX_DEPTHS 6

typedef struct {
    Env       *env;
    Render    *r;
    Model     *m;
    VolTable  *vols;          /* multi-disk (docs/37): mounted library volumes  */
    WindowPtr  win;
    int        mode;
    int        menuSel;
    int        menuRows[8];   /* visible Esc-menu rows (MROW_*), built per-environment */
    int        nmenu;         /* count in menuRows[] */
    int        safe;          /* 1 = "no catalog" recoverable screen */
    char       status[96];    /* transient line (e.g. launch error)  */
    Art       *previewPic;    /* loaded art while in UI_MODE_PREVIEW  */
    Art       *listArt;       /* selected item's art for the inline pane */
    const CatItem *artFor;    /* item listArt was loaded for (NULL = none) */
    const ArtCaps *caps;      /* docs/44: art tiers this machine can hold (P2 budget cap) */
    int        settingsFocus; /* 1 = gear focused on the list screen (Left)   */
    int        setSel;        /* selected row in the Settings panel           */
    short      depths[UI_MAX_DEPTHS]; /* screen depths the device supports     */
    int        ndepths;       /* count in depths[]                            */
    int        vol;           /* speaker volume 0..SOUND_VOL_MAX (-1 = n/a)   */
    int        artPref;       /* 0 = Box Art, 1 = Screenshot (the `shot` field) */
    int        view;          /* current browse view (VIEW_CAROUSEL/ICON/LIST)  */
    int        gridStyle;     /* Icon Grid layout: 0 = Finder (2-line names), 1 = At Ease tiles */
    int        listFocus;     /* List view: 0 = categories pane, 1 = items pane  */
    int        sortMode;      /* List view sort: SORT_NONE/NAME/TYPE/YEAR (model.h) */
    int        sortDesc;      /* 1 = descending                                     */
    char       filter[24];    /* List view name filter; "" = off (type to filter)   */
    int        listColType;   /* List view: px from the right (minus the scroll bar) of
                              * the Name|Type divider — draggable; Year stays at 40    */
    int        lastDrawnItem; /* selection at the last FULL browse-view draw, so an */
    int        lastDrawnTop;  /* in-page selection move can repaint just the changed */
    int        lastDrawnCat;  /* cells (else a scroll/category change -> full draw)   */
    int        lastDrawnFocus;/* List pane focus at the last full draw (focus change -> full) */
    ControlHandle scrollV;    /* real Control-Manager vertical scroll bar (grid + list) */
    ControlHandle launch;     /* real "Launch" push button                              */
    ControlHandle quitBtn;    /* quit-confirm dialog: "Quit" (default) push button      */
    ControlHandle cancelBtn;  /* quit-confirm dialog: "Cancel" push button              */
    ControlHandle settingsBtn;/* header-left: real push button (gear) -> Quick-Launch menu */
    int        controlsReady; /* 1 once the controls have been created                  */
    int        setupSel;      /* selected row on the first-run UI_MODE_SETUP screen */
    int        setupDrawn;    /* 1 = chooser fully drawn, so a Up/Down repaints just the
                              * two affected rows (not the whole welcome screen)        */
    int        lastSetupSel;  /* chooser row last drawn selected (-1 = none)            */
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
    int        hideMenuBar;   /* 1 = hide the System menu bar (Settings; main applies) */
    int        hideTitleBar;  /* 1 = hide the window's WM title bar (plainDBox)        */
    int        chromeDirty;   /* set by settings_adjust when a *bar toggle changed, so
                              * ui_key/ui_click return UI_CHROME_DIRTY (main re-lays out) */
    int        overlayDrawn;  /* 1 = the menu/settings panel is fully drawn, so a
                              * selection move repaints only the changed rows     */
    int        lastSel;       /* overlay row last drawn selected (-1 = none)      */
    Rect       panelRect;     /* the overlay panel's window rect, so ui_draw can blit
                              * just that region (not the whole window) when only the
                              * overlay changed — and erase it on an overlay switch  */
} Ui;

void      ui_init(Ui *u, Env *env, Render *r, Model *m, WindowPtr win, int safe);
void      ui_draw(Ui *u);
void      ui_draw_art(Ui *u);   /* targeted: repaint only the detail cover area */
UiCommand ui_key(Ui *u, char ch);
UiCommand ui_key_cmd(Ui *u, char ch);  /* Cmd-modified shortcut (theme/info/art/hotkey) */
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
/* Scroll-bar interaction (called by main.c after TrackControl): step the selection
 * by an arrow/page part, or jump to the thumb's value. */
void      ui_scroll_step(Ui *u, short part);
void      ui_scroll_to(Ui *u, short val);

/* Open the "Are you sure you want to quit?" confirmation (the close box / File >
 * Quit / Cmd-Opt-Q route through this instead of quitting directly). */
void      ui_confirm_quit(Ui *u);

/* Set the content text size in points (9 / 10 / 12 = Small / Medium / Large); the
 * row height scales with it and the whole layout reflows. main applies the saved
 * preference at startup; the Settings "Text Size" row changes it live. */
void      ui_set_text_size(Ui *u, int pts);

void      ui_show_about(Ui *u);              /* Apple > About MacAtrium     */
void      ui_show_info(Ui *u);               /* File  > Get Info            */
void      ui_set_view(Ui *u, int view);      /* View  > Carousel/Icon/List  */

/* ---- Settings model (the real Settings window in main.c renders + drives these,
 * so the actual setting logic stays here as the single source of truth) ---------
 * Each row is one of: a checkbox (binary), a stepper (multi-value, < / > steps),
 * or an action (Control Panels). The dialog walks rows 0..ui_setting_count()-1. */
enum { SETTING_CHECK = 0, SETTING_STEPPER, SETTING_ACTION };
int         ui_setting_count(void);
int         ui_setting_kind(int row);                 /* SETTING_*                 */
const char *ui_setting_label(int row);                /* fixed row label           */
int         ui_setting_checked(Ui *u, int row);       /* checkbox state (CHECK)    */
void        ui_setting_value(Ui *u, int row, char *out); /* value text (STEPPER); out >= 24 */
int         ui_setting_step(Ui *u, int row, int dir);    /* apply; 1 if it changed */

/* ---- Quick-Launch menu model (the real menu window in main.c renders these) ----
 * The rows are actions (Settings / Show Finder / Exit / Restart / Shut Down), built
 * per-environment in ui_init. ui_menu_command maps a row to the command main runs. */
int         ui_menu_count(Ui *u);
const char *ui_menu_label(Ui *u, int i);
UiCommand   ui_menu_command(Ui *u, int i);

/* Re-blit the browse screen from the off-screen buffer WITHOUT re-rendering it —
 * for closing a real modal window (menu / Settings) that left the buffer intact.
 * Falls back to a full ui_draw on the direct-draw path or when the buffer is stale. */
void        ui_reblit(Ui *u);

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
