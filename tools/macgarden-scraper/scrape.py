#!/usr/bin/env python3
"""MacGarden screenshot scraper.

Downloads the screenshot / box-art images referenced by the Infinite-Mac /
Macintosh Garden data dump (see docs/MacintoshGardenArchive.md), rate-limited and
resumable. Images only — not the .sit/.iso downloads or the PDF manuals.

Source of truth: the `games.ndjson` / `apps.ndjson` inside the Infinite-Mac
tarball. Each record's `screenshots[].filename` resolves to:

    https://macintoshgarden.org/sites/macintoshgarden.org/files/screenshots/<filename>

(confirmed live for both games and apps; box art lives in the same namespace and
is distinguishable by filename, e.g. *_box_front.jpg).

The download rate is capped GLOBALLY at `--rate` images/sec by a thread-safe gate;
a small pool of worker threads runs downloads concurrently so that the cap — not
per-request latency — is the binding constraint.

Output layout (a NEW folder, reused later by the enrichment pipeline):

    <out>/
      metadata/games.ndjson  apps.ndjson    (copied out of the tarball)
      games/<nid>/info.json  <screenshot files…>
      apps/<nid>/info.json   <screenshot files…>
      manifest.csv                          (kind,nid,filename,status,bytes,http,url)

Resumable: a file that already exists non-empty is skipped, so re-running just
fills gaps. Ctrl-C stops scheduling new work and lets in-flight downloads finish.

Usage:
    ./scrape.py                       # both kinds, 10 img/s, -> ~/macgarden-archive
    ./scrape.py --kinds games --limit 5   # smoke test: first 5 games only
    ./scrape.py --rate 20             # raise the cap (be polite)
"""
import argparse
import csv
import json
import os
import signal
import sys
import tarfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed

import glob

SCREENSHOT_BASE = "https://macintoshgarden.org/sites/macintoshgarden.org/files/screenshots/"
USER_AGENT = "MacAtrium-archive/0.1 (local research tool; contact: danifunkervogt@gmail.com)"


def _default_archive_src():
    """Newest ~/Infinite-Mac_*.tar.gz, so the user needn't know the dated name."""
    hits = sorted(glob.glob(os.path.expanduser("~/Infinite-Mac_*.tar.gz")))
    return hits[-1] if hits else os.path.expanduser("~/Infinite-Mac.tar.gz")


# The data store: $MACATRIUM_MG_ARCHIVE, else ~/macgarden-archive (shared with the
# atrium CLI and the Manager so one path drives everything).
DEFAULT_ARCHIVE = _default_archive_src()
DEFAULT_OUT = os.environ.get("MACATRIUM_MG_ARCHIVE") or os.path.expanduser("~/macgarden-archive")

_stop = False


def _handle_sigint(_signum, _frame):
    global _stop
    if _stop:  # second Ctrl-C: hard exit
        sys.exit(130)
    _stop = True
    print("\n[!] Ctrl-C received — no new downloads will start; letting in-flight "
          "ones finish. (Ctrl-C again to force.)", file=sys.stderr)


signal.signal(signal.SIGINT, _handle_sigint)


class RateGate:
    """Caps the *start* rate of downloads to `rate` per second, GLOBALLY across
    threads (a ceiling, not a target). Thread-safe."""

    def __init__(self, rate):
        self.min_interval = (1.0 / rate) if rate and rate > 0 else 0.0
        self.next_slot = 0.0
        self.lock = threading.Lock()

    def wait(self):
        if self.min_interval <= 0:
            return
        with self.lock:
            now = time.monotonic()
            slot = max(now, self.next_slot)
            self.next_slot = slot + self.min_interval
        delay = slot - time.monotonic()
        if delay > 0:
            time.sleep(delay)


