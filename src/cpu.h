/*
 * cpu.h — THE CPU-generation table (docs/40). One ordered list of every Mac CPU
 * generation, 68000 → PowerPC G4. Both per-title facets (`minCPU`/`maxCPU`) and
 * the runtime probe (env.c `cpuGen`) index THIS table, so a single `<` / `>`
 * answers "is this Mac new enough / too new" for any pair.
 *
 * Deliberately separate from the OS-compatibility tiers (TIER_* in env.h): those
 * lump 68000+68020 together because they share an OS ceiling (7.5.5), which is
 * right for the System-Folder chooser but wrong for per-title CPU requirements.
 * env.c derives the OS tier FROM the generation, so there is still one probe.
 *
 * Pure C (no Toolbox); host-tested.
 */
#ifndef MACATRIUM_CPU_H
#define MACATRIUM_CPU_H

/* Ordered by capability — the ordering IS the comparison. Keep the NAMES and their
 * ORDER in sync with CPU_GENS in tools/atrium-tool/src/catalog.rs (the facet
 * normalizer); the two need not agree on index values, because the catalog carries
 * canonical NAMES, not indices — only this side ever compares indices.
 *
 * CPU_GEN_NONE is 0 so a zero-initialised CatItem means "no CPU bound" — the same
 * absent-by-default rule as every other optional facet. A real generation is >= 1. */
enum {
    CPU_GEN_NONE = 0,   /* facet absent / generation unknown */
    CPU_68000,
    CPU_68020,
    CPU_68030,
    CPU_68040,        /* incl. 68LC040 — it lacks an FPU; see the `fpu` facet */
    CPU_PPC_601,
    CPU_PPC_603,
    CPU_PPC_604,
    CPU_PPC_G3,       /* 750 */
    CPU_PPC_G4,
    CPU_GEN_COUNT     /* one past G4; the table has a placeholder row at NONE */
};

/* Canonical table name for a generation ("68040", "G4"); "" if out of range.
 * This is the exact string the catalog carries and the dataset authors. */
const char *cpu_gen_name(int gen);

/* Human label for a message ("a 68040", "a PowerPC G4"); "" if out of range. */
const char *cpu_gen_label(int gen);

/* Canonical name (case-insensitive) -> generation index; CPU_GEN_NONE if unknown.
 * The build tool normalizes aliases ("040", "68LC040", "PPC") to canonical names,
 * so the launcher only ever sees canonical ones. */
int cpu_gen_from_name(const char *s);

#endif /* MACATRIUM_CPU_H */
