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

# Stamp the build with the current git commit (short hash, +"-dirty" when the
# tree has uncommitted changes) so the launcher can show its version in the Menu.
# Captured at configure time — re-run cmake to refresh after committing.
execute_process(
    COMMAND git -C ${CMAKE_SOURCE_DIR} rev-parse --short HEAD
    OUTPUT_VARIABLE MACATRIUM_GIT_HASH
    OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
if(NOT MACATRIUM_GIT_HASH)
    set(MACATRIUM_GIT_HASH "unknown")
endif()
execute_process(
    COMMAND git -C ${CMAKE_SOURCE_DIR} diff --quiet HEAD
    RESULT_VARIABLE MACATRIUM_GIT_DIRTY ERROR_QUIET)
if(MACATRIUM_GIT_DIRTY)
    set(MACATRIUM_GIT_HASH "${MACATRIUM_GIT_HASH}-dirty")
endif()
configure_file(${CMAKE_SOURCE_DIR}/src/version.h.in ${CMAKE_BINARY_DIR}/version.h @ONLY)
include_directories(${CMAKE_BINARY_DIR})

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
    src/mem.c                # MEM_DEBUG on-screen memory probe (no-op unless -DMEM_DEBUG=ON)
    src/macatrium.r          # overrides SIZE (-1): 4 MB partition + suspend/resume
    src/macatrium-icon.r     # Finder app icon (greenhouse): ICN#/icl8/BNDL/FREF
    TYPE "APPL"
    CREATOR "ATRM"
)

# Dev-only on-screen memory probe (overlay top-left), for sizing the SIZE
# partition. OFF by default so production builds exclude it entirely.
option(MEM_DEBUG "On-screen memory probe overlay (dev only)" OFF)
if(MEM_DEBUG)
    target_compile_definitions(MacAtrium PRIVATE MEM_DEBUG)
endif()

# Keep the embedded launcher (tools/atrium-tool/assets/MacAtrium.bin, baked into
# the atrium tooling via include_bytes! so users build images without Retro68) in
# sync with the freshly built launcher. copy_if_different avoids spurious diffs
# when the bytes are unchanged; commit the asset when it does change.
if(TARGET MacAtrium_APPL)
    add_custom_command(TARGET MacAtrium_APPL POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            ${CMAKE_BINARY_DIR}/MacAtrium.bin
            ${CMAKE_SOURCE_DIR}/tools/atrium-tool/assets/MacAtrium.bin
        COMMENT "Refreshing embedded launcher -> tools/atrium-tool/assets/MacAtrium.bin")
endif()

# Keep the binary small: drop unused code and MacsBug symbols.
set_target_properties(MacAtrium PROPERTIES COMPILE_OPTIONS -ffunction-sections)
if(CMAKE_SYSTEM_NAME MATCHES Retro68)
    set_target_properties(MacAtrium PROPERTIES
        LINK_FLAGS "-Wl,-gc-sections -Wl,--mac-strip-macsbug")
endif()
