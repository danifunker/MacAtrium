/*
 * spikes/launch-return — the keystone de-risk.
 *
 * Proves the single most important architectural assumption: that an app can
 * sub-launch another app with `launchContinue` and CONTROL RETURNS to it when
 * the child quits — on System 6.0.8 (+MultiFinder), 7.1, 7.5.5, 7.6.1.
 *
 * What it does:
 *   - Reports whether Gestalt says launch-can-return is available (the guard).
 *   - 'L' -> Standard-File pick an APPL, launch it with launchContinue.
 *           When that app quits, we land back in our event loop and a counter
 *           increments. A growing counter == control returned. QED.
 *   - 'F' -> bring the real Finder to the front (return-to-Finder path).
 *   - 'R' -> Restart via the Shutdown Manager.
 *   - 'Q' -> quit (dev convenience; the real shell never quits).
 *
 * Target: Retro68 / Universal Interfaces, 68k. This is a DRAFT — expect minor
 * tweaks against the exact Retro68 headers (StandardFile signatures, qd global).
 *
 * Observe in Snow: set a system-trap breakpoint on _Launch (A9F2), step over it,
 * and watch execution return to our loop. Also inspect the LaunchParamBlockRec.
 */

#include <Quickdraw.h>     /* pulls in the whole Multiverse header, incl. text + ShutDwn */
#include <Fonts.h>
#include <Windows.h>
#include <Events.h>
#include <Menus.h>
#include <Dialogs.h>
#include <TextUtils.h>
#include <Processes.h>
#include <Gestalt.h>
#include <StandardFile.h>
#include <Files.h>
#include <Memory.h>     /* BlockMoveData */

/* Retro68's multiversal headers are leaner than Apple's Universal Interfaces:
 * a few well-known Process Manager / Gestalt constants aren't defined. Supply
 * them with the exact values confirmed from Apple's headers in
 * docs/11-derisk-log.md §A (Processes.h / GestaltEqu.h). */
#ifndef launchNoFileFlags
#define launchNoFileFlags 0x0800   /* we resolve the FSSpec ourselves */
#endif
#ifndef gestaltLaunchCanReturn
#define gestaltLaunchCanReturn 1   /* bit in gestaltOSAttr: resident launch */
#endif
#ifndef gestaltLaunchControl
#define gestaltLaunchControl 3     /* bit in gestaltOSAttr */
#endif

/* ---- state ---------------------------------------------------------------- */
static WindowPtr gWin;
static Boolean   gQuit       = false;
static Boolean   gCanReturn  = false;   /* gestaltLaunchCanReturn */
static long      gOSAttr     = 0;
static long      gQDVers     = 0;
static long      gSysVers    = 0;
static int       gLaunchCount= 0;       /* times control returned from a launch */
static OSErr     gLastErr    = noErr;
static Str255    gLastApp;               /* set to "(none)" at startup */

/* ---- helpers -------------------------------------------------------------- */

static void DrawLine(short *v, const unsigned char *p)
{
    MoveTo(12, *v);
    DrawString(p);
    *v += 16;
}

static void DrawNum(short *v, const unsigned char *label, long n)
{
    Str255 num;
    MoveTo(12, *v);
    DrawString(label);
    NumToString(n, num);
    DrawString(num);
    *v += 16;
}

static void Redraw(void)
{
    Rect r = gWin->portRect;
    short v = 28;

    SetPort(gWin);
    EraseRect(&r);

    DrawLine(&v, "\plaunch-return spike");
    v += 6;
    DrawNum(&v,  "\pSystem version:    $", gSysVers);
    DrawNum(&v,  "\pQuickDraw version: $", gQDVers);
    DrawLine(&v, gCanReturn ? "\plaunchCanReturn:   YES (resident launch OK)"
                            : "\plaunchCanReturn:   NO  (would quit on launch!)");
    v += 6;
    DrawNum(&v,  "\p>>> RETURNS FROM LAUNCH: ", (long)gLaunchCount);
    DrawNum(&v,  "\plast LaunchApplication err: ", (long)gLastErr);
    MoveTo(12, v); DrawString("\plast app: "); DrawString(gLastApp); v += 22;

    DrawLine(&v, "\pL = launch an app    F = Finder");
    DrawLine(&v, "\pR = restart          Q = quit");
}

