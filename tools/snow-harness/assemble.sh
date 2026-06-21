#!/bin/bash
# Assemble a pristine MacAtrium test image from a System 7.5.5 source disk.
# Usage: assemble.sh <source.hda> <out.hda>
set -eu
RB=/home/dani/repos/rusty-backup/target/release/rb-cli
APP=/home/dani/repos/MacAtrium/build/MacAtrium.bin
RUN=/tmp/macatrium-run
SRC="$1"; OUT="$2"

cp "$SRC" "$OUT"

# /MacAtrium tree
$RB mkdir "$OUT" "/MacAtrium" -q
$RB mkdir "$OUT" "/MacAtrium/metadata" -q
$RB mkdir "$OUT" "/MacAtrium/Apps" -q
$RB mkdir "$OUT" "/MacAtrium/Apps/SimpleText" -q

# catalog + a real launch target (SimpleText, both forks) + the launcher (Startup Items)
$RB put         "$OUT" "$RUN/test_catalog.jsonl" "/MacAtrium/metadata/catalog.jsonl" --type TEXT --creator ttxt -q
$RB put-binhex  "$OUT" "$RUN/SimpleText.hqx" --dst-dir "/MacAtrium/Apps/SimpleText" -q
$RB put-macbinary "$OUT" "$APP" --dst-dir "/System Folder/Startup Items" -q
echo "assembled $OUT"
