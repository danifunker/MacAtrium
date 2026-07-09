#!/usr/bin/env python3
"""Generate the 9 small demo build configs (3 OS x 3 colour depths).

Each build installs the same handful of five classic games (Prince of Persia is
always included) via a curated-id `selection` list, so the harvested stubs keep
their library ids and reconnect to metadata + Macintosh Garden cover art.

The three colour "sets" follow the conventions already encoded in
data/templates.json:
  1-bit  (B&W)      -> art_depths ["1"],           512/384 KB launcher partition
  256    (colour)   -> art_depths ["1","8"],      1024/768 KB, art bounded 384px
  full   (millions) -> art_depths ["1","8","24"], 3584/3072 KB (24-bit ~1.4MB PICTs)
"""
import json
import os
import pathlib

REPO = pathlib.Path(__file__).resolve().parents[1]  # builds/ is one level below the repo root
OUT_DIR = os.path.join(REPO, "build")  # where the .hda images land (inside the repo)
CFG_DIR = os.path.join(REPO, "builds")

# The handful of games — same for every build. Curated library ids (donor
# sources resolve via data/donors.json + the configured MacPack folder).
GAMES = [
    "prince-of-persia",     # REQUIRED — from the 6.0.8 PoP base image
    "dark-castle",          # supplement
    "beyond-dark-castle",   # supplement
    "arkanoid-1-10",        # Supplement.vhd
    "space-invaders",       # Supplement.vhd
]

# (os key in templates.json, filename slug)
OSES = [
    ("6.0.8", "6.0.8"),
    ("7.1",   "7.1"),
    ("7.5.5", "7.5.5"),
]

# (depth slug, art_depths, [pref_kb, min_kb], max_art_size or None)
DEPTHS = [
    ("1bit",      ["1"],           [512, 384],   None),
    ("256color",  ["1", "8"],      [1024, 768],  "384x384"),
    ("fullcolor", ["1", "8", "24"], [3584, 3072], None),
]

# Small, only-grows disk targets (MB). Base images: 6.0.8/7.1 = 42 MB, 7.5.5 = 73 MB.
# Each target exceeds its base file size (so the grow step fires) with headroom
# for 5 games + art at that depth, while staying compact.
SIZE_MB = {
    ("6.0.8", "1bit"): 56,  ("6.0.8", "256color"): 64,  ("6.0.8", "fullcolor"): 80,
    ("7.1",   "1bit"): 56,  ("7.1",   "256color"): 64,  ("7.1",   "fullcolor"): 80,
    ("7.5.5", "1bit"): 88,  ("7.5.5", "256color"): 96,  ("7.5.5", "fullcolor"): 112,
}

manifest = []
for os_key, os_slug in OSES:
    for depth_slug, art_depths, app_mem, max_art in DEPTHS:
        name = f"MacAtrium-{os_slug}-{depth_slug}"
        cfg = {
            "base_os": os_key,
            "out": os.path.join(OUT_DIR, f"{name}.hda"),
            "launcher": os.path.join(REPO, "build/MacAtrium.bin"),
            "dataset": os.path.join(REPO, "data/library.jsonl"),
            "compatibility": os.path.join(REPO, "data/compatibility.jsonl"),
            "mg_archive": "/path/to/your/macintosh-garden-archive",
            "selection": {"mode": "list", "ids": GAMES},
            "art_depths": art_depths,
            "app_mem_kb": app_mem,
            "disk_size_mb": SIZE_MB[(os_key, depth_slug)],
            "rb_cli": "/path/to/rb-cli",
        }
        if max_art is not None:
            cfg["max_art_size"] = max_art
        cfg_path = os.path.join(CFG_DIR, f"gen-{os_slug}-{depth_slug}.json")
        with open(cfg_path, "w") as f:
            json.dump(cfg, f, indent=2)
            f.write("\n")
        manifest.append((os_key, depth_slug, cfg_path, cfg["out"], cfg["disk_size_mb"]))
        print(f"wrote {cfg_path}  ->  {cfg['out']}  ({cfg['disk_size_mb']} MB)")

print(f"\n{len(manifest)} configs generated.")
