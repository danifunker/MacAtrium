/*
 * filebrowse.c — see filebrowse.h. The SD-card browser's model: where we are, what is
 * in it, and whether this target can do any of it at all (docs/46).
 */
#include "filebrowse.h"
#ifndef TOOLBOX_HOST_TEST
#include "macbin.h"
#include "macfs.h"
#endif

#include <string.h>

/* ---- pure path arithmetic --------------------------------------------------- */

int fb_path_join(char *dir, int cap, const char *name)
{
    int dl, nl, sep;

    if (!dir || !name || !name[0] || cap <= 0) return 0;
    /* A listing entry is a single component; anything else would let the guest walk
     * out of the shared folder, which the handoff spec explicitly warns about. */
    if (strchr(name, '/') || strchr(name, '\\')) return 0;
    if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) return 0;

    dl  = (int)strlen(dir);
    nl  = (int)strlen(name);
    sep = (dl > 0 && dir[dl - 1] != '/') ? 1 : 0;   /* "/" root needs no extra slash */
    if (dl + sep + nl + 1 > cap) return 0;          /* too deep for TB_MAX_PATH */

    if (sep) dir[dl++] = '/';
    memcpy(dir + dl, name, (size_t)nl);
    dir[dl + nl] = '\0';
    return 1;
}

int fb_path_parent(char *dir, const char *base)
{
    int dl, bl;

    if (!dir) return 0;
    dl = (int)strlen(dir);
    bl = base ? (int)strlen(base) : 0;
    if (dl <= bl) return 0;                    /* already at the base — never above it */

    while (dl > 0 && dir[dl - 1] != '/') dl--; /* drop the trailing component */
    if (dl > 0) dl--;                          /* and the separator before it */
    if (dl < bl) dl = bl;                      /* clamp: the base is the floor */
    dir[dl] = '\0';
    return 1;
}

#ifndef TOOLBOX_HOST_TEST
/* ---- session state ----------------------------------------------------------
 * One resident listing (~4.6 KB) rather than a stack copy: the 68k stack is small and
 * the browser is only ever open once (same reasoning as cdswap's cached CD listing). */
static TbEntry      gEnts[TB_MAX_FILES];
static int          gN;                   /* entries currently listed               */
static int          gTrunc;               /* listing hit the firmware's cap         */
static char         gBase[TB_MAX_PATH];   /* the target's default shared folder     */
static char         gDir[TB_MAX_PATH];    /* the directory on screen                */
static short        gId;                  /* Toolbox device serving the file ops    */
static unsigned char gCaps;
static int          gProbed, gOk;

/* Probe once per session. Three steps, because each answers a different question:
 * which device serves the file Toolbox, what it can do, and whether it really
 * implements the file ops at all (the open MiSTer question). */
static void fb_probe(void)
{
    int n = 0;

    if (gProbed) return;
    gProbed = 1;
    gOk = 0;
    if (!toolbox_probe_file_id(&gId)) return;
    /* v0 targets may not answer 0xD9 at all; that is not fatal, it just means no
     * advertised capabilities (and therefore no directory navigation). */
    (void)toolbox_file_caps(gId, &gCaps);
    if (!toolbox_count_files(gId, &n)) return;   /* the real "are the file ops here" test */
    gOk = 1;

    if (gCaps & TB_CAP_WORKDIR) {
        if (toolbox_get_working_dir(gId, gBase, (int)sizeof gBase)) {
            strncpy(gDir, gBase, sizeof gDir - 1);
            gDir[sizeof gDir - 1] = '\0';
        }
    }
}

int fb_available(void) { fb_probe(); return gOk; }

int fb_can_navigate(void)
{
    fb_probe();
    return gOk && (gCaps & TB_CAP_WORKDIR) && gBase[0] ? 1 : 0;
}

short fb_device_id(void) { fb_probe(); return gId; }

const TbEntry *fb_entries(int *n) { if (n) *n = gN; return gEnts; }

int fb_truncated(void) { return gTrunc; }

const char *fb_dir(void) { return gDir; }

int fb_refresh(void)
{
    fb_probe();
    gN     = 0;
    gTrunc = 0;
    if (!gOk) return 0;
    if (!toolbox_list_files(gId, gEnts, TB_MAX_FILES, &gN)) return 0;
    gTrunc = (gN >= TB_MAX_FILES);
    return 1;
}

/* Move to `next`, but only keep it if the target accepted the move — otherwise the
 * on-screen path would claim a directory we are not actually listing. */
static int fb_goto(const char *next)
{
    if (!toolbox_set_working_dir(gId, next)) return 0;
    strncpy(gDir, next, sizeof gDir - 1);
    gDir[sizeof gDir - 1] = '\0';
    return fb_refresh();
}

