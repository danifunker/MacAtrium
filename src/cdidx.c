/*
 * cdidx.c — see cdidx.h. Pure parse of metadata/cdindex.jsonl plus the reverse
 * (mounted-volume-name -> host image) lookup the CD Library uses to mark the disc
 * that is ACTUALLY in the drive (docs/45). No Toolbox, no I/O — main.c reads the
 * file and hands the buffer here, mirroring the catalog loader.
 */
#include "cdidx.h"
#include "json.h"
#include "toolbox.h"   /* toolbox_name_eq: the docs/45 case-insensitive name compare */

#include <string.h>

/* Advance *i past one line in buf[0..len); return the bytes before its CR/LF/CRLF
 * terminator. Mirrors catalog.c's next_line so both walk JSONL identically. */
static long cdidx_next_line(const char *buf, long len, long *i)
{
    long start = *i;
    long lineLen;
    while (*i < len && buf[*i] != '\n' && buf[*i] != '\r') (*i)++;
    lineLen = *i - start;
    if (*i < len && buf[*i] == '\r') {             /* swallow CR, CRLF as one */
        (*i)++;
        if (*i < len && buf[*i] == '\n') (*i)++;
    } else if (*i < len && buf[*i] == '\n') {
        (*i)++;
    }
    return lineLen;
}

int cdidx_parse(const char *buf, long len, CdIdxEntry *out, int cap)
{
    long i = 0;
    int  n = 0;

    while (i < len && n < cap) {
        long start   = i;
        long lineLen = cdidx_next_line(buf, len, &i);
        const JsonField *im, *vol;
        /* A JsonObject is ~26 KB; keep it off the (small, deep-at-runtime) 68k stack
         * — json_parse_object re-inits it per call and there's no reentrancy, so one
         * static instance is correct (same reasoning as catalog_parse_into). */
        static JsonObject obj;

        if (lineLen <= 0) continue;                /* blank line */
        if (json_parse_object(buf + start, lineLen, &obj) <= 0) continue;

        im  = json_get(&obj, "image");
        vol = json_get(&obj, "volume");
        if (im && im->type == JT_STR && im->str[0] &&
            vol && vol->type == JT_STR && vol->str[0]) {
            strncpy(out[n].image, im->str, sizeof out[n].image - 1);
            out[n].image[sizeof out[n].image - 1] = '\0';
            strncpy(out[n].volume, vol->str, sizeof out[n].volume - 1);
            out[n].volume[sizeof out[n].volume - 1] = '\0';
            n++;
        }
    }
    return n;
}

const char *cdidx_image_for_volume(const CdIdxEntry *idx, int n, const char *volName)
{
    int i;
    if (!volName || !volName[0]) return 0;
    for (i = 0; i < n; i++) {
        if (toolbox_name_eq(idx[i].volume, volName)) return idx[i].image;
    }
    return 0;
}
