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

/* Set volume 0..SOUND_VOL_MAX (clamped) WITHOUT the feedback beep — used to
 * restore a saved volume at startup (a boot-time beep would be obnoxious). */
void sound_apply_vol(int v);

/* Play the `snd ` resource (id 128) from the resource file at the /MacAtrium-
 * relative path `relToRoot` (e.g. "sounds/startup"). `async` returns immediately
 * (the startup chime, so the UI isn't blocked); otherwise it blocks until the
 * clip finishes (the shutdown chime, played before power-off). Best-effort: a
 * missing file/resource or absent Sound Manager is a silent no-op. */
void sound_play_file(const char *relToRoot, int async);

#endif /* MACATRIUM_SOUND_H */
