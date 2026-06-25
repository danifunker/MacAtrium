# How System 6 records the MultiFinder "Set Startup" choice on disk

*A small reverse-engineering write-up. Tested on a Macintosh LC running System
6.0.8, disk images inspected offline with [`rusty-backup`](#tools)'s `rb-cli`.*

## TL;DR

Booting System 6 under **MultiFinder**, and having it **auto-open an application
at startup**, are recorded in two completely separate places on the HFS volume —
neither of which is documented in an obvious spot:

1. **Activating MultiFinder** rewrites exactly **one boot-block field**: the
   `Str15` at offset **`+0x5A`** (`bbHelloName`, the "startup program" name),
   changing it from `"Finder"` to `"MultiFinder"`. The shell field `bbShellName`
   at `+0x1A` is **left untouched** at `"Finder"`. So the System-6 startup code
   launches whatever is named in **`bbHelloName`**, not `bbShellName` — which is
   why the common "just point `bbShellName` at MultiFinder" trick does nothing.

2. **The list of apps to auto-open at startup** lives in a normal file,
   **`System Folder:Finder Startup`** (type `FDOC`, creator `MACS`). Its **data
   fork is empty**; the list is a single resource of type **`'fndr'` (ID 0)** in
   the **resource fork**, encoding each entry as *(application name, volume name,
   HFS directory IDs)*. System 6 has no *Startup Items* folder (that arrives with
   System 7); this file is its equivalent.

Everything below is the evidence and the method, so you can reproduce or correct
it.

---

## Background — why bother

I maintain a single-application "appliance" build of System 6.0.8: a small launcher
installed *as* the Finder (it replaces `System Folder:Finder`, retyped `FNDR/MACS`),
so the machine boots straight into the launcher with no desktop. That works, but
because the bare 6.0.8 boot has **no Process Manager** (no Finder, no MultiFinder),
the launcher can only use the original Segment-Loader `_Launch`. That path is
non-returning and, more painfully, does **not** set the launched application's
working directory — so any app that opens sibling data files *by name* dies with
**File System error −43 (`fnfErr`)**. Prince of Persia is the canonical victim: it
launches, its menus appear, then it can't find `Persia(COLOR)` / `Persia(BW)` and
bombs, even though those files sit right next to it.

System 7 doesn't have this problem: the Process Manager is always present, so the
extended `_Launch` sets the working directory and returns control. The interesting
question is the in-between: **System 6 + MultiFinder** also has the Process Manager.
MultiFinder ships in the 6.0.8 System Folder (`MultiFinder`, type `ZSYS/MACS`) — it
just isn't *activated* by default. So: **what, exactly, on disk, activates it?**

Folklore says "Set Startup writes the boot blocks." Some say it's `bbShellName`.
Pointing `bbShellName` at `"MultiFinder"` empirically does nothing. So I diffed real
disks to find the truth.

---

## Method

The cheap, decisive approach is a **before/after diff** rather than disassembly:

1. Build a clean 6.0.8 disk (real Finder, MultiFinder present but inactive). This is
   the **baseline** — and note it has never been booted, so it carries zero
   first-boot noise.
2. Boot a copy, do **only** `Special → Set Startup`, choose **MultiFinder** and mark
   one app to open at startup, then `Special → Shut Down` (clean, so the choice
   flushes). Change nothing else.
3. Diff the two images: the boot blocks, then the file tree.

The two images here are both 40 MB APM disks with the HFS partition starting at
LBA 96 (byte offset `0xC000`). The boot blocks are the first 1 KiB of the partition.

---

## Finding 1 — the activation flag is `bbHelloName` (+0x5A)

### The boot-block header

System 6/7 boot blocks begin with a `BootBlkHdr` (Inside Macintosh: Files). The
fields relevant here, with their byte offsets inside the partition's first sector:

| Offset | Field | Type | Usual value |
|-------:|-------|------|-------------|
| `+0x00` | `bbID` | word | `0x4C4B` (`'LK'`) |
| `+0x02` | `bbEntry` | long | branch to boot code |
| `+0x06` | `bbVersion` | word | `0x0017` here |
| `+0x0A` | `bbSysName` | Str15 | `System` |
| `+0x1A` | **`bbShellName`** | Str15 | `Finder` |
| `+0x2A` | `bbDbg1Name` | Str15 | `Macsbug` |
| `+0x3A` | `bbDbg2Name` | Str15 | `Disassembler` |
| `+0x4A` | `bbScreenName` | Str15 | `StartUpScreen` |
| `+0x5A` | **`bbHelloName`** | Str15 | `Finder` |
| `+0x6A` | `bbScrapName` | Str15 | `Clipboard File` |

(A `Str15` is one length byte + 15 bytes of characters = 16 bytes.)

### The diff

Comparing the baseline and the MultiFinder-set boot blocks, **exactly 16 bytes
differ — the entire `bbHelloName` field at `+0x5A`**. Nothing else in the boot
blocks changes. `bbShellName` at `+0x1A` stays `Finder`.

Baseline (`+0x5A`):

```
0000005A: 06 46 69 6E 64 65 72 20 20 20 20 20 20 20 20 20   .Finder
          ^len="Finder"(6) + space padding
```

MultiFinder-set (`+0x5A`):

```
0000005A: 0B 4D 75 6C 74 69 46 69 6E 64 65 72 30 2A 83 00   .MultiFinder0*..
          ^len=11 "MultiFinder" + leftover padding (not zeroed)
```

So the "Set Startup → MultiFinder" choice **is** simply: write the Pascal string
`MultiFinder` into `bbHelloName`. (Trailing bytes after the name aren't cleared —
a harmless cosmetic detail, but it tells you the Finder overwrites length+chars in
place rather than rebuilding the field.)

### Why this matters / why `bbShellName` is a red herring

The takeaway is that **the System-6 startup code launches the program named in
`bbHelloName` (`+0x5A`)**, treating it as *the* startup program. `bbShellName`
(`+0x1A`) is not the field that selects what boots. Set `bbHelloName` to
`MultiFinder` and the boot loads MultiFinder, which brings up the Process Manager
and then launches the real shell; leave it `Finder` and you get the plain,
Process-Manager-less Finder. Every attempt I'd seen (mine included) that swapped
`bbShellName` failed for the obvious reason in hindsight: wrong field.

### Setting it without a Mac

You can patch `bbHelloName` straight into the image. The field is at
`partition_offset + 0x5A`; write a 16-byte `Str15` (`0x0B` + `"MultiFinder"` +
padding). With the partition at `0xC000` that's absolute offset `0xC05A`:

```sh
# 0x0B 'M' 'u' 'l' 't' 'i' 'F' 'i' 'n' 'd' 'e' 'r' then pad to 16 bytes
printf '\x0bMultiFinder\x00\x00\x00\x00' \
  | dd of=disk.hda bs=1 seek=$((0xC05A)) conv=notrunc
```

(Find `partition_offset` with `rb-cli inspect disk.hda`; it's `start_lba * 512`.)

---

## Finding 2 — the startup-app list: `Finder Startup`

The file-tree diff (root + System Folder) showed **one new file** plus expected
first-boot churn:

```
> FILE  0     FDOC MACS  System Folder:Finder Startup    (new)
  FILE  1280→2816 DTFL DMGR  Desktop DF                  (rebuilt — boot noise)
```

`Finder Startup` (`FDOC/MACS`) is where System 6 records which applications open at
startup. Its **data fork is empty (0 bytes)**; everything is in the **resource
fork** (400 bytes on this disk).

### Resource-fork anatomy

Standard classic-Mac resource fork. Header:

```
+0x000  00 00 01 00   offset to resource data = 0x100
+0x004  00 00 01 5E   offset to resource map  = 0x15E
+0x008  00 00 00 5E   length of resource data = 0x5E
+0x00C  00 00 00 32   length of resource map  = 0x32
```

The resource **map** declares a single type, **`'fndr'`**, with one resource,
**ID 0, no name**, whose data is at `+0x100`:

```
type list:  66 6E 64 72 ('fndr')  count-1=0
ref list:   id=0x0000  name=0xFFFF(none)  dataOff=0x000000  → data at 0x100
```

(There's also a custom `ICN#`-style icon bitmap earlier in the fork — the little
document icon the Finder draws for this file — which I've left out; it isn't part
of the startup list.)

### The `'fndr' (0)` payload — the actual list

Resource data at `+0x100` (length `0x5E`; the inner record is `0x5A` bytes):

```
+0x100  00 00 00 5A             resource data length (0x5A)
+0x104  00 00 00 01             version / tag = 1
+0x108  00 00 00 01             entry count   = 1   (one startup app)
+0x10C  00 00 00 24             record length = 0x24
+0x110  00 00 09 'MacAtrium'    app name (Pascal string, len 9)
        80 00 00 78 00 00 5B E4 ...   flags + parent dir ID (0x5BE4)
+0x12C  00 00 08 'MacLC_HD'     volume name (Pascal string, len 8)
        81 00 00 78 00 00 5C 90 ...   flags + volume dir ID (0x5C90)
        00 50 00 97 00 00 00 83 01 80
```

So each startup item is a **lightweight file reference** — the application's name
plus its volume's name plus HFS catalog/directory IDs (`0x5BE4`, `0x5C90`) — a
proto-alias the Finder resolves at boot. On this disk the single entry is the
application **`MacAtrium`** on the volume **`MacLC_HD`**.

I have **not** fully nailed every flag/ID byte (the `0x80`/`0x81` flags and the
trailing `0x0083 01 80` are reverse-engineered, not from a spec), but the structure
is clear: *count, then one variable-length record per startup app carrying name +
volume + IDs.* Adding a second startup app should bump the count at `+0x108` and
append another record; that's the next thing to verify.

### Why two name records per entry?

The app name and the volume name are stored separately because the Finder needs to
locate the app even if drive mapping changes — name-match within the recorded
directory ID, fall back to volume name. It's the same philosophy as a minimal
alias record, predating the Alias Manager (System 7).

---

## Implications and what's still open

With both facts in hand, activating "MultiFinder + auto-open my app" on a built
image is purely offline surgery: patch `bbHelloName`, and synthesize a
`Finder Startup` file with an `'fndr' (0)` resource naming the app. No Mac, no
ResEdit, no booting required.

Two threads remain open (and are where I'd love other eyes):

- **Does MultiFinder 6.x advertise launch-can-return via Gestalt?** My launcher
  decides between the extended (returning, working-dir-setting) `_Launch` and the
  old Segment-Loader `_Launch` by testing `Gestalt(gestaltOSAttr)` bit
  `gestaltLaunchCanReturn`. If MultiFinder 6.0.8 has the Process Manager but does
  **not** set that bit (it may be a System-7-era attribute), code keying off it will
  wrongly fall back to the non-returning path — and still hit the −43. The robust
  System-6 signal for "MultiFinder is here" is probably **`WaitNextEvent` being
  implemented** (it's an unimplemented trap on the bare 6.0.8 boot). To confirm,
  I'm adding a temporary on-screen readout of the Gestalt bit and the trap address
  under an actual MultiFinder boot.

- **The shell contract.** When the auto-opened app draws full-screen, "Show Finder"
  (the Application menu / clicking the desktop) didn't bring the Finder forward in
  my first test. A well-behaved MultiFinder client has to yield (`WaitNextEvent`),
  honor suspend/resume events, and not monopolize the menu bar. That's the next
  thing to get right for a full-screen app to coexist with MultiFinder.

---

## Reproduce it yourself

Read boot blocks and the Finder Startup forks straight out of an image with
`rb-cli` (no mounting):

```sh
# partition offset (start_lba * 512)
rb-cli inspect disk.hda

# dump boot blocks (first 1 KiB of the HFS partition; here at 0xC000)
dd if=disk.hda bs=1 skip=$((0xC000)) count=1024 | xxd | less

# list the System Folder; pull the startup file's forks (both forks as BinHex)
rb-cli ls "disk.hda@1" "/System Folder"
rb-cli get-binhex "disk.hda@1" "/System Folder/Finder Startup" fs.hqx
```

To find the activation flag from scratch, make two disks (Set Startup = Finder vs =
MultiFinder), diff the first 1 KiB of each partition, and the only delta is
`bbHelloName`.

## <a name="tools"></a>Tools

- `rb-cli` from **rusty-backup** — reads/writes classic HFS (and much else) inside
  raw disk images directly: `inspect`, `ls`, `get`, `get-binhex`, `put-macbinary`.
- `dd` + `xxd` for raw boot-block reads.
- A short BinHex 4.0 decoder to split `get-binhex` output into data/resource forks
  for hex inspection.

## Appendix — raw evidence

Boot-block `bbHelloName` field (`+0x5A`), both disks:

```
Finder-only :  06 "Finder"  + spaces
MultiFinder :  0B "MultiFinder" + 30 2A 83 00
```

`Finder Startup` resource fork (400 bytes), key regions:

```
0x000  0000 0100 0000 015E 0000 005E 0000 0032   resource-fork header
0x100  0000 005A 0000 0001 0000 0001 0000 0024   data: len, tag, count=1, reclen
0x110  0000 09 'MacAtrium' 80 00 0078 0000 5BE4   entry: app name + parent ID
0x12C  0000 08 'MacLC_HD'  81 00 0078 0000 5C90   volume name + volume ID
0x15E  ... resource map: one type 'fndr', id 0, no name, data @ 0x100 ...
```

*Corrections welcome — especially on the `Finder Startup` flag/ID bytes and on
whether MultiFinder 6.x sets `gestaltLaunchCanReturn`.*
