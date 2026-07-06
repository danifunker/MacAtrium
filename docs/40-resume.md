# 40 — Resume: OS-compatibility data model + 6.0.4 floor; swap-warning designed

Paste into a fresh session **on the WSL box** to continue. **State: the hardware/OS
compatibility work is now a data model the launcher can consume.** Built this session
(Mac side — data/docs/tooling only, **no C compiled**): a Gestalt-verified 155-model →
System table, a 5-tier CPU→OS-range table, a browsable matrix, and the pipeline that
makes them. The project floor moved **6.0.8 → 6.0.4**. Nothing is wired into the C
launcher yet — that's the remaining work, specced below with code.

## 0. Environment (don't re-learn) — full recipe in docs/39 §0
- 68k C launcher, **Retro68 in WSL** at `~/repos/MacAtrium`. Build:
  `export RETRO68=~/repos/Retro68-build && cd ~/repos/MacAtrium && cmake --build build -j`
  → `build/MacAtrium.bin`. Snow harness for headless verify (docs/39 §0). Commit to main.
- **This session ran on the Mac** (no Retro68 here) — so every C snippet below is
  **written but UNBUILT**. First step on WSL: build + Snow-verify as you wire each piece.

## 1. DONE this session (committed)
- **`data/models.jsonl`** — 155 Macs (79 68K / 76 PPC) from LowEndMac profiles. Fields:
  `model, gestaltID, modelNumber` (Apple M#), `codeName, arch, group, introduced,
  minSystem, maxOS` (dotted), `minKey/maxKey` (= major*10000+minor*100+bug), `inEnvelope`.
  Gestalt IDs **verified against Apple Gestalt.h**: 4 corrected (IIsi 10→**18**, PB190/190cs
  122→**85**, Duo 2300c 118→**124**), 12 filled. New-World PPC (iMac/iBook/G3/G4) report no
  numeric machineType (all share generic **406**) → `gestaltID` null there.
- **`data/os-tiers.json`** — the launcher-consumable table. **5 tiers**, each with min/max
  OS as dotted + BCD (matching `gestaltSystemVersion`, `0xMMmb`). Envelope floor **6.0.4**.
- **`docs/models-matrix.html`** — self-contained sortable/filterable table (open in a browser).
- **`tools/compat-matrix/`** — reproducible pipeline: `scrape/*.jsonl` (raw per-family
  scrapes, provenance), `merge.py` (dedup + corrections + Gestalt.h verify), `gen_artifact.py`
  (HTML), `build.sh` (regen → copies into `data/` + `docs/`). See its README.
- **Floor 6.0.8 → 6.0.4** across the data. 6.0.4 is the honest floor: the **Gestalt Manager**
  first shipped in System 6.0.4, and `env.c`'s whole probe is Gestalt-based. Below 6.0.4
  (6.0.2/6.0.3, System 4/5) would need a `SysEnvirons` fallback — deferred.

## 2. Key decisions / findings (the "why")
- **OS support does not vary by model — it clusters by CPU/ROM into 5 tiers.** maxOS is a
  clean function of CPU class (the scrape confirms: every 68K ceiling is 68K-only, every PPC
  ceiling PPC-only). See the tier table below.
- **minOS is NOT tier-clean** (the 68030 tier spans 6.0.4 IIci → 7.1 Color Classic) → the true
  per-machine min stays in `models.jsonl`; a tier carries only the **ceiling** + the single
  project floor (6.0.4).
- **Detect the tier by CPU, not machine ID.** `gestaltNativeCPUtype` returns the real chip
  **even under 68k emulation**, so it identifies the 45 New-World Macs that have no machineType.
  Do NOT use `gestaltProcessorType` on PPC (it returns the emulated 68LC040). Coarse first cut:
  `gestaltSysArchitecture` (68k vs PPC).
- **New-World is a CPU *set*, not a threshold**: G3 = `0x0108` sorts *below* 604e = `0x0109`.
  Old-World set {601,603,603e,603ev,604,604e,604ev} → 9.1; New-World {750/G3, G4} → 9.2.2.

Tier table (from `data/os-tiers.json`):

| tier id | detect (native CPU) | minOS | maxOS (BCD) |
|---|---|---|---|
| `m68k_early`   | 68000, 68020 | 6.0.4 | 7.5.5 (0x0755) |
| `m68030`       | 68030 | 6.0.4 | 7.6.1 (0x0761) |
| `m68040`       | 68040/LC040 | 7.1 | 8.1 (0x0810) |
| `ppc_oldworld` | 601/603/604 | 7.1.2 | 9.1 (0x0910) |
| `ppc_newworld` | G3(750)/G4 | 8.1 | 9.2.2 (0x0922) |

## 3. REMAINING (do on WSL) — all specced
1. **`env.c`: add a CPU-tier probe** (needs `gestaltSysArchitecture 'sysa'`,
   `gestaltNativeCPUtype 'cput'`, and the `gestaltCPU*` constants in `mac_compat.h`):
   ```c
   enum { TIER_68K_EARLY, TIER_68030, TIER_68040, TIER_PPC_OLDWORLD, TIER_PPC_NEWWORLD };
   /* Env gains: int tier; long maxOSbcd; */
   static const long kTierMaxBcd[] = { 0x0755, 0x0761, 0x0810, 0x0910, 0x0922 };

   long arch, cpu;
   if (Gestalt(gestaltSysArchitecture, &arch) == noErr && arch == gestaltPowerPC) {
       Gestalt(gestaltNativeCPUtype, &cpu);           /* real PPC chip, even under emulation */
       e->tier = (cpu == gestaltCPU750 || cpu >= gestaltCPUG4)
                 ? TIER_PPC_NEWWORLD : TIER_PPC_OLDWORLD;
   } else {                                           /* 68k: prefer cput, fall back to proc */
       int is030, is040;
       if (Gestalt(gestaltNativeCPUtype, &cpu) == noErr) {          /* cput: 030=3, 040=4 */
           is030 = (cpu == gestaltCPU68030); is040 = (cpu == gestaltCPU68040);
       } else { Gestalt(gestaltProcessorType, &cpu);                /* proc: 030=4, 040=5 */
           is030 = (cpu == gestalt68030);    is040 = (cpu == gestalt68040); }
       e->tier = is040 ? TIER_68040 : is030 ? TIER_68030 : TIER_68K_EARLY;
   }
   e->maxOSbcd = kTierMaxBcd[e->tier];
   ```
2. **Chooser compatibility gating** (docs/36 §"Compatibility gating"): in `run_os_chooser`,
   gray out (`HiliteControl(ctl, 255)`) any System Folder whose version `v` is outside
   `[0x0604, gEnv.maxOSbcd]`. That's the whole gate — `bootable = v >= 0x0604 && v <= gEnv.maxOSbcd`.
3. **Swap warning** (this session's ask; the user-facing half of docs/39 item #2). MacAtrium's
   creator is `'ATRM'`; on 7.x it (or its alias — aliases carry the target's creator) sits in
   the folder's `Startup Items`. Add `int macatriumReady` to `SysFolder`, set it in
   `bless_enumerate`, and in `run_os_chooser` show a ⚠ marker + status line — *"MacAtrium isn't
   installed in this System Folder — you'll boot to the Finder."* Warn, don't block. System 6
   folders warn by default (no Startup Items; that install path is Milestone 4).
   ```c
   /* bless.c — is MacAtrium set to auto-launch under System Folder `sysDir` (version v)? */
   static int macatrium_ready(short vref, long sysDir, long v)
   {
       CInfoPBRec pb; Str63 nm; long siDir; short i;
       if (v < 0x0700) return 0;                            /* System 6: no Startup Items */
       BlockMoveData("\pStartup Items", nm, 14);
       memset(&pb, 0, sizeof pb);
       pb.dirInfo.ioNamePtr = nm; pb.dirInfo.ioVRefNum = vref;
       pb.dirInfo.ioDrDirID = sysDir; pb.dirInfo.ioFDirIndex = 0;
       if (PBGetCatInfoSync(&pb) != noErr) return 0;
       if (!(pb.dirInfo.ioFlAttrib & ioDirMask)) return 0;   /* no Startup Items folder */
       siDir = pb.dirInfo.ioDrDirID;
       for (i = 1; i < 256; i++) {                           /* scan for an 'ATRM' file/alias */
           nm[0] = 0; memset(&pb, 0, sizeof pb);
           pb.hFileInfo.ioNamePtr = nm; pb.hFileInfo.ioVRefNum = vref;
           pb.hFileInfo.ioDirID = siDir; pb.hFileInfo.ioFDirIndex = i;
           if (PBGetCatInfoSync(&pb) != noErr) break;
           if (pb.hFileInfo.ioFlAttrib & ioDirMask) continue;
           if (pb.hFileInfo.ioFlFndrInfo.fdCreator == 'ATRM') return 1;
       }
       return 0;
   }
   ```
4. **Docs prose → 6.0.4**: reconcile `docs/01` ("System 6.0.8 — hard requirement"),
   `docs/02` (compat table row), `docs/38` (envelope statements). The data is already 6.0.4.
5. **Decide bake vs runtime-read** for the tiers. Recommend **baking a 5-row compiled table**
   (it must work before any disk I/O, on a bare compact) — codegen a header from
   `data/os-tiers.json`, or hand-keep a small C table and validate it against the JSON.

## 4. Files & sources
- `data/models.jsonl` · `data/os-tiers.json` · `docs/models-matrix.html` · `tools/compat-matrix/`
- Browsable table built this session: https://claude.ai/code/artifact/aec85ae7-fd5f-4692-8927-d3f1e34eaa93
- Sources: LowEndMac model profiles; Apple **Gestalt.h** (machine + CPU constants); E-Maculation
  "Macintosh Gestalt IDs". Regenerate the data: `tools/compat-matrix/build.sh`.
