#!/usr/bin/env python3
"""Merge the 8 LowEndMac scrape batches into one canonical model->System table."""
import json, re, glob, os

SCRAPE_DIR = os.path.join(os.path.dirname(__file__), "scrape")
OUT_JSONL  = os.path.join(os.path.dirname(__file__), "macatrium-models.jsonl")
OUT_MD     = os.path.join(os.path.dirname(__file__), "macatrium-models.md")

GROUPS = {
    "A": ("Compact & Mac II",        1),
    "B": ("LC & all-in-one",         1),
    "C": ("Centris/Quadra/Performa", 1),
    "D": ("68K PowerBook/Duo",       1),
    "E": ("PPC desktop",             2),
    "F": ("PPC laptop / G3-G4",      2),
    "G": ("iMac/iBook G3",           2),
    "H": ("Clone",                   2),
}
GROUP_ORDER = [g[0] for g in GROUPS.values()]

# Redundant combined/alias rows whose split forms exist elsewhere.
DROP = {
    "Mac LC 475 / Quadra 605",
    "Mac LC 630 / Performa 630",
    "Power Mac 5400 / Performa 5400",
    "Power Mac One",
}
# 68030 machines LowEndMac lists at "Mac OS 8.1" but which cannot run Mac OS 8
# (needs 68040+). True stock ceiling is 7.6.1.
DUO_68030_FIX = {"PowerBook Duo 210", "PowerBook Duo 230",
                 "PowerBook Duo 250", "PowerBook Duo 270c"}
# Verified against Apple Gestalt.h. Keyed by norm_name(), applied AFTER dedup.
# FIX = scraped value was wrong; FILL = scraped null, canonical has a value.
GESTALT_FIX = {
    "iisi": 18,                 # scrape had 10 (that's the Portable)
    "powerbook 190": 85,        # scrape had 122 (a Quadra 650 constant)
    "powerbook 190cs": 85,
    "powerbook duo 2300c": 124, # scrape had 118 (a Quadra 950 constant)
}
GESTALT_FILL = {
    "powerbook 140": 25, "powerbook 3400c": 306,
    "power mac 5500": 512, "power mac 6500": 513,
    "performa 5200": 41, "performa 5260": 41, "performa 5300": 41,
    "power mac 6400": 58, "performa 6400": 58,
    "kanga powerbook g3": 313, "wallstreet powerbook g3 series": 312,
    "pdq powerbook g3 series ii": 314,
}

FLOOR = 60004   # System 6.0.4 as an int key

def vkey(dotted, verbatim):
    """Return (normalized_dotted, int_key) from a dotted field (possibly
    dot-stripped like '922') falling back to the verbatim string."""
    s = (dotted or "").strip()
    digits = None
    if s:
        if "." in s:
            m = re.match(r"^(\d+(?:\.\d+){0,3})", s)
            if m: digits = m.group(1)
        elif s.isdigit():
            d = s
            digits = d[0] + "." + (d[1] if len(d) > 1 else "0") + \
                     (("." + d[2]) if len(d) > 2 else "")
    if not digits and verbatim:
        m = re.search(r"(?:System|Mac OS)?\s*(\d+)\.(\d+)(?:\.(\d+))?", verbatim)
        if m:
            digits = m.group(1) + "." + m.group(2) + \
                     (("." + m.group(3)) if m.group(3) else "")
        else:
            m = re.search(r"(?:System|Mac OS)\s+(\d+)\b", verbatim)
            if m: digits = m.group(1) + ".0"
    if not digits:
        return (None, None)
    p = [int(x) for x in digits.split(".")][:3]
    while len(p) < 3: p.append(0)
    key = p[0]*10000 + p[1]*100 + p[2]
    norm = f"{p[0]}.{p[1]}" + (f".{p[2]}" if p[2] else "")
    return (norm, key)

def norm_name(m):
    m = m.lower()
    m = re.sub(r"^macintosh\s+", "", m)      # "Macintosh Performa 600" == "Performa 600"
    m = m.replace("+", " plus ")             # keep LC III+ distinct from LC III
    m = re.sub(r"\(.*?\)", "", m)            # drop parentheticals
    m = re.sub(r"(\d)cd\b", r"\1", m)        # 5300cd -> 5300
    m = re.sub(r"[^a-z0-9]+", " ", m).strip()
    return re.sub(r"\s+", " ", m)

# ---- load -------------------------------------------------------------------
rows = []
for path in sorted(glob.glob(os.path.join(SCRAPE_DIR, "*.jsonl"))):
    letter = os.path.basename(path)[0].upper()
    gname, gprio = GROUPS.get(letter, ("?", 0))
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if not line: continue
            r = json.loads(line)
            if r["model"] in DROP: continue
            r["group"], r["_gprio"] = gname, gprio
            rows.append(r)

RAW = len(rows)

# ---- corrections ------------------------------------------------------------
for r in rows:
    if r["model"] in DUO_68030_FIX:
        r["maxOS"] = "Mac OS 7.6.1"; r["maxOSDotted"] = "7.6.1"
        r["_note"] = "maxOS corrected 8.1->7.6.1 (68030 cannot run Mac OS 8)"

