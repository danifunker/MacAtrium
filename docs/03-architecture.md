# 03 — Architecture

## The central problem: the launch model

How an app launches another app is the single biggest architectural force in
this project, and it differs by system:

- **Bare System 6 (no MultiFinder):** the Segment Loader `Launch` trap *replaces*
  the running process. Our shell would quit, the chosen app runs, and when it
  quits the OS relaunches the **startup app** (us). State would have to be
  rebuilt from disk every time. We avoid this by **requiring MultiFinder**.
- **System 6 + MultiFinder, and System 7+:** the *extended* `Launch` trap (a
  `LaunchParamBlockRec` with the **`launchContinue`** flag set) **sub-launches**
  — our shell stays resident, the chosen app runs in its own layer, and control
  returns to us when it quits. System 7's `LaunchApplication()` is glue over this
  same mechanism.

**Decision:** target the extended, resident `Launch` path everywhere. One code
path: build a `LaunchParamBlockRec`, set `launchControlFlags` to
`launchContinue | launchNoFileFlags` (resolve the target `FSSpec` ourselves),
call it, and resume our event loop on return. Guard with
`Gestalt(gestaltOSAttr)` → `gestaltLaunchCanReturn` so we degrade gracefully if
the capability is somehow absent. 🔬 Verify exact flags/behavior per system.

Details and the control-panel/shutdown specifics live in
[08-launching-system.md](08-launching-system.md).

## Process lifecycle (resident shell)

```
boot ──▶ OS launches our shell as the startup app
          │
          ▼
   ┌──────────────────────────────┐
   │  init: Toolbox, Gestalt,     │
   │  detect QD backend + screen, │
   │  load theme, load catalog    │
   └──────────────┬───────────────┘
                  ▼
   ┌──────────────────────────────┐  ◀── back here when a launched app quits
   │  main event loop (resident)  │
   │  WaitNextEvent / GetNextEvent│
   └───┬───────────┬───────────┬──┘
       │ key/mouse │ launch    │ shutdown/restart
       ▼           ▼           ▼
   navigate    Launch(...)   ShutDwnPower()/ShutDwnStart()
               continue→
               app runs,
               returns
```

Because the shell is the **startup application**, "quit" is not a normal user
action — quitting would leave the user at nothing. Our top-level "exit" actions
are **Launch Finder**, **Shutdown**, and **Restart** only.

## Module breakdown

Keep modules small and testable; isolate everything OS-version- or
depth-specific behind a thin interface so the bulk of the code is portable C.

| Module | Responsibility |
|--------|----------------|
| `main` | Toolbox init, environment detection, event loop, lifecycle |
| `env` | `Gestalt`/trap probes: OS version, Color QD, launch caps, screen bounds/depth, Shutdown Mgr |
| `catalog` | Read + parse the catalog JSONL into in-memory item/category structs |
| `model` | In-memory library: categories, items, the synthetic "All", current selection/cursor |
| `theme` | Palette + font config; defaults + load overrides |
| `render` | Drawing API used by the UI; dispatches to one of two backends |
| `render_qd` | Classic-QuickDraw **B&W** backend |
| `render_cqd` | **Color QuickDraw** backend (16/256/thousands) |
| `ui` | Layout (rows/pages from screen rect), navigation state machine, input handling |
| `input` | Normalize key + mouse events to abstract UI commands (Up/Down/Page/Select/Back) |
| `launch` | Build `LaunchParamBlockRec`, resolve target `FSSpec`/alias, sub-launch, return |
| `sysctl` | Shutdown/Restart; open the supported control panels |

Dependencies flow one way: `ui` → `model`/`render`/`input`/`launch`; backends
depend only on `theme` + Toolbox. `env` is consulted at startup and its results
are passed down (no module re-probes Gestalt ad hoc).

## Rendering strategy (summary)

- Pick **one** backend at startup from `env` (Color QD present? what depth?).
- The `render` interface exposes primitives the UI needs (clear, fill rect,
  draw text in Chicago, draw selection highlight, draw optional icon/PICT), so
  `ui` never branches on depth.
- Off-screen drawing: on the Color path use a `GWorld` for flicker-free
  composition; on the classic B&W path use an off-screen `BitMap`/`GrafPort`.
  Blit to the window with `CopyBits`.

Full UI detail in [07-ui-ux.md](07-ui-ux.md).

## Memory & footprint

- 68k, MultiFinder/System 7: set a sensible **`SIZE` resource** partition. The
  catalog (potentially hundreds of items) lives in a handle-based structure;
  artwork is loaded lazily and purged. Target running comfortably in a few
  hundred KB so it fits modest configurations (and MacPlus-class for the B&W
  build).
- No reliance on temporary memory or System-7-only memory calls on the path
  shared with System 6.

## Error handling

- Missing/corrupt catalog → show a built-in "no catalog found" screen with the
  expected path, not a crash. The shell must **never** leave the user at a dead
  end, because there's no Finder behind it.
- Failed launch (file moved, wrong type) → non-fatal alert, return to the list.
- Everything degrades to the B&W/text path if color or a fancy feature is
  unavailable.
