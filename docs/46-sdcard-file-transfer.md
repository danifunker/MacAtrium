# 46 — SD-card file transfer (BlueSCSI Toolbox file ops)

Move ordinary files between the emulated Mac and the SD card, without rebuilding the
disk image. The CD half of the Toolbox is [docs/45](45-cd-based-titles.md); this is
the *file* half, which the same device serves.

Reached from the **Esc menu → SD Card**. The dialog lists the host's shared folder,
copies a file **in** (`Copy` / Return), and sends a Mac file **out** (`Send…`).

---

## 1. What the protocol gives us — and what it doesn't

Verified against the BlueSCSI-v2 firmware, the MiSTer `BLUESCSI_HANDOFF.md` spec, and
snow's `toolbox.rs`. All three agree on the commands:

| Op | Code | Notes |
|---|---|---|
| `LIST FILES` | `0xD0` | 40-byte entries — the SAME layout as `LIST CDS`, so `toolbox_parse_cd_entry` is reused verbatim. Byte 1: `0x00`=dir, `0x01`=file |
| `GET FILE` | `0xD1` | `cdb[1]`=index, `cdb[2..5]`=offset in **4 KB blocks**, `cdb[6]`=block count |
| `COUNT FILES` | `0xD2` | also our "does this target do file ops at all" probe |
| `SEND FILE PREP` | `0xD3` | filename via DataOut, exactly **33 bytes**; also truncates |
| `SEND FILE DATA` | `0xD4` | `cdb[6]`×512, or legacy `cdb[1..2]` byte count; `cdb[3..5]`=offset |
| `SEND FILE END` | `0xD5` | **no data phase** |
| `METADATA` | `0xD9` | `0x01` GET CAPABILITIES, `0x02/0x03` SET/GET WORKING DIR |

**There is no delete, rename or move.** Nothing in the dispatch does it. Faking delete
(truncate to 0) leaves a stub; faking rename costs a full round-trip and still leaves
one. So the browser reads and copies — it does not manage.

**Hard limits:** 100 entries per listing (`MAX_FILE_LISTING_FILES`), 32-char names
(`MAX_MAC_PATH`), 64-char paths (`MAX_FILE_PATH`). The UI says so when a listing is
capped rather than implying it saw everything.

**Browsing cannot disturb CD switching.** File ops resolve through `getEffectiveDir()`
(the working-dir override); CD ops use a separate per-target `CD_IMG_DIR`.

---

## 2. Why MacBinary, and not AppleDouble or .sit

A Mac file is a data fork **plus** a resource fork plus Finder info; lose the resource
fork and an application is dead. Of the fork-preserving options only MacBinary works
here:

- **AppleDouble is impossible.** `toolboxFilenameValid()` rejects any name starting
  with `.`, and `get_file_from_index()` applies the *same* filter — so a `._name`
  sidecar could never be listed *or* read back.
- **`.sit`** needs a proprietary compressor; not something a 68k launcher ships.
- **MacBinary** is one self-contained file: 128-byte header, data fork padded to 128,
  resource fork padded to 128. `src/macbin.c` builds and parses it (pure, host-tested).

Copying **out**, the wrapper is only offered when the file *has* a resource fork —
a plain document should land on the card directly usable. Wrapped files get `.bin`.

Copying **in**, `macbin_parse` decides: a valid header is unwrapped into both forks
plus type/creator, anything else is written through as a plain data fork. That check
must not be fooled by an ordinary file, so it validates the reserved zero bytes, the
name field, the CRC (when non-zero) and refuses absurd fork lengths.

---

## 3. Two traps worth remembering

**The `SEND FILE` offset means different things on different targets.** The handoff
spec (§4.5), snow and MiSTer **seek absolutely** to `offset × 512`. The BlueSCSI-v2
firmware does `gFile.seekCur(offset * 512)` — a **relative** seek. Send the wrong one
and everything past the first chunk is corrupt, and nothing advertises which it is.

`fb_copy_out` therefore **settles it at run time**: send with absolute offsets, check
the resulting size via `LIST FILES`, and if it disagrees flip to relative and resend
once. Cached per session, so only the first multi-chunk send pays for it. A
single-chunk file cannot tell the two apart, and correctly teaches us nothing.

**Closing a fork does not commit anything.** Classic Mac OS buffers the catalog and
file data until a clean unmount. A copy can report success at every step and still
vanish on a reset — which is exactly what happened in testing: the UI said "Copied"
and the image had no such folder. `macfs_flush_vol` (`FlushVol`) after each copy is
what makes it real.

---

## 4. Capability gating

`GET CAPABILITIES` reports `LARGE_TRANSFERS` / `LARGE_SEND` / `SET_WORKING_DIR`.
Only real BlueSCSI implements the working-dir subcommands — **neither snow nor the
MiSTer spec does**, and they correctly leave that bit clear. So directory navigation
is gated on it and degrades to a flat listing of the shared folder, with `Open` dimmed
and the reason on screen.

The file Toolbox is served by the **hard disk**, not the CD, so `toolbox_probe_file_id`
deliberately does *not* require a CD-ROM peripheral type (unlike `toolbox_probe_id`).
The two probes can land on different SCSI ids. Availability is confirmed with a trial
`COUNT FILES`; if that fails the browser says so instead of failing mid-transfer.

---

## 5. Where the code lives

| File | Role |
|------|------|
| `src/toolbox.{c,h}` | CDB builders (pure, host-tested) + the file transport |
| `src/scsimgr.h` | `SCSIWrite` (selector 6) — the DataOut half the send needs |
| `src/filebrowse.{c,h}` | model: probe, listing, path arithmetic (pure), copy in/out |
| `src/macbin.{c,h}` | MacBinary build/parse/CRC — pure, host-tested |
| `src/macfs.{c,h}` | resource-fork open, set Finder info, mkdir, **flush** |
| `src/main.c` | the dialog, fork prompt, StandardFile picker (with the other browsers, which own the event pump) |

Buffers are 4 KB and resident, not stack — a B&W build may only have a 384 KB
partition ([docs/44](44-memory-and-art-modes.md)). Transfers run over the handshaked
SCSI Manager and are genuinely slow, so progress and cancel are required, not optional.

---

## 6. Verifying

```sh
cd tests && make && ./host_test      # MacBinary round-trip, path arithmetic, CDBs
```

End-to-end in snow — the harness exposes the shared folder with `--shared-dir`
(separate from `--cd-dir`, mirroring the firmware's split):

```sh
macatrium_harness <rom> <mdc_rom> disk.hda out 5000000000 \
  --shared-dir /path/to/shared \
  --keys "2800000000:enter;3100000000:esc;3250000000:down;3350000000:down;3450000000:down;3600000000:enter;3900000000:enter"
```

Then confirm the file really landed — size *and* content:

```sh
rb-cli ls disk.hda /MacAtrium/Incoming
rb-cli get disk.hda /MacAtrium/Incoming/FILE.DAT /tmp/got.bin && cmp /tmp/got.bin <source>
```

Two testing notes learned the hard way: build the harness with `--features
snow_core/mmap` or **the disk is read-only and nothing persists**; and start each run
from a *fresh* copy of the image, because a writable disk means the launcher's
first-run view chooser is answered once and every scripted key sequence after that
lands somewhere different.
