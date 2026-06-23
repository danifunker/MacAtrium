# MacAtrium — 68k launcher shell, built with Retro68.
#
#   export RETRO68=/path/to/Retro68-build
#   cmake -S . -B build \
#     -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
#   cmake --build build
#   # -> build/MacAtrium.bin (MacBinary; inject with rb-cli), .APPL, .dsk
#
# The portable core (json/catalog/model) is also unit-tested off-target with host
# gcc; see tests/.
cmake_minimum_required(VERSION 3.9)
project(MacAtrium C)

add_application(MacAtrium
    src/main.c
    src/env.c
    src/macfs.c
    src/json.c
    src/catalog.c
    src/model.c
    src/render.c
    src/render_qd.c
    src/render_cqd.c
    src/art.c
    src/ui.c
    src/launch.c
    src/sysctl.c
    src/controlpanels.c
    src/display.c
    src/sound.c
    src/prefs.c
    src/macatrium.r          # overrides SIZE (-1): 4 MB partition + suspend/resume
    src/macatrium-icon.r     # Finder app icon (greenhouse): ICN#/icl8/BNDL/FREF
    TYPE "APPL"
    CREATOR "ATRM"
)

# Keep the binary small: drop unused code and MacsBug symbols.
set_target_properties(MacAtrium PROPERTIES COMPILE_OPTIONS -ffunction-sections)
if(CMAKE_SYSTEM_NAME MATCHES Retro68)
    set_target_properties(MacAtrium PROPERTIES
        LINK_FLAGS "-Wl,-gc-sections -Wl,--mac-strip-macsbug")
endif()
