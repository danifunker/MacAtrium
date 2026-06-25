# 19 — System-6 Process Manager (MultiFinder) — issue + spike plan (handoff)

**Status:** 🔬 open / parked. Start a fresh session from this doc.
**Owner action:** run Spike A; fall back to B; C is the no-work escape hatch.

---

## The issue (root cause)

On the **bare 6.0.8 appliance** MacAtrium is installed *as* `/System Folder/Finder`
(FNDR/MACS) and boots as the shell with **no Finder and no MultiFinder running →
no Process Manager**. Everything below follows from that:

- **`File System error -43` (fnfErr) launching companion-file apps.** With no
  Process Manager, `launch.c` uses the original Segment-Loader `_Launch` (classic,
  non-returning). That path does **not** set the launched app's **working
  directory** to its own folder, so an app that opens sibling data files by name
  can't find them. Verified with **Prince of Persia**: it launches (its `File Edit
  Game` menus appear) then immediately bombs with -43 because it can't find
  `Persia(BW)/Persia(COLOR)/Persia(LC)`. **Those files ARE present** in the image
  (confirmed by `rb-cli get-binhex` extracting all three) — it's purely a
  working-directory problem. Self-contained apps (Dark Castle) work; companion-file
  apps don't.
- **No launch-and-return.** The classic launch replaces MacAtrium; the System
  relaunches the shell (= the "Finder" file = MacAtrium) when the game quits. So
  there's no clean "quit → back to the launcher", which also makes the
  set-depth-for-game / **restore-depth-on-quit** model (docs: per-game `maxDepth`)
  awkward — the restore has to ride on the relaunch instead of returning in place.

**Contrast — 7.1 / System 7 works:** the Process Manager is always present, so the
*extended* `_Launch` (`launchContinue`) sets the working directory and returns
control. PoP on 7.1 launched fine (found its files, **no -43**); it was only B&W,
which is a *separate*, depth-only issue. So companion-file / color games are fine
on 7.1/9.2.2 today.

**Fact that unblocks the fix:** **MultiFinder is already installed** on the 6.0.8
base (`MacLC_6-0-8.hda` → `/System Folder/MultiFinder`, `ZSYS MACS`). It just isn't
*activated*. The user's `MacLC_6-0-8-POP.hda` (a known-good PoP disk) also has it
but likewise boots the plain Finder.

**Key unknown to settle first (blocks both A and B):** how to **activate
MultiFinder** in the build. **SOLVED (2026-06-25, Spike R):** activation = one
boot-block field, **`bbHelloName` (Str15 @ +0x5A)**, set `"Finder"` →
`"MultiFinder"`. **`bbShellName` (+0x1A) is NOT the field** (stays `"Finder"`) —
that's exactly why earlier `bbShellName→MultiFinder` swaps just booted the plain
Finder. The "auto-open an app" marking is a separate file **`/System
Folder/Finder Startup`** (`FDOC/MACS`): empty data fork, one **`'fndr'` id 0**
resource listing `(appName, volName, dirIDs)`. Found by diffing a clean
(Finder-set) image against a user-produced MultiFinder-set image — full write-up,
hex evidence, and how to patch it via `dd`/`rb-cli`:
**[20-system6-multifinder-set-startup-on-disk.md](20-system6-multifinder-set-startup-on-disk.md)**.

---

## Spike A — MultiFinder + MacAtrium **as the shell** (PRIORITY)

**Goal:** keep the appliance look (MacAtrium is the shell, no visible Finder) but
gain the Process Manager from MultiFinder, so launches set the working directory
and return.

**Hypothesis:** with MultiFinder active, the boot launches the file named "Finder"
(= MacAtrium, FNDR/MACS) as the shell; the Process Manager is provided by
MultiFinder regardless of which app is the shell. Then `gEnv.canLaunchReturn`
becomes **true**, `launch.c` takes the extended-launch branch (already written +
proven on 7.1), PoP finds its files (no -43) and returns to MacAtrium on quit.

**Steps**
1. **Activate MultiFinder (the crux).** Determine + apply the System-6 activation.
   Start by `rb-cli partmap`/boot-block dump on a disk after *Set Startup →
   MultiFinder* (have the user produce one, or diff `-POP.hda` boot blocks +
   Finder prefs against a Finder-only disk). Likely a boot-block flag and/or a
   Finder-prefs bit. Build a 6.0.8 image with that set + MacAtrium installed as the
   "Finder" (the existing `finder_replace` path).
2. **Verify MultiFinder is running.** Boot in Snow. Confirm the Process Manager is
   present — simplest signal: MacAtrium logs/branches `canLaunchReturn == true`
   (Gestalt `gestaltOSAttr` bit `gestaltLaunchCanReturn`). Add a temporary on-screen
   readout if needed (the launcher already has `env.canLaunchReturn`).
3. **Launch a companion-file app (PoP).** Expect **no -43** (working dir set) and a
   clean **return to MacAtrium** on quit.
4. **Re-test the depth flow.** With launch-and-return, the per-game depth
   set→restore (`do_launch`) now restores in place; confirm PoP comes up colour at
   8-bit (also needs the depth committed — see harness caveat below; verify on real
   Snow).

**Success:** PoP launches with no -43, returns to MacAtrium on quit, and the
appliance still boots straight into MacAtrium (no Finder visible).

**Risks / unknowns**
- MultiFinder activation mechanism (step 1) is the main unknown.
- Does MultiFinder *tolerate a non-Finder shell*? The shell is expected to handle
  some Finder duties (desktop, app-died/`appDied` events). MacAtrium already runs
  as the bare shell, so it may be fine, but watch for hangs/odd behavior when a
  launched app quits.
- RAM: System + MultiFinder + MacAtrium (2 MB SIZE) + game. 8 MB Mac II ceiling is
  plenty (PoP colour wants ~1.6 MB), but keep an eye on it.

---

## Spike B — MultiFinder + MacAtrium **as a startup app** (FALLBACK)

**Goal:** boot the real Finder under MultiFinder; auto-launch MacAtrium full-screen
over it. Most standard/robust; Finder stays resident (covered by MacAtrium).

**Steps**
1. Activate MultiFinder (same as A.1).
2. **Register MacAtrium as a startup app.** System 6 has **no Startup-Items folder**
   (that's System 7; our 7.1 build uses it). Replicate *Set Startup* marking the
   app — research where it's recorded (Desktop file? the app's Finder flags? a
   `STR `/`finf` in the System?). This is the hard part.
3. Build + boot: confirm the Finder comes up, MacAtrium auto-launches full-screen,
   and Cmd-Opt-Q drops back to the Finder.
4. Launch PoP → expect no -43 + return (Process Manager present).

**Success:** same as A but with the Finder present behind MacAtrium.

**Risks:** the startup-app registration is the blocker; less clean than A (Finder
running). If A works, skip B.

---

## Spike C — escape hatch (no new work)

Keep the bare 6.0.8 appliance for **self-contained** games (the B&W classics);
route **companion-file / colour** games (PoP) to the **7.1 / 9.2.2** builds, which
already have the Process Manager and run them (no -43). 6.0.8 simply never runs
PoP-class apps. Pick this if MultiFinder activation becomes a tar-pit.

---

## Spike R — reverse-engineer Finder + MultiFinder (foundational; feeds A & B)

The point: understand exactly what System 6's **Finder** and **MultiFinder** do, so
we can (a) find the **activation flag**, (b) learn the **shell contract** MacAtrium
must satisfy to run as the shell under MultiFinder (A), (c) learn how **startup apps
are recorded** (B), and (d) generally **maximize how correctly MacAtrium behaves on
6.0.8** — ideally implementing the minimal Finder/MultiFinder duties ourselves so we
depend on the real ones as little as possible.

**Artifacts to study** (pull from a 6.0.8 System Folder — `MacLC_6-0-8.hda` and the
known-good `MacLC_6-0-8-POP.hda`):
- `Finder` (FNDR/MACS) — resources + CODE.
- `MultiFinder` (ZSYS MACS) — the boot loader + the Process Manager it carries.
- **Boot blocks** (first ~1 KB of the HFS partition) — `bbShellName` (Str15 @ +0x1A)
  and the surrounding `bbVersion`/`bbPageFlags`/other fields.
- Finder prefs / `finf` / System resources that record *Set Startup*.

**Questions to answer**
1. **Activation:** what does *Special → Set Startup → MultiFinder* change vs
   Finder-only? (a boot-block field beyond `bbShellName`? a `finf` bit? a System
   resource?) — the flag that makes the boot launch MultiFinder.
2. **Shell launch:** how MultiFinder decides which app is the shell and launches it,
   so MacAtrium-as-"Finder" is started correctly.
3. **Shell contract:** what the shell must handle under MultiFinder (the `appDied`/
   core AppleEvents, the desktop/`GrayRgn`, SwitchLaunch) so MacAtrium doesn't wedge
   when a game quits — and which of these we should implement ourselves.
4. **Startup-app marking** (for B): how the Finder records which apps open at
   startup on System 6 (no Startup-Items folder until System 7).
5. **Process Manager surface** MultiFinder exposes on 6.0.8, cross-checked against
   L1/L2 in this doc (extended `_Launch` + `launchContinue` + working-dir behavior).

**Tools / method**
- Resource forks: `rb-cli get-binhex IMG "/System Folder/Finder" out.hqx` (both
  forks); decode + Retro68 `DeRez` for templates, or **ResEdit in-emulator** for
  interactive browsing (ResEdit 2.1.3 is on `-POP.hda`).
- Boot blocks: `rb-cli partmap` + a raw read of the partition's first 1 KB.
- **68k disassembly** of CODE resources: Ghidra (68k), or `m68k-apple-macos-objdump`
  / the Retro68 toolchain on extracted CODE — focus the Finder's Set-Startup handler
  and MultiFinder's shell-launch entry.
- Cross-reference: *Inside Macintosh: Processes* (Process Manager / `_Launch` param
  block), Apple's *Programmer's Guide to MultiFinder* + the System-6 Tech Notes.
- **Diff-first shortcut (do this before any disasm):** make two disks — *Set Startup
  = Finder* vs *= MultiFinder* — and diff boot blocks + changed System-Folder files.
  The delta IS the activation flag, far cheaper than disassembly. (Ask the user to
  produce the MultiFinder-set disk, or use `-POP.hda` if its config is known.)

**Deliverables → fold into A/B**
- The exact MultiFinder-activation flag + how to set it via rb-cli (boot-block patch
  and/or a prefs/resource write).
- The minimal shell contract MacAtrium must meet to be a stable MultiFinder shell.
- The startup-app registration format (B).
- Confirmation that 6.0.8+MultiFinder gives `launchCanReturn` + working-dir-correct
  launches.

**Scope discipline:** prefer diff/observe for the *activation* and *startup-marking*
questions; reserve full disassembly for the *shell contract* (what MacAtrium must do)
and only if MultiFinder misbehaves with a non-Finder shell.

## Handoff context (state as of this session)

- **Code that's relevant:**
  - `src/launch.c` — branches on `canReturn`: extended `_Launch`+`launchContinue`
    (System 7 / MultiFinder, proven) vs the **classic Segment-Loader launch**
    (bare System 6, the -43 culprit; `PBHSetVolSync` to the app's parent + old
    param block). Spikes A/B make `canReturn` true → the extended branch runs and
    the classic branch becomes dead code on 6.0.8.
  - `src/main.c` `do_launch()` — per-game depth set→launch→restore (notice via
    `show_switch_message`). Forced-256-at-startup was **removed** this session
    (colour is now user-controlled via Settings → Color Depth).
- **Test images:**
  - `~/MacAtrium-PoP-test.hda` — PoP on the **6.0.8 appliance** base → this is the
    one that **-43s** (wrong environment). Rebuild PoP on a 7.1 base, or on a
    MultiFinder-6.0.8 once a spike lands.
  - `~/MacAtrium-7.1.hda` — has PoP; runs it without -43 (B&W due to depth).
- **Harness caveats** (`tools/snow-harness/macatrium_harness.rs`): keyboard-only,
  **cannot drive Finder type-select** (so can't launch via a real Finder), and
  **cannot commit a screen depth *raise* to 8-bit at launch** (uncapped, no VBL —
  a *drop* to 1-bit does commit). Verify colour/return on **real Snow**.
- **Reference disk:** `~/MacOS_SampleDisks/MacLC_6-0-8-POP.hda` — MultiFinder
  installed, PoP installed, known-good. Diff its boot blocks + System Folder prefs
  vs a Finder-only 6.0.8 disk to find the MultiFinder-activation flag.
- **Related docs/memory:** [05-finder-replacement.md](05-finder-replacement.md),
  [08-launching-system.md](08-launching-system.md),
  [16-startup-items.md](16-startup-items.md); memories `system-608-boot-shell`,
  `overrides-db-maxdepth`, `build-base-from-user-disks`.

## Still-open follow-ups (separate from this spike)
- **PoP colour** needs an 8-bit screen committed at launch (depth, not the launch
  model) — solvable once the launch model is fixed; verify on real Snow.
- **Persist the user's Color-Depth choice** across boots (prefs) — TODO noted in
  `main.c`; lets the appliance come back at the user's depth after a game.