int fb_enter(const char *name)
{
    char next[TB_MAX_PATH];

    if (!fb_can_navigate()) return 0;
    strncpy(next, gDir, sizeof next - 1);
    next[sizeof next - 1] = '\0';
    if (!fb_path_join(next, (int)sizeof next, name)) return 0;
    return fb_goto(next);
}

int fb_parent(void)
{
    char next[TB_MAX_PATH];

    if (!fb_can_navigate()) return 0;
    strncpy(next, gDir, sizeof next - 1);
    next[sizeof next - 1] = '\0';
    if (!fb_path_parent(next, gBase)) return 0;
    return fb_goto(next);
}

/* ---- copying (docs/46) ------------------------------------------------------ */

/* One 4 KB transfer block, resident rather than on the stack — the 68k stack is small
 * and a B&W build may only have a 384 KB partition (docs/44). */
static unsigned char gBlk[TB_GET_BLOCK];

/* Build the destination spec for `name` under /MacAtrium/Incoming, creating the
 * folder on demand. HFS allows 31-character names and the protocol allows 32, so the
 * name is clamped rather than left to fail in the File Manager. */
static FbResult fb_dest_spec(const char *name, FSSpec *spec)
{
    char  rel[TB_NAME_MAX + 16];
    int   i, n;
    OSErr err;

    if (macfs_mkdir(FB_INCOMING) != noErr) return FB_ERR_WRITE;
    strcpy(rel, FB_INCOMING "/");
    n = (int)strlen(rel);
    for (i = 0; name[i] && i < 31; i++) rel[n + i] = name[i];
    rel[n + i] = '\0';

    err = macfs_make_spec(rel, spec);
    /* fnfErr just means the leaf does not exist yet — which is the normal case. */
    return (err == noErr || err == fnfErr) ? FB_OK : FB_ERR_WRITE;
}

/* Write whatever part of the block at stream position `pos` falls inside [rs,re) to
 * `ref`. Each fork's spans arrive in increasing order, so a plain sequential FSWrite
 * lands them correctly without seeking. */
static int fb_span(short ref, const unsigned char *blk, long pos, long got, long rs, long re)
{
    long s = (pos > rs) ? pos : rs;
    long e = (pos + got < re) ? (pos + got) : re;
    long count;

    if (s >= e) return 1;                       /* this block misses the region */
    count = e - s;
    return FSWrite(ref, &count, (Ptr)(blk + (s - pos))) == noErr;
}

long fb_rsrc_len(const FSSpec *src)
{
    short refNum;
    long  eof = 0;

    if (macfs_open_rf(src, fsRdPerm, &refNum) != noErr) return 0;
    if (GetEOF(refNum, &eof) != noErr) eof = 0;
    FSClose(refNum);
    return eof;
}

/* Which way this target reads the SEND FILE offset field. The BLUESCSI_HANDOFF spec
 * (and snow, and MiSTer) seeks ABSOLUTELY to offset*512; the BlueSCSI-v2 firmware
 * does a RELATIVE seekCur instead. Sending the wrong one corrupts everything past the
 * first chunk, and nothing advertises which it is — so we send, check the resulting
 * size, and if it is wrong flip the mode and resend. Learned once per session. */
static int gSeekAbs = -1;          /* -1 unknown, 1 absolute, 0 relative */

/* Size the target reports for `name` in the current folder, or -1 if absent. */
static long fb_remote_size(const char *name)
{
    int i, n = 0;

    if (!toolbox_list_files(gId, gEnts, TB_MAX_FILES, &n)) return -1;
    gN = n;
    for (i = 0; i < n; i++) {
        if (!gEnts[i].isDir && toolbox_name_eq(gEnts[i].name, name))
            return (long)gEnts[i].size;
    }
    return -1;
}

/* Emit the whole logical stream (MacBinary header + padded forks, or just the data
 * fork) to `destName`. `absOff` selects the offset convention under test. */
