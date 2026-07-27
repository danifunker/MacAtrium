/*
 * toolbox.c — see toolbox.h. Pure protocol logic (entry parse, name match, CDB
 * build) plus, on-target, the SCSI Manager transport. Compiling with
 * -DTOOLBOX_HOST_TEST keeps only the pure half so it links into the host tests.
 */
#include "toolbox.h"

#include <string.h>

/* ---- pure logic (always compiled) ----------------------------------------- */

static char tb_lower(char c)
{
    return (c >= 'A' && c <= 'Z') ? (char)(c - 'A' + 'a') : c;
}

int toolbox_parse_cd_entry(const unsigned char *e, TbEntry *out)
{
    int i;

    out->index = e[0];
    out->isDir = (e[1] == 0x00) ? 1 : 0;        /* type: 0x01 = file, 0x00 = dir */

    for (i = 0; i < TB_NAME_MAX; i++) {
        unsigned char c = e[TB_NAME_OFF + i];
        if (c == 0) break;                       /* NUL-padded */
        out->name[i] = (char)c;
    }
    out->name[i] = '\0';

    /* 5-byte big-endian size at offset 35. Byte 35 (bits 32..39) is 0 for any CD
     * image (< 4 GB), so keep the low 32 bits; clamp if a >4 GB file ever appears. */
    if (e[TB_SIZE_OFF] != 0) {
        out->size = 0xFFFFFFFFUL;
    } else {
        out->size = ((unsigned long)e[36] << 24) | ((unsigned long)e[37] << 16) |
                    ((unsigned long)e[38] <<  8) |  (unsigned long)e[39];
    }
    return 1;
}

int toolbox_name_eq(const char *a, const char *b)
{
    while (*a && *b) {
        if (tb_lower(*a) != tb_lower(*b)) return 0;
        a++; b++;
    }
    return *a == '\0' && *b == '\0';
}

/* A host filename longer than 32 chars comes back from LIST CDS clipped to 32
 * (the MacRoman name field is TB_NAME_MAX wide). This matches such a clipped
 * `entry` against the full catalog `wanted` name: a case-insensitive prefix
 * compare, gated on the entry sitting at the 32-char clip boundary so short
 * names can't fuzzy-match by accident (docs/45). */
static int tb_name_is_trunc_prefix(const char *wanted, const char *entry)
{
    int len = 0;
    while (entry[len]) len++;
    if (len < TB_NAME_MAX) return 0;          /* not clipped: the exact pass handles it */
    while (*entry) {
        if (!*wanted) return 0;               /* wanted is shorter than the entry       */
        if (tb_lower(*wanted) != tb_lower(*entry)) return 0;
        wanted++; entry++;
    }
    return 1;                                 /* the 32-char entry is a prefix of wanted */
}

int toolbox_find_cd(const char *imageName, const TbEntry *entries, int n)
{
    int i;
    if (!imageName || !imageName[0]) return -1;

    /* Pass 1 — exact (case-insensitive). Unambiguous, always preferred. */
    for (i = 0; i < n; i++) {
        if (entries[i].isDir) continue;
        if (toolbox_name_eq(imageName, entries[i].name)) return entries[i].index;
    }
    /* Pass 2 — clipped-name fallback: a catalog name longer than 32 chars whose
     * on-disk name arrived truncated. Only 32-char (clipped) entries qualify, so
     * this never loosens matching for names that fit. */
    for (i = 0; i < n; i++) {
        if (entries[i].isDir) continue;
        if (tb_name_is_trunc_prefix(imageName, entries[i].name)) return entries[i].index;
    }
    return -1;
}

void toolbox_cdb_list_cds(unsigned char *cdb)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_LIST_CDS;
}

void toolbox_cdb_set_next_cd(unsigned char *cdb, int index)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_SET_NEXT_CD;
    cdb[1] = (unsigned char)index;
}

