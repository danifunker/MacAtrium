# data/ — sample catalog for MVP

`catalog.jsonl` is a tiny hand-authored catalog to develop the MVP launcher
against, before the host tooling and LaunchBox enrichment exist. Schema v2 is
documented in [../docs/06-content-pipeline.md](../docs/06-content-pipeline.md).

Three entries, deliberately minimal (just `id`, `name`, `categories`, `app`,
`year` — no images; MVP is text-only):

- **Prince of Persia (1992)** — the one you asked for. In `Games` *and* `Action`.
- Dark Castle (1986), Lemmings (1991) — filler so the list has rows to arrow
  through and more than one category to switch between (Games / Action / Puzzle,
  plus the synthesized **All**). Trim to just Prince of Persia if you want.

Note Prince of Persia carries **two** categories — that's the many-to-many model:
one app, many categories, no duplicate files.

## Using it for the MVP

1. This repo file uses normal LF endings. The host tooling will re-emit it as
   **CR / MacRoman / type `TEXT`** when targeting a Mac volume (the launcher's
   parser tolerates LF/CR/CRLF anyway).
2. Inject it: `rb-cli put test.dsk data/catalog.jsonl /MacAtrium/metadata/catalog.jsonl --type TEXT`
3. **To actually test launch-and-return:** the `app` paths are relative to
   `/MacAtrium`, so put a real app (or an alias) at the referenced path — e.g.
   copy an app to `/MacAtrium/Apps/Prince of Persia/Prince of Persia`. For a
   quick first test you can point one entry's `app` at any app you know is on the
   disk (the 7.x sample disks have **SimpleText**).
