/*
 * cpu.c — see cpu.h. The table and nothing else; pure C.
 */
#include "cpu.h"

/* THE table. Row order = capability order (the whole point). Keep in sync with
 * CPU_GENS in tools/atrium-tool/src/catalog.rs. */
static const struct {
    const char *name;    /* canonical: what the dataset authors + the catalog carries */
    const char *label;   /* display: what a warning says */
} kGens[CPU_GEN_COUNT] = {
    { "",      ""              },   /* CPU_GEN_NONE — placeholder so gen indexes directly */
    { "68000", "a 68000"       },
    { "68020", "a 68020"       },
    { "68030", "a 68030"       },
    { "68040", "a 68040"       },
    { "601",   "a PowerPC 601" },
    { "603",   "a PowerPC 603" },
    { "604",   "a PowerPC 604" },
    { "G3",    "a PowerPC G3"  },
    { "G4",    "a PowerPC G4"  },
};

const char *cpu_gen_name(int gen)
{
    return (gen < 0 || gen >= CPU_GEN_COUNT) ? "" : kGens[gen].name;
}

const char *cpu_gen_label(int gen)
{
    return (gen < 0 || gen >= CPU_GEN_COUNT) ? "" : kGens[gen].label;
}

int cpu_gen_from_name(const char *s)
{
    int i;
    if (!s || !s[0]) return CPU_GEN_NONE;
    for (i = CPU_68000; i < CPU_GEN_COUNT; i++) {   /* skip the NONE placeholder */
        const char *a = kGens[i].name;
        const char *b = s;
        /* case-insensitive compare (ASCII: |32 lowercases letters, leaves digits) */
        while (*a && *b && ((*a | 32) == (*b | 32))) { a++; b++; }
        if (!*a && !*b) return i;
    }
    return CPU_GEN_NONE;
}