void toolbox_cdb_device_info(unsigned char *cdb, int subcmd)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_DEVICE_INFO;
    cdb[1] = (unsigned char)subcmd;
    /* CDB[8] = allocation length; 0 => 8 bytes (v0 backward-compat). */
}

/* ---- file-transfer CDBs (docs/46) ------------------------------------------ */

void toolbox_cdb_list_files(unsigned char *cdb)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_LIST_FILES;
}

void toolbox_cdb_count_files(unsigned char *cdb)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_COUNT_FILES;
}

void toolbox_cdb_get_file(unsigned char *cdb, int index, unsigned long blockOff, int blocks)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_GET_FILE;
    cdb[1] = (unsigned char)index;                        /* index in the listing    */
    cdb[2] = (unsigned char)((blockOff >> 24) & 0xFF);    /* 32-bit block offset, BE */
    cdb[3] = (unsigned char)((blockOff >> 16) & 0xFF);
    cdb[4] = (unsigned char)((blockOff >> 8) & 0xFF);
    cdb[5] = (unsigned char)(blockOff & 0xFF);
    cdb[6] = (unsigned char)blocks;                       /* 4 KB blocks; 0 reads as 1 */
}

void toolbox_cdb_send_file_prep(unsigned char *cdb)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_SEND_FILE_PREP;    /* the filename follows in the DataOut phase */
}

void toolbox_cdb_send_file_10(unsigned char *cdb, int blocks, unsigned long blockOff)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_SEND_FILE_10;
    cdb[3] = (unsigned char)((blockOff >> 16) & 0xFF);    /* 24-bit block offset, BE */
    cdb[4] = (unsigned char)((blockOff >> 8) & 0xFF);
    cdb[5] = (unsigned char)(blockOff & 0xFF);
    cdb[6] = (unsigned char)blocks;   /* 512-byte blocks; 0 selects the legacy form */
}

void toolbox_cdb_send_file_bytes(unsigned char *cdb, int nbytes, unsigned long blockOff)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_SEND_FILE_10;
    cdb[1] = (unsigned char)((nbytes >> 8) & 0xFF);   /* legacy u16 BE byte count */
    cdb[2] = (unsigned char)(nbytes & 0xFF);
    cdb[3] = (unsigned char)((blockOff >> 16) & 0xFF);
    cdb[4] = (unsigned char)((blockOff >> 8) & 0xFF);
    cdb[5] = (unsigned char)(blockOff & 0xFF);
    cdb[6] = 0;                    /* 0 selects the CDB[1..2] byte-count encoding */
}

void toolbox_cdb_send_file_end(unsigned char *cdb)
{
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_SEND_FILE_END;
}

void toolbox_cdb_mode_sense_p31(unsigned char *cdb)
{
    memset(cdb, 0, 6);
    cdb[0] = TB_MODE_SENSE_6;         /* MODE SENSE(6) */
    cdb[2] = TB_PAGE_TOOLBOX;         /* PC=0 (current values), page code 0x31 */
    cdb[4] = 64;                      /* allocation length — enough for the page */
}

/* 1 if the NUL-terminated `needle` occurs anywhere in buf[len]. */
static int tb_contains(const unsigned char *buf, int len, const char *needle, int mlen)
{
    int i;
    for (i = 0; i + mlen <= len; i++) {
        if (memcmp(buf + i, needle, (size_t)mlen) == 0) return 1;
    }
    return 0;
}

int toolbox_has_magic(const unsigned char *buf, int len)
{
    static const char blue[] = TB_MAGIC;
    static const char zulu[] = TB_MAGIC_ZULU;
    /* BlueSCSI and ZuluSCSI share the whole command set and differ only here, so
     * accept either signature — one client, both devices (docs/45). */
    return tb_contains(buf, len, blue, (int)sizeof(blue) - 1) ||
           tb_contains(buf, len, zulu, (int)sizeof(zulu) - 1);
}

