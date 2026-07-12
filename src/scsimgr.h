/*
 * scsimgr.h — classic (original) SCSI Manager trap glue.
 *
 * Retro68's headers do NOT ship the original SCSI Manager (there is no SCSI.h and
 * no SCSISelect / SCSIInstr anywhere in the toolchain), so we declare the routines
 * we need here. They are selector-based calls through _SCSIDispatch ($A815): each
 * pushes a routine selector via `move.w #sel,-(sp)` (opcode 0x3F3C) and traps — the
 * identical Pascal stack-dispatch idiom the multiversal headers already use for
 * _Pack7 (NumToString) and _ColorUtilities (CMY2RGB). Selectors and the TIB opcodes
 * are from Inside Macintosh: Devices, "The SCSI Manager".
 *
 * This is the ORIGINAL SCSI Manager (handshaked SCSIRead/SCSIWrite), present on
 * every classic Mac from the Plus onward under System 6.0.8 / 7.x — the minimum
 * MacAtrium targets. (Not SCSI Manager 4.3, which needs a newer API.)
 */
#ifndef MACATRIUM_SCSIMGR_H
#define MACATRIUM_SCSIMGR_H

#include <MacTypes.h>

/* One entry of a SCSI transfer instruction block (TIB). 10 bytes on the classic
 * 2-byte-aligned 68k ABI (short + two longs, no padding). */
typedef struct SCSIInstr {
    unsigned short scOpcode;
    long           scParam1;
    long           scParam2;
} SCSIInstr;

/* TIB opcodes (Inside Macintosh: Devices). We use scInc (transfer N bytes into a
 * buffer, incrementing) terminated by scStop (end of block). */
enum {
    scInc   = 1,
    scNoInc = 2,
    scAdd   = 3,
    scMove  = 4,
    scLoop  = 5,
    scNop   = 6,
    scStop  = 7,
    scComp  = 8
};

/* _SCSIDispatch ($A815) routines. Pascal stack convention (no register #pragma):
 * push the selector with 0x3F3C (move.w #imm,-(sp)), then trap 0xA815. */
pascal OSErr SCSIReset(void)                    M68K_INLINE(0x3F3C, 0x0000, 0xA815);
pascal OSErr SCSIGet(void)                      M68K_INLINE(0x3F3C, 0x0001, 0xA815);
pascal OSErr SCSISelect(short targetID)         M68K_INLINE(0x3F3C, 0x0002, 0xA815);
pascal OSErr SCSICmd(Ptr buffer, short count)   M68K_INLINE(0x3F3C, 0x0003, 0xA815);
pascal OSErr SCSIComplete(short *stat, short *message, unsigned long wait)
                                                M68K_INLINE(0x3F3C, 0x0004, 0xA815);
pascal OSErr SCSIRead(Ptr tibPtr)               M68K_INLINE(0x3F3C, 0x0005, 0xA815);

#endif /* MACATRIUM_SCSIMGR_H */
