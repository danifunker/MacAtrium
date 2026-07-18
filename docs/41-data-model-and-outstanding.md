# 41 — Data model: consolidation decisions + outstanding work

A working handoff: the decisions taken about the JSON/JSONL sprawl, and the
outstanding feature work (A/B/D) they shape. Read with [docs/06](06-content-pipeline.md)
(content pipeline), [data/README.md](../data/README.md) (data model), and
[docs/40](40-hardware-compatibility-gating.md) (the compat facets these build on).

## The file landscape (what each actually is)

Three kinds of file — only the **hand** ones are candidates for consolidation:

| File | Kind | Job |
|---|---|---|
| MG mirror (`/mnt/c/Temp/macgarden-archive/…`) | external | source of MG titles: `index.jsonl` (catalogue) → `games/<nid>/info.json` `files[]` (download options) → `downloads/` cache |
| `data/library.jsonl` | **generated** | identity from a MacPack scan (`library scan`) — a re-scan clobbers hand edits, so it can never merge with an overlay |
| `data/curated.jsonl` | **hand** | per-title overlay: identity corrections + sourcing (`harvest_src`) + whole records for non-MacPack titles |
| `data/compatibility.jsonl` | **hand** | per-title overlay: requirement facets (`color`/`minCPU`/`maxCPU`/`minOS`/`maxOS`/`minDepth`/`maxDepth`/`minMem`/`fpu`/`arch`) |
| `data/collections/*.json` | hand | the **selection** — id list (+ `recommended`) that names an image |
| `data/targets.json` | hand | named machine profiles (`base_os` + `art_depths` + `app_mem_kb` + disk size) |
| `build-*.json` (BuildConfig) | hand | the **recipe** — paths + machine profile (today inlined) + a `collection` pointer |
| `os-tiers` · `models` · `taxonomy` · `categories` · `donors` · `dependencies` · `templates` | reference | distinct infra jobs — NOT per-title info; keep as-is |

**`build-*.json` is the recipe, `collections/*.json` is the ingredient list** — the
recipe already points at the list by name (`"collection": "Mac68KColorGames_v1"`),
it does not duplicate ids. These two do **not** merge.

## Decisions

**D1 — Merge `curated.jsonl` + `compatibility.jsonl` into ONE per-title overlay.**
They are mechanically identical (hand-authored, id-keyed, overlay-wins over the
generated `library.jsonl`); the split is only semantic (sourcing/identity vs
requirements). One file = one place to edit everything known about a title, and
the natural home for `mg.files` (decision D3) and any future per-title field.
`library split` seeds facets into the unified file instead of `compatibility.jsonl`.
**Deliberate, tested refactor** — touches `merge.rs`, `library split`
([library.rs](../tools/atrium-tool/src/library.rs)), the build's merge order, and
every consumer. Do it as its own change with the host/cargo suites green, not
folded into a feature.

**D2 — `BuildConfig` references a *named target* instead of inlining the machine
profile.** Today a build config inlines `base_os` + `art_depths` + `app_mem_kb` —
which *are* a `targets.json` entry. Add `"target": "<name>"` and apply it (the GUI
already applies a Target via [targets.rs](../tools/atrium-tool/src/targets.rs)
`apply_to`; wire the same into the CLI `BuildConfig`). Removes the duplication and
is the hook for **B** (system-class picker): "B&W compacts / 68020-030 / Quadras
(68040) / early PPC" become named targets a build selects.

**D3 — `mg.files` (the MG download pick) lives in the per-title overlay** (the D1
merged file; `curated.jsonl` until then, alongside `harvest_src` — it is *sourcing*,
not a requirement, so **not** `compatibility.jsonl` as raw facets, and **never** in
the scraped `info.json`, which regenerates). Shape: `"mg": {"nid": 15475, "files":
["SimCity_2000_1.2.hqx"]}` — `files` a LIST (some titles need several disks).

**Keep separate:** the **generated** (`library.jsonl`), the **selection**
(`collections/*.json`), the **profiles** (`targets.json`), and the **reference
tables**. The end state is: MG archive → generated identity → one hand overlay →
named selection → named target → thin recipe.

## Outstanding feature work

**A — MG file-pick (durable, data-driven).** `atrium fetch --file` (CLI override)
is DONE (`d5324f8`). Remaining, per D3:
1. `mg: {nid, files}` in the per-title overlay.
2. `fetch` honors it: `match_dataset` ([fetch.rs](../tools/atrium-tool/src/fetch.rs):128)
   returns only `Vec<nid>` today → change to `(nid, files)` pairs so `run()` passes
   the picks to `pick_file`; also make the **default** `pick_file` smarter
   (deprioritise updater/demo/readme, prefer newest full version) so "auto" is rarely
   wrong.
3. GUI dropdown (mgmt-ui `mg` tab, `run_mg_download` ~769): read `files[]` from the
   info.json, "Auto" default + explicit select, write `mg.files` into the curated
   stub it already creates.

**B — variant-group resolver + system-class targets.** Resolver CORE is built +
unit-tested (`47df1b2`). Remaining: a `group:"<key>"` field relates editions of one
title; the resolver filters a group to editions whose facet envelope admits the
TARGET, picks the best (fit + rank + version), collapses the group to one (also
de-dups the catalog). Wire into [image.rs](../tools/atrium-tool/src/image.rs) /
`add_to_disk`. Depends on **D2** (targets carry the CPU profile the resolver matches).

**D — SimCity 2000 + colour classic SimCity (content-blocked).** MG hosts only
installer disks for SC2000 v1.2 → **user is creating the `.mar`**
([[manual-captures]]). When it lands: import → overlay record (id `simcity-2000`,
`harvest_src` → reservoir) + facets (`color:true, minDepth:8, minMem, minOS:"7.0"`;
FAT 68k+PPC) + collection add + `atrium add --config` (in place — see CLAUDE.md
`fetch → add`). SC2000 is its own title, not a variant of classic SimCity. Colour
CLASSIC SimCity (to replace the baked B&W one) needs colour files (1.4c) we don't
have — would be `group:"simcity"` with the resolver picking colour on a colour target.

## Loose ends
- **Push** ~16 unpushed local commits (Windows `gh`, HTTPS).
- **Enable auto-sizing**: set `disk_size_mb` to `null` in the build config (still
  `2000`, an explicit override that beats auto; auto DONE+verified `5e5097f`).
- **"215 titles" oddity**: MacAtrium Status shows "11 categories, 215 titles" on a
  95-title disk — per-category counts double-count multi-category titles. Pre-existing.
- Optional: OS/RAM facet sweep for the remaining borderline titles.

> Verify file/line citations against the code before acting — this is a
> point-in-time handoff and a few will drift.
