# [SOLVED 2026-06-26] rb-cli corrupted the creator code → harvested PoP ran B&W

**Root cause was NOT a catalog-write field (the hypothesis below was wrong).** The
catalog *write* was fine; the bug was on the *read/encode* side: `decode_fourcc`
rendered type/creator to a **lossy display string** (non-ASCII → `.`), and
`get-binhex`/`cp` rebuilt the bytes *from that string*. PoP's creator `50 6f C4 50`
(`PoƒP`, `0xC4` florin) became `50 6f 2E 50` (`Po.P`) → generic icons + PoP couldn't
find its `Persia(COLOR)` data (located by creator) → B&W.

**Fix (rusty-backup):** full byte-based type-model redesign — `FileEntry.type_code`/
`creator_code` are now raw `Option<[u8;4]>`; display derived only at the boundary
(`type_code_display()` / `hfs_common::decode_ostype`); ProDOS type moved to
`prodos_file_type: Option<u8>`; `CreateFileOptions.os_type`/`os_creator` carry raw
bytes through every write path (get-binhex, cp, edit_queue, remote). MFS
`create_file` now honours `options` too. See `docs/bug_type_creator_fidelity.md`.

**Verified:** rebuilt `~/MacAtrium-7.1-working.hda`; all harvested PoP files have
creator `506fc450`; PoP launches in **256 colour** in the Snow harness (colour
intro + palace). 2117 lib tests + clippy clean.

> Everything below is the ORIGINAL (now-disproven) investigation handoff, kept for
> context only. The "Other uncommitted state" section at the bottom is still useful.

---

# Resume prompt — find the rb-cli HFS-write bug that makes harvested PoP run B&W

Paste this into a fresh session. Goal: pin down **which catalog/Finder-info field
rb-cli's HFS `create_file` writes differently from a real Mac**, fix it in
rusty-backup, then re-verify Prince of Persia launches in **colour** from a
MacAtrium-built image.

## The bug (confirmed root cause)
MacAtrium-built images harvest game apps via rb-cli. Harvested **Prince of Persia**
(and other apps) show **generic icons** in the Finder and PoP runs in **black &
white** (should be 256-colour on a colour screen).

**The user proved it's the file-copy method, not the files' content or the
environment:** copying the *identical* PoP files into the *same*
`/MacAtrium/Apps/Prince of Persia` folder **through the Mac Finder** (overwriting
the rb-cli-written ones) makes PoP launch in **colour** with correct icons. So the
fork bytes are fine — **rb-cli's HFS write produces a file a real Mac treats
differently.**