static FbResult fb_send_stream(const FSSpec *src, const char *destName, int wrap,
                               long dataLen, long rsrcLen, long total,
                               int absOff, const FbUI *ui)
{
    unsigned char hdr[MACBIN_HDR];
    short         dfRef = 0, rfRef = 0;
    long          pos = 0, dataStart, dataEnd, rsrcStart, rsrcEnd;
    FbResult      rc = FB_OK;
    FInfo         fi;

    dataStart = wrap ? MACBIN_HDR : 0;
    dataEnd   = dataStart + dataLen;
    rsrcStart = wrap ? (MACBIN_HDR + macbin_padded(dataLen)) : 0;
    rsrcEnd   = rsrcStart + rsrcLen;

    if (wrap) {
        char name[MACBIN_NAME_MAX + 1];
        int  i, nl = src->name[0];
        if (nl > MACBIN_NAME_MAX) nl = MACBIN_NAME_MAX;
        for (i = 0; i < nl; i++) name[i] = (char)src->name[1 + i];
        name[nl] = '\0';
        if (macfs_get_finfo(src, &fi) != noErr) return FB_ERR_READ;
        macbin_build_header(hdr, name, (unsigned long)dataLen, (unsigned long)rsrcLen,
                            (unsigned long)fi.fdType, (unsigned long)fi.fdCreator, 0);
    }

    if (macfs_open_df(src, fsRdPerm, &dfRef) != noErr) return FB_ERR_READ;
    if (wrap && rsrcLen > 0 && macfs_open_rf(src, fsRdPerm, &rfRef) != noErr) {
        FSClose(dfRef);
        return FB_ERR_READ;
    }
    if (!toolbox_send_file_prep(gId, destName)) {   /* also truncates a previous try */
        FSClose(dfRef);
        if (rfRef) FSClose(rfRef);
        return FB_ERR_WRITE;
    }

    while (pos < total && rc == FB_OK) {
        long n = total - pos;
        long off = 0;

        if (n > (long)sizeof gBlk) n = (long)sizeof gBlk;
        /* Keep every chunk 512-aligned so the offset field stays meaningful; only the
         * final short tail is unaligned, and that one rides the legacy byte count. */
        if (n >= TB_SEND_BLOCK) n -= (n % TB_SEND_BLOCK);

        memset(gBlk, 0, (size_t)n);              /* pad regions are zeros by construction */
        for (off = 0; off < n; ) {
            long p = pos + off, chunk;
            if (wrap && p < MACBIN_HDR) {                       /* the header */
                chunk = MACBIN_HDR - p;
                if (chunk > n - off) chunk = n - off;
                memcpy(gBlk + off, hdr + p, (size_t)chunk);
            } else if (p >= dataStart && p < dataEnd) {         /* data fork */
                chunk = dataEnd - p;
                if (chunk > n - off) chunk = n - off;
                if (FSRead(dfRef, &chunk, (Ptr)(gBlk + off)) != noErr) { rc = FB_ERR_READ; break; }
            } else if (rfRef && p >= rsrcStart && p < rsrcEnd) { /* resource fork */
                chunk = rsrcEnd - p;
                if (chunk > n - off) chunk = n - off;
                if (FSRead(rfRef, &chunk, (Ptr)(gBlk + off)) != noErr) { rc = FB_ERR_READ; break; }
            } else {                                            /* padding */
                chunk = n - off;
            }
            if (chunk <= 0) { rc = FB_ERR_READ; break; }
            off += chunk;
        }
        if (rc != FB_OK) break;

        if (!toolbox_send_file_data(gId, gBlk, n, absOff ? (unsigned long)(pos / TB_SEND_BLOCK) : 0UL)) {
            rc = FB_ERR_WRITE;
            break;
        }
        pos += n;
        if (ui && ui->tick && !ui->tick(ui->ctx, pos, total)) rc = FB_ERR_CANCEL;
    }

    FSClose(dfRef);
    if (rfRef) FSClose(rfRef);
    if (!toolbox_send_file_end(gId) && rc == FB_OK) rc = FB_ERR_WRITE;
    return rc;
}

FbResult fb_copy_out(const FSSpec *src, const char *destName, int wrap, const FbUI *ui)
{
    short dfRef;
    long  dataLen = 0, rsrcLen = 0, total;
    FbResult rc;

    if (!fb_available()) return FB_ERR_UNSUPPORTED;
    if (!src || !destName || !destName[0]) return FB_ERR_RANGE;

    if (macfs_open_df(src, fsRdPerm, &dfRef) != noErr) return FB_ERR_READ;
    if (GetEOF(dfRef, &dataLen) != noErr) dataLen = 0;
    FSClose(dfRef);
    if (wrap) rsrcLen = fb_rsrc_len(src);

    total = wrap ? (MACBIN_HDR + macbin_padded(dataLen) + macbin_padded(rsrcLen)) : dataLen;
    if (ui && ui->message) ui->message(ui->ctx, "Sending to the SD card...");

    rc = fb_send_stream(src, destName, wrap, dataLen, rsrcLen, total,
                        (gSeekAbs != 0), ui);

    /* First send of the session: confirm the offset convention actually took, and
     * transparently redo it the other way if the target disagreed. A multi-chunk file
     * is the only case that can differ, so a single-chunk send teaches us nothing. */
    if (rc == FB_OK && gSeekAbs < 0) {
        if (total <= TB_SEND_BLOCK) {
            /* too small to tell them apart — leave the mode unknown */
        } else if (fb_remote_size(destName) == total) {
            gSeekAbs = 1;                               /* spec / snow / MiSTer */
        } else {
            gSeekAbs = 0;                               /* BlueSCSI firmware    */
            if (ui && ui->message) ui->message(ui->ctx, "Adjusting for this device...");
            rc = fb_send_stream(src, destName, wrap, dataLen, rsrcLen, total, 0, ui);
        }
    }
    return rc;
}

