/*
 * sound.h — system alert (speaker) volume, the same 0-7 scale the Sound control
 * panel uses. Backed by the Sound Manager's SysBeepVolume; degrades to
 * "unavailable" if that isn't present (older Sound Manager).
 */
#ifndef MACATRIUM_SOUND_H
#define MACATRIUM_SOUND_H

#define SOUND_VOL_MAX 7

/* 1 if the volume can be read/set on this system. */
int  sound_available(void);

/* Current volume 0..SOUND_VOL_MAX (0 if unavailable). */
int  sound_get_vol(void);

/* Set volume 0..SOUND_VOL_MAX (clamped) and beep once at the new level. */
void sound_set_vol(int v);

#endif /* MACATRIUM_SOUND_H */