## Ruled out this session (don't re-investigate)
- **ROM**: user got PoP colour on Mac II **FDHD** after setting 256 colours in the
  control panel. Not a ROM/32-Bit-QuickDraw issue. (Aside: PoP's colour gate is a
  probe of toolbox trap `0xAB03` via `GetToolboxTrapAddress` — disassembled in
  CODE_2 of the app; but FDHD has it, so this isn't the blocker.)
- **MacAtrium residence/launch**: user removed MacAtrium from `/System
  Folder/Startup Items`, Finder-launched PoP → **still B&W** with our files.
- **Screen depth** (8-bit confirmed via Settings + colour box art), **memory**
  (bumped PoP `SIZE -1` 1000K→2000K, still B&W), **Desktop DB** (deleted it so the
  Finder rebuilt at boot, still B&W).
- **Both** rb-cli copy paths fail identically: `get-binhex`→`put-binhex` AND `cp`
  (they share `create_file`). Forks read back **byte-identical** via `get-binhex`
  (`cmp` of the `.hqx`), and PoP launches — so it's a **non-fork catalog attribute**.

## The idea to explore (the task)
rb-cli's `create_file` / `build_file_record` leaves several HFS catalog +
Finder-info fields **zero** that a real Mac write sets, and omits the file thread
record. **Byte-compare the on-disk 102-byte catalog file-record of "Prince of
Persia" between a Finder-written copy and an rb-cli-written copy; the differing
field(s) are the bug.**

Compare:
- **Finder-written**: `~/MacOS_SampleDisks/MacLC_6-0-8-POP.hda` →
  `/Games/Prince of Persia ƒ/Prince of Persia` (or the user's
  `~/MacLC_7-1-POP.hda` → `/Prince of Persia ƒ/Prince of Persia`).
- **rb-cli-written**: build a 7.1 image and look at
  `/MacAtrium/Apps/Prince of Persia/Prince of Persia`, OR cp the donor folder into a
  copy (`rb-cli cp -r <donor@1> "/Games/Prince of Persia ƒ" <img@1> "/MacAtrium/Apps/Prince of Persia"`).

### rusty-backup code references (the suspects)
- `src/fs/hfs.rs:1802` `build_file_record` — builds the 102-byte HFS file record.
  Sets: cdrType(rec[0]), type/creator(rec[4..12]), CNID(rec[20..24]), data
  start/len/phys(rec[24..34]), rsrc start/len/phys(rec[34..44]), create/mod
  dates(rec[44..52]), first extents(rec[74..78] data, rec[86..90] rsrc).
  **Leaves ZERO:** `filFlags` rec[2], `filTyp` rec[3], FInfo `fdFlags` rec[12..14]
  (set later by set_finder_info), `fdLocation` rec[14..18], `fdFldr` rec[18..20],
  `filBkDat` rec[52..56], **FXInfo** rec[56..72], **`filClpSize`** rec[72..74].
- `src/fs/hfs.rs:2807` `create_file` — at ~line 2896 **deliberately omits the file
  thread record** ("Finder/CiderPress2 don't emit them"). Re-check whether a real
  Mac actually needs it for files the Finder/Process Manager must look up by CNID.
- `src/fs/hfs.rs:1374` `set_finder_info` — writes FInfo rec[4..20] + FXInfo
  rec[56..72]; put-binhex/cp call it with fdLocation/fdFldr/FXInfo = 0.
- HFS file-record offsets (Inside Macintosh: Files): 2=filFlags, 3=filTyp,
  4..20=FInfo(type,creator,fdFlags,fdLocation,fdFldr), 20=filFlNum,
  24/26/30=data stBlk/lgLen/pyLen, 34/36/40=rsrc, 44/48/52=cr/md/bk dates,
  56..72=FXInfo, 72=filClpSize, 74..86=data extents, 86..98=rsrc extents.

### Extraction method (caveat)
A naive scan of the raw partition for `b"APPLPo.P"` (FInfo type+creator) with
`rec[0]==2` **found the record in the rb-cli image (1) but NOT in the donor (0)** —
so write a proper **HFS catalog B-tree walk** instead: MDB is at `partition_start +
0x400`; the MDB gives the catalog file's extent; parse the B-tree header node →
leaf nodes → find the leaf record whose key is `(parentCNID, "Prince of Persia")`;
the 102 bytes after the key are the file record. Partition start for these 40 MB
APM disks = `LBA 96 * 512 = 0xC000` (confirm with `rb-cli inspect <img>`). Diff the
two 102-byte records; the differing bytes name the field. Candidates in priority
order: **filClpSize**, **FXInfo (fdScript/fdXFlags)**, **fdFldr/fdLocation**,
**filFlags**, **missing thread record**.

## Fix + verify loop
1. Set the missing field(s) in `build_file_record`/`create_file` like a Mac (and/or
   emit the file thread record). rb-cli: `cd ~/repos/rusty-backup && cargo build
   --release --bin rb-cli` (release binary used everywhere:
   `~/repos/rusty-backup/target/release/rb-cli`).
2. Re-harvest a test 7.1 image:
   `cd ~/repos/MacAtrium && tools/atrium-tool/target/release/atrium image --config builds/7.1.json`
   (→ `~/MacAtrium-7.1-working.hda`).
3. Launch PoP in the Snow harness and check colour (harness MacAtrium-launch is a
   valid colour test — user confirmed correct files + MacAtrium-launch = colour):
   ```sh
   H=~/repos/snow/target/release/macatrium_harness
   ROM=~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom   # FDHD = the B&W repro rig
   MDC=/tmp/mdc/3410868.bin
   cp ~/MacAtrium-7.1-working.hda /tmp/t.hda
   "$H" "$ROM" "$MDC" /tmp/t.hda /tmp/out 8500000000 --snap-every 250000000 \
     --wall-secs 900 --keys "3500000000:right;3800000000:right;4100000000:return"
   ```
   Carousel order is BDC(0), DC(1), **PoP(2)** → `right;right;return` launches PoP.
   B&W intro frames are ~50–100 KB PNGs (palace, dithered). A colour PoP renders
   clearly colour — view the late snapshots (`/tmp/out/snap_03x_*.png`).
   (MDC ROM: `unzip ~/repos/mame/roms/nb_mdc824.zip -d /tmp/mdc` if `/tmp/mdc` gone.)

## Other uncommitted state to be aware of (do NOT lose)
All UNCOMMITTED. The PoP-colour bug is the blocker; these are done/working:
- **Launcher (`src/`)**: mouse support (gear→menu, ◀▶ arrows, Launch button,
  click-to-dismiss, side-tile select); colour box-art carousel centre; **maxDepth
  depth-drop fix** (defer restore to osEvt resume — BDC now runs at 1-bit);
  `noNewDevice` on the render GWorld; **Exit-to-Finder** menu item (UI_QUIT);
  PRAM-write on every depth change.
- **rusty-backup**: `get-binhex --clear-inited` (clears hasBeenInited) — committed?
  check `git -C ~/repos/rusty-backup status`. The catalog-write fix goes here too.
- **atrium-tool**: `cp --sparse=always` for the base copy (image.rs); harvest's
  `put_binhex` passes `--clear-inited`; MVC modules; `builds/7.5.5.json`.
- **Images** (`~/MacAtrium-{608,7.1,7.5.5}-working.hda`, `~/MacAtrium-9.2.2-working.qcow2`)
  all need rebuilding after the rb-cli catalog-write fix lands.
- 9.2.2 is built on a **256 MB HFS** base (`~/MacAtrium-9.2.2.qcow2` lineage), NOT
  the 10 GB HFS+ UTM disk (rb-cli corrupts HFS+ B-trees; snow_core also can't boot
  the 10 GB image — `MouseUpdateAbsolute` mode + RTE frame-format-0x10 crash).

## Memory
Project memories worth reading: `build-tool-mvc-architecture`, `system-608-boot-shell`,
`overrides-db-maxdepth`, `build-base-from-user-disks`, `snow-harness-verify-gotchas`.
Consider adding one for THIS finding (PoP B&W = rb-cli HFS catalog-write fidelity).
