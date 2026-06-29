# 26 — Resume: beta1 tagged (3 finals, screenshot covers, curated Recommended)

Supersedes docs/25. **State: `git tag beta1` is cut. All three final disks are
rebuilt with the screenshot-primary launcher + the curated Recommended list,
fsck-clean. The Quadra edition is q800-boot-verified showing the new behavior.**
The user will review the front end next.

## The three beta1 finals (artifacts on /home/dani, not in the repo)

| Final | OS / art | titles | file / used / free |
|---|---|---|---|
| `MacAtrium-final-bw-608.hda` | 6.0.8, 1-bit | 1218 | 1.4 GB / 1.2 GiB / 132 MB |
| `MacAtrium-final-color-71.hda` | 7.1, 1+8-bit @384px | 1218 | 1.6 GB / 1.3 GiB / 215 MB |
| `MacAtrium-final-quadra-755.hda` | 7.5.5, 1+8+24-bit @448px | 1218 | 2.0 GB / 1.6 GiB / 318 MB |

Apps are ~1.19 GB on every disk (the shared bulk; see the `[footprint]` build
lines — `apps N MB · covers + art N MB`, measured via `show fs-info` deltas since
rb-cli `ls` can't size resource forks). Rebuild from the per-edition configs;
build to a `.new` temp → fsck → swap. See memory `beta1-finals-and-recommendations`.

## What shipped in beta1

- **Screenshot-primary launcher** (`5f6c3b4`, `src/ui.c`): cover defaults to the
  gameplay screenshot; box art on the `P` key ("P box art" footer). Artwork setting
  still flips the main pane. q800-verified (Bolo gameplay cover, footer correct).
- **Curated Recommended** (`7693fa7`): 23 community-matched titles added (41 total
  in `data/categories.jsonl` + `taxonomy.json` seed). **Not gated on MacPack** —
  only ~12/41 install on the Quadra now; the rest persist and auto-appear when a
  donor is added. Full list incl. ~80 wishlist titles in `data/recommendations.md`.
- **README.md** with a screenshots gallery (`docs/screenshots/`, captured from the
  q800). **Footprint preflight** (`d7ca658`): apps vs covers/art as two real values.
- **rbcli hardening** (`5588f5b`): resolve the rb-cli binary deterministically +
  log the resolved path/version (killed the stale-$PATH-binary trap — see docs/25).

## NEXT (in order)

1. **Front-end review** (user, tomorrow) — polish the launcher UI.
2. **24-bit "Millions" at full scale on the q800** — boot the Quadra final, push
   Settings→Color Depth→Millions, confirm the `[3584,3072]` partition holds the
   ~1.4 MB `.24.pict` covers without a Type-28 (still only computed). Harness: a
   SHORT out dir (AF_UNIX limit), `ret` launches, `esc` menu.
3. Optionally re-capture a clean **box-art-overlay** screenshot (the `P` feature) —
   this session's capture raced the keypress; the cover screenshots are good.
4. Backlog: **multi-volume library** (docs/23) for >2 GB / full-res art; **MacPack↔
   MacGarden corroboration** (only ~285–537/1489 have donors — would install far more
   of the Recommended titles); add library **stub records for the wishlist** titles
   so they're promotable.
5. Cosmetic: 283 art `<id>.shot.24.pict` names exceed HFS 31 chars → silently
   skipped; shorten art ids on write.

## Memory to read
`beta1-finals-and-recommendations`, `qemu-q800-harness`,
`rbcli-hfs-catalog-btree-scaling`, `macpack-vs-macgarden-corroboration`,
`color-art-memory-budget`, `build-and-snow-are-local`, `commit-directly-to-main`,
`workflow-verify-in-emulator`.