#ifndef TOOLBOX_HOST_TEST
/* ---- Toolbox transport (target only; classic SCSI Manager) ------------------
 * One command per SCSIGet…SCSIComplete: arbitrate the bus, select the Toolbox
 * device, send the 10-byte vendor CDB, (optionally) read the DataIn phase with a
 * TIB, always SCSIComplete to release the bus. Short timeouts, no background
 * polling — a polite bus citizen (docs/45). */
#include "scsimgr.h"

/* SCSIComplete wait, in ticks (60/sec): a few seconds — long enough for a slow
 * host to answer, short enough that a wedged bus can't hang the launcher. */
#define TB_SCSI_TIMEOUT  300L

/* Read `nbytes` of the current command's DataIn phase into `dst`. Handshaked
 * SCSIRead returns non-noErr when the target has ended the phase (fewer bytes
 * available than requested), which is how a chunked list read finds its end. */
static OSErr tb_read(void *dst, long nbytes)
{
    SCSIInstr tib[2];
    tib[0].scOpcode = scInc;
    tib[0].scParam1 = (long)dst;
    tib[0].scParam2 = nbytes;
    tib[1].scOpcode = scStop;
    tib[1].scParam1 = 0;
    tib[1].scParam2 = 0;
    return SCSIRead((Ptr)tib);
}

/* DataOut counterpart of tb_read: hand `nbytes` from `src` to the target. Used by the
 * file send (SEND FILE PREP/DATA) and SET WORKING DIR (docs/46). */
static OSErr tb_write(const void *src, long nbytes)
{
    SCSIInstr tib[2];
    tib[0].scOpcode = scInc;
    tib[0].scParam1 = (long)src;
    tib[0].scParam2 = nbytes;
    tib[1].scOpcode = scStop;
    tib[1].scParam1 = 0;
    tib[1].scParam2 = 0;
    return SCSIWrite((Ptr)tib);
}

/* Select `id` and send a `cdbLen`-byte CDB (10 for the vendor ops, 6 for MODE
 * SENSE). Caller must reach SCSIComplete regardless (to release the bus). Returns
 * noErr once the CDB is accepted. */
static OSErr tb_begin(short id, const unsigned char *cdb, int cdbLen)
{
    OSErr err = SCSISelect(id);
    if (err == noErr) err = SCSICmd((Ptr)cdb, (short)cdbLen);
    return err;
}

