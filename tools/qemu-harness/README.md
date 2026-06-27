# QEMU Quadra 800 harness

Boots a built MacAtrium disk on QEMU's **Quadra 800** (Motorola 68040) headless
and captures framebuffer screenshots over QMP. This is the verification path for
the **System 7.5.5** and **24-bit "Millions"** variants — Snow tops out at a
Mac II (68020, 8-bit), so it can't run the 68040 / deeper-colour builds.

Verified: a 7.5.5 build boots Mac OS, auto-launches MacAtrium, and responds to
keystrokes — all headless, screenshot-captured.

## One-time setup

**1. Quadra 800 ROM** (1 MiB, crc32 `4e70e3c0`). Extract from MAME's `macqd800`:

```sh
unzip -o ~/repos/mame/roms/macqd800.zip -d /tmp/q800rom
# -> /tmp/q800rom/f1acad13.rom   (the 4e70e3c0 one; f1a6f343.rom is the other half)
```

**2. QEMU** with the q800 machine — `qemu-system-m68k` ≥ 8.x
(`qemu-system-m68k -M q800 -machine help` should list it). Debian/Ubuntu:
`apt install qemu-system-m68k`.

**3. A bootable disk.** `atrium image` builds an APM disk; make sure it carries the
Apple SCSI driver + DDR + boot blocks so the q800 ROM boots it over SCSI:

```sh
rb-cli make-bootable /home/dani/MacAtrium-7.5.5-working.hda   # idempotent
```

## Run

```sh
python3 tools/qemu-harness/q800_harness.py \
    /tmp/q800rom/f1acad13.rom \
    /home/dani/MacAtrium-7.5.5-working.hda \
    out_dir \
    100 \
    --snap-every 15 \
    --keys "80:down;85:down;90:right"
```

- `100` = seconds to run. **Boot to the launcher takes ~45–60 s** in QEMU, so give
  it ≥ 70 s before expecting the MacAtrium screen.
- Screenshots land in `out_dir/snap_NNN_<sec>s.png` and `out_dir/final.png`
  (PNG via QMP `screendump`; falls back to PPM→PNG if a QEMU lacks the png arg).
- `--keys "T:key;..."` sends a QMP key at T seconds. Names are QMP QKeyCodes:
  `ret esc spc up down left right tab` and `a`–`z` / `0`–`9`. Carousel nav mirrors
  Snow: `left`/`right` move items, `up`/`down` change category, `ret` launches,
  `esc` opens the menu.
- `--ram 128` (default), `--qemu qemu-system-m68k`, `--snap-every 10` (default).
- The disk is opened with QEMU `-snapshot`, so the original image is never mutated.

## Notes / limits

- The q800's on-board `macfb` framebuffer is ~640×480. The screen depth is whatever
  Mac OS boots at (slot PRAM / the launcher's saved depth). To verify the **24-bit**
  path, build a `1,8,24` disk and have it come up at 24-bit ("Millions") — the
  launcher then loads the `.24.pict` covers (the ~1.4 MB ones the 3 MB partition is
  sized for; see memory `color-art-memory-budget`).
- Mirrors `tools/snow-harness/macatrium_harness` (Mac II / 8-bit) — use Snow for
  the B&W / colour variants and this for 7.5.5 / 24-bit.
