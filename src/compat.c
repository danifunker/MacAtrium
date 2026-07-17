/*
 * compat.c — see compat.h. Pure string-building over CatItem + Env; the depth
 * reachability is read from the pre-probed Env (env->maxScreenDepth), so this
 * stays Toolbox-free and host-testable.
 */
#include "compat.h"

static char *cr_str(char *p, const char *s) { while (*s) *p++ = *s++; return p; }

static char *cr_uint(char *p, long v)
{
    char t[12]; int k = 0;
    if (v <= 0) { *p++ = '0'; return p; }
    while (v > 0 && k < 11) { t[k++] = (char)('0' + (int)(v % 10)); v /= 10; }
    while (k > 0) *p++ = t[--k];
    return p;
}

/* Append a gestaltSystemVersion BCD (e.g. 0x0755) as "7.5.5" (a trailing .0 dropped). */
static char *cr_osver(char *p, int v)
{
    int maj = (v >> 8) & 0xFF, mnr = (v >> 4) & 0x0F, bug = v & 0x0F;
    if (maj >= 10) *p++ = (char)('0' + maj / 10);
    *p++ = (char)('0' + maj % 10);
    *p++ = '.';
    *p++ = (char)('0' + mnr);
    if (bug) { *p++ = '.'; *p++ = (char)('0' + bug); }
    return p;
}

/* "Needs " for the first clause, " and " to join later ones. */
static char *cr_needs(char *p, int *n) { p = cr_str(p, (*n)++ ? " and " : "Needs "); return p; }

int compat_reason(const CatItem *it, const Env *e, char *out)
{
    /* Required min CPU tier -> the name of the CPU it needs (TIER_* in env.h). */
    static const char *kNeed[] = { "", "a 68030", "a 68040", "a PowerPC" };
    /* A tolerated-max tier -> the CPU class it was made for (index by tier 0..4). */
    static const char *kMade[] = { "a 68000/020", "a 68030", "a 68040", "a PowerPC", "a PowerPC" };
    char *p = out;
    int   n = 0;

    if (it->minCPU > 0 && e->tier < it->minCPU) {           /* needs a faster CPU */
        int t = it->minCPU > 3 ? 3 : it->minCPU;
        p = cr_needs(p, &n); p = cr_str(p, kNeed[t]);
    }
    if (it->needsFPU && !e->hasFPU) {                        /* needs a hardware FPU */
        p = cr_needs(p, &n); p = cr_str(p, "an FPU");
    }
    if (it->minDepth > 0 && it->minDepth > e->maxScreenDepth) {   /* depth unreachable */
        p = cr_needs(p, &n);
        p = cr_str(p, e->hasColorQD ? "a deeper colour display" : "a colour display");
    }
    if (it->minMem > 0 && e->ramKB > 0 && e->ramKB < it->minMem) {   /* needs more RAM */
        p = cr_needs(p, &n); p = cr_uint(p, it->minMem / 1024); p = cr_str(p, " MB of memory");
    }
    if (it->minOS > 0 && e->sysVers > 0 && e->sysVers < it->minOS) {  /* needs a newer System */
        p = cr_needs(p, &n); p = cr_str(p, "System "); p = cr_osver(p, it->minOS);
    }
    if (n > 0) *p++ = '.';

    /* maxCPU: a title that breaks on a FASTER Mac (self-modifying code vs the 68040
     * instruction cache; timing loops). Stored as (max tolerated tier + 1); 0 = no
     * ceiling — so it fires when this Mac's tier is at or past the first broken one. */
    if (it->maxCPU > 0 && e->tier >= it->maxCPU) {
        int mt = it->maxCPU - 1;                             /* highest tolerated tier */
        if (mt < 0) mt = 0; else if (mt > 4) mt = 4;
        if (n > 0) *p++ = ' ';
        p = cr_str(p, "May crash on this Mac (made for ");
        p = cr_str(p, kMade[mt]);
        p = cr_str(p, " or older).");
        n++;
    }

    /* maxOS: a title that breaks on a NEWER System than it was written for (traps
     * removed or changed). Fires when the running System is past the title's ceiling
     * — e.g. the user booted 7.6 via the chooser for a title made for 7.1. */
    if (it->maxOS > 0 && e->sysVers > 0 && e->sysVers > it->maxOS) {
        if (n > 0) *p++ = ' ';
        p = cr_str(p, "May not run on this System (made for System ");
        p = cr_osver(p, it->maxOS);
        p = cr_str(p, " or earlier).");
        n++;
    }

    *p = '\0';
    return n > 0;
}
