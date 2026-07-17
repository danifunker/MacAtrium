# 40 — Hardware compatibility: OS detection & per-title gating

How MacAtrium reasons about **what a machine can boot** (the System-Folder
chooser) and **what a title can run on** (the per-game launch gate). Both read the
one startup probe in [`src/env.c`](../src/env.c) (`env_probe`, [docs/03](03-architecture.md));
neither hardcodes a machine. This doc is the spec the code comments point at.

The human-facing hardware/OS map is [docs/38](38-compatibility-matrix.md); the
machine data is [`data/os-tiers.json`](../data/os-tiers.json) +
[`data/models.jsonl`](../data/models.jsonl); the per-title facets are
[`data/compatibility.jsonl`](../data/compatibility.jsonl).

## 1. OS detection is CPU-tier-based, not model-based

The highest System a Mac can boot is a function of its **CPU/ROM generation, not
its model**. `env_probe` reads the *native* CPU (`gestaltSysArchitecture` +
`gestaltNativeCPUtype`, falling back to `gestaltProcessorType`) and maps it to one
of five tiers, each carrying an OS **ceiling** (`maxOSbcd`) and **floor**
(`minOSbcd`), baked from [`os-tiers.json`](../data/os-tiers.json):

| Tier (`TIER_*`) | CPU | OS floor | OS ceiling |
|---|---|---|---|
| `TIER_68K_EARLY` | 68000 / 68020 | 6.0.4 | 7.5.5 |
| `TIER_68030` | 68030 | 6.0.4 | 7.6.1 |
| `TIER_68040` | 68040 / 68LC040 | 7.1 | 8.1 |
| `TIER_PPC_OLDWORLD` | 601 / 603 / 604 | 7.1.2 | 9.1 |
| `TIER_PPC_NEWWORLD` | G3 / G4 | 8.1 | 9.2.2 |

**Why CPU and not `gestaltMachineType` (the model/box ID)?** Two hard facts in our
own data:

- **New-World PowerPC Macs report no machine ID** — iMac/iBook/G3/G4 share a
  generic `machineType` 406 ([compat-matrix](../tools/compat-matrix/README.md)),
  so a model-ID table can't identify them at all. The CPU probe works everywhere.
- **Board-family Gestalt IDs collide** — one `gestaltID` maps to up to six models
  in `models.jsonl`. The ID alone can't pick a single model.

So the boot decision collapses all 155 models to their CPU tier. `models.jsonl` is
a reference/provenance artifact (and feeds the per-model floor below); the launcher
never reads it at runtime.

### 1a. Per-model OS-floor refinement

The tier *ceiling* is exact, but the tier *floor* is too permissive for some
68020/68030 Macs: a Color Classic / LC III / Mac IIvx (all 68030, tier floor
6.0.4) actually needs **System 7.1**. So `env_probe` refines `minOSbcd` upward via
a small `kModelMinOS[]` table in [`env.c`](../src/env.c) — every machine (by
`gestaltMachineType`) whose minimum System exceeds its tier floor, baked from
`models.jsonl` (68K models with `minSystem >= 7.1`, collapsed per `gestaltID` to
the **most permissive** floor so shared board IDs never over-grey). New-World Macs
(no ID) fall back to the tier floor — moot, since they boot 8.1+ anyway.

### 1b. The System-Folder chooser gate

`osc_bootable` ([main.c](../src/main.c)) greys any candidate System Folder outside
`[minOSbcd, maxOSbcd]`. The focused-item status line explains it — *"Won't boot on
this Mac - needs System 7.1"* (too old) or *"- max System 7.5.5"* (too new). The
running (blessed) System, an unreadable version, and a failed tier probe are always
allowed, so a folder is never falsely greyed.

## 2. Per-title hardware gating (the launch gate)

The same probe fields let the launcher stop a title that needs **more than this
Mac** — the "don't run Marathon 2 on a Mac LC" guard. Requirements are per-title
**facets** in [`compatibility.jsonl`](../data/compatibility.jsonl):

| Facet | Meaning | Probe compared against | Runtime behaviour |
|---|---|---|---|
| `minCPU` | oldest CPU generation it runs on (`"68040"`) | `gEnv.cpuGen` | flag if this Mac is older |
| `maxCPU` | newest generation it tolerates (breaks on faster) | `gEnv.cpuGen` | flag if this Mac is newer |
| `fpu` | needs a hardware FPU | `gEnv.hasFPU` (`gestaltFPUType`) | flag if absent (catches a 68LC040) |
| `minDepth` | min screen bpp (e.g. 8 = 256 colours) | `gEnv.pixelSize` / the display | **raise** the screen to it, or flag if unreachable |
| `maxDepth` | max screen bpp tolerated | current depth | **lower** the screen to it (existing) |
| `minMem` | min machine RAM in whole **MB** (e.g. `8`) | `gEnv.ramKB` (`gestaltPhysicalRAMSize`) | flag if the Mac has less |

### 2a. The CPU-generation table

