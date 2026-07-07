/*
 * theme.c — see theme.h. The trait table + the auto-resolution rule.
 *
 * Data-driven for now: a small trait table the render primitives read. As the
 * per-era drawing grows (Platinum bevels via the Appearance Manager, sys6 lined
 * chrome), the more elaborate hooks can split into theme_sys{6,7,8}.c behind this
 * same interface (docs/36 Phase 3) without touching the call sites.
 */
#include "theme.h"

static const Theme kThemes[APPEAR_N] = {
    /* APPEAR_SYS6 */ { 0, 0, 0, 1 },   /* square, flat frames, flat tiles, inverted selection */
    /* APPEAR_SYS7 */ { 6, 0, 1, 0 },   /* == today: rounded key-caps, flat frames, raised, tint */
    /* APPEAR_SYS8 */ { 4, 1, 1, 0 },   /* Platinum: gentler corners, bevelled frames, raised, tint */
};

int appearance_resolve(long sysVers, int hasAppearanceMgr, int pref)
{
    if (pref == APPEAR_SYS6 || pref == APPEAR_SYS7 || pref == APPEAR_SYS8)
        return pref;                                   /* forced by prefs / Settings */

    /* AUTO — match the running System. */
    if (sysVers > 0 && sysVers < 0x0700)
        return APPEAR_SYS6;
    if (sysVers >= 0x0800 && hasAppearanceMgr)
        return APPEAR_SYS8;                            /* true Platinum only where real */
    return APPEAR_SYS7;
}

const Theme *theme_for(int appearance)
{
    if (appearance < 0 || appearance >= APPEAR_N)
        appearance = APPEAR_SYS7;
    return &kThemes[appearance];
}

const char *appearance_name(int appearance)
{
    switch (appearance) {
        case APPEAR_SYS6: return "System 6";
        case APPEAR_SYS8: return "Platinum";
        default:          return "System 7";
    }
}
