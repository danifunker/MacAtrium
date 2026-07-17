/*
 * models.h — the Mac model table: gestaltMachineType -> model name + the minimum
 * System that machine can boot (docs/40).
 *
 * Two consumers, one table:
 *   - env.c raises the OS floor (`minOSbcd`) for a model that needs a System newer
 *     than its CPU tier's floor — a Color Classic boots 7.1, not 6.0.x.
 *   - the MacAtrium Status screen names the machine, so a wrong probe is visible.
 *
 * Baked from data/models.jsonl; regenerate when that changes. Board-family Gestalt
 * IDs are SHARED (one id can cover several models), so `name` is representative of
 * the board rather than exact, and `minOSbcd` is the MOST PERMISSIVE floor across
 * the models sharing the id — never over-greying a System that some of them boot.
 * New-World Macs report a generic id and simply miss the table (NULL).
 *
 * Pure C (no Toolbox); host-testable.
 */
#ifndef MACATRIUM_MODELS_H
#define MACATRIUM_MODELS_H

typedef struct {
    short       id;         /* gestaltMachineType */
    short       minOSbcd;   /* earliest System this board boots (BCD); 0 = unknown.
                             * May be BELOW the 6.0.4 envelope floor (a Plus reports
                             * System 3.2) — callers clamp with the tier floor. */
    const char *name;       /* representative model name for the board */
} MacModel;

/* Look up a gestaltMachineType; NULL when the id is unknown (New-World Macs share
 * a generic id, and anything newer than the table simply misses). */
const MacModel *model_by_id(long machineID);

#endif /* MACATRIUM_MODELS_H */
