/*
 * cdidx.h — the CD-title reverse index (docs/45).
 *
 * A build emits `metadata/cdindex.jsonl`: one `{image, volume}` record per CD
 * title (every catalog item carrying BOTH a host `cdImage` and an expected
 * `cdVolume`). The CD Library uses it to reverse-map the CD-ROM volume ACTUALLY
 * mounted back to its host image, so the "(in drive)" marker names the right disc
 * even right after a reboot — WITHOUT persisting any session state.
 *
 * Pure C (parse + lookup), host-tested; the launcher reads the file (main.c) and
 * feeds the buffer to cdidx_parse, exactly as it does the paged catalog.
 */
#ifndef MACATRIUM_CDIDX_H
#define MACATRIUM_CDIDX_H

#include "catalog.h"   /* ITEM_CDIMG_LEN, ITEM_CDVOL_LEN */

/* How many CD titles the reverse index holds. Matches TB_MAX_CDS (the host-image
 * enumeration cap): there can't be more mounted-disc identities than listable
 * images, and the whole table is ~9 KB — fine to keep resident. */
#define CDIDX_MAX 100

/* One CD title's identity: its host SD image filename and the HFS volume that
 * mounts when that image is inserted. */
typedef struct {
    char image[ITEM_CDIMG_LEN];
    char volume[ITEM_CDVOL_LEN];
} CdIdxEntry;

/* Parse a whole cdindex.jsonl buffer (CR/LF/CRLF tolerant, like the catalog) into
 * out[cap]; keeps only records with a non-empty image AND volume. Returns the
 * count loaded (<= cap). Pure. */
int cdidx_parse(const char *buf, long len, CdIdxEntry *out, int cap);

/* The host image whose CD volume name matches `volName` (case-insensitive, as HFS
 * matches), or NULL if no indexed CD title uses that volume. Pure. */
const char *cdidx_image_for_volume(const CdIdxEntry *idx, int n, const char *volName);

#endif /* MACATRIUM_CDIDX_H */
