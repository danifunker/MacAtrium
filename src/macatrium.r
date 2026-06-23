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
 * Multiverse.r. We stay notHighLevelEventAware: we *send* `odoc` AppleEvents
 * (which needs no SIZE flag) but never receive them, so no AE handler is needed.
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
	notHighLevelEventAware,
	onlyLocalHLEvents,
	notStationeryAware,
	dontUseTextEditServices,
	reserved,
	reserved,
	reserved,
	4096 * 1024,
	1024 * 1024
};
