# 45 — CD-based titles: BlueSCSI Toolbox disc switching

**Goal.** Launch games and apps that live on a CD by having MacAtrium **insert the
right disc for a title on demand**, over the BlueSCSI Toolbox vendor SCSI command
set. One `folder` of CD images on the host is exposed as a single CD-ROM whose
mounted disc the guest can swap at runtime — so a curated library can span dozens
of discs behind one drive. Works identically against the **Snow** emulator (dev
loop), a real **BlueSCSI**, and the **MiSTer** MacLC core.

Status: ✅ **built** — probe, list, fuzzy match, swap, unmount, startup cache, and
the CD Library browser are implemented and host-tested. The launch wiring
(`cdswap_ensure`) is in place; wiring CD titles into the launch UI is the next step.

---

## 0. The pieces

| Module | Role |
|--------|------|
| `src/toolbox.{c,h}` | BlueSCSI Toolbox wire protocol: detect the device, `LIST CDS`, `SET NEXT CD`, entry parse, fuzzy name match. Pure logic split from the SCSI-Manager transport (`-DTOOLBOX_HOST_TEST` keeps the pure half for `tests/host_test.c`). |
| `src/cdswap.{c,h}` | Orchestration: the **session CD cache**, and `cdswap_ensure()` — "insert the disc this title needs, wait for it, verify it." |
| `src/macfs.{c,h}` | Volume scan / unmount helpers (`macfs_find_cd_vol`, `macfs_unmount`, `macfs_find_vol_by_name`). |
| `src/main.c` | Startup scan call; the **CD Library** browser (`run_cd_list_dialog`). |
| `src/scsimgr.h` | Original SCSI Manager trap glue (`SCSISelect`/`SCSICmd`/`SCSIRead`/…) — System 6.0.8 → 7.x. |

The host (Snow) side is a folder-backed CD-ROM in
`snow/core/src/mac/scsi/cdrom/` + `toolbox.rs`; see that repo for the device
implementation.

---

## 1. Wire protocol (what we speak)

All Toolbox commands are **10-byte CDBs**. We only use a small, portable subset
(the MiSTer RTL implements `0xD0–0xD9`, so we avoid `0xDA COUNT CDS`).

| Op | Name | CDB | Data |
|----|------|-----|------|
| `0x1A` | MODE SENSE(6) page `0x31` | `1A 00 31 00 40 00` | vendor page carrying the magic string `"BlueSCSI is the BEST…"` — **device detection** |
| `0x12` | INQUIRY | `12 00 00 00 24 00` | 36 bytes; byte 0 low 5 bits = peripheral type (`0x05` = CD-ROM) — **device-type check** |
| `0xD7` | LIST CDS | `D7 00…` | `N × 40`-byte entries (see below) |
| `0xD8` | SET NEXT CD | `D8 <index> 00…` | none; switches the mounted image |

### LIST CDS entry (40 bytes, firmware-exact)

```
byte 0      index          (SET NEXT CD uses this)
byte 1      type           0x01 = file, 0x00 = directory
byte 2..34  name           NUL-terminated MacRoman, **max 32 chars** (clipped)
byte 35..39 size           40-bit big-endian (we read the low 32 bits at 36..39)
```

Two things that bite:

1. **Names are clipped to 32 chars.** A host filename longer than 32 bytes comes
   back truncated. See §4 (fuzzy match).
2. **One `SCSIRead` must drain the whole DataIn phase.** The original SCSI Manager
   fills a single TIB per command; issuing one `SCSIRead` per 40-byte entry leaves
   the follow-up reads untransferred, so entries come back as uninitialised garbage
   (the classic "row of empty boxes"). `toolbox_list_cds` reads the entire listing
   into one buffer, then parses it — the same single-read shape the MODE SENSE probe
   uses.

---

## 2. Device probe (which SCSI id is the CD)

`toolbox_probe_id(pin, &id)` finds the drive once per session (cached in RAM):

1. For each id in `{6,0,1,2,3,4,5}`: MODE SENSE(6) page `0x31`, check for the magic.
2. **Then confirm it's a CD-ROM** via INQUIRY (peripheral type `0x05`).

Step 2 matters: a BlueSCSI **hard disk also answers page `0x31`** (it serves the
file-sharing Toolbox), so page `0x31` alone can aim the CD ops at the HDD and you
get `Unknown command D7h`/`D8h` on the disk. The type check singles out the CD.

`pin >= 0` forces an id and skips probing (a future `cdId` pref).

---

## 3. The session cache — scan once at startup

So launches don't re-walk the SCSI bus each time, and the app can answer
"**is this title's disc present, and at what index?**" up front, the listing is
scanned **once at startup** and held in RAM (`src/cdswap.c`):

```c
void cdswap_scan(void);                 /* probe + LIST CDS -> cache. Call at startup. */
int  cdswap_ready(short *id);           /* 1 (+*id) if a CD device was found            */
const TbEntry *cdswap_cds(int *n, int *found, short *id);  /* the cached listing        */
int  cdswap_find(const char *cdImage);  /* fuzzy find -> SET NEXT CD index, or -1        */
```

