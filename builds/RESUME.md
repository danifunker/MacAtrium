# Resume — per-config launcher `SIZE` partition  ✅ DONE + Snow-verified

**Goal (achieved 2026-06-26):** make the MacAtrium launcher's preferred/minimum
memory partition (`SIZE` (-1) resource) **per-config and much smaller** than the
old fixed 2 MB / 1 MB — a Mac Plus/SE B&W build should be a few hundred KB.

A compact 6.0.8 B&W appliance now ships **512 KB preferred / 384 KB minimum**
(4×/2.7× smaller than before) and was verified end-to-end in Snow: boots → catalog
loads → navigate (cover load/dispose churn) → launches a game, all inside that
partition.

## What shipped this session

### 1. Cut the real runtime peak — `gCat` is dynamic
- `Catalog.items` is now a heap pointer (`CatItem *items; int cap;`) instead of an
  inline `CatItem items[256]` (~390 KB of static, allocated even for a 3-item lib).
- `src/catalog.c` split into pure-C, allocation-free `catalog_count_lines()` +
  `catalog_parse_into(buf,len,items,cap,*dropped)` (shared `next_line` walker).
- `src/main.c load_catalog()` counts lines → `NewPtr((Size)min(count,256)*sizeof
  CatItem)` → `catalog_parse_into`. The catalog lives for the session (never freed).
- `tests/host_test.c` keeps a local malloc-based `catalog_parse` shim so its 67
  checks are unchanged. **Host tests 67/67 pass; launcher builds on Retro68.**

### 2. Per-config `SIZE` patcher (`atrium-tool`)
- New module `tools/atrium-tool/src/size_rsrc.rs`: `patch_app_mem(bytes,
  pref_bytes, min_bytes)` walks the launcher MacBinary → resource fork → resource
  map → `'SIZE'` id -1, and overwrites the two u32s at body+2 / body+6, leaving the
  flags word (0x48c0: suspend/resume, 32-bit, high-level-event) intact. 2 unit tests.
- Config field `app_mem_kb: Option<[u32;2]>` (`[preferred,minimum]` KB) +
  `BuildConfig::effective_app_mem()` (explicit-only; min clamped ≤ pref).
  **Deliberately not auto-derived from B&W art** — a B&W-art build can still run on
  a colour screen and pay for the off-screen GWorld, so small SIZE is opted into.
- `image.rs apply_app_mem()` patches the launcher bytes in BOTH install paths
  (finder_replace + Startup-Items) before injection; logs e.g.
  `[size] launcher partition -> 512 KB / 384 KB (was 2048 KB / 1024 KB)`.

### 3. Compact profile + GUI
- `config::COMPACT_APP_MEM_KB = (512, 384)` KB (verified value).
- mgmt-ui: the "Mac Plus / SE (B&W only)" toggle auto-applies the compact default;
  new "launcher RAM KB" pref/min fields under Advanced for explicit control.
- `builds/6.0.8-bw.json` — compact B&W appliance (6.0.8 finder_replace,
  `art_depths:["1"]`, `app_mem_kb:[512,384]`, 120 MB disk).

## Why 384 KB min holds
`render.c`: `useOffscreen = hasColorQD && (sysVers >= 0x0700)`. A **6.0.8 build
never allocates the off-screen GWorld** (it draws direct), and a 1-bit-art build
loads only small `.raw` covers (~44 KB). So the real peak is a few hundred KB.

