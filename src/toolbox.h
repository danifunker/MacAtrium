/*
 * toolbox.h — BlueSCSI Toolbox client: enumerate host CD images and switch the
 * mounted disc, so a CD title can "insert" its disc before launch (docs/45).
 *
 * The wire protocol is the BlueSCSI Toolbox v0 vendor command set, spoken to the
 * Toolbox device over the classic SCSI Manager. The SAME client works against the
 * snow emulator (dev loop), a real BlueSCSI, a real ZuluSCSI (identical protocol,
 * different page-0x31 signature — see TB_MAGIC_ZULU), and the MiSTer MacLC core.
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
#define TB_OP_LIST_FILES     0xD0
#define TB_OP_GET_FILE       0xD1   /* read a listed file, whole 4 KB blocks (docs/46) */
#define TB_OP_COUNT_FILES    0xD2
#define TB_OP_SEND_FILE_PREP 0xD3   /* create/truncate; filename via DataOut           */
#define TB_OP_SEND_FILE_10   0xD4   /* write, CDB[6] x 512-byte blocks                 */
#define TB_OP_SEND_FILE_END  0xD5   /* sync + close                                    */
#define TB_OP_LIST_CDS     0xD7
#define TB_OP_SET_NEXT_CD  0xD8
#define TB_OP_DEVICE_INFO  0xD9   /* aka METADATA / LIST DEVICES */

/* 0xD9 subcommands (CDB[1]). */
#define TB_SUB_LIST_DEVICES    0x00
#define TB_SUB_GET_CAPS        0x01
#define TB_SUB_SET_WORKING_DIR 0x02   /* path via DataOut; only if TB_CAP_WORKDIR     */
#define TB_SUB_GET_WORKING_DIR 0x03

/* GET CAPABILITIES (0xD9 / 0x01) reply: byte 0 = API version, byte 1 = these bits.
 * NOTE: snow implements the file ops but NOT the working-dir subcommands, and
 * correctly leaves TB_CAP_WORKDIR clear — so directory navigation must be gated on
 * this bit and degrade to a flat listing of the shared dir (docs/46). */
#define TB_CAP_LARGE_XFER  0x01   /* DataIn transfers larger than 512 bytes           */
#define TB_CAP_LARGE_SEND  0x02   /* 32 KB send chunks                                */
#define TB_CAP_WORKDIR     0x04   /* SET / GET WORKING DIR supported                  */

/* Toolbox device detection: MODE SENSE(6) vendor page 0x31 returns a magic string
 * (BlueSCSI Toolbox Developer Docs). This is the canonical, safe way to find the
 * device — a standard command any device tolerates, not a vendor opcode.
 *
 * BlueSCSI and ZuluSCSI are the same Toolbox protocol (both Rabbit Hole Computing,
 * one SCSI2SD firmware base) — every opcode below is identical. They differ ONLY in
 * this page-0x31 signature, so `toolbox_has_magic` accepts either and one client
 * drives both. The full ZuluSCSI payload is "ZuluSCSI is GPLv3 FTW
 * RabbitHoleComputing"; we match the brand, which is stable across firmware builds
 * where the tagline suffix is not (ZuluSCSI-firmware lib/SCSI2SD/.../mode.c). */
#define TB_MODE_SENSE_6   0x1A   /* MODE SENSE(6) opcode (6-byte CDB)              */
#define TB_PAGE_TOOLBOX   0x31   /* vendor page carrying the magic string          */
#define TB_MAGIC          "BlueSCSI is the BEST"   /* BlueSCSI detection prefix (docs) */
#define TB_MAGIC_ZULU     "ZuluSCSI"               /* ZuluSCSI brand (page-0x31)    */

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

/* File-transfer geometry, from the firmware (BlueSCSI_Toolbox.h + .cpp, docs/46).
 * A listing is CAPPED at TB_MAX_FILES entries and names are truncated to TB_NAME_MAX
 * — both are firmware behaviour, so the browser has to surface them rather than
 * assume it saw the whole directory. */
#define TB_MAX_FILES     100      /* MAX_FILE_LISTING_FILES — entries per listing      */
#define TB_MAX_PATH      64       /* MAX_FILE_PATH — full path incl. NUL               */
#define TB_GET_BLOCK     4096L    /* GET FILE moves whole 4 KB blocks                  */
#define TB_SEND_BLOCK    512      /* SEND FILE 10 moves whole 512-byte blocks          */
#define TB_SEND_NAME_LEN 32       /* SEND FILE PREP filename bytes (33 sent, with NUL) */

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

/* ---- file-transfer CDBs (docs/46) ---- */
void toolbox_cdb_list_files(unsigned char *cdb);
void toolbox_cdb_count_files(unsigned char *cdb);
/* GET FILE: `index` from the listing, `blockOff` counted in TB_GET_BLOCK units, and
 * `blocks` = how many 4 KB blocks this transfer moves (the firmware reads 0 as 1). */
