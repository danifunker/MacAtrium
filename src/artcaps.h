/*
 * artcaps.h — the runtime art-capability set (docs/44).
 *
 * One `'SIZE'` partition, but the single multi-OS binary (docs/36 forks, docs/37
 * multi-disk) runs everywhere from a 4 MB 1-bit compact to a 32 MB 24-bit Quadra.
 * So it can't bake one art depth: it must decide, at startup, which art tiers the
 * machine it woke up on can actually *show* (VRAM) and *hold* (partition).
 *
 * `art_caps_probe` measures the live machine once and fills an `ArtCaps` — a purely
 * descriptive snapshot. P1 only reports it (run_status_dialog); the budget-aware
 * loader (P2) is the first consumer that changes behaviour.
 *
 * The measurement (Toolbox calls) and the *derivation* (pure arithmetic on the
 * measured numbers) are split: art_caps_derive() takes an ArtCapsInput and needs
 * no Toolbox, so the gating logic is host-tested across machine profiles Snow can't
 * emulate (the 1-bit compact and the 24-bit Quadra) — see tests/host_test.c.
 * Compiling with -DARTCAPS_HOST_TEST drops the Mac-only probe half.
 *
 * Three art tiers, by pixel depth: 1-bit (ABMP), 8-bit and 24-bit (PICT). Index an
 * ArtCaps array with ART_MODE_*.
 */
#ifndef MACATRIUM_ARTCAPS_H
#define MACATRIUM_ARTCAPS_H

enum { ART_MODE_1BIT = 0, ART_MODE_8BIT, ART_MODE_24BIT, ART_MODE_COUNT };

typedef struct {
    long  grantedPartition;   /* whole partition granted (processSize, or heap zone on 6.x) */
    long  partitionFree;      /* free in the partition at probe time                        */
    long  maxBlock;           /* largest contiguous free block — the single-alloc ceiling   */
    long  tempFree;           /* TempFreeMem(): ~0 on bare Sys6 ⇒ GWorld lives in-partition  */
    long  artBudget;          /* bytes we estimate remain for resident art after reserves   */
    short maxCardDepth;       /* deepest depth the card can display (1 if B&W / no Color QD) */

    long  peakArtBytes[ART_MODE_COUNT];  /* conservative resident estimate per tier          */
    int   displayable[ART_MODE_COUNT];   /* VRAM   — can the card show this tier?             */
    int   affordable[ART_MODE_COUNT];    /* memory — artBudget >= peakArtBytes[M]?            */
    int   enabled[ART_MODE_COUNT];       /* displayable && affordable                        */

    short maxAffordableDepth; /* deepest *affordable* art depth (1/8/24); always >= 1         */
    short defaultMode;        /* deepest *enabled* art depth (1/8/24); 1 when nothing richer  */
} ArtCaps;

/* Raw measurements the derivation runs on — filled by art_caps_probe from the live
 * Toolbox, or by a test with a synthetic machine profile. */
typedef struct {
    long  grantedPartition;   /* processSize / heap-zone extent                 */
    long  partitionFree;      /* processFreeMem / FreeMem()                     */
    long  maxBlock;           /* MaxBlock()                                     */
    long  tempFree;           /* TempFreeMem() (0 when unavailable / bare Sys6) */
    short maxCardDepth;       /* deepest displayable depth (1 for B&W)          */
    long  screenW, screenH;   /* main-screen size in pixels                     */
    short screenDepth;        /* current screen depth in bits                   */
} ArtCapsInput;

/* Derive the capability set from measured inputs. Pure arithmetic, no Toolbox —
 * the host-tested heart of the model. */
void art_caps_derive(ArtCaps *out, const ArtCapsInput *in);

#ifndef ARTCAPS_HOST_TEST
#include "env.h"
/* Measure the live machine (partition + Color QD depths) then derive. Loads no art
 * and changes nothing. Call once in main() after env_probe + display setup. */
void art_caps_probe(ArtCaps *out, const Env *e);
#endif

#endif /* MACATRIUM_ARTCAPS_H */
