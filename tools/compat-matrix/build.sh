#!/bin/sh
# Regenerate data/models.jsonl and docs/models-matrix.html from scrape/*.jsonl.
# os-tiers.json is hand-maintained and is NOT regenerated here.
set -e
cd "$(dirname "$0")"
python3 merge.py
python3 gen_artifact.py
cp macatrium-models.jsonl ../../data/models.jsonl
cp models-table.html      ../../docs/models-matrix.html
echo "regenerated: data/models.jsonl, docs/models-matrix.html"
