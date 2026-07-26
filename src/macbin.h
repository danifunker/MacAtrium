/*
 * macbin.h — MacBinary II encode/decode (docs/46).
 *
 * The SD card is a flat FAT volume; a classic Mac file is not. It carries a data
 * fork, a resource fork, and Finder info (type/creator) — lose the resource fork and
 * an application is simply dead. MacBinary packs all three into ONE file, and it is
 * the only fork-preserving option this protocol can actually round-trip:
 * AppleDouble needs a "._name" sidecar, and the BlueSCSI firmware skips every
 * filename beginning with '.' in BOTH its directory listing and its index lookup, so
 * such a sidecar could never be listed or read back. StuffIt (.sit) needs a
 * proprietary compressor that has no business in a 68k launcher.
 *
 * Layout: a 128-byte header, then the data fork padded to a 128-byte boundary, then
 * the resource fork padded the same way.
 *
 * Pure logic, host-tested — the launcher streams fork bytes past these helpers rather
 * than buffering whole files (a 68k launcher may only have a 384 KB partition).
 */
#ifndef MACATRIUM_MACBIN_H
#define MACATRIUM_MACBIN_H

#define MACBIN_HDR      128    /* header size, and the fork padding quantum */
#define MACBIN_NAME_MAX 63     /* header filename field is 63 bytes         */

/* What a MacBinary header says about the file it wraps. `type`/`creator` are OSTypes
 * kept as 32-bit values so this stays free of Mac headers and host-testable. */
typedef struct {
    char          name[MACBIN_NAME_MAX + 1];  /* NUL-terminated */
    unsigned long dataLen;
    unsigned long rsrcLen;
    unsigned long type;
    unsigned long creator;
    unsigned char finderFlags;                /* high byte of the Finder flags */
} MacBinInfo;

/* Round `n` up to the next MACBIN_HDR boundary — how both forks are padded. */
long macbin_padded(long n);

/* CRC-16/CCITT over `buf[len]` (poly 0x1021, init 0) — the MacBinary II header
 * checksum. Exposed so the round-trip test can assert the stored value. */
unsigned short macbin_crc(const unsigned char *buf, long len);

/* Write a 128-byte MacBinary II header into `hdr`. `name` is truncated to
 * MACBIN_NAME_MAX. */
void macbin_build_header(unsigned char *hdr, const char *name,
                         unsigned long dataLen, unsigned long rsrcLen,
                         unsigned long type, unsigned long creator,
                         unsigned char finderFlags);

/* Validate a 128-byte header and fill `out`. Returns 1 when `buf` really looks like
 * MacBinary, 0 otherwise — this is what decides whether a file arriving FROM the SD
 * card is unwrapped into two forks or written through as plain data, so it must not
 * be fooled by an ordinary file that happens to start with a zero byte. */
int macbin_parse(const unsigned char *buf, long len, MacBinInfo *out);

#endif /* MACATRIUM_MACBIN_H */