# ---- dedup by normalized model name ----------------------------------------
merged = {}
conflicts = []
for r in rows:
    k = norm_name(r["model"])
    if k not in merged:
        merged[k] = dict(r)
        continue
    b = merged[k]
    if b.get("gestaltID") is None and r.get("gestaltID") is not None:
        b["gestaltID"] = r["gestaltID"]
    elif (b.get("gestaltID") is not None and r.get("gestaltID") is not None
          and b["gestaltID"] != r["gestaltID"]):
        conflicts.append((r["model"], b["gestaltID"], r["gestaltID"]))
    for f in ("modelNumber", "codeName"):
        if not b.get(f) and r.get(f): b[f] = r[f]
    if r["_gprio"] > b["_gprio"]:
        b["group"], b["_gprio"] = r["group"], r["_gprio"]

# ---- normalize versions + envelope -----------------------------------------
out = []
nfix = nfill = 0
for r in merged.values():
    nk = norm_name(r["model"])
    if nk in GESTALT_FIX:
        r["gestaltID"] = GESTALT_FIX[nk]; nfix += 1
    elif r.get("gestaltID") is None and nk in GESTALT_FILL:
        r["gestaltID"] = GESTALT_FILL[nk]; nfill += 1
    r["minSystemDotted"], r["minKey"] = vkey(r.get("minSystemDotted"), r.get("minSystem"))
    r["maxOSDotted"],     r["maxKey"] = vkey(r.get("maxOSDotted"),     r.get("maxOS"))
    r["inEnvelope"] = (r["maxKey"] is not None and r["maxKey"] >= FLOOR)
    out.append(r)

out.sort(key=lambda r: (GROUP_ORDER.index(r["group"]),
                        r.get("minKey") or 0, r.get("maxKey") or 0, r["model"]))

# ---- write canonical jsonl --------------------------------------------------
HEADER = (
    "# data/models.jsonl - Macintosh hardware -> System(OS) compatibility, one row per model.\n"
    "# Scraped from LowEndMac profiles; Gestalt IDs verified against Apple Gestalt.h.\n"
    "# gestaltID = numeric machineType (null for New-World PPC, which share generic 406).\n"
    "# minKey/maxKey = major*10000+minor*100+bug, for range compares. inEnvelope = maxOS>=6.0.4.\n"
    "# OS support is tier-based (see data/os-tiers.json); this per-model list is the reference.\n"
)
with open(OUT_JSONL, "w") as fh:
    fh.write(HEADER)
    for r in out:
        fh.write(json.dumps({
            "model": r["model"], "gestaltID": r.get("gestaltID"),
            "modelNumber": r.get("modelNumber"), "codeName": r.get("codeName"),
            "arch": r["arch"], "group": r["group"], "introduced": r.get("introduced"),
            "minSystem": r["minSystemDotted"], "maxOS": r["maxOSDotted"],
            "minKey": r.get("minKey"), "maxKey": r.get("maxKey"),
            "inEnvelope": r["inEnvelope"],
        }) + "\n")

# ---- write review markdown --------------------------------------------------
def cell(x): return "-" if x in (None, "") else str(x)
with open(OUT_MD, "w") as fh:
    fh.write("# MacAtrium — Mac model → System compatibility (from LowEndMac)\n\n")
    fh.write(f"{len(out)} distinct models. Min/Max = System range the *stock* machine "
             "boots. ⚠ = outside MacAtrium's 6.0.4–9.2.2 envelope.\n\n")
    for g in GROUP_ORDER:
        grp = [r for r in out if r["group"] == g]
        if not grp: continue
        fh.write(f"## {g} ({len(grp)})\n\n")
        fh.write("| Model | Gestalt | Model # | Code name | CPU | Min Sys | Max OS | Intro |\n")
        fh.write("|---|--:|---|---|---|---|---|---|\n")
        for r in grp:
            flag = "" if r["inEnvelope"] else " ⚠"
            fh.write(f"| {cell(r['model'])}{flag} | {cell(r.get('gestaltID'))} "
                     f"| {cell(r.get('modelNumber'))} | {cell(r.get('codeName'))} "
                     f"| {r['arch']} | {cell(r['minSystemDotted'])} "
                     f"| {cell(r['maxOSDotted'])} | {cell(r.get('introduced'))} |\n")
        fh.write("\n")

# ---- report -----------------------------------------------------------------
print(f"raw rows (after DROP): {RAW}")
print(f"distinct models:       {len(out)}")
print(f"gestalt IDs: {nfix} corrected, {nfill} filled from Gestalt.h")
print(f"  68K: {sum(1 for r in out if r['arch']=='68K')}   "
      f"PPC: {sum(1 for r in out if r['arch']=='PPC')}")
print(f"  outside envelope (<6.0.4): "
      + ", ".join(r["model"] for r in out if not r["inEnvelope"]))
print(f"  missing minSystem: "
      + (", ".join(r["model"] for r in out if not r.get("minKey")) or "none"))
print(f"  missing gestaltID: {sum(1 for r in out if r.get('gestaltID') is None)}")
by_gid = {}
for r in out:
    if r.get("gestaltID") is not None:
        by_gid.setdefault(r["gestaltID"], []).append(r["model"])
shared = {k: v for k, v in by_gid.items() if len(v) > 1}
print(f"  gestalt IDs shared by >1 model (board families / to verify): {len(shared)}")
for k in sorted(shared):
    print(f"     {k}: {', '.join(shared[k])}")
if conflicts:
    print("  merge gestalt conflicts:", conflicts)
print(f"\nwrote {OUT_JSONL}\nwrote {OUT_MD}")