- `main()` calls `cdswap_scan()` right after the catalog loads.
- `cdswap_find()` re-scans **once** on a miss (a disc may have been added since boot).
- The **CD Library** browser refreshes the cache on open (folder may have changed).
- The cache is `static TbEntry gCdCds[TB_MAX_CDS]` (~4.4 KB) — the only copy; the
  launch path and the browser both read it (no per-call stack arrays).

**Availability check for the UI:** `cdswap_find(it->cdImage) >= 0` ⇒ the disc is
present. Use this to gate/annotate CD titles (not yet wired into the list UI).

---

## 4. Fuzzy filename match

`toolbox_find_cd(wanted, entries, n)` is two-pass:

1. **Exact**, case-insensitive — always preferred.
2. **Clip fallback**: `wanted` longer than 32 chars matches an entry that is a
   case-insensitive **prefix** of it *and sits at the 32-char clip boundary*.

Short/unclipped names still require an exact match, so nothing fuzzy-matches by
accident. (Host tests cover clip-match, exact-wins, and no-false-positive.) If a
catalog name differs by more than truncation (spaces/punctuation), extend this
with a normalized pass — carefully, to avoid inserting the wrong disc.

---

## 5. Launching a CD title — `cdswap_ensure`

This is the call site for CD-based games. Catalog fields that describe a CD title
(`catalog.jsonl`, see [docs/06](06-content-pipeline.md)):

| Field | Meaning |
|-------|---------|
| `cdImage` | host CD image filename to insert (e.g. `"MYST.iso"`) |
| `cdVolume` | the HFS volume name the disc mounts as (e.g. `"Myst"`) — used to detect/verify the mount |
| `cdApp` | app path **relative to the CD volume root** for a run-from-CD title; empty ⇒ app-on-HD that just reads the disc |
| `cdRequired` | 1 ⇒ the disc is required (default for a CD title); 0 ⇒ optional |

```c
CdResult cdswap_ensure(const CatItem *it, const CdSwapUI *ui, short *cdVref);
```

Flow:

1. **Fast-path** — if `cdVolume` is already mounted, return `CD_OK` (+ its `vRefNum`).
2. **Locate** the CD from the cache (`cdswap_ready`).
3. **Unmount** the disc we last inserted, if it's a different one — classic Mac OS
   otherwise nags forever once the media changes under it.
4. **Find** `cdImage` in the cache (`cdswap_find`, fuzzy; re-scans once on a miss).
5. **`SET NEXT CD`** to that index.
6. **Wait** (abortable, timed) for `cdVolume` to mount, then verify by name.

Results (`CdResult`): `CD_OK`, `CD_UNSUPPORTED` (no CD device), `CD_NOT_FOUND`
(image not in the folder), `CD_UNMOUNT_BUSY` (open files on the outgoing disc),
`CD_TIMEOUT`, `CD_ABORTED`. The caller decides whether to still launch per
`it->cdRequired`.

The `CdSwapUI` hooks (`message`, `wait_tick`, `timeoutTicks`) keep `cdswap` free of
window/event code — the launcher passes a status-line callback and an event pump.

### Sketch: launch a CD title

```c
short   cdVref = 0;
CdSwapUI ui = { say_status, pump_events, ctx, /*timeout*/ 900, /*pin*/ -1 };
CdResult r = cdswap_ensure(it, &ui, &cdVref);
if (r == CD_OK || (r != CD_UNSUPPORTED && !it->cdRequired)) {
    /* app-on-HD: launch it->app as usual.
     * run-from-CD (it->cdApp[0]): build an FSSpec on cdVref's root via
     *   macfs_make_spec_root(cdVref, it->cdApp, &spec) and launch that. */
}
```

---

## 6. Manual browse — the CD Library

`run_cd_list_dialog` (Esc menu) lists the cached images and lets the user insert
one by hand: right-aligned **index column**, then the name; the active disc is
marked. **Insert** (button / Return) unmounts the outgoing CD volume
(`macfs_find_cd_vol` — hardware-locked/write-protected media) then `SET NEXT CD`,
so no swap nag.

---

## 7. Insert / swap / eject semantics (host side)

- **Attach a folder** as a CD-ROM → Snow mounts the alphabetically-first image
  automatically (matches BlueSCSI).
- **`SET NEXT CD`** replaces the mounted image and raises a UNIT ATTENTION
  ("medium may have changed") so the guest re-reads the TOC and remounts.
- **Eject** (Mac OS `START/STOP UNIT` / vendor `0xC0`) drops the disc but keeps the
  folder — the drive stays a functional, empty CD-ROM; `LIST`/`SET` still work, so a
  disc with a bad filesystem that Mac OS ejects just leaves the drive empty and
  re-selectable.

---

## 8. Gotchas / notes

- **`Unknown command D7h`/`D8h` on `scsi::disk`** ⇒ the probe hit the HDD; ensure the
  INQUIRY type-5 check is in place (§2).
- **A row of empty boxes in the list** ⇒ the client read one entry per `SCSIRead`
  instead of draining the phase in one read (§1).
- **`COUNT CDS` (`0xDA`) is intentionally unused** — outside the MiSTer RTL's
  `0xD0–0xD9` window; the count is however many entries precede the first empty name.
- **Host tests**: `cd tests && make test` exercises the pure half (entry parse,
  name match incl. fuzzy, CDB build, magic detection) with host gcc — no Toolbox.