/* The keystone: pick an app, launch it resident, return here when it quits. */
static void LaunchAndReturn(void)
{
    StandardFileReply   reply;
    SFTypeList          types;
    LaunchParamBlockRec pb;

    types[0] = 'APPL';
    StandardGetFile(NULL, 1, types, &reply);
    if (!reply.sfGood) return;

    BlockMoveData(reply.sfFile.name, gLastApp, reply.sfFile.name[0] + 1);

    pb.launchBlockID       = extendedBlock;
    pb.launchEPBLength     = extendedBlockLen;
    pb.launchFileFlags     = 0;
    pb.launchControlFlags  = launchContinue | launchNoFileFlags;
    pb.launchAppSpec       = &reply.sfFile;
    pb.launchAppParameters = NULL;

    gLastErr = LaunchApplication(&pb);   /* _Launch (A9F2) */

    /* If launchContinue was honored, we reach here after the child quits. */
    if (gLastErr == noErr)
        gLaunchCount++;

    /* We come back as a background process; pull ourselves forward + redraw. */
    SelectWindow(gWin);
    SetPort(gWin);
    Redraw();
}

/* Return-to-Finder: find creator 'MACS' in the process list, bring it forward. */
static void BringFinderForward(void)
{
    ProcessSerialNumber psn;
    ProcessInfoRec      info;
    Str31               nameBuf;

    psn.highLongOfPSN = 0;
    psn.lowLongOfPSN  = kNoProcess;
    while (GetNextProcess(&psn) == noErr) {
        info.processInfoLength = sizeof(info);
        info.processName       = nameBuf;
        info.processAppSpec     = NULL;
        if (GetProcessInformation(&psn, &info) == noErr &&
            info.processSignature == 'MACS') {
            SetFrontProcess(&psn);
            return;
        }
    }
    SysBeep(10);   /* Finder not resident (we may BE the only shell) */
}

static void ProbeEnvironment(void)
{
    if (Gestalt(gestaltSystemVersion, &gSysVers) != noErr)     gSysVers = 0;
    if (Gestalt(gestaltQuickdrawVersion, &gQDVers) != noErr)   gQDVers  = 0;
    if (Gestalt(gestaltOSAttr, &gOSAttr) == noErr)
        gCanReturn = (gOSAttr & (1L << gestaltLaunchCanReturn)) != 0;
}

static void HandleKey(char c)
{
    switch (c) {
        case 'l': case 'L': LaunchAndReturn();       break;
        case 'f': case 'F': BringFinderForward();    break;
        case 'r': case 'R': ShutDwnStart();          break;   /* restart */
        case 'q': case 'Q': gQuit = true;            break;
    }
}

int main(void)
{
    EventRecord evt;
    Rect        bounds;

    InitGraf(&qd.thePort);
    InitFonts();
    InitWindows();
    InitMenus();
    TEInit();
    InitDialogs(NULL);
    InitCursor();

    ProbeEnvironment();

    {   /* Str255 can't be statically initialised from a "\p" literal here, so
         * seed the "last app" label at runtime. */
        const unsigned char *def = "\p(none)";
        BlockMoveData(def, gLastApp, def[0] + 1);
    }

    SetRect(&bounds, 40, 60, 40 + 460, 60 + 300);
    gWin = NewWindow(NULL, &bounds, "\plaunch-return spike",
                     true, documentProc, (WindowPtr)-1L, false, 0);
    SetPort(gWin);
    TextFont(systemFont);     /* Chicago */
    Redraw();

    while (!gQuit) {
        if (WaitNextEvent(everyEvent, &evt, 10L, NULL)) {
            switch (evt.what) {
                case keyDown:
                case autoKey:
                    HandleKey((char)(evt.message & charCodeMask));
                    break;
                case updateEvt:
                    BeginUpdate((WindowPtr)evt.message);
                    Redraw();
                    EndUpdate((WindowPtr)evt.message);
                    break;
                case mouseDown:
                    /* bring us forward if backgrounded; ignore otherwise */
                    SelectWindow(gWin);
                    break;
            }
        }
    }
    return 0;
}
