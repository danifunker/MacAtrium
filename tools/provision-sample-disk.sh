#!/usr/bin/env bash
#
# provision-sample-disk.sh — build a multi-OS MacAtrium demo disk end to end.
#
# Takes a multi-System base disk (e.g. 6.0.8 / 7.1 / 7.5.5 + System Picker),
# grows it, drops in the all-depth /MacAtrium tree (1/8/24-bit art + catalog +
# apps), installs the launcher into every System Folder, and — optionally —
# primes the volume Desktop DB and resets first-run state so the disk ships clean.
#
# PIPELINE
#   assemble  (always, offline, verified):
#     1. expand   base multi-OS disk  -> larger sample (room for the art tree)
#     2. cp       the all-depth /MacAtrium tree from a source image into it
#     3. install  the launcher (SIZE-patched) into every System Folder
#                 (Startup Items on 7.x, as the Finder on 6.0.x)
#     [3b] optional: delete the stale inherited Desktop DB/DF so the FIRST real
#          boot builds a clean one (persists on read-write hardware; the user
#          sees one harmless rebuild). Enable with DEL_STALE_DB=1.
#   prime     (optional, needs a WRITE-CAPABLE boot; enable with PRIME=1):
#     4. bless the heavy System (7.5.5), boot it so the Finder rebuilds the
#        Desktop DB, drive a graceful Shut Down (esc -> up -> return) so
#        ShutDwnPower flushes it, delete the MacAtrium Prefs the boot wrote
#        (restore first-run), then re-bless the shipping default (7.1).
#
# *** WHY THE PRIME NEEDS A SPECIAL HARNESS ***
#   The stock macatrium_harness mounts SCSI disks READ-ONLY (snow_core built
#   without the `mmap` feature). An in-emulator Desktop rebuild is then discarded
#   on exit — the .hda is byte-identical and every boot rebuilds again. To make
#   the prime actually persist, build a read-write harness first:
#
#       cd <snow>/testrunner && \
#         cargo build --release --bin macatrium_harness --features snow_core/mmap
#
#   ...and point HARNESS= at it. Or run the prime on real hardware / a read-write
#   emulator. There is NO host-side tool that rebuilds the classic Desktop
#   Manager btree — only a live Finder builds it, so a boot is unavoidable.
#
#   VERIFIED 2026-07-09 (Snow + mmap harness): priming under 7.5.5 rebuilds the
#   volume Desktop DB (69632 -> 65536 B) and it PERSISTS; both 7.5.5 and 7.1 then
#   boot with no "Rebuilding the desktop file", and the prefs delete brings the
#   first-run SETUP chooser back. The rebuild is one-time, not recurring.
#
#   6.0.8 needs NO prime: MacAtrium is installed AS the Finder there (System-6
#   appliance), so there is no Finder to rebuild the desktop — it boots straight
#   into the launcher (verified), and booting it does not re-dirty the 7.x DB.
#   Priming under 7.5.5 alone leaves the whole disk clean.
#
# REQUIRES: rb-cli, atrium, macatrium_harness, the Mac II + MDC 8*24 ROMs, and a
#           multi-System base disk. Paths below are overridable via the environment.
#
set -euo pipefail

# ---- config (override via env) ---------------------------------------------
BASE_DISK="${BASE_DISK:-/mnt/c/temp/MacAtrium_Sys-shrunk.hda}"    # your multi-OS base disk
ART_SRC="${ART_SRC:-/tmp/artfull/MacAtrium-7.5.5-fullcolor.hda}"  # image holding the all-depth /MacAtrium tree
LAUNCHER="${LAUNCHER:-$HOME/repos/MacAtrium/build/MacAtrium.bin}" # built launcher (MacBinary)
OUT="${OUT:-/mnt/c/temp/MacAtrium_Sample_AllDepths.hda}"         # output disk
DISK_SIZE="${DISK_SIZE:-96M}"                                    # expanded volume size
SIZE_PREF_KB="${SIZE_PREF_KB:-3072}"                             # launcher 'SIZE' preferred (multi-OS)
SIZE_MIN_KB="${SIZE_MIN_KB:-1024}"                               # launcher 'SIZE' minimum
SHIP_SYS="${SHIP_SYS:-/System Folder 7.1}"                       # blessed default at ship

