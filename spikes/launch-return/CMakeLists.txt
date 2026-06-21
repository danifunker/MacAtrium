# Retro68 build for the launch-return spike.
#
# Configure with the Retro68 68k toolchain file, e.g.:
#
#   export RETRO68=/path/to/Retro68-build          # your install
#   cmake -S . -B build \
#     -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
#   cmake --build build
#
# (Adjust the toolchain path to match your Retro68 install; some installs put it
# at .../toolchain/cmake/... — check your tree.)
#
# Outputs in build/:
#   launch-return.bin    MacBinary (use this with `rb-cli put`)
#   launch-return.APPL    the application (AppleDouble: data + .rsrc)
#   launch-return.dsk     a small disk image containing just the app
#
cmake_minimum_required(VERSION 3.9)
project(launch-return C)

# add_application() is provided by the Retro68 toolchain. It builds a classic
# 68k APPL with its resource fork and emits .bin/.APPL/.dsk. No CONSOLE here —
# this is a real Toolbox app, not the Retro68 console.
add_application(launch-return main.c)
