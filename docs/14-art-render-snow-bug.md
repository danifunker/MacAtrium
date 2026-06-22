# 14 — in-launcher art rendering crashes Snow on some art

> **RESOLVED (2026-06-21).** Fix #1 below — render 1-bit art via `CopyBits` of a
> raw bitmap instead of `DrawPicture` of a PICT — landed and is **verified in
> Snow**. Shufflepuck Café (the canonical crasher, 194×256 1-bit) now renders
> both in the inline pane and the full-screen `P` preview, with the emulator
> running clean through the cycle (2520M) where it used to bomb.
> Evidence: [evidence/art-raw-copybits-shufflepuck-inline.png](evidence/art-raw-copybits-shufflepuck-inline.png),
> [evidence/art-raw-copybits-shufflepuck-preview.png](evidence/art-raw-copybits-shufflepuck-preview.png).
> What shipped — see "Resolution" at the bottom. The original investigation
> follows for the record.

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

---

## Resolution — CopyBits of a raw 1-bit bitmap (fix #1)

1-bit art now ships as a **raw bitmap sidecar** the launcher blits with
`CopyBits`, never touching the PICT/`DrawPicture` opcode interpreter that faults
Snow. Colour depths (8/16-bit, untested headless, where `DrawPicture` is fine)
still use PICT.

**Encoder (`tools/atrium-tool`).** New `pict::run_raw1` + `atrium pict --raw`.
`atrium image` writes the 1-bit variant as `<id>.1.raw` (Mac type `ABMP`) instead
of `<id>.1.pict`; colour variants stay `<id>.<d>.pict`. The catalog `image` field
is unchanged (still the base path; depth/extension resolved on-device).

Format (big-endian, must match `art.c`): `"AB"`, `u16 ver=1`, `u16 w`, `u16 h`,
`u16 rowBytes` (even), `u16 depth=1`, then `rowBytes*h` MSB-first rows (set bit =
black, matching the PICT 1-bit index). 12-byte header. Reuses the existing
1-bit `quantize` (Bayer dither) + even-`rowBytes` padding.

**Launcher (`src/art.{c,h}`).** `Art` is now an opaque object holding *either* a
`PicHandle` (→ `DrawPicture`) *or* a raw bitmap buffer (→ `CopyBits`). `art_load`
picks by extension (`.raw` vs `.pict`); `art_draw_fit` builds an old-style
`BitMap {baseAddr = buf+12, rowBytes, bounds}` and `CopyBits`es it (aspect-fit,
`srcCopy`, fore=black/back=white) into the current port — the same port the
renderer already blits its off-screen GWorld to every frame. The destination
`BitMap*` is derived from the current port (its PixMap when the rowBytes high bit
marks a colour port/GWorld, else its `portBits`), so it works in both the
off-screen and direct-draw paths.

**UI (`src/ui.{c,h}`).** `previewPic`/`listArt` are now `Art *`. `load_item_art`
prefers `<base>.1.raw` over `<base>.1.pict` for the 1-bit candidate (and accepts
an explicit `.raw` path), so old PICT-only images still load.

**Why this was the right call.** `CopyBits` is the one blit that has never
crashed Snow here (it composites every frame), and the 68kmla "Think Pascal"
thread reaches the same conclusion independently (offscreen GWorld + `CopyBits`
is the reliable image path). The fix sidesteps the data-dependent `DrawPicture`
fault rather than chasing the exact opcode that trips Snow.

**Verified.** Host: `atrium` 37 tests + launcher core 49 checks green. Snow
(System 7.1, Mac II, 1-bit): typing `s` → Shufflepuck renders inline; `P` →
full-screen preview renders; emulator runs the full 2900M cycles with no
illegal-instruction bomb (vs. the old bomb at ~2520M). Beyond Dark Castle / Dark
Castle / etc. still render (now via the `.raw` path too).

*Follow-up (non-blocking):* a single-depth-1 `atrium image` build (no
`art_depths`) emits `<id>.raw` with an explicit catalog path — covered. The
colour `DrawPicture` path is unchanged and still only exercised once a colour
depth can be set in the headless harness (see docs/13 §5).
