#!/bin/bash
# Assemble a pristine MacAtrium test image from a System 6/7 source disk.
# Usage: assemble.sh <source.hda> <out.hda> [startup_items_dir]
#   startup_items_dir defaults to "/System Folder/Startup Items".
#   (7.0.1's blessed folder is "System Folder 7.0.1"; System 6 has none.)
set -eu
RB=/home/dani/repos/rusty-backup/target/release/rb-cli
APP=/home/dani/repos/MacAtrium/build/MacAtrium.bin
RUN=/tmp/macatrium-run
SRC="$1"; OUT="$2"
STARTUP="${3:-/System Folder/Startup Items}"

cp "$SRC" "$OUT"

# /MacAtrium tree
$RB mkdir "$OUT" "/MacAtrium" -q
$RB mkdir "$OUT" "/MacAtrium/metadata" -q
$RB mkdir "$OUT" "/MacAtrium/Apps" -q
$RB mkdir "$OUT" "/MacAtrium/Apps/SimpleText" -q
$RB mkdir "$OUT" "/MacAtrium/Apps/Prince of Persia" -q

# catalog
$RB put "$OUT" "$RUN/test_catalog.jsonl" "/MacAtrium/metadata/catalog.jsonl" --type TEXT --creator ttxt -q

# launch targets (both forks)
$RB put-binhex "$OUT" "$RUN/SimpleText.hqx" --dst-dir "/MacAtrium/Apps/SimpleText" -q
# the real Prince of Persia (app + its Persia(BW/COLOR/LC) data files)
for h in Prince_of_Persia Persia_BW_ Persia_COLOR_ Persia_LC_; do
    $RB put-binhex "$OUT" "$RUN/pop/$h.hqx" --dst-dir "/MacAtrium/Apps/Prince of Persia" -q
done

# the launcher in Startup Items (skip if the folder doesn't exist, e.g. System 6)
if $RB ls "$OUT" "$STARTUP" >/dev/null 2>&1; then
    $RB put-macbinary "$OUT" "$APP" --dst-dir "$STARTUP" -q
    echo "assembled $OUT (launcher in: $STARTUP)"
else
    # No Startup Items (System 6): drop the launcher at the volume root so it can
    # be launched from the Finder.
    $RB put-macbinary "$OUT" "$APP" --dst-dir "/" -q
    echo "assembled $OUT (no Startup Items; launcher at volume root)"
fi
