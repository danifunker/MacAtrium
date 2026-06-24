/*
 * macatrium.r — override the default Retro68 'SIZE' (-1) resource.
 *
 * Two reasons:
 *   1. A 4 MB preferred partition (the default template ships 1 MB). The shell
 *      now caches a small icon per list row plus depth-matched art, and bigger
 *      catalogs need headroom; 1 MB is tight.
 *   2. acceptSuspendResumeEvents (+ doesActivateOnFGSwitch / MultiFinder-aware):
 *      so we are told when we are suspended/resumed by the app switcher. That
 *      lets the shell re-hide the menu bar when it's brought back to the front
 *      after "Show Finder" (main.c handles the osEvt resume).
 *
 * Field order mirrors the template (Retro68APPL.r) and the 'SIZE' type in
 * Multiverse.r. isHighLevelEventAware opens our AppleEvent/PPC port so we can
 * *send* the `odoc` to the Finder (without it, AESend fails -903 noPortErr);
 * main.c installs the four required AE handlers and processes incoming events.
 */
#include "Processes.r"

resource 'SIZE' (-1) {
	reserved,
	acceptSuspendResumeEvents,
	reserved,
	cannotBackground,
	doesActivateOnFGSwitch,
	backgroundAndForeground,
	dontGetFrontClicks,
	ignoreChildDiedEvents,
	is32BitCompatible,
	isHighLevelEventAware,
	onlyLocalHLEvents,
	notStationeryAware,
	dontUseTextEditServices,
	reserved,
	reserved,
	reserved,
	2048 * 1024,    /* preferred: real peak is ~1 MB now (8-bit-capped off-screen
	                   buffer + catalog), so 2 MB is ample headroom (was 4 MB) */
	1024 * 1024     /* minimum */
};
