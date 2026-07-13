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

int toolbox_find_cd(const char *imageName, const TbEntry *entries, int n)
{
    int i;
    if (!imageName || !imageName[0]) return -1;
    for (i = 0; i < n; i++) {
        if (entries[i].isDir) continue;
        if (toolbox_name_eq(imageName, entries[i].name)) return entries[i].index;
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

void toolbox_cdb_mode_sense_p31(unsigned char *cdb)
{
    memset(cdb, 0, 6);
    cdb[0] = TB_MODE_SENSE_6;         /* MODE SENSE(6) */
    cdb[2] = TB_PAGE_TOOLBOX;         /* PC=0 (current values), page code 0x31 */
    cdb[4] = 64;                      /* allocation length — enough for the page */
}

int toolbox_has_magic(const unsigned char *buf, int len)
{
    static const char magic[] = TB_MAGIC;
    int mlen = (int)sizeof(magic) - 1;   /* drop the terminating NUL */
    int i;
    for (i = 0; i + mlen <= len; i++) {
        if (memcmp(buf + i, magic, (size_t)mlen) == 0) return 1;
    }
    return 0;
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
    static unsigned char raw[TB_MAX_CDS * TB_ENTRY_SIZE];
    unsigned char cdb[TB_CDB_LEN];
    OSErr err;
    short stat = -1, msg = 0;
    int count = 0, i;

    *n = 0;
    memset(raw, 0, sizeof raw);
    toolbox_cdb_list_cds(cdb);
    if (SCSIGet() != noErr) return 0;
    err = tb_begin(id, cdb, TB_CDB_LEN);
    if (err == noErr) (void)tb_read(raw, (long)sizeof raw);
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
#endif /* TOOLBOX_HOST_TEST */