## Verification (Snow, Mac II rig)
```sh
# 1. build the compact image (note the [size] log line)
tools/atrium-tool/target/release/atrium image --config builds/6.0.8-bw.json
# 2. boot + navigate + launch
H=~/repos/snow/target/release/macatrium_harness
ROM=~/repos/lbmactwo_MiSTer/releases/MacIIFDHD.rom ; MDC=/tmp/mdc/3410868.bin
cp ~/MacAtrium-608-bw-working.hda /tmp/bw.hda ; rm -rf /tmp/bwout ; mkdir /tmp/bwout
"$H" "$ROM" "$MDC" /tmp/bw.hda /tmp/bwout 4200000000 --snap-every 200000000 --wall-secs 360 \
  --keys "1500000000:right;1900000000:right;2500000000:return"   # carousel: BDC(0) DC(1) PoP(2)
```
Confirmed: carousel renders, navigation works, PoP launches and runs in colour.
Probe the injected launcher's SIZE with the python snippet in the session history.

## Build commands
```sh
cd ~/repos/MacAtrium && export RETRO68=/home/dani/repos/Retro68-build && cmake --build build
cargo build --release --manifest-path tools/atrium-tool/Cargo.toml
cargo build --release --manifest-path tools/macatrium-mgmt-ui/Cargo.toml
( cd tests && make test )   # 67/67
```

## Colour builds — measured + shrunk (2026-06-26)
Added a **`MEM_DEBUG` on-screen memory probe** (`src/mem.c`/`mem.h`; CMake
`option(MEM_DEBUG)`, OFF in production = no-op; hooked in `main.c`'s loop). It
paints `GetProcessInformation` partition size/free (Sys7) + `FreeMem`/`MaxBlock` +
`TempFreeMem` low-water top-left, readable straight off a Snow frame (no
disk-write-persistence needed). Build it with:
```sh
cmake -S . -B build -DMEM_DEBUG=ON && cmake --build build   # then OFF to ship
```
**Result (7.1 colour, 8-bit screen): partition PEAK used = 472 KB**, `tmp` free
barely moved (~4.7 MB) — the off-screen GWorld is allocated `useTempMem`-first, so
its ~300 KB lives in **system temp memory, not the SIZE partition**. So 7.1/7.5.5
are set to `app_mem_kb:[1024,768]` (headroom for a bigger library + the
GWorld-falls-to-heap case → degrades to direct draw). Verified: patched 7.1 booted,
overlay `part=1040K used=472K`, colour covers rendered, PoP launched. (`processSize
≈ preferred + ~16 KB`.)

## GWorld now supports higher depth (`render.c render_begin`)
Was hard-capped at 8-bit; now composites at the **screen's own depth** (full-fidelity
deep art + gradients), stepping down a ladder `[screenDepth, 8]` then to direct draw,
each tried temp-mem then heap. Memory-safe (deep buffer = temp mem) and no 8-bit
regression (ladder=[8] at an 8-bit screen = the old path; verified). To SEE >8-bit
fidelity you must also bake deeper art (`art_depths` incl. 16/24) and boot a deep
screen — then re-measure SIZE, since the deeper `.pict` Handle DOES land in the
partition (the 8-bit cover Handle is ~318 KB).

## Remaining / optional follow-ups (lower priority)
- **Bake + verify 16/24-bit art on a deep screen** to exercise the new GWorld depth;
  re-measure colour SIZE then (deeper art Handle grows the partition).
- **Bound `rowIcon[MAX_ITEMS]`** (`ui.c`) to a window around the selection — for a
  big library it caches up to 256 row icons (grows the colour peak past 472 KB);
  fine for the small libs today.
- **Compact Mac Plus/SE boot in Snow** still blocked (built-in video → 0 frames in
  the harness; see memory `snow-harness-verify-gotchas`). Verified on the Mac II rig
  instead; the SIZE partition is honoured regardless of machine.
- **Blank 1-bit cover on a colour 8-bit screen** (PoP preview pane was near-blank,
  icon fine) — pre-existing `art.c` raw-CopyBits issue, unrelated to memory.

## Memory
Read: `shrink-size-partition-per-config` (now DONE), `build-tool-mvc-architecture`,
`system-608-boot-shell`, `overrides-db-maxdepth`, `snow-harness-verify-gotchas`,
`workflow-verify-in-emulator`, `color-depth-in-slot-pram`.