FbResult fb_copy_in(int index, const FbUI *ui)
{
    const TbEntry *ents;
    int            n = 0, wrapped = 0;
    FSSpec         spec;
    MacBinInfo     mi;
    long           total, pos = 0;
    long           dataStart = 0, dataEnd = 0, rsrcStart = 0, rsrcEnd = 0;
    unsigned long  blockOff = 0;
    short          dfRef = 0, rfRef = 0;
    FbResult       rc = FB_OK;

    if (!fb_available()) return FB_ERR_UNSUPPORTED;
    ents = fb_entries(&n);
    if (index < 0 || index >= n || ents[index].isDir) return FB_ERR_RANGE;
    total = (long)ents[index].size;

    if (ui && ui->message) ui->message(ui->ctx, "Reading from the SD card...");

    /* Block 0 decides the shape of everything that follows: a MacBinary wrapper is
     * split back into two forks, anything else is copied through verbatim. */
    memset(gBlk, 0, sizeof gBlk);
    if (!toolbox_get_file_block(gId, index, 0, gBlk, (long)sizeof gBlk)) return FB_ERR_READ;
    wrapped = macbin_parse(gBlk, (total < (long)sizeof gBlk) ? total : (long)sizeof gBlk, &mi);
    if (wrapped) {
        dataStart = MACBIN_HDR;
        dataEnd   = dataStart + (long)mi.dataLen;
        rsrcStart = MACBIN_HDR + macbin_padded((long)mi.dataLen);
        rsrcEnd   = rsrcStart + (long)mi.rsrcLen;
    }

    rc = fb_dest_spec(wrapped ? mi.name : ents[index].name, &spec);
    if (rc != FB_OK) return rc;
    (void)HDelete(spec.vRefNum, spec.parID, spec.name);    /* replace any previous copy */
    if (macfs_create(&spec,
                     wrapped ? (OSType)mi.creator : (OSType)'????',
                     wrapped ? (OSType)mi.type    : (OSType)'BINA') != noErr)
        return FB_ERR_WRITE;

    if (macfs_open_df(&spec, fsWrPerm, &dfRef) != noErr) return FB_ERR_WRITE;
    if (wrapped && mi.rsrcLen > 0 &&
        macfs_open_rf(&spec, fsWrPerm, &rfRef) != noErr) { FSClose(dfRef); return FB_ERR_WRITE; }

    for (;;) {
        long remain = total - pos;
        long got;

        if (remain <= 0) break;
        got = (remain < (long)sizeof gBlk) ? remain : (long)sizeof gBlk;

        if (blockOff != 0) {           /* block 0 is already sitting in gBlk */
            memset(gBlk, 0, sizeof gBlk);
            if (!toolbox_get_file_block(gId, index, blockOff, gBlk, (long)sizeof gBlk)) {
                rc = FB_ERR_READ;
                break;
            }
        }

        if (!wrapped) {
            if (!fb_span(dfRef, gBlk, pos, got, 0, total)) { rc = FB_ERR_WRITE; break; }
        } else {
            if (!fb_span(dfRef, gBlk, pos, got, dataStart, dataEnd)) { rc = FB_ERR_WRITE; break; }
            if (rfRef && !fb_span(rfRef, gBlk, pos, got, rsrcStart, rsrcEnd)) {
                rc = FB_ERR_WRITE;
                break;
            }
        }

        pos += got;
        blockOff++;
        if (ui && ui->tick && !ui->tick(ui->ctx, pos, total)) { rc = FB_ERR_CANCEL; break; }
    }

    FSClose(dfRef);
    if (rfRef) FSClose(rfRef);

    if (rc == FB_OK && wrapped) {
        /* Without the Finder info a perfectly copied application is just an
         * unopenable document, so this failing fails the copy. */
        FInfo fi;
        if (macfs_get_finfo(&spec, &fi) == noErr) {
            fi.fdType    = (OSType)mi.type;
            fi.fdCreator = (OSType)mi.creator;
            if (macfs_set_finfo(&spec, &fi) != noErr) rc = FB_ERR_WRITE;
        } else {
            rc = FB_ERR_WRITE;
        }
    }
    if (rc != FB_OK) (void)HDelete(spec.vRefNum, spec.parID, spec.name);  /* no half-files */
    /* Commit the volume either way: on success so the file actually survives a reset,
     * on failure so the delete above is not left buffered either. */
    (void)macfs_flush_vol(spec.vRefNum);
    return rc;
}
#endif /* TOOLBOX_HOST_TEST */
