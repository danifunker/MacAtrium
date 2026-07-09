# Retro68 build for the startup-disk spike (docs/42 Phase 0).
#
#   export RETRO68=/path/to/Retro68-build
#   cmake -S . -B build \
#     -DCMAKE_TOOLCHAIN_FILE=$RETRO68/toolchain/m68k-apple-macos/cmake/retro68.toolchain.cmake
#   cmake --build build
#   # -> build/startup-disk.bin  (MacBinary; inject with `rb-cli put-macbinary`)
#
cmake_minimum_required(VERSION 3.9)
project(startup-disk C)
add_application(startup-disk main.c)
