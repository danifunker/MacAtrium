# CLAUDE.md

Guidance for AI agents (and humans) working in this repo. **Read the code
guidelines first** — repo map, the `atrium` build pipeline, the content model,
conventions, and the expensive-to-learn invariants:

@docs/CODE_GUIDELINES.md

## Top rules (don't relearn these the hard way)

- **Code is the source of truth**, not prose. Some `.md`s have drifted; verify
  against the source and fix the doc when they disagree.
- A build **never mutates `data/*.jsonl`** (it works off a throwaway copy).
- **HFS bootable volume ≤ 2 GB**; on-volume names ≤ **31 chars**; the on-Mac
  catalog is **MacRoman + CR**, ≤ 256 items single-file / ≤ 128 per page.
- `atrium harvest` **renames folders to the picked app and re-picks the launch
  binary** — right for messy MacPack donors, wrong for already-installed content.
  Copy installed folders **verbatim with `rb-cli cp`** instead.
- **Never `git stash` for before/after image builds** — a stashed source + an
  un-rebuilt binary costs hours; use a separate output path or a worktree.
- **To add a title (incl. from Macintosh Garden), use the built-in `atrium`
  pipeline — never hand-download / `curl`.** The flow is **`fetch` → `add`**, see
  below. Raw `curl` of MG file URLs 403s; `atrium fetch` uses the mirror over
  native `ureq` with the right headers.

## Adding a title to an existing image (`fetch` → `add`)

`atrium image` is a from-scratch build; to grow an **already-built** disk use these
two verbs and don't rebuild:

1. **Load the data** — `atrium fetch` pulls a title from the MG static mirror,
   rb-cli-extracts it, and injects it into a reservoir image + a dataset stub:
   ```sh
   atrium fetch --nid <MG_nid> [--file <exact-name.hqx>] \
     --mg-archive <archive> --into <donor.hfv> --append-to data/curated.jsonl
   ```
   - `--file` steers past the auto-picker, which takes the **first archive** — so a
     `.sea.hqx` *updater* wins over a plain-`.hqx` *full game* (the SimCity 2000
     case: bare `fetch --nid 15475` grabs the 1.2 **updater**, not the game).
   - **Installer-disk titles** (SC2000 v1.2 ships Disk Copy images whose Disk 1 is an
     *installer*) extract to an installer, **not** a runnable app → run it in the
     emulator and capture the installed app as a `.mar`, then import that. A title
     that extracts straight to an `APPL` skips this.

2. **Add to the disk** — `atrium add --config <cfg>` harvests the new selection into
   the existing `.hda` **in place**: art baked, catalog records merged with the
   disk's current catalog, no base copy or launcher reinstall (image.rs `add_to_disk`).
   `cfg.out` = the existing disk, `cfg.selection` = the new ids, `base_os`/`art_depths`
   matching the disk's Target.

Author the title's `data/compatibility.jsonl` facets and its `group` / collection
membership **before** the `add`, so the regenerated catalog carries them.

## Operating in this environment (Windows 11 → WSL/Ubuntu)

The repo lives in WSL (`~/repos/MacAtrium`, i.e.
`\\wsl.localhost\Ubuntu-24.04\home\dani\repos\MacAtrium` from Windows). The Rust
tool builds anywhere; the **68k launcher, `rb-cli`, and the emulators run in
WSL**, driven from Windows.

- **Drive WSL binaries from the Windows shell** with
  `MSYS_NO_PATHCONV=1 wsl.exe bash -c '…'` (the Git Bash tool) or
  `wsl.exe bash -lc '…'` (PowerShell). Without `MSYS_NO_PATHCONV=1`, Git Bash
  rewrites `/mnt/...` and leading `/abs` paths.
- **Never use `$VAR` inside the `-c '…'` string** — it expands **empty** (even a
  var you assign on the previous line). Inline **literal paths**, or put the logic
  in a **script file** and run it (`wsl.exe bash /mnt/c/…/script.sh`); a file's
  own internal vars work normally.
- **`rb-cli`**: `/home/dani/.local/bin/rb-cli` (WSL). Always pass an **absolute**
  path so a stale `rb-cli` on `$PATH` can't shadow it.
- **git**: run from **Windows** (`gh` / HTTPS). WSL has no SSH key and hangs.
- **ROMs, donor disks, build artifacts, emulator assets** live under `/mnt/c`
  (Windows side), e.g. `C:\Temp\macatrium-build` — not the WSL home.

### Build & verify

```sh
# atrium (host tool) — pure Rust, builds anywhere
cd tools/atrium-tool && cargo build --release && cargo test

# 68k launcher (WSL/Retro68) + portable C-core tests
cmake --build ~/repos/MacAtrium/build -j        # -> build/MacAtrium.bin
cd tests && make && ./host_test
```

Headless verify in the Snow harness (Mac II, 8-bit) or QEMU `q800` (68040 /
colour); see [docs/44](docs/44-memory-and-art-modes.md) and the
`tools/*-harness/` READMEs.
