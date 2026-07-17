/*
 * compat.h — per-title hardware compatibility check (docs/40). Pure C (no
 * Toolbox): given a catalog item and the probed environment, report whether THIS
 * Mac falls short of — or is too new for — the title's declared requirements.
 * Shared by the launch-time confirm (main.c) and the browse detail flag (ui.c),
 * and unit-tested off-target (tests/host_test.c).
 */
#ifndef MACATRIUM_COMPAT_H
#define MACATRIUM_COMPAT_H

#include "catalog.h"
#include "env.h"

/* Fill `out` (>= COMPAT_REASON_LEN bytes) with a short human reason THIS Mac can't
 * properly run `it`: a higher CPU tier / an FPU / more RAM / a deeper display it
 * needs, or that it is too FAST for (maxCPU — e.g. self-modifying code that breaks
 * on the 68040 cache). Returns 1 when `out` is a non-empty sentence, 0 when the
 * Mac is adequate (out[0] = '\0'). A min depth the screen CAN reach is not flagged
 * here — the launcher raises the depth instead. Pure; no Toolbox calls. */
#define COMPAT_REASON_LEN 160
int compat_reason(const CatItem *it, const Env *e, char *out);

#endif /* MACATRIUM_COMPAT_H */
