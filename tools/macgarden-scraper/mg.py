#!/usr/bin/env python3
"""mg.py — index & search the Macintosh Garden archive.

Companion to scrape.py. Turns the archive (see docs/MacintoshGardenArchive.md) into
something you can look things up in fast, and is the join point for the enrichment
pipeline. Everything keys on `nid` (Macintosh Garden node id) — the folder name,
the ndjson, the manifest, and this index all share it.

    mg.py index                         # (re)build index.jsonl + index.csv at the archive root
    mg.py search "monkey island"        # substring match on title
    mg.py search --vendor broderbund --with-art --kind games
    mg.py show 10000                    # full record + image list + folder path

The index has one row per title:
    nid, kind, title, year, publisher, author, category, url_alias,
    n_images (referenced), n_box (box/cover detected), on_disk (files present), dir
index.jsonl additionally carries the `images` filename list.
"""
import argparse
import csv
import json
import os
import re
import sys

# The data store: $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive (shared with the
# atrium CLI, the Manager, and scrape.py so one path drives everything).
DEFAULT_ARCHIVE = os.environ.get("MACATRIUM_MG_ARCHIVE") or os.path.expanduser("~/macgarden-archive")
BOX_RE = re.compile(r"box|cover|_back|_front", re.I)
SOURCES = [("games", "game", "games.ndjson"), ("apps", "app", "apps.ndjson")]


def load_records(archive):
    for kind, key, fn in SOURCES:
        path = os.path.join(archive, "metadata", fn)
        if not os.path.exists(path):
            continue
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                obj = json.loads(line)
                rec = obj.get("data", {}).get(key)
                if rec is not None:
                    yield kind, obj.get("nid"), rec


def title_row(archive, kind, nid, rec):
    shots = rec.get("screenshots") or []
    files = [s.get("filename") for s in shots if s and s.get("filename")]
    box = [f for f in files if BOX_RE.search(f)]
    rel = os.path.join(kind, str(nid))
    absdir = os.path.join(archive, rel)
    on_disk = 0
    if os.path.isdir(absdir):
        on_disk = len([f for f in os.listdir(absdir)
                       if f != "info.json" and not f.endswith(".part")])
    return {
        "nid": nid,
        "kind": kind,
        "title": rec.get("title", ""),
        "year": rec.get("year", ""),
        "publisher": rec.get("publisher") or [],
        "author": rec.get("author") or [],
        # games use `category`, apps use `category_app`
        "category": rec.get("category") or rec.get("category_app") or [],
        "url_alias": rec.get("url_alias", ""),
        "n_images": len(files),
        "n_box": len(box),
        "on_disk": on_disk,
        "dir": rel,
        "images": files,
    }


def cmd_index(archive, _args):
    rows = [title_row(archive, k, n, r) for k, n, r in load_records(archive)]
    if not rows:
        sys.exit(f"error: no metadata ndjson under {archive}/metadata (run scrape.py first)")
    jl = os.path.join(archive, "index.jsonl")
    with open(jl, "w", encoding="utf-8") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    cv = os.path.join(archive, "index.csv")
    with open(cv, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["nid", "kind", "title", "year", "publisher", "category",
                    "n_images", "n_box", "on_disk", "url_alias", "dir"])
        for r in rows:
            w.writerow([r["nid"], r["kind"], r["title"], r["year"],
                        "; ".join(r["publisher"]), "; ".join(r["category"]),
                        r["n_images"], r["n_box"], r["on_disk"], r["url_alias"], r["dir"]])
    games = sum(1 for r in rows if r["kind"] == "games")
    with_art = sum(1 for r in rows if r["on_disk"] > 0)
    print(f"indexed {len(rows)} titles ({games} games, {len(rows) - games} apps; "
          f"{with_art} have images on disk)")
    print(f"  -> {jl}")
    print(f"  -> {cv}")


def load_index(archive):
    jl = os.path.join(archive, "index.jsonl")
    if not os.path.exists(jl):
        print("[index.jsonl missing — building it]", file=sys.stderr)
        cmd_index(archive, None)
    with open(jl, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def cmd_search(archive, args):
    idx = load_index(archive)
    q = (args.query or "").lower()

    def hit(r):
        if q and q not in r["title"].lower():
            return False
        if args.kind and r["kind"] != args.kind:
            return False
        if args.year and str(r["year"]) != str(args.year):
            return False
        if args.category and not any(args.category.lower() in c.lower() for c in r["category"]):
            return False
        if args.vendor and not any(args.vendor.lower() in p.lower()
                                   for p in (r["publisher"] + r["author"])):
            return False
        if args.with_art and r["on_disk"] == 0:
            return False
        return True

    hits = sorted((r for r in idx if hit(r)), key=lambda r: (r["kind"], r["title"].lower()))
    for r in hits[:args.limit]:
        title = (r["title"] or "")[:48]
        print(f'{r["kind"]:5} {str(r["nid"]):>6}  {str(r["year"] or "----")[:4]:4}  '
              f'{title:48}  art:{r["on_disk"]}/{r["n_images"]}  {r["dir"]}')
    shown = min(args.limit, len(hits))
    print(f"\n{len(hits)} match(es)" + (f" (showing {shown}; raise --limit)" if len(hits) > shown else ""))


def cmd_show(archive, args):
    for kind in ("games", "apps"):
        d = os.path.join(archive, kind, str(args.nid))
        info = os.path.join(d, "info.json")
        if os.path.exists(info):
            with open(info, encoding="utf-8") as f:
                print(json.dumps(json.load(f), ensure_ascii=False, indent=2))
            imgs = sorted(f for f in os.listdir(d)
                          if f != "info.json" and not f.endswith(".part"))
            print(f"\n# {len(imgs)} image(s) in {d}:")
            for f in imgs:
                print("  ", f)
            return
    sys.exit(f"nid {args.nid} not found on disk under {archive}")


def main():
    ap = argparse.ArgumentParser(description="Index & search the Macintosh Garden archive.")
    ap.add_argument("--archive", default=DEFAULT_ARCHIVE, help="archive root (default ~/macgarden-archive)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("index", help="(re)build index.jsonl + index.csv")

    sp = sub.add_parser("search", help="search titles")
    sp.add_argument("query", nargs="?", default="", help="substring matched against the title")
    sp.add_argument("--kind", choices=["games", "apps"], help="restrict to games or apps")
    sp.add_argument("--year", help="exact year")
    sp.add_argument("--category", help="category substring")
    sp.add_argument("--vendor", help="publisher/author substring")
    sp.add_argument("--with-art", action="store_true", help="only titles with images on disk")
    sp.add_argument("--limit", type=int, default=40, help="max rows to print (default 40)")

    sh = sub.add_parser("show", help="print a title's full record + image list")
    sh.add_argument("nid", type=int)

    args = ap.parse_args()
    archive = os.path.expanduser(args.archive)
    {"index": cmd_index, "search": cmd_search, "show": cmd_show}[args.cmd](archive, args)


if __name__ == "__main__":
    main()
