# compat-matrix — Mac model → System(OS) compatibility data

Builds the hardware/OS compatibility data the launcher uses to reason about which
System a given machine can run. See [`docs/40-resume.md`](../../docs/40-resume.md) for
the design and the remaining launcher wiring.

## Outputs (committed, canonical)
- **`../../data/models.jsonl`** — 155 Macs, one row each: `model, gestaltID, modelNumber,
  codeName, arch, group, introduced, minSystem, maxOS, minKey/maxKey, inEnvelope`.
- **`../../data/os-tiers.json`** — the 5-tier CPU→OS-range table the launcher consumes
  (hand-maintained; **not** generated here).
- **`../../docs/models-matrix.html`** — self-contained sortable/filterable table.

## Pipeline
- **`scrape/*.jsonl`** — raw per-family scrapes from LowEndMac model profiles (provenance;
  one file per model family, e.g. `A-compact-macii`, `E-ppc-desktops`).
- **`merge.py`** — merges the batches: dedups by model (board-family Gestalt IDs are shared,
  so the key is the name), normalizes versions, applies corrections, and emits
  `macatrium-models.jsonl` + a `.md`.
- **`gen_artifact.py`** — renders the merged data to `models-table.html`.
- **`build.sh`** — runs both and copies the results into `data/` and `docs/`.

```sh
./build.sh        # regenerate data/models.jsonl and docs/models-matrix.html
```

## Corrections applied over the raw scrape (see `merge.py`)
- Gestalt IDs **verified against Apple `Gestalt.h`**: IIsi 10→18, PowerBook 190/190cs 122→85,
  Duo 2300c 118→124; 12 nulls filled. New-World PPC (iMac/iBook/G3/G4) have no numeric
  machineType (share generic 406) → `gestaltID` null.
- 68030 PowerBook Duos capped at 7.6.1 (LowEndMac lists 8.1, but Mac OS 8 needs a 68040+).
- Centris/Quadra 660AV set to 60. Clone clock-speed SKUs collapsed to one row per board.

## Sources
LowEndMac model profiles · Apple **Gestalt.h** (machine + CPU constants) ·
E-Maculation "Macintosh Gestalt IDs".