def ensure_metadata(archive, metadir):
    """Extract games.ndjson / apps.ndjson out of the tarball into <out>/metadata
    (only if not already there). Returns {kind: path}."""
    os.makedirs(metadir, exist_ok=True)
    wanted = {"games": "games.ndjson", "apps": "apps.ndjson"}
    paths = {}
    missing = {k: fn for k, fn in wanted.items()
               if not os.path.exists(os.path.join(metadir, fn))}
    if missing:
        if not os.path.exists(archive):
            sys.exit(f"error: archive not found: {archive}")
        with tarfile.open(archive, "r:*") as tar:
            members = {os.path.basename(m.name): m for m in tar.getmembers()
                       if m.isfile() and os.path.basename(m.name) in missing.values()}
            for fn, member in members.items():
                src = tar.extractfile(member)
                dest = os.path.join(metadir, fn)
                with open(dest, "wb") as out:
                    out.write(src.read())
                print(f"  extracted {fn} -> {dest}")
    for k, fn in wanted.items():
        p = os.path.join(metadir, fn)
        if os.path.exists(p):
            paths[k] = p
    return paths


def iter_records(ndjson_path, key):
    """Yield (nid, record) for each line. `key` is 'game' or 'app'."""
    with open(ndjson_path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            rec = obj.get("data", {}).get(key)
            if rec is not None:
                yield obj.get("nid"), rec


def build_tasks(kind, ndjson_path, out_root, limit):
    """Walk the ndjson, write each title's info.json, and return the list of
    pending downloads (skipping files already present). A task is a dict with
    kind/nid/filename/dest/url."""
    rec_key = "game" if kind == "games" else "app"
    kind_dir = os.path.join(out_root, kind)
    tasks = []
    seen_total = skipped = 0
    n_titles = 0
    for nid, rec in iter_records(ndjson_path, rec_key):
        if limit and n_titles >= limit:
            break
        n_titles += 1
        shots = rec.get("screenshots") or []
        if not shots:
            continue
        title_dir = os.path.join(kind_dir, str(nid))
        os.makedirs(title_dir, exist_ok=True)
        info_path = os.path.join(title_dir, "info.json")
        if not os.path.exists(info_path):
            with open(info_path, "w", encoding="utf-8") as f:
                json.dump({"nid": nid, **rec}, f, ensure_ascii=False, indent=1)

        seen_here = set()
        for shot in shots:
            filename = (shot or {}).get("filename")
            if not filename or filename in seen_here:
                continue
            seen_here.add(filename)
            seen_total += 1
            dest = os.path.join(title_dir, filename)
            if os.path.exists(dest) and os.path.getsize(dest) > 0:
                skipped += 1
                continue
            tasks.append({
                "kind": kind, "nid": nid, "filename": filename, "dest": dest,
                "url": SCREENSHOT_BASE + urllib.parse.quote(filename),
            })
    return tasks, seen_total, skipped


def download(task, gate, timeout, retries):
    """Rate-gated download. Returns the task dict augmented with status/bytes/http."""
    req = urllib.request.Request(task["url"], headers={"User-Agent": USER_AGENT})
    last_http = ""
    for attempt in range(1, retries + 1):
        if _stop:
            break
        gate.wait()
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                data = resp.read()
            tmp = task["dest"] + ".part"
            with open(tmp, "wb") as out:
                out.write(data)
            os.replace(tmp, task["dest"])
            return {**task, "status": "ok", "bytes": len(data), "http": 200}
        except urllib.error.HTTPError as e:
            last_http = e.code
            if e.code in (404, 403, 410):  # not coming back — don't retry
                return {**task, "status": "missing", "bytes": 0, "http": e.code}
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            last_http = getattr(e, "reason", e).__class__.__name__
        if attempt < retries and not _stop:
            time.sleep(min(2 ** attempt, 10))
    return {**task, "status": "error", "bytes": 0, "http": last_http}


def main():
    ap = argparse.ArgumentParser(description="Scrape Macintosh Garden screenshots/box art (images only).")
    ap.add_argument("--archive", default=DEFAULT_ARCHIVE, help="Infinite-Mac tarball (source of the ndjson)")
    ap.add_argument("--out", default=DEFAULT_OUT, help="output folder (created if needed)")
    ap.add_argument("--kinds", default="games,apps", help="comma list: games,apps")
    ap.add_argument("--rate", type=float, default=10.0, help="max images/sec, global ceiling (default 10)")
    ap.add_argument("--workers", type=int, default=24, help="concurrent download threads (to fill the rate cap)")
    ap.add_argument("--limit", type=int, default=0, help="max titles per kind (0=all; for smoke tests)")
    ap.add_argument("--timeout", type=float, default=30.0, help="per-request timeout (s)")
    ap.add_argument("--retries", type=int, default=3, help="attempts per file on transient errors")
    ap.add_argument("--progress-every", type=int, default=100, help="print progress every N fetched")
    args = ap.parse_args()

    kinds = [k.strip() for k in args.kinds.split(",") if k.strip()]
    out_root = os.path.expanduser(args.out)
    os.makedirs(out_root, exist_ok=True)

    print(f"[*] archive : {args.archive}")
    print(f"[*] out     : {out_root}")
    print(f"[*] kinds   : {kinds}   rate: {args.rate}/s   workers: {args.workers}   limit: {args.limit or 'all'}")
    meta = ensure_metadata(args.archive, os.path.join(out_root, "metadata"))

    # Plan: walk the ndjson(s), write info.json, collect pending downloads.
    tasks = []
    pre_skipped = pre_seen = 0
    for kind in kinds:
        if kind not in meta:
            print(f"[!] no ndjson for kind '{kind}', skipping")
            continue
        kt, seen, skipped = build_tasks(kind, meta[kind], out_root, args.limit)
        print(f"  {kind}: {seen} images referenced, {skipped} already present, {len(kt)} to fetch")
        tasks.extend(kt)
        pre_seen += seen
        pre_skipped += skipped

    counts = {"seen": pre_seen, "ok": 0, "skipped": pre_skipped, "missing": 0, "error": 0, "bytes": 0}
    if not tasks:
        print("\n[done] nothing to fetch (all present).")
        return

    gate = RateGate(args.rate)
    manifest_path = os.path.join(out_root, "manifest.csv")
    new_manifest = not os.path.exists(manifest_path)
    start = time.monotonic()
    fetched = 0
    print(f"\n[*] fetching {len(tasks)} images with {args.workers} workers, capped at {args.rate}/s ...")
    with open(manifest_path, "a", newline="", encoding="utf-8") as mf:
        writer = csv.writer(mf)
        if new_manifest:
            writer.writerow(["kind", "nid", "filename", "status", "bytes", "http", "url"])
        with ThreadPoolExecutor(max_workers=args.workers) as pool:
            futures = {pool.submit(download, t, gate, args.timeout, args.retries): t for t in tasks}
            try:
                for fut in as_completed(futures):
                    r = fut.result()
                    counts[r["status"]] = counts.get(r["status"], 0) + 1
                    counts["bytes"] += r["bytes"]
                    writer.writerow([r["kind"], r["nid"], r["filename"], r["status"],
                                     r["bytes"], r["http"], r["url"]])
                    fetched += 1
                    if fetched % args.progress_every == 0:
                        mf.flush()
                        _print_progress(counts, start)
            except KeyboardInterrupt:
                pass

    print("\n[done]" if not _stop else "\n[interrupted]")
    _print_progress(counts, start)
    print(f"  manifest: {manifest_path}")


def _print_progress(counts, start):
    mb = counts["bytes"] / (1024 * 1024)
    elapsed = max(time.monotonic() - start, 0.001)
    rate = counts["ok"] / elapsed
    print(f"  ok={counts['ok']} skip={counts['skipped']} missing={counts['missing']} "
          f"err={counts['error']}  {mb:.1f} MB  {rate:.1f} img/s  {elapsed:.0f}s", flush=True)


if __name__ == "__main__":
    main()
