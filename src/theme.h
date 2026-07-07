/*
 * theme.h — per-OS control "appearance": the era look of the launcher's OWN
 * hand-drawn chrome (docs/36 Phase 3). ONE universal binary picks the look at
 * runtime from the detected System (env), overridable by a prefs / Settings
 * choice — consistent with the single-binary goal (docs/01) and the runtime
 * backend/depth/tier detection already in env/render.
 *
 * This themes only what the launcher draws itself (list rows, key-cap hints,
 * panel frames, selection). The real Toolbox window frame, menu bar, and
 * Control-Manager push buttons are already drawn by the System per era, so they
 * look native for free.
 *
 * Pure C (no Toolbox) — host-testable, like json/model.
 */
#ifndef MACATRIUM_THEME_H
#define MACATRIUM_THEME_H

/* Resolved appearances — an index into the theme table. */
enum { APPEAR_SYS6 = 0, APPEAR_SYS7, APPEAR_SYS8, APPEAR_N };

/* Pref / Settings value: APPEAR_AUTO matches the running System; the concrete
 * APPEAR_SYS* values force a specific look regardless of the OS. */
enum { APPEAR_AUTO = -1 };

/* Era-specific chrome traits the render primitives consult. `sys7` reproduces
 * today's look exactly (the parity baseline), so routing existing drawing through
 * these traits is a no-op until an appearance other than sys7 is selected. New
 * traits are appended here as more of the chrome is themed. */
typedef struct {
    short capCorner;   /* rounded-frame / key-cap corner diameter (px); 0 = square */
    short frameBevel;  /* window/panel frame: 0 = flat 1px, 1 = Platinum 3-D bevel */
    short tileRaised;  /* Icon-Grid tile: 1 = raised "At Ease" button, 0 = flat (sys6) */
} Theme;

/* Resolve a pref value (APPEAR_AUTO or a forced APPEAR_SYS*) to a concrete
 * appearance for this machine. AUTO: sysVers < 7.0 -> sys6; >= 8.0 with the
 * Appearance Manager present -> sys8 (true Platinum); otherwise sys7. */
int          appearance_resolve(long sysVers, int hasAppearanceMgr, int pref);

/* The trait table for a resolved appearance (APPEAR_SYS6/7/8); never NULL — an
 * out-of-range value falls back to sys7. */
const Theme *theme_for(int appearance);

/* A short human label for Settings / About ("System 6" / "System 7" / "Platinum"). */
const char  *appearance_name(int appearance);

#endif /* MACATRIUM_THEME_H */
