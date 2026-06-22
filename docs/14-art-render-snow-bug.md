# 14 — Resume prompt: in-launcher art rendering crashes Snow on some art

Paste this into a fresh session to pick up the one open defect from the Priority-2
push. Everything else (atrium toolchain, type-ahead, aliases, dark mode, CI) is
done and on `main`.

---

## The bug (one sentence)

The launcher's **in-UI art rendering** — the inline box-art pane (commit
`2d4d9d5`) and the full-screen `P` preview (in `14c7627`) — **`DrawPicture`s a
PICT and crashes the emulated Mac** ("Sorry, a system error occurred … illegal
instruction", sometimes a hang) on **certain valid box-art images**, while others
render fine. So `main` currently has a launcher that bombs when you browse to some
titles' art.

- **Crashes:** Shufflepuck Café (194×256), and likely other titles.
- **Works:** Dark Castle (168×256), Beyond Dark Castle, Crystal Quest, and the
  synthetic 128×128 test art.

## What's already ruled out (don't re-litigate)

1. **The PICT is byte-perfect.** A standalone decoder walks every PackBitsRect
   row: each unpacks to exactly `rowBytes`, there are exactly `H` rows, and the
   stream ends with `0x00FF` (OpEndPic). Shufflepuck and Dark Castle decode
   identically-structured. (Decoder snippet is in the session log; re-derivable.)
2. **Odd `rowBytes` was a *separate* bug, already fixed** (`b1bdeab`): a portrait
   width like 180 → `ceil(180/8)=23` (odd) made `DrawPicture` **hang**. QuickDraw
   requires even rowBytes; `atrium pict` now pads to even. All current art has
   even rowBytes. This is NOT the illegal-instruction crash.
3. **PixMap vs BitMap doesn't matter.** Tried 1-bit as a PixMap+CLUT and as an
   old-style BitMap (high-bit-clear rowBytes). Both crash on Shufflepuck.
4. **Scaling doesn't matter.** Tried `DrawPicture` at native **1:1** (picFrame ==
   dstRect, no scaling). Still crashes on Shufflepuck.
5. **Type-ahead / the launcher logic is fine.** Typing `d` (→ Dark Castle) jumps +
   renders cleanly; a no-art build lets `s s` (→ Shufflepuck → SimCity) jump fine.
   The crash is *only* when `DrawPicture` touches the specific art.

## Leading conclusion

A **Snow QuickDraw `DrawPicture` emulation bug** on certain valid 1-bit picture
data — same family as the already-documented 4-bit PICT faults (F-line crash when
packed, hang when unpacked; see `docs/13` §5). Not our encoder.

## The fix to pursue (most promising first)

1. **Render art via `CopyBits` of a raw bitmap instead of `DrawPicture` of a
   PICT.** `CopyBits` is the routine `render_end` already uses to composite the
   off-screen GWorld to the window every frame, and it has *never* crashed.
   Concretely:
   - `atrium`: emit art as a **raw 1-bit bitmap** sidecar (e.g. header
     `[u16 w][u16 h][u16 rowBytes]` + MSB-first bits, rowBytes even) instead of
     (or alongside) the PICT. A new `atrium pict --raw1` or an `image` art option.
   - launcher `src/art.c`: `art_load` reads the raw bitmap into a handle;
     `art_draw_fit` builds a `BitMap {baseAddr, rowBytes, bounds}` and
     `CopyBits(&bm, GetPortBitMapForCopyBits(port), &src, &dst, srcCopy, NULL)`
     into the off-screen GWorld (scaled). This bypasses the PICT opcode
     interpreter entirely. **Highest confidence this fixes it.**
2. If you must keep PICT: confirm it's Snow-specific by rendering the *same*
   `shufflepuck-cafe.1.pict` on **real hardware or another emulator** (Mini vMac /
   Basilisk II). If it renders there, it's purely a Snow bug — file upstream and
   use the CopyBits path here regardless.
3. Optional: binary-search the picture data to find the minimal pixel pattern that
   trips Snow's `DrawPicture`, to report a precise upstream repro.

## Decision needed

Until the CopyBits fix lands, the in-launcher art (inline pane + `P` preview) is
**unsafe on `main`** (crashes on some art). Either:
- (a) implement the CopyBits-raw-bitmap art (delivers art reliably), or
- (b) temporarily disable in-launcher art rendering (set `showArt = 0` in
  `draw_list`, drop the `p`/`P` preview case so `p` type-aheads) to keep the
  appliance stable, then do (a).

## Reproduce (headless, ~70s)

```sh
cd ~/repos/MacAtrium && export RETRO68=~/repos/Retro68-build
cmake --build build                                   # launcher
# build an appliance with downloaded box art at depth variants:
tools/atrium-tool/target/release/atrium image --config /tmp/atrium-out/build-variants.json
# (build-variants.json: system MacLC_7-1.hda, dataset data/library.jsonl,
#  metadata ~/launchbox/Metadata.xml, download_art true, art_depths ["1","8"])
cp /tmp/macatrium-run/image_variants.hda /tmp/macatrium-run/run.hda
~/repos/snow/target/release/macatrium_harness \
  ~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom /tmp/mdc/3410868.bin \
  /tmp/macatrium-run/run.hda /tmp/macatrium-run/out 2900000000 \
  --snap-every 30000000 --keys "2450000000:s"          # 's' -> Shufflepuck -> bomb
# Read /tmp/macatrium-run/out/snap_*_2520*.png  (shows the illegal-instruction bomb)
```

Generated art lands in `/tmp/atrium-image-stage/<id>.{1,8}.pict`. The crashing one
is `shufflepuck-cafe.1.pict`; the working comparison is `dark-castle.1.pict`.

## Pointers

- `src/art.c` / `src/art.h` — PICT load + `DrawPicture` (the thing to replace).
- `src/ui.c` — `draw_list` inline pane (`showArt`), `load_item_art`, `p`/`P` case.
- `tools/atrium-tool/src/pict.rs` — the encoder (`encode_indexed`, even rowBytes).
- `docs/13-handoff.md` §5 — the broader Priority-2 status + the 4-bit/colour-depth
  Snow limitations (same emulator-quirk family).
