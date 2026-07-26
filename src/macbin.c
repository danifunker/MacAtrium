/*
 * macbin.c — see macbin.h. MacBinary II header build/parse plus the CRC, kept pure so
 * the whole thing is exercised off-target by tests/host_test.c (docs/46).
 *
 * Header field offsets (MacBinary II):
 *    0  version (must be 0)          83  data fork length   (u32 BE)
 *    1  filename length (1..63)      87  resource fork len  (u32 BE)
 *    2  filename (63 bytes)          91  creation date      (u32 BE)
 *   65  file type    (4)             95  modification date  (u32 BE)
 *   69  file creator (4)             99  Get Info comment length (u16)
 *   73  Finder flags, high byte     101  Finder flags, low byte
 *   74  must be 0                   122  version used to pack   (129)
 *   81  "protected" flag            123  version needed to unpack (129)
 *   82  must be 0                   124  CRC-16 of bytes 0..123
 */
#include "macbin.h"

#include <string.h>

long macbin_padded(long n)
{
    long r = n % MACBIN_HDR;
    return r ? n + (MACBIN_HDR - r) : n;
}

unsigned short macbin_crc(const unsigned char *buf, long len)
{
    unsigned short crc = 0;
    long i;
    int  b;

    for (i = 0; i < len; i++) {
        crc ^= (unsigned short)((unsigned short)buf[i] << 8);
        for (b = 0; b < 8; b++) {
            if (crc & 0x8000) crc = (unsigned short)((crc << 1) ^ 0x1021);
            else              crc = (unsigned short)(crc << 1);
        }
    }
    return crc;
}

/* Store a 32/16-bit value big-endian (the Mac's own byte order, and MacBinary's). */
static void put32(unsigned char *p, unsigned long v)
{
    p[0] = (unsigned char)((v >> 24) & 0xFF);
    p[1] = (unsigned char)((v >> 16) & 0xFF);
    p[2] = (unsigned char)((v >> 8) & 0xFF);
    p[3] = (unsigned char)(v & 0xFF);
}

static unsigned long get32(const unsigned char *p)
{
    return ((unsigned long)p[0] << 24) | ((unsigned long)p[1] << 16) |
           ((unsigned long)p[2] << 8)  | (unsigned long)p[3];
}

void macbin_build_header(unsigned char *hdr, const char *name,
                         unsigned long dataLen, unsigned long rsrcLen,
                         unsigned long type, unsigned long creator,
                         unsigned char finderFlags)
{
    int nl = 0;

    if (!hdr) return;
    memset(hdr, 0, MACBIN_HDR);
    while (name && name[nl] && nl < MACBIN_NAME_MAX) nl++;

    hdr[0] = 0;                                  /* version */
    hdr[1] = (unsigned char)nl;
    if (nl > 0) memcpy(hdr + 2, name, (size_t)nl);
    put32(hdr + 65, type);
    put32(hdr + 69, creator);
    hdr[73] = finderFlags;
    hdr[74] = 0;
    hdr[82] = 0;
    put32(hdr + 83, dataLen);
    put32(hdr + 87, rsrcLen);
    hdr[122] = 129;                              /* packed by MacBinary II   */
    hdr[123] = 129;                              /* needs MacBinary II       */
    {   /* CRC covers bytes 0..123 and is what marks this as MacBinary II */
        unsigned short crc = macbin_crc(hdr, 124);
        hdr[124] = (unsigned char)((crc >> 8) & 0xFF);
        hdr[125] = (unsigned char)(crc & 0xFF);
    }
}

int macbin_parse(const unsigned char *buf, long len, MacBinInfo *out)
{
    int  nl, i;
    unsigned long dataLen, rsrcLen;

    if (!buf || len < MACBIN_HDR) return 0;

    /* The cheap structural invariants first. Bytes 0, 74 and 82 are defined zero in
     * every MacBinary version, which is most of what separates a real header from an
     * ordinary file that merely starts with a NUL. */
    if (buf[0] != 0 || buf[74] != 0 || buf[82] != 0) return 0;

    nl = buf[1];
    if (nl < 1 || nl > MACBIN_NAME_MAX) return 0;
    /* A filename with embedded NULs or a path separator is not a Mac filename. */
    for (i = 0; i < nl; i++) {
        if (buf[2 + i] == 0 || buf[2 + i] == '/' || buf[2 + i] == ':') return 0;
    }

    dataLen = get32(buf + 83);
    rsrcLen = get32(buf + 87);
    /* Fork lengths are u32 on the wire. Anything at or beyond 2 GB is not a file this
     * launcher will ever move, and accepting it would hand the copy loop an absurd
     * transfer length (and overflow the signed longs it counts with). */
    if (dataLen > 0x7FFFFFFFUL || rsrcLen > 0x7FFFFFFFUL) return 0;

    /* MacBinary II stores a CRC of bytes 0..123; MacBinary I leaves it zero. Accept
     * either, but reject a NON-zero CRC that does not match — that is corruption, not
     * an older writer. */
    {
        unsigned short stored = (unsigned short)(((unsigned short)buf[124] << 8) | buf[125]);
        if (stored != 0 && stored != macbin_crc(buf, 124)) return 0;
    }

    if (out) {
        memcpy(out->name, buf + 2, (size_t)nl);
        out->name[nl]     = '\0';
        out->dataLen      = dataLen;
        out->rsrcLen      = rsrcLen;
        out->type         = get32(buf + 65);
        out->creator      = get32(buf + 69);
        out->finderFlags  = buf[73];
    }
    return 1;
}