int toolbox_set_next_cd(short id, int index)
{
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;

    toolbox_cdb_set_next_cd(cdb, index);
    if (SCSIGet() != noErr) return 0;                     /* bus busy → try later */
    err = tb_begin(id, cdb, TB_CDB_LEN);                  /* SET NEXT CD: no data phase */
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

/* One shared 4 KB staging buffer for LIST CDS / LIST FILES. Both drain a whole DataIn
 * phase in a single SCSIRead and are never in flight at the same time, so they share
 * this rather than each carrying its own 4 KB static — the B&W build runs a 384 KB
 * partition (docs/44). TB_MAX_CDS and TB_MAX_FILES are both 100. */
static unsigned char gTbList[TB_MAX_FILES * TB_ENTRY_SIZE];

int toolbox_list_cds(short id, TbEntry *buf, int cap, int *n)
{
    /* One SCSIRead must span the WHOLE DataIn phase. The original SCSI Manager
     * fills a single TIB per command; issuing one SCSIRead per 40-byte entry
     * leaves the follow-up reads untransferred, so their entries come back as
     * uninitialised garbage (the "boxes" in the CD Library). Instead drain the
     * entire listing into one static buffer (off the small 68k stack), then parse
     * it — the same single-read shape the MODE SENSE probe already uses. The
     * target sends N x 40 bytes and changes phase; SCSIRead transfers what's there
     * and stops. No COUNT command is issued (COUNT CDS is 0xDA, outside the MiSTer
     * RTL's 0xD0-0xD9 window); the count is however many populated entries precede
     * the first empty name. */
    unsigned char *raw = gTbList;            /* shared staging buffer (see gTbList) */
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;
    int count = 0, i;

    *n = 0;
    memset(raw, 0, sizeof gTbList);
    toolbox_cdb_list_cds(cdb);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(raw, (long)sizeof gTbList);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    if (err != noErr) return 0;

    for (i = 0; i < cap && i < TB_MAX_CDS; i++) {
        const unsigned char *e = &raw[i * TB_ENTRY_SIZE];
        if (e[TB_NAME_OFF] == 0) break;          /* empty name -> end of the listing */
        toolbox_parse_cd_entry(e, &buf[count++]);
    }
    /* CHECK CONDITION with no data → the host has no Toolbox CD support (feature
     * silently unavailable). GOOD with zero entries → supported, folder empty. */
    if (count == 0 && (stat & 0xFF) != 0) return 0;
    *n = count;
    return 1;
}

/* Confirm the device at `id` is a CD-ROM via a standard INQUIRY (peripheral
 * device type 5). A BlueSCSI hard disk also carries the page-0x31 magic (it serves
 * the file Toolbox), so page 0x31 alone isn't enough to aim the CD opcodes — without
 * this the probe can select the HDD and LIST/SET CDS fail ("Unknown command D7h"). */
static int tb_is_cdrom(short id)
{
    unsigned char cdb[6];
    unsigned char resp[36];
    OSErr err;
    short stat = -1, msg = 0;

    memset(cdb, 0, sizeof cdb);
    cdb[0] = TB_INQUIRY_6;                    /* INQUIRY */
    cdb[4] = (unsigned char)sizeof resp;      /* allocation length */
    memset(resp, 0, sizeof resp);

    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, 6);
    if (err == noErr) (void)tb_read(resp, (long)sizeof resp);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);

    return (err == noErr) && ((resp[0] & 0x1F) == TB_PDT_CDROM);
}

