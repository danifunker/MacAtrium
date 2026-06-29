# 27 — Classic Mac UI redesign (options + wireframes)

The launcher reads "modern app in a beige costume": a **custom in-window header
band** instead of the real menu bar, **no window frame**, **no scroll bars**, a
flat full-screen carousel, and a "light" theme with off-era colours. This doc
grounds a fix in the classic Mac HIG and lays out 3–4 directions. Wireframes are
in `docs/wireframes/` (rendered 640×480, the Mac II/MDC screen, flat B&W).

## What "classic" actually means (Macintosh HIG 1992 + Tog)

Grounded in *Macintosh Human Interface Guidelines* (Apple, 1992), *The Apple
Desktop Interface* (1987), and Tognazzini's *Tog on Interface* / First Principles:

- **Menu bar pinned to the top screen edge.** Fitts's Law: the top edge has
  "infinite" depth — you can't overshoot it. A custom in-window header throws
  that away. **20 px tall, white, 1 px black bottom rule, Chicago 12**, Apple
  menu (icon) at far left, then File / Edit / View / one app menu. **Edit is
  required even if mostly disabled** (desk accessories rely on the host's Edit).
- **Content in a real window:** 1 px black frame; a **title bar (~18 px) with the
  ~6-line "racing-stripe" pattern** and the title in a centred white gap; **close
  box** left, **zoom box** right (System 7+), **15×15 grow box** bottom-right.
- **Scroll bars: 16 px**, a 16×16 arrow box at each end (black triangle), a **50%
  gray dither track**, and a **FIXED ~16 px white thumb** — proportional thumbs
  are Mac OS 8 Appearance-Manager era, an anachronism here. Drawn empty white
  when there's nothing to scroll / the window is inactive.
- **Flat black-on-white + 50% dither. No** gradients, drop shadows, rounded
  corners, translucency, **hover states** (classic Mac has none — feedback is
  inversion on mouse-down), anti-aliased fonts, or saturated coloured chrome.
- **Selection highlight:** B&W = invert (XOR); colour = a **pale, low-saturation
  tint** (the System 7 Highlight colour), never a bold accent fill.
- **Fonts:** Chicago (chrome/titles), Geneva 9/10 (dense lists), Monaco (mono).
- **Precedent:** Apple's own launchers — the System 7.5 **Launcher** control
  panel and **At Ease** — are category-organised button/icon windows under the
  menu bar. Finder's icon and list views are the other reference.

## The options (see `docs/wireframes/`)

| # | Wireframe | What it is | Trade-off |
|---|---|---|---|
| 1 | `1-windowed-carousel.png` | Keep the 5/7-up carousel, but in a framed window under a real menu bar, with a **horizontal scroll bar to page** the category. | **Smallest change**; preserves the keyboard coverflow we built. The carousel is still the least "Finder-like" element, but the chrome grounds it. |
| 2 | `2-finder-icon-grid.png` | Finder "by Icon" view: a scrollable icon grid, **vertical scroll bar**, "N items" info bar, grow box. | **Most authentically Mac**; great for mouse paging. Drops the coverflow; needs a real icon-grid model + selection. |
| 3 | `3-two-pane-browser.png` | Categories list (left, scrollable) + apps "by Name" list (right, scrollable) + a detail strip with Launch. | Scales best to a big library; classic browser feel (cf. Extensions Manager). Biggest rework; two scroll regions. |
| 4 | `4-theme-and-palette.png` | Not a layout — the **theme fix**: authentic System 7 (colour) vs System 6 (B&W), with KEEP vs AVOID palette swatches. | Applies to whichever layout we pick; directly answers the bad "light" theme. |

## Recommendation

Two parts:

1. **Do the chrome + theme regardless of layout** — this is 80% of the "classic"
   win and applies to all three: real top menu bar (Apple / File / Edit / View /
   Library), a framed window with a striped title bar, a 16 px fixed-thumb scroll
   bar, and the authentic flat B&W / pale-tint theme (wireframe 4). This alone
   fixes "looks modern" and the light-theme colours.

2. **Make the content a classic `View` menu** — Mac apps switch views (Finder:
   by Icon / by Name). Offer **View ▸ Carousel / Icon / List**, implement
   **wireframe 1 (Carousel) first** (least disruption, keeps our coverflow as the
   default), and add the icon grid (2) and list (3) as alternate view modes over
   time. The horizontal scroll bar (carousel) and vertical scroll bars (grid /
   list) give the mouse paging asked for.

This restores the menu bar, adds the scroll bars, fixes the theme, and keeps the
keyboard-first carousel — while opening the door to the more Finder-like views.

## Implementation notes (when we proceed)

- Menu bar: real `MenuBarHeight`/`InsertMenu` menus; the launcher currently owns
  the whole screen, so it must draw under a 20 px bar and add an Apple/About menu.
- Window: a real document `WindowPtr` with `goAway`/`zoom`/grow, or a borderless
  full-screen window that *draws* the chrome (keeps the appliance feel). TBD.
- Scroll bars: real `ControlHandle` scroll bars (`NewControl`/`TrackControl`) so
  the mouse paging is native; arrows page the carousel / scroll the grid.
- Theme: drop the current "light" palette; use white/​black/​50%-dither and a pale
  highlight (colour) or invert (B&W). Wire the Chicago bitmap font for chrome.

## Decision (chosen direction)

- **View menu, user-configurable**, offering **by Carousel (default)** and **by
  List / two-pane browser** (wireframe 3); icon grid (2) optional later. The
  choice persists. (See `wireframes/3b-view-menu-and-esc.png`.)
- **Iterating on the two-pane (wireframe 3):** the detail strip focuses on the
  **screenshot**, not box art — box art moves to the `P` key (consistent with the
  current launcher). See the updated `wireframes/3-two-pane-browser.png`.
- **Keyboard selection works in every view** (arrows move the selection — shown as
  the inverted/pale row; Return launches). Shortcuts live in the menus (⌘) the
  classic way.
- **The ESC quick-menu stays** (Settings / About / Show Finder / Restart / Shut
  Down) for keyboard + controller use, coexisting with the real menu bar — a
  deliberate, slightly-non-classic concession for the appliance/controller model.
- Build the **full layout in one go** once the wireframe is settled.
