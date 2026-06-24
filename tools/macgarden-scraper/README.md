# macgarden-scraper

Standalone, rate-limited, resumable scraper for the **Macintosh Garden** screenshot
/ box-art images referenced by the Infinite-Mac data dump. Background and dataset
analysis: [`docs/MacintoshGardenArchive.md`](../../docs/MacintoshGardenArchive.md).

> **Not committed yet** — left untracked on purpose while we iterate.

Python 3, **stdlib only** (no pip install). Images only — it does not fetch the
`.sit`/`.iso` downloads or the PDF manuals (easy to add later).

## Run

```sh
# both kinds, 10 images/sec (the ceiling), -> ~/macgarden-archive
tools/macgarden-scraper/scrape.py

# smoke test first:
tools/macgarden-scraper/scrape.py --kinds games --limit 5

# tune the cap later:
tools/macgarden-scraper/scrape.py --rate 20
```

Long run? Use `nohup … &` or `screen`/`tmux`; it's resumable, so a re-run just
fills in whatever's missing.

## Flags

| flag | default | meaning |
|------|---------|---------|
| `--archive` | `~/Infinite-Mac_20260312_214929.tar.gz` | tarball holding the ndjson |
| `--out` | `~/macgarden-archive` | output folder (created if needed) |
| `--kinds` | `games,apps` | comma list |
| `--rate` | `10` | max images/sec (ceiling) |
| `--limit` | `0` (all) | max titles per kind — for smoke tests |
| `--timeout` | `30` | per-request timeout (s) |
| `--retries` | `3` | attempts per file on transient errors |

## Output

```
~/macgarden-archive/
  metadata/games.ndjson  apps.ndjson      copied out of the tarball
  games/<nid>/info.json  <screenshot files…>
  apps/<nid>/info.json   <screenshot files…>
  manifest.csv          kind,nid,filename,status,bytes,http,url  (one row per fetch)
  scrape.log            (if you redirect output)
```

`status` is `ok` (downloaded), `skipped` (already present), `missing` (404/403/410
— stale entry, won't retry), or `error` (transient, retried then gave up).

## Looking things up — `mg.py`

Once images are down, `mg.py` indexes and searches the archive (keyed on `nid`):

```sh
mg.py index                      # build ~/macgarden-archive/index.{jsonl,csv}
mg.py search "monkey island"     # substring on title
mg.py search --vendor broderbund --kind games --with-art
mg.py show 1                     # full record + image list + folder path
```

`index.csv` (greppable / spreadsheet) and `index.jsonl` (adds the image filename
list) are the lookup table and the intended input to the enrichment pipeline.
`manifest.csv` is the download audit log, not a content index.

## URL scheme (confirmed live)

```
https://macintoshgarden.org/sites/macintoshgarden.org/files/screenshots/<filename>
```

Box art and gameplay screenshots share this one namespace; box art is recognizable
by filename (`_box_front`, `_box_back`, `cover`, …).
