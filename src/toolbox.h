/*
 * toolbox.h — BlueSCSI Toolbox client: enumerate host CD images and switch the
 * mounted disc, so a CD title can "insert" its disc before launch (docs/45).
 *
 * The wire protocol is the BlueSCSI Toolbox v0 vendor command set, spoken to the
 * Toolbox device over the classic SCSI Manager. The SAME client works against the
 * snow emulator (dev loop), a real BlueSCSI, and the MiSTer MacLC core.
 *
 * Verified opcodes (snow toolbox.rs + MiSTer BLUESCSI_HANDOFF.md + BlueSCSI-v2):
 *   0xD7 LIST CDS      DataIn = N x 40-byte entries (same layout as LIST FILES)
 *   0xD8 SET NEXT CD   CDB[1] = index from the last LIST CDS; no data phase
 *   0xD9 DEVICE INFO   CDB[1] = subcommand (0x00 list devices / 0x01 capabilities)
 * (COUNT CDS is 0xDA — outside the MiSTer RTL's 0xD0-0xD9 window — so we never
 * issue it; the count comes from LIST CDS's returned length.)
 *
 * Like artcaps.c, the pure logic (entry parse, name match, CDB build) is split from
 * the Toolbox transport so it is unit-tested off-target with host gcc: compiling
 * with -DTOOLBOX_HOST_TEST drops the SCSI-Manager half (see tests/host_test.c).
 */
#ifndef MACATRIUM_TOOLBOX_H
#define MACATRIUM_TOOLBOX_H

/* Vendor opcodes (10-byte CDBs). */
#define TB_OP_LIST_FILES   0xD0
#define TB_OP_COUNT_FILES  0xD2
#define TB_OP_LIST_CDS     0xD7
#define TB_OP_SET_NEXT_CD  0xD8
#define TB_OP_DEVICE_INFO  0xD9   /* aka METADATA / LIST DEVICES */

/* 0xD9 subcommands (CDB[1]). */
#define TB_SUB_LIST_DEVICES  0x00
#define TB_SUB_GET_CAPS      0x01

/* Toolbox device detection: MODE SENSE(6) vendor page 0x31 returns a magic string
 * (BlueSCSI Toolbox Developer Docs). This is the canonical, safe way to find the
 * device — a standard command any device tolerates, not a vendor opcode. */
#define TB_MODE_SENSE_6   0x1A   /* MODE SENSE(6) opcode (6-byte CDB)              */
#define TB_PAGE_TOOLBOX   0x31   /* vendor page carrying the magic string          */
#define TB_MAGIC          "BlueSCSI is the BEST"   /* detection prefix (docs)       */

/* Standard INQUIRY (6-byte CDB): the peripheral device type is the low 5 bits of
 * response byte 0. A BlueSCSI hard disk also answers page 0x31 (it serves the file
 * Toolbox too), so we confirm a page-0x31 match is a CD-ROM before aiming the CD
 * opcodes at it — otherwise LIST/SET land on the HDD ("Unknown command D7h"). */
#define TB_INQUIRY_6      0x12   /* INQUIRY opcode (6-byte CDB)                    */
#define TB_PDT_CDROM      0x05   /* INQUIRY peripheral device type: CD-ROM / MMC   */

/* LIST entry wire layout (40 bytes; verified firmware-exact). */
#define TB_ENTRY_SIZE   40
#define TB_CDB_LEN      10
#define TB_NAME_OFF     2
#define TB_NAME_MAX     32     /* Mac filename length (bytes 2..34)               */
#define TB_SIZE_OFF     35     /* 5-byte big-endian size; byte 35 is bits 32..39  */

/* A parsed LIST CDS / LIST FILES entry. `name` is the raw MacRoman filename,
 * NUL-terminated (<= 32 chars). `size` holds the low 32 bits of the firmware's
 * 5-byte size (CD images never reach 4 GB, so byte 35 is always 0). */
typedef struct {
    int           index;      /* 0-based enumeration index (entry byte 0)         */
    int           isDir;      /* 1 = directory (type 0x00), 0 = file (type 0x01)  */
    char          name[TB_NAME_MAX + 1];
    unsigned long size;
} TbEntry;

/* How many CD images we enumerate at once. BlueSCSI counts fit a single byte and
 * the OSD convention tops out near 100 discs, so 100 is a documented, generous cap. */
#define TB_MAX_CDS  100

/* ---- pure logic (always compiled; host-tested) ----------------------------- */

/* Parse one 40-byte LIST entry at `e` into `out`. Always succeeds (the wire format
 * is fixed-width); returns 1 for convenience. */
int  toolbox_parse_cd_entry(const unsigned char *e, TbEntry *out);

/* Case-insensitive (ASCII A-Z fold; high MacRoman bytes compared raw) exact
 * filename compare. Returns 1 if equal. */
int  toolbox_name_eq(const char *a, const char *b);

/* Find the CD image named `imageName` among `n` parsed entries (files only,
 * case-insensitive). Returns the matching entry's enumeration index (its byte-0
 * field, what SET NEXT CD expects), or -1 if not found. */
int  toolbox_find_cd(const char *imageName, const TbEntry *entries, int n);

/* Fill a 10-byte CDB. `cdb` must have room for TB_CDB_LEN bytes. */
void toolbox_cdb_list_cds(unsigned char *cdb);
void toolbox_cdb_set_next_cd(unsigned char *cdb, int index);
void toolbox_cdb_device_info(unsigned char *cdb, int subcmd);

/* Fill a 6-byte MODE SENSE(6) CDB requesting vendor page 0x31 (Toolbox detection).
 * `cdb` must have room for 6 bytes. */
void toolbox_cdb_mode_sense_p31(unsigned char *cdb);

/* 1 if the BlueSCSI page-0x31 magic prefix (TB_MAGIC) appears anywhere in buf[len]. */
int  toolbox_has_magic(const unsigned char *buf, int len);

#ifndef TOOLBOX_HOST_TEST
/* ---- Toolbox transport (target only; SCSI Manager) ------------------------- */

/* Locate the Toolbox device by probing DEVICE INFO across SCSI IDs and cache the
 * result for the session (docs/45: "probe on first use"). `pin` >= 0 forces that
 * ID (the optional cdId pref) and skips probing. Returns 1 and writes *outId on
 * success; 0 if no Toolbox device answers (feature silently unavailable). */
int  toolbox_probe_id(int pin, short *outId);

/* LIST CDS on `id` into `buf[cap]`; sets *n to the count. Returns 1 on GOOD,
 * 0 on CHECK CONDITION / bus timeout (host has no Toolbox CD support). */
int  toolbox_list_cds(short id, TbEntry *buf, int cap, int *n);

/* SET NEXT CD `index` on `id`. Returns 1 on GOOD, 0 otherwise. The host remounts
 * its CD drive with the chosen image; the guest re-reads the TOC and mounts it. */
int  toolbox_set_next_cd(short id, int index);
#endif /* TOOLBOX_HOST_TEST */

#endif /* MACATRIUM_TOOLBOX_H */
