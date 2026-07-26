/*
 * filebrowse.h — the SD-card side of the BlueSCSI Toolbox: browse the shared folder
 * the host exposes, so files can move between the Mac and the SD card without
 * rebuilding the disk image (docs/46).
 *
 * This is the MODEL half — session probe, listing, and path arithmetic — with no
 * window or event code, the same way cdswap.c stays free of UI. The dialog lives in
 * main.c beside the other browsers, which own the event pump.
 *
 * The path helpers are pure and host-tested; everything else needs the Toolbox
 * transport and so is target-only.
 */
#ifndef MACATRIUM_FILEBROWSE_H
#define MACATRIUM_FILEBROWSE_H

#include "toolbox.h"   /* TbEntry, TB_MAX_FILES, TB_MAX_PATH */

/* ---- pure path arithmetic (host-tested) ------------------------------------- */

/* Append "/name" to `dir` (`cap` bytes incl. NUL). Returns 1 on success, 0 if the
 * result would not fit — the firmware's own limit is TB_MAX_PATH, so a deep tree is
 * simply unreachable rather than silently truncated into the wrong directory.
 * Rejects `name` containing '/', "." or ".." so the guest cannot climb out of the
 * shared folder (the hardening the handoff spec asks for). */
int fb_path_join(char *dir, int cap, const char *name);

/* Trim the last component of `dir`, never going above `base`. Returns 1 if it moved. */
int fb_path_parent(char *dir, const char *base);

#ifndef TOOLBOX_HOST_TEST
#include "macfs.h"     /* FSSpec, for the copy-out source */

/* ---- session state (target only) -------------------------------------------- */

/* 1 if this target implements the Toolbox file ops. Probed once per session: locate
 * the file Toolbox device, read GET CAPABILITIES, then confirm with a trial COUNT
 * FILES — a target that serves the CD ops but not the file ops answers 0 here and the
 * menu hides the browser rather than failing mid-transfer. */
int  fb_available(void);

/* 1 if directory navigation is possible (TB_CAP_WORKDIR). Neither snow nor the MiSTer
 * handoff spec implements the working-dir subcommands, so this is false there and the
 * browser simply shows the shared folder flat. */
int  fb_can_navigate(void);

/* The Toolbox device serving the file ops — the copy paths address it directly. */
short fb_device_id(void);

/* The current listing; `*n` receives the entry count. */
const TbEntry *fb_entries(int *n);

/* 1 when the listing hit TB_MAX_FILES, i.e. the directory may hold more than we can
 * see. Surfaced in the UI rather than pretending the listing is complete. */
int  fb_truncated(void);

/* The directory being shown ("" when the target cannot report one). */
const char *fb_dir(void);

/* (Re)read the current directory. Returns 1 on success. */
int  fb_refresh(void);

/* Descend into `name` / climb back out. Both return 0 unless fb_can_navigate(). */
int  fb_enter(const char *name);
int  fb_parent(void);

/* ---- copying (docs/46) ------------------------------------------------------
 * Transfers run over the handshaked SCSI Manager, so a large file is slow: progress
 * and cancellation are required, not decorative. Same hook shape as CdSwapUI, which
 * keeps window/event code out of this module. */
typedef struct {
    void (*message)(void *ctx, const char *msg);          /* transient status line   */
    int  (*tick)(void *ctx, long done, long total);        /* 0 = user cancelled      */
    void  *ctx;
} FbUI;

typedef enum {
    FB_OK = 0,
    FB_ERR_UNSUPPORTED,   /* this target has no Toolbox file ops     */
    FB_ERR_RANGE,         /* nothing usable selected                 */
    FB_ERR_READ,          /* the SD-card side failed                 */
    FB_ERR_WRITE,         /* the Mac side failed                     */
    FB_ERR_CANCEL         /* the user stopped it                     */
} FbResult;

/* Folder (under /MacAtrium) that received files land in. */
#define FB_INCOMING "Incoming"

/* Copy listing entry `index` from the SD card into /MacAtrium/Incoming. A MacBinary
 * wrapper is unwrapped into both forks plus type/creator; anything else is written
 * through as a plain data fork so it stays usable as-is. */
FbResult fb_copy_in(int index, const FbUI *ui);

/* Resource-fork length of `src`, 0 if it has none. This is what decides whether a
 * copy OUT needs the MacBinary wrapper — and therefore whether to ask the user at
 * all: a plain data file should reach the SD card directly usable, not wrapped. */
long fb_rsrc_len(const FSSpec *src);

/* Send a Mac file to the SD card as `destName` (<= 32 chars). `wrap` selects
 * MacBinary (both forks + type/creator) over a raw data-fork copy. The first
 * multi-chunk send of a session also settles which SEND FILE offset convention this
 * target uses, redoing the transfer once if the first guess was wrong. */
FbResult fb_copy_out(const FSSpec *src, const char *destName, int wrap, const FbUI *ui);
#endif /* TOOLBOX_HOST_TEST */

#endif /* MACATRIUM_FILEBROWSE_H */
