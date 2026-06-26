/*
 * mem.c — see mem.h. Entire body is compiled only under MEM_DEBUG.
 *
 * The number that drives the `SIZE` partition is PEAK used = processSize -
 * min(processFreeMem) over the session (Process Manager, System 7+). We also show
 * the low-water FreeMem / MaxBlock (heap fragmentation headroom) and TempFreeMem
 * (whether the off-screen GWorld, allocated useTempMem-first, is living in system
 * temp memory rather than our partition — see render.c).
 */
#include "mem.h"

#ifdef MEM_DEBUG

#include <Gestalt.h>
#include <Memory.h>
#include <Processes.h>
#include <Quickdraw.h>
#include <Fonts.h>
#include <string.h>

#define BIG 0x7fffffffL

static long gMinFree = BIG;     /* low-water partition free (Sys7)  */
static long gMinHeap = BIG;     /* low-water FreeMem()              */
static long gMinBlk  = BIG;     /* low-water MaxBlock()             */
static long gMinTemp = BIG;     /* low-water TempFreeMem()          */
static long gPart    = 0;       /* partition size (Sys7)            */

static long sysver(void)
{
    long v = 0;
    (void)Gestalt(gestaltSystemVersion, &v);
    return v;
}

/* Current process partition size + free bytes (System 7 Process Manager). */
static int proc_mem(long *size, long *freeb)
{
    ProcessSerialNumber psn;
    ProcessInfoRec info;
    if (GetCurrentProcess(&psn) != noErr) return 0;
    memset(&info, 0, sizeof info);
    info.processInfoLength = (long)sizeof info;   /* name/appSpec left nil */
    if (GetProcessInformation(&psn, &info) != noErr) return 0;
    *size  = (long)info.processSize;
    *freeb = (long)info.processFreeMem;
    return 1;
}

/* Append "<label>NNNNK " (bytes rounded to KB) to a C string. */
static void appK(char *dst, const char *label, long bytes)
{
    long k = bytes / 1024L;
    char tmp[12];
    int  t = 0, i = (int)strlen(dst);
    while (*label) dst[i++] = *label++;
    if (k < 0) { dst[i++] = '-'; k = -k; }
    if (k == 0) tmp[t++] = '0';
    while (k) { tmp[t++] = (char)('0' + k % 10); k /= 10; }
    while (t) dst[i++] = tmp[--t];
    dst[i++] = 'K'; dst[i++] = ' '; dst[i] = '\0';
}

static void c2p(const char *s, Str255 out)
{
    int n = 0;
    while (s[n] && n < 255) { out[n + 1] = (unsigned char)s[n]; n++; }
    out[0] = (unsigned char)n;
}

void mem_debug_tick(WindowPtr w)
{
    long heap = FreeMem();
    long blk  = MaxBlock();
    long temp = TempFreeMem();
    long psize = 0, pfree = 0;
    long used;
    char l1[96], l2[96];
    Str255 p;
    Rect box;

    if (heap < gMinHeap) gMinHeap = heap;
    if (blk  < gMinBlk)  gMinBlk  = blk;
    if (temp < gMinTemp) gMinTemp = temp;
    if (sysver() >= 0x0700 && proc_mem(&psize, &pfree)) {
        gPart = psize;
        if (pfree < gMinFree) gMinFree = pfree;
    }

    used = (gMinFree == BIG) ? 0 : (gPart - gMinFree);   /* peak partition use */

    l1[0] = '\0';
    appK(l1, "PEAK part=", gPart);
    appK(l1, "used=", used);
    l2[0] = '\0';
    appK(l2, "fre=", (gMinHeap == BIG) ? 0 : gMinHeap);
    appK(l2, "blk=", (gMinBlk == BIG) ? 0 : gMinBlk);
    appK(l2, "tmp=", (gMinTemp == BIG) ? 0 : gMinTemp);

    SetPort(w);
    SetRect(&box, 0, 0, 320, 30);
    ForeColor(blackColor);
    BackColor(whiteColor);
    PaintRect(&box);                  /* black backdrop so white text reads */
    ForeColor(whiteColor);
    TextFont(systemFont);             /* Chicago — legible in a 640px frame */
    TextSize(10);
    TextFace(normal);
    MoveTo(3, 12);  c2p(l1, p); DrawString(p);
    MoveTo(3, 26);  c2p(l2, p); DrawString(p);
    ForeColor(blackColor);
    BackColor(whiteColor);
}

#endif /* MEM_DEBUG */