# feature switches
DEL_STALE_DB="${DEL_STALE_DB:-0}"   # 1 = drop the stale Desktop DB/DF (clean first-boot rebuild)
PRIME="${PRIME:-0}"                 # 1 = run the emulator prime pass (needs read-write harness!)
PRIME_SYS="${PRIME_SYS:-/System Folder 7.5.5}"   # heavy System to rebuild the Desktop DB under

# tools + emulator assets
RB="${RB:-rb-cli}"
ATRIUM="${ATRIUM:-$HOME/repos/MacAtrium/tools/atrium-tool/target/release/atrium}"
HARNESS="${HARNESS:-$HOME/repos/snow/target/release/macatrium_harness}"
ROM="${ROM:-$HOME/mac-mdverify/macii.rom}"
MDC="${MDC:-$HOME/mac-mdverify/mdc.bin}"

# prime key timing, in emulator cycles (SETUP appears ~1.3B after the ~0.7B rebuild)
P_SETUP="${P_SETUP:-1600000000}"   # dismiss the first-run SETUP chooser
P_ESC="${P_ESC:-1900000000}"       # open the launcher menu
P_UP="${P_UP:-2000000000}"         # focus wraps to the last row (Shut Down)
P_RET="${P_RET:-2100000000}"       # select Shut Down -> ShutDwnPower (flush)
P_CYCLES="${P_CYCLES:-3200000000}" # total run (leaves time for the shutdown to settle)

say(){ printf '\n>>> %s\n' "$*"; }
scratch="$(mktemp -d)"; trap 'rm -rf "$scratch"' EXIT

# ---- 1-3: assemble (offline, deterministic) --------------------------------
say "1/4  expand base multi-OS disk -> ${DISK_SIZE} sample : ${OUT}"
"$RB" expand "$BASE_DISK" --size "$DISK_SIZE" --output "$OUT"
"$RB" make-bootable "$OUT" --boot-from "$BASE_DISK" >/dev/null 2>&1 || true

say "2/4  copy all-depth /MacAtrium tree (1/8/24-bit art + catalog + apps)"
"$RB" cp -r "$ART_SRC" /MacAtrium "$OUT" /MacAtrium

say "3/4  install launcher (SIZE ${SIZE_PREF_KB}/${SIZE_MIN_KB} KB) into every System Folder"
"$ATRIUM" size --launcher "$LAUNCHER" --pref "$SIZE_PREF_KB" --min "$SIZE_MIN_KB" \
    --out "$scratch/launcher.bin" >/dev/null
"$ATRIUM" install-all-systems --image "$OUT" --launcher "$scratch/launcher.bin"

if [ "$DEL_STALE_DB" = "1" ]; then
    say "3b/4 drop the stale inherited Desktop DB/DF (first real boot rebuilds clean)"
    "$RB" rm "$OUT" "/Desktop DB" 2>/dev/null || true
    "$RB" rm "$OUT" "/Desktop DF" 2>/dev/null || true
fi

# ---- 4: prime the Desktop DB (optional, needs a write-capable harness) ------
if [ "$PRIME" = "1" ]; then
    say "4/4  PRIME Desktop DB under '${PRIME_SYS}'  (REQUIRES a read-write harness — see header)"
    "$RB" bless set "$OUT" "$PRIME_SYS"
    "$HARNESS" "$ROM" "$MDC" "$OUT" "$scratch/prime" "$P_CYCLES" \
        --keys "${P_SETUP}:return;${P_ESC}:esc;${P_UP}:up;${P_RET}:return" \
        --wall-secs 1800 >/dev/null 2>&1 || true
    say "     reset first-run: delete the MacAtrium Prefs the prime boot wrote"
    "$RB" rm "$OUT" "${PRIME_SYS}/Preferences/MacAtrium Prefs" 2>/dev/null || true
    "$RB" bless set "$OUT" "$SHIP_SYS"
    echo "     verify on read-write HW: boot '${PRIME_SYS}' — no 'Rebuilding the desktop file',"
    echo "     and MacAtrium shows the first-run SETUP chooser again."
else
    say "4/4  prime SKIPPED  (set PRIME=1 with a read-write harness to pre-build the Desktop DB)"
    "$RB" bless set "$OUT" "$SHIP_SYS" >/dev/null 2>&1 || true
fi

say "DONE -> ${OUT}  (blessed '${SHIP_SYS}')"