int toolbox_probe_id(int pin, short *outId)
{
    /* Session cache (RAM): "probe on first use each boot" (docs/45). */
    static short cached = -1;
    static int   done   = 0;
    unsigned char cdb[6];
    unsigned char resp[64];
    /* Try the conventional Toolbox id (6, the primary disk on BlueSCSI/MiSTer)
     * first, then 0..5. snow answers on whichever attached id we hit. */
    static const short order[7] = { 6, 0, 1, 2, 3, 4, 5 };
    int i;

    if (pin >= 0 && pin <= 6) {              /* explicit id override (cdId pref) */
        *outId = (short)pin;
        return 1;
    }
    if (done) {
        if (cached < 0) return 0;
        *outId = cached;
        return 1;
    }
    done = 1;

    /* Canonical BlueSCSI detection (Toolbox Developer Docs): MODE SENSE(6) vendor
     * page 0x31 returns a magic string. It's a standard command, so a non-Toolbox
     * device simply rejects the unknown page — safe to send to every id while
     * probing (unlike firing a vendor opcode at unknown devices). */
    toolbox_cdb_mode_sense_p31(cdb);
    for (i = 0; i < 7; i++) {
        short id = order[i];
        OSErr err;
        short stat = -1, msg = 0;
        if (SCSIGet() != noErr) continue;           /* bus busy this pass */
        memset(resp, 0, sizeof resp);
        err = tb_begin(id, cdb, 6);                  /* MODE SENSE(6): 6-byte CDB */
        if (err == noErr) (void)tb_read(resp, (long)sizeof resp);  /* page data */
        (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
        if (err == noErr && toolbox_has_magic(resp, (int)sizeof resp) && tb_is_cdrom(id)) {
            cached = id;
            *outId = id;
            return 1;
        }
    }
    cached = -1;
    return 0;
}

/* ---- file transfer (docs/46) ----------------------------------------------- */

int toolbox_file_caps(short id, unsigned char *caps)
{
    unsigned char cdb[TB_CDB_LEN];
    unsigned char resp[8];
    OSErr err;
    short stat = -1, msg = 0;

    if (caps) *caps = 0;
    memset(resp, 0, sizeof resp);
    toolbox_cdb_device_info(cdb, TB_SUB_GET_CAPS);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(resp, (long)sizeof resp);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    if (err != noErr || (stat & 0xFF) != 0) return 0;
    if (caps) *caps = resp[1];   /* byte 0 = API version, byte 1 = the TB_CAP_* bits */
    return 1;
}

int toolbox_count_files(short id, int *n)
{
    unsigned char cdb[TB_CDB_LEN];
    unsigned char resp[8];
    OSErr err;
    short stat = -1, msg = 0;

    if (n) *n = 0;
    memset(resp, 0, sizeof resp);
    toolbox_cdb_count_files(cdb);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(resp, (long)sizeof resp);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    /* CHECK CONDITION here is exactly how a target that serves the CD ops but NOT the
     * file ops announces itself (the open MiSTer question) — the caller hides the
     * browser instead of failing mid-transfer. */
    if (err != noErr || (stat & 0xFF) != 0) return 0;
    if (n) *n = resp[0];
    return 1;
}

int toolbox_list_files(short id, TbEntry *buf, int cap, int *n)
{
    /* Same single-SCSIRead-per-DataIn rule as toolbox_list_cds — one TIB must span the
     * whole phase or the tail entries come back as uninitialised garbage. */
    unsigned char *raw = gTbList;            /* shared staging buffer (see gTbList) */
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;
    int count = 0, i;

    *n = 0;
    memset(raw, 0, sizeof gTbList);
    toolbox_cdb_list_files(cdb);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(raw, (long)sizeof gTbList);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    if (err != noErr) return 0;

    /* Same 40-byte wire layout as LIST CDS, so the CD entry parser is reused verbatim;
     * for files `isDir` is meaningful (type 0x00 = directory). */
    for (i = 0; i < cap && i < TB_MAX_FILES; i++) {
        const unsigned char *e = &raw[i * TB_ENTRY_SIZE];
        if (e[TB_NAME_OFF] == 0) break;          /* empty name -> end of the listing */
        toolbox_parse_cd_entry(e, &buf[count++]);
    }
    if (count == 0 && (stat & 0xFF) != 0) return 0;
    *n = count;
    return 1;
}

int toolbox_get_file_block(short id, int index, unsigned long blockOff, void *dst, long cap)
{
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;
    long  want = (cap < TB_GET_BLOCK) ? cap : TB_GET_BLOCK;

    /* One 4 KB block per command: the baseline every target implements (the
     * CAP_LARGE_TRANSFERS flag only matters for asking for several at once). Offset 0
     * is what makes the firmware (re)open the file; later offsets seek within it. */
    toolbox_cdb_get_file(cdb, index, blockOff, 1);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    /* A short DataIn at EOF makes the handshaked SCSIRead report an error even though
     * the bytes did land, and it cannot say HOW many arrived — so the caller sizes the
     * final block from the listing's file size instead of from a returned count. */
    if (err == noErr) (void)tb_read(dst, want);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

int toolbox_send_file_prep(short id, const char *name)
{
    unsigned char cdb[TB_CDB_LEN];
    unsigned char nm[TB_SEND_NAME_LEN + 1];
    OSErr err;
    short stat = -1, msg = 0;
    int   i;

    /* Exactly 33 bytes: up to 32 name characters plus the NUL the firmware scans for.
     * PREP also truncates any existing file, which is what makes a retry clean. */
    memset(nm, 0, sizeof nm);
    for (i = 0; i < TB_SEND_NAME_LEN && name && name[i]; i++) nm[i] = (unsigned char)name[i];
    toolbox_cdb_send_file_prep(cdb);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_write(nm, (long)sizeof nm);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

int toolbox_send_file_data(short id, const void *src, long nbytes, unsigned long blockOff)
{
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;

    if (nbytes <= 0) return 1;
    /* Whole 512-byte blocks use the block encoding; a short tail uses the legacy byte
     * count, because padding a RAW copy out to a block boundary would append zeros to
     * the file the user asked us to send (docs/46). */
    if ((nbytes % TB_SEND_BLOCK) == 0 && (nbytes / TB_SEND_BLOCK) <= 255)
        toolbox_cdb_send_file_10(cdb, (int)(nbytes / TB_SEND_BLOCK), blockOff);
    else
        toolbox_cdb_send_file_bytes(cdb, (int)nbytes, blockOff);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_write(src, nbytes);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

int toolbox_send_file_end(short id)
{
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;

    toolbox_cdb_send_file_end(cdb);          /* no data phase — straight to status */
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

int toolbox_probe_file_id(short *outId)
{
    /* Session cache, same shape as toolbox_probe_id. Deliberately does NOT require a
     * CD-ROM peripheral type: the file Toolbox is served by the emulated HARD DISK
     * (the handoff spec implements it there, and snow answers page 0x31 from
     * disk.rs), so take the first device carrying the magic and try id 0 first. */
    static short cached = -1;
    static int   done   = 0;
    unsigned char cdb[6];
    unsigned char resp[64];
    static const short order[7] = { 0, 1, 2, 3, 4, 5, 6 };
    int i;

    if (done) {
        if (cached < 0) return 0;
        *outId = cached;
        return 1;
    }
    done = 1;
    toolbox_cdb_mode_sense_p31(cdb);
    for (i = 0; i < 7; i++) {
        short id = order[i];
        OSErr err;
        short stat = -1, msg = 0;
        if (SCSIGet() != noErr) continue;
        memset(resp, 0, sizeof resp);
        err = tb_begin(id, cdb, 6);
        if (err == noErr) (void)tb_read(resp, (long)sizeof resp);
        (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
        if (err == noErr && toolbox_has_magic(resp, (int)sizeof resp)) {
            cached = id;
            *outId = id;
            return 1;
        }
    }
    cached = -1;
    return 0;
}

int toolbox_set_working_dir(short id, const char *path)
{
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;
    int len = 0;

    while (path && path[len] && len < TB_MAX_PATH) len++;
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_DEVICE_INFO;
    cdb[1] = TB_SUB_SET_WORKING_DIR;
    cdb[8] = (unsigned char)len;      /* 0 => reset to the default shared folder */
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr && len > 0) (void)tb_write(path, (long)len);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    return (err == noErr && (stat & 0xFF) == 0) ? 1 : 0;
}

int toolbox_get_working_dir(short id, char *out, int cap)
{
    unsigned char cdb[TB_CDB_LEN];
    unsigned char resp[TB_MAX_PATH + 1];
    OSErr err;
    short stat = -1, msg = 0;
    int i;

    if (out && cap > 0) out[0] = '\0';
    memset(resp, 0, sizeof resp);
    memset(cdb, 0, TB_CDB_LEN);
    cdb[0] = TB_OP_DEVICE_INFO;
    cdb[1] = TB_SUB_GET_WORKING_DIR;
    cdb[8] = TB_MAX_PATH;                       /* allocation length */
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(resp, (long)TB_MAX_PATH);
    (void)SCSIComplete(&stat, &msg, TB_SCSI_TIMEOUT);
    if (err != noErr || (stat & 0xFF) != 0) return 0;
    if (!out || cap <= 0) return 1;
    for (i = 0; i < cap - 1 && i < TB_MAX_PATH && resp[i]; i++) out[i] = (char)resp[i];
    out[i] = '\0';
    return 1;
}
#endif /* TOOLBOX_HOST_TEST */
