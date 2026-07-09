/*
 * spikes/startup-disk — prove the Start Manager sets the boot disk via PRAM (docs/42).
 *
 * Confirms the cross-disk-chooser keystone: SetDefaultStartup ($A07E) +
 * GetDefaultStartup ($A07D). It picks a NON-boot mounted volume, sets it as the
 * default startup device (its driver reference number from PBHGetVInfo's
 * ioVDRefNum), reads it back to verify the PRAM round-trip, and on 'R' restarts
 * into it. In the 2-disk Snow harness this reboots the machine onto the OTHER
 * disk, and the spike then shows "BOOTED FROM: <the other disk>".
 *
 * It is entirely OS-mediated: the traps do _WriteXPRam/_ReadXPRam on 4 bytes at
 * XPRAM $78 (SuperMario OS/StartMgr/StartSearch.a) — no raw PRAM or hardware poke.
 *
 * Target: Retro68 / 68k. Retro68's multiversal defines DefStartRec but not the
 * Set/GetDefaultStartup prototypes, so we emit the A-traps directly.
 */
#include <Quickdraw.h>
#include <Fonts.h>
#include <Windows.h>
#include <Events.h>
#include <Files.h>
#include <Memory.h>
#include <TextUtils.h>   /* NumToString */
#include <string.h>

/* The 4-byte default-startup-device record the ROM keeps at XPRAM $78, per
 * SuperMario StartSearch.a: { INTEGER drvNum; INTEGER refNum }. drvNum is $FFFF
 * when the drive number is irrelevant; refNum is the driver reference number. */
typedef struct { short drvNum; short refNum; } StartDev;

/* Start Manager A-traps (numbers from SuperMario Interfaces/AIncludes/Traps.a).
 * Both take the record pointer in A0; the ROM destroys D0-D2/A0-A1. */
static void SetDefaultStartup_(StartDev *p)
{
    register StartDev *a0 __asm__("a0") = p;
    __asm__ __volatile__(".short 0xA07E" : "+a"(a0) : : "d0","d1","d2","a1","cc","memory");
}
static void GetDefaultStartup_(StartDev *p)
{
    register StartDev *a0 __asm__("a0") = p;
    __asm__ __volatile__(".short 0xA07D" : "+a"(a0) : : "d0","d1","d2","a1","cc","memory");
}

static WindowPtr gWin;
static Str63 gBootName, gTgtName;
static short gBootV, gTgtV, gTgtDrv, gTgtRef, gReadDrv, gReadRef;
static int   gHaveTgt, gMatch;

static void putc_(short x, short y, const char *s)
{ MoveTo(x, y); DrawText((Ptr)s, 0, (short)strlen(s)); }

static void putnum_(short x, short y, const char *label, long v)
{ Str255 n; MoveTo(x, y); DrawText((Ptr)label, 0, (short)strlen(label)); NumToString(v, n); DrawString(n); }

static void draw(void)
{
    short y = 22;
    SetPort(gWin);
    EraseRect(&gWin->portRect);
    TextFont(4); TextSize(9);   /* Monaco 9 */
    putc_(16, y, "startup-disk spike (docs/42): SetDefaultStartup / GetDefaultStartup"); y += 20;
    MoveTo(16, y); DrawText((Ptr)"BOOTED FROM: ", 0, 13); DrawString(gBootName); y += 15;
    putnum_(16, y, "  boot vRefNum = ", gBootV); y += 20;
    if (!gHaveTgt) { putc_(16, y, "NO second (target) volume found."); return; }
    MoveTo(16, y); DrawText((Ptr)"TARGET VOLUME: ", 0, 15); DrawString(gTgtName); y += 15;
    putnum_(16, y, "  target vRefNum          = ", gTgtV);   y += 15;
    putnum_(16, y, "  ioVDrvInfo (drive#)     = ", gTgtDrv); y += 15;
    putnum_(16, y, "  ioVDRefNum (driver ref) = ", gTgtRef); y += 20;
    putc_(16, y, "wrote {drvNum,refNum} via SetDefaultStartup; read back via GetDefaultStartup:"); y += 15;
    putnum_(16, y, "  readback drvNum = ", gReadDrv); y += 15;
    putnum_(16, y, "  readback refNum = ", gReadRef); y += 20;
    putc_(16, y, gMatch ? ">>> READBACK MATCHES - PRAM WRITE OK <<<"
                        : ">>> READBACK MISMATCH <<<"); y += 24;
    putc_(16, y, "Press R to RESTART into the target disk.  (Q quits.)");
}

int main(void)
{
    EventRecord evt;
    StartDev setR, getR;
    short i;

    InitGraf(&qd.thePort); InitFonts(); InitWindows(); InitCursor();
    { Rect r; SetRect(&r, 8, 40, 632, 460);
      gWin = NewWindow(0L, &r, "\pstartup-disk", true, 0, (WindowPtr)-1L, false, 0); }
    SetPort(gWin);

    /* boot volume (normalize to a real vRefNum) + its name */
    GetVol(0L, &gBootV);
    { HParamBlockRec hp; memset(&hp, 0, sizeof hp);
      hp.volumeParam.ioNamePtr = gBootName; hp.volumeParam.ioVRefNum = gBootV; hp.volumeParam.ioVolIndex = 0;
      if (PBHGetVInfoSync(&hp) == noErr) gBootV = hp.volumeParam.ioVRefNum; else gBootName[0] = 0; }

    /* first mounted volume that is NOT the boot volume = the target disk */
    gHaveTgt = 0;
    for (i = 1; i < 24; i++) {
        HParamBlockRec hp; memset(&hp, 0, sizeof hp);
        hp.volumeParam.ioNamePtr = gTgtName; hp.volumeParam.ioVolIndex = i;
        if (PBHGetVInfoSync(&hp) != noErr) break;      /* nsvErr: past the last volume */
        if (hp.volumeParam.ioVRefNum == gBootV) continue;
        gTgtV   = hp.volumeParam.ioVRefNum;
        gTgtDrv = hp.volumeParam.ioVDrvInfo;
        gTgtRef = hp.volumeParam.ioVDRefNum;
        gHaveTgt = 1;
        break;
    }

    /* set the target as the default startup device, then read it back */
    if (gHaveTgt) {
        setR.drvNum = gTgtDrv;
        setR.refNum = gTgtRef;
        SetDefaultStartup_(&setR);
    }
    getR.drvNum = 0; getR.refNum = 0;
    GetDefaultStartup_(&getR);
    gReadDrv = getR.drvNum;
    gReadRef = getR.refNum;
    gMatch = (gHaveTgt && gReadRef == gTgtRef);

    draw();

    for (;;) {
        if (WaitNextEvent(everyEvent, &evt, 20L, 0L)) {
            if (evt.what == updateEvt) { BeginUpdate(gWin); draw(); EndUpdate(gWin); }
            else if (evt.what == keyDown || evt.what == autoKey) {
                char c = (char)(evt.message & charCodeMask);
                if (c == 'r' || c == 'R') ShutDwnStart();   /* does not return */
                if (c == 'q' || c == 'Q') break;
            }
        }
    }
    return 0;
}
