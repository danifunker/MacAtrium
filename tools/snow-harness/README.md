# tools/snow-harness — headless Snow verification

This is how the MVP launcher (and the launch-return keystone) were verified
**without a display server**: a small Rust harness drives Snow's emulator core
directly — boots a Mac II + System 7.5.5 hard disk, injects keystrokes at given
CPU-cycle marks, and dumps the framebuffer to PNG so the result can be inspected
programmatically.

It is the practical answer to docs/04's open item *"automating emulator
boot/screenshot is desirable but unproven."* It's proven now.

## What it needs (all already on the dev machine)

| Piece | Path used | Notes |
|-------|-----------|-------|
| Mac II ROM | `~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom` | 256 KB; Snow auto-detects "Macintosh II (FDHD)" |
| Display-card ROM | MAME `nb_mdc824.zip` → `3410868.bin` | 32 KB; the **Macintosh Display Card 8•24** — a Mac II has no built-in video, Snow needs this for a framebuffer (`ExtraROMs::MDC12`) |
| Boot disk | `~/MacOS_SampleDisks/MacLC_*.hda` | raw SCSI images; **7.0.1 / 7.1 / 7.5.5** all boot + auto-launch + launch/return verified on the Mac II ROM. 6.0.8 boots + MultiFinder activates but the launcher needs a port (docs/11 §C″). |

Snow's CPU tops out at SE/30 class but the **Mac II (68020)** covers System
7.x with Color QD — see docs/11 §B. The 7.5.5 image renders at 1-bit here, so
the runs exercise the **B&W** render backend (the hard MVP requirement); the
Color backend is implemented but needs a colour depth to exercise.

## Build the harness

It depends on `snow_core`, so build it as a bin inside Snow's `testrunner`
crate (reuses Snow's warm build artifacts):

```sh
cp macatrium_harness.rs ~/repos/snow/testrunner/src/bin/macatrium_harness.rs
cd ~/repos/snow
cargo build -r -p testrunner --bin macatrium_harness
# -> ~/repos/snow/target/release/macatrium_harness
```

## Assemble a test image

`assemble.sh` builds a pristine image from a System 6/7 source disk: it creates
`/MacAtrium/{metadata,Apps}`, injects `test_catalog.jsonl`, drops two real launch
targets (both forks, via `rb-cli get-binhex`/`put-binhex`) — **SimpleText** and
the **real Prince of Persia** (app + its `Persia(BW/COLOR/LC)` data files) — and
places the built `build/MacAtrium.bin` in the blessed **Startup Items** folder so
it auto-launches (or at the volume root if there's no Startup Items, e.g. System 6).

```sh
mkdir -p /tmp/macatrium-run/pop
cp test_catalog.jsonl /tmp/macatrium-run/
# one-time: extract the launch targets (forks preserved)
rb-cli get-binhex ~/MacOS_SampleDisks/MacLC_7-5-5_OG.hda /SimpleText /tmp/macatrium-run/SimpleText.hqx
POPHDA=~/MacOS_SampleDisks/MacLC_6-0-8-POP.hda
POPDIR='/Games/Prince of Persia ƒ'
rb-cli get-binhex "$POPHDA" "$POPDIR/Prince of Persia" /tmp/macatrium-run/pop/Prince_of_Persia.hqx
rb-cli get-binhex "$POPHDA" "$POPDIR/Persia(BW)"      /tmp/macatrium-run/pop/Persia_BW_.hqx
rb-cli get-binhex "$POPHDA" "$POPDIR/Persia(COLOR)"   /tmp/macatrium-run/pop/Persia_COLOR_.hqx
rb-cli get-binhex "$POPHDA" "$POPDIR/Persia(LC)"      /tmp/macatrium-run/pop/Persia_LC_.hqx

# assemble (3rd arg = Startup Items dir; 7.0.1's blessed folder is "System Folder 7.0.1")
./assemble.sh ~/MacOS_SampleDisks/MacLC_7-1.hda       /tmp/macatrium-run/master_71.hda
./assemble.sh ~/MacOS_SampleDisks/MacLC_7-0-1.hda     /tmp/macatrium-run/master_701.hda "/System Folder 7.0.1/Startup Items"
./assemble.sh ~/MacOS_SampleDisks/MacLC_7-5-5_OG.hda  /tmp/macatrium-run/master_755.hda
```

(Adjust the `RUN`/paths at the top of `assemble.sh` if needed. Always work on a
copy — `assemble.sh` copies the source first.)

To activate MultiFinder on a System 6 disk (prerequisite for the resident-launch
model there — docs/11 §C″), swap the boot-block shell name "Finder"→"MultiFinder"
(Str15 at HFS partition offset `0x1A`):

```sh
printf '\x0bMultiFinder    ' | dd of=copy_608.hda bs=1 seek=49178 conv=notrunc  # 49152 = LBA96*512
```

## Run a scripted test

```
macatrium_harness <rom> <mdc_rom> <hdd.img> <out_dir> <max_cycles> \
    [--snap-every N] [--keys "CYCLE:KEY;CYCLE:KEY;..."] [--wall-secs S] [--disk2 <hdd2.img>]
```

- `--snap-every N` dumps `snap_NNN_<cycle>.png` every N cycles; `final.png` at the end.
- `--disk2 <hdd2.img>` attaches a **second** SCSI disk (id 1) — for the multi-disk
  library verification (docs/37/41): a 2nd volume carrying its own `/MacAtrium`. The
  launcher aggregates both, tags categories `[0]`/`[1]`, and lists them in Status.
- `--keys` taps keys at the given cycle marks. KEY ∈ letters, `enter`/`return`,
  `esc`, `up`/`down`/`left`/`right`, `space`, or `cmd-<key>` (e.g. `cmd-q`).
- Boot to the desktop + Startup-Items launch takes ~2 G cycles (~45 s wall on
  this box at ~44 M cycles/s).

The end-to-end launch/return verification (results in docs/11 §C, screenshots in
docs/evidence/):

```sh
cp /tmp/macatrium-run/master.hda /tmp/macatrium-run/run.hda
macatrium_harness \
  ~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom \
  /tmp/mdc/3410868.bin \
  /tmp/macatrium-run/run.hda \
  /tmp/macatrium-run/out 4000000000 \
  --snap-every 100000000 \
  --keys "2200000000:down;2280000000:down;2360000000:down;2480000000:enter;3300000000:cmd-q"
# down x3 -> SimpleText, Enter -> launches it, Cmd-Q -> quits it,
# control returns to MacAtrium with the selection intact.
```
