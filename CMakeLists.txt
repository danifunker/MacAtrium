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
    src/cpu.c                # docs/40: the one CPU-generation table (pure)
    src/compat.c             # docs/40: per-title hardware compatibility check (pure)
    src/model.c
    src/render.c
    src/render_qd.c
    src/render_cqd.c
    src/theme.c              # Phase 3: runtime per-OS control appearance (docs/36)
    src/art.c
    src/ui.c
    src/launch.c
    src/sysctl.c
    src/bless.c              # Phase 2: enumerate System Folders + set the blessed one
    src/controlpanels.c
    src/display.c
    src/artcaps.c            # docs/44: runtime art-capability set (partition + VRAM gate)
    src/sound.c
    src/prefs.c
    src/toolbox.c            # docs/45: BlueSCSI Toolbox CD client (SCSI Manager) — parse/match host-tested
    src/cdswap.c             # docs/45: launch-time CD insert orchestration (probe/list/set/wait)
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

# The atrium build tool reads the launcher from build/MacAtrium.bin at build time
# (config.rs launcher_path) — it is NOT embedded in the tool or committed to git, so
# there's nothing to copy/sync here. `cmake --build build` produces build/MacAtrium.bin;
# a "release" user can drop a prebuilt MacAtrium.bin there instead.

# Keep the binary small: drop unused code and MacsBug symbols.
set_target_properties(MacAtrium PROPERTIES COMPILE_OPTIONS -ffunction-sections)
if(CMAKE_SYSTEM_NAME MATCHES Retro68)
    set_target_properties(MacAtrium PROPERTIES
        LINK_FLAGS "-Wl,-gc-sections -Wl,--mac-strip-macsbug")
endif()
