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

typedef enum {
    UI_NONE = 0,
    UI_LAUNCH,     /* launch the current item   */
    UI_FINDER,     /* Launch Finder             */
    UI_RESTART,
    UI_SHUTDOWN
} UiCommand;

enum { UI_MODE_LIST = 0, UI_MODE_MENU, UI_MODE_PREVIEW };

typedef struct {
    Env       *env;
    Render    *r;
    Model     *m;
    WindowPtr  win;
    int        mode;
    int        menuSel;
    int        safe;          /* 1 = "no catalog" recoverable screen */
    char       status[96];    /* transient line (e.g. launch error)  */
    PicHandle  previewPic;    /* loaded art while in UI_MODE_PREVIEW  */
} Ui;

void      ui_init(Ui *u, Env *env, Render *r, Model *m, WindowPtr win, int safe);
void      ui_draw(Ui *u);
UiCommand ui_key(Ui *u, char ch);
void      ui_set_status(Ui *u, const char *msg);

/* The current item's app path (for main to launch); NULL if none. */
const char *ui_current_app(Ui *u);
const char *ui_current_name(Ui *u);

#endif /* MACATRIUM_UI_H */