void toolbox_cdb_get_file(unsigned char *cdb, int index, unsigned long blockOff, int blocks);
void toolbox_cdb_send_file_prep(unsigned char *cdb);
/* SEND FILE 10: `blocks` = 512-byte blocks in this transfer; `blockOff` fills the
 * CDB[3..5] offset field.
 *
 * CAUTION — the implementations disagree, and copy-out has to resolve this before it
 * can write a multi-chunk file (docs/46):
 *   - the MiSTer BLUESCSI_HANDOFF spec (SS4.5) and snow SEEK ABSOLUTELY to
 *     offset x 512, so a sequential write sends the CUMULATIVE block offset;
 *   - the BlueSCSI-v2 firmware instead does `gFile.seekCur(offset * 512)`, a
 *     RELATIVE seek, for which a sequential write must send 0 on every chunk.
 * Either value corrupts the other target once the write position leaves 0, so the mode
 * is settled at run time rather than assumed here. This builder only pins the wire
 * encoding. */
void toolbox_cdb_send_file_10(unsigned char *cdb, int blocks, unsigned long blockOff);
/* Legacy encoding for a short tail: CDB[1..2] = byte count, CDB[6] = 0. Padding a raw
 * copy up to a 512-byte block instead would append zeros to the user's file. */
void toolbox_cdb_send_file_bytes(unsigned char *cdb, int nbytes, unsigned long blockOff);
void toolbox_cdb_send_file_end(unsigned char *cdb);

/* Fill a 6-byte MODE SENSE(6) CDB requesting vendor page 0x31 (Toolbox detection).
 * `cdb` must have room for 6 bytes. */
void toolbox_cdb_mode_sense_p31(unsigned char *cdb);

/* 1 if a Toolbox page-0x31 signature — BlueSCSI (TB_MAGIC) or ZuluSCSI
 * (TB_MAGIC_ZULU) — appears anywhere in buf[len]. */
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

/* TEST UNIT READY on `id`: 1 if a disc is loaded (including an audio / non-HFS disc
 * that mounts no Mac volume), 0 if the drive is empty. Lets the CD Library name what
 * is in the drive even when the File Manager sees no volume (docs/45). */
int  toolbox_media_present(short id);

/* ---- file transfer (docs/46) ------------------------------------------------
 * The SD-card side of the Toolbox. These are the READ half; the send half lands
 * with the copy-out path. All operate on the target's current working/shared dir. */

/* GET CAPABILITIES (0xD9 / 0x01) into *caps (the TB_CAP_* bits). Returns 1 on GOOD.
 * A target can answer the CD ops and still not implement the file ops, so callers
 * confirm with toolbox_count_files before offering the browser. */
int  toolbox_file_caps(short id, unsigned char *caps);

/* COUNT FILES in the current dir; *n receives the count. Returns 1 on GOOD, 0 when
 * the target doesn't implement the file ops — which is how we detect that. */
int  toolbox_count_files(short id, int *n);

/* LIST FILES into `buf[cap]`; *n receives the entry count (<= TB_MAX_FILES, the
 * firmware's cap). `isDir` marks directories. Returns 1 on GOOD, 0 otherwise. */
int  toolbox_list_files(short id, TbEntry *buf, int cap, int *n);

/* GET FILE: read the 4 KB block at `blockOff` of listing entry `index` into `dst`
 * (`cap` bytes). Returns 1 on GOOD. The handshaked SCSI Manager cannot report a SHORT
 * transfer's length, so the caller works out how much of the final block is real from
 * the file size the listing gave it, and zeroes `dst` beforehand. */
int  toolbox_get_file_block(short id, int index, unsigned long blockOff, void *dst, long cap);

/* SEND FILE, the upload half: PREP creates/truncates `name` (<=32 chars) in the
 * current folder, DATA writes a chunk at `blockOff` (512-byte units), END flushes and
 * closes. See toolbox_cdb_send_file_10 for the offset caveat that decides what
 * `blockOff` must contain. All return 1 on GOOD. */
int  toolbox_send_file_prep(short id, const char *name);
int  toolbox_send_file_data(short id, const void *src, long nbytes, unsigned long blockOff);
int  toolbox_send_file_end(short id);

/* Locate the Toolbox device serving the FILE ops (cached per session). Unlike
 * toolbox_probe_id this does NOT require a CD-ROM: the file Toolbox lives on the
 * emulated HARD DISK, so the two probes can land on different SCSI ids. */
int  toolbox_probe_file_id(short *outId);

/* SET / GET WORKING DIR (0xD9 / 0x02 / 0x03) — only where TB_CAP_WORKDIR is set;
 * neither snow nor the MiSTer handoff spec implements them, so directory navigation
 * is a real-BlueSCSI-only nicety and the browser falls back to a flat listing.
 * An empty `path` resets the target to its default shared folder. */
int  toolbox_set_working_dir(short id, const char *path);
int  toolbox_get_working_dir(short id, char *out, int cap);
#endif /* TOOLBOX_HOST_TEST */

#endif /* MACATRIUM_TOOLBOX_H */