`minCPU` and `maxCPU` are **two bounds on one ordered table** — every Mac CPU
generation, 68000 → G4 — defined in [`src/cpu.h`](../src/cpu.h)/[`cpu.c`](../src/cpu.c)
and mirrored as `CPU_GENS` in [`catalog.rs`](../tools/atrium-tool/src/catalog.rs):

| | | | | | | | | |
|---|---|---|---|---|---|---|---|---|
| `68000` | `68020` | `68030` | `68040` | `601` | `603` | `604` | `G3` | `G4` |

**The order is the comparison.** `env_probe` resolves the host into the same table
(`gEnv.cpuGen`), so each bound is a single compare — `cpuGen < minCPU` (too old) or
`cpuGen > maxCPU` (too new) — with no arithmetic and no special cases:

```c
if (it->minCPU != CPU_GEN_NONE && e->cpuGen < it->minCPU)  /* "Needs a 68040." */
if (it->maxCPU != CPU_GEN_NONE && e->cpuGen > it->maxCPU)  /* "…made for a 68030 or older." */
```

Three properties worth keeping:

- **The catalog carries canonical *names*, not indices** (`"minCPU":"68040"`), so a
  catalog dump is readable and the two tables need only agree on the name list and
  its order — never on index values. `minOS`/`maxOS` follow the same rule: the catalog
  carries the dotted version (`"7.1"`, `"9.2.2"`) and the launcher parses it into the
  gestalt BCD it compares — readable on disk, cheap at runtime.
- **The tool normalizes aliases** to canonical names (`"040"`, `"68LC040"` → `68040`;
  `"PPC"` → `601`, the PowerPC floor), so the dataset can be authored loosely and the
  launcher only ever parses one spelling.
- **`CPU_GEN_NONE` is 0**, so a zero-initialised `CatItem` means "no CPU bound" — the
  same absent-by-default rule as every other facet. Real generations start at 1.

This is deliberately *not* the `TIER_*` table (§1): tiers lump 68000+68020 together
because they share an OS ceiling, which is right for the chooser and wrong here. The
probe resolves the fine generation and **derives** the tier from it, so there is still
a single CPU probe.

### 2b. The gate itself

`do_launch` ([main.c](../src/main.c)) builds a reason with `compat_reason` and, if
the Mac is under-spec, shows a two-button confirm **before** it inserts any disc,
changes depth, or launches:

```
Marathon 2
Needs a 68040 and 8 MB of memory.
It may not run on this Mac. Launch anyway?          [Launch anyway]  [Cancel]
```

**Cancel is the default** (ring + Return/Esc/Cmd-.) — a "may crash" prompt must not
bomb an LC because someone held Return; Proceed is a deliberate click. This mirrors
the informative-but-not-hard OS-chooser greying (§1b): the user is told, and stays
in control.

`minDepth` is the exception that self-heals: if the screen *can* reach the floor,
the launcher raises the depth (the inverse of `maxDepth`'s cap — see the depth block
in `do_launch`) and restores it on quit, no prompt. Only a floor the screen
physically can't reach (a B&W Mac, or a device topping out below it) becomes a
flag. CPU/FPU/RAM can't be fixed at runtime, so those always prompt.

## 3. The data path

```
compatibility.jsonl (minCPU/fpu/minDepth/minMem)
  → atrium merge  (overlay wins; copies every facet key)      merge.rs
  → SourceItem    (serde)                                     catalog.rs
  → OutItem       (minCPU string→tier int; others pass through)
  → catalog JSONL (on-volume, MacRoman/CR)                    catalog.rs / docs/06
  → CatItem       (parsed)                                    catalog.c
  → compat_reason / depth gate                                main.c
```

**To add a title's requirements:** edit `compatibility.jsonl` (hand-verified wins),
then rebuild — `atrium merge` folds the facet over the library and the catalog
generator emits it. Requirements come from the title's documented specs;
Macintosh Garden's `architecture`/`system` fields (in the scrape metadata)
distinguish 68k-vs-PPC and the OS list but **not** the CPU minimum *within* 68k,
so `minCPU`/`fpu` are hand-set.

## Cross-references

- **Hardware/OS map (human-facing):** [38-compatibility-matrix.md](38-compatibility-matrix.md)
- **Tier + model data:** [os-tiers.json](../data/os-tiers.json) · [models.jsonl](../data/models.jsonl) · [compat-matrix](../tools/compat-matrix/README.md)
- **Per-title facets:** [compatibility.jsonl](../data/compatibility.jsonl) · [data/README.md](../data/README.md)
- **Runtime probe & gate:** [src/env.c](../src/env.c) · [src/main.c](../src/main.c) · [src/display.c](../src/display.c)
- **Catalog plumbing:** [catalog.rs](../tools/atrium-tool/src/catalog.rs) · [src/catalog.c](../src/catalog.c) · [docs/06](06-content-pipeline.md)
- **Colour depth & backends:** [15-settings-and-color-depth.md](15-settings-and-color-depth.md)
