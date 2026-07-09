# Contributing to MacAtrium

Thanks for your interest! MacAtrium is a keyboard-driven launcher that boots a
vintage Mac into a curated library of games and apps. Contributions to the code,
the library data, and the Recommended list are all welcome.

## Building

See the **Building** section of the [README](README.md). In short: build the 68k
launcher with [Retro68](https://github.com/autc04/Retro68), build the `atrium`
tool with `cargo`, then assemble a disk image from a build config — start from
[`builds/example.json`](builds/example.json). The portable core (`json` / `catalog`
/ `model`) has host tests that run under plain `gcc`:

```sh
cd tests && make && ./host_test
```

## Ways to contribute

- **Library & metadata** — corrections and additions to `data/` (titles,
  categories, compatibility, art). The library is curated at build time; see
  [`data/README.md`](data/README.md).
- **Recommended list** — community favourites in `data/recommendations.md`.
- **Code** — the launcher (`src/`, C for Retro68) and the `atrium` build tool
  (`tools/atrium-tool/`, Rust). Please keep the portable core host-testable.
- **Design** — the `docs/` set describes the architecture and the locked decisions;
  read those before a large change.

## Pull requests

- Branch off `main`, keep each change focused, and describe **what** you changed and
  **how you verified it**.
- For launcher changes, run the host tests; for anything user-visible, a Snow/QEMU
  screenshot is very helpful.
- Please **do not** include copyrighted ROMs, system disks, or game binaries in a PR.

## Provenance

The bundled library data derives from the Macintosh Garden — see [NOTICE.md](NOTICE.md).
