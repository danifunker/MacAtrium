# Recommendations dataset

Community-curated "must-try" classic Mac games & software. Recommendations live
here **independent of whether a donor/MacPack source exists** — a title is
recommended on its merits, and once it's added to the library it appears in the
launcher's **Recommended** category automatically (via `data/taxonomy.json`
`recommended` → `atrium library categorize`).

Sourced from:
- r/VintageApple — [Best classic Mac games](https://www.reddit.com/r/VintageApple/comments/gexywa/best_classic_mac_games/)
- r/VintageApple — [Favorite games and other software for classic Mac](https://www.reddit.com/r/VintageApple/comments/1hrk3lb/favorite_games_and_other_software_for_classic_mac/)
- 68kMLA — [Must-try software for the Performa 475](https://68kmla.org/bb/threads/any-must-try-software-for-the-performa-475.51959/)
- ResetEra — [Mac games of the late 90s/early 2000s](https://www.resetera.com/threads/mac-users-of-the-late-90s-early-2000s-what-are-some-games-that-you-remember-playing.1145799/)

## In the library → tagged Recommended

These resolve to a `data/library.jsonl` record and carry the `Recommended` tag in
`data/categories.jsonl`. (Some only appear on a built disk once a donor provides
the app — the tag persists regardless.)

Spectre · Crystal Quest · SimAnt · SimCity · SimEarth · Prince of Persia ·
Shufflepuck Café · Oxyd · Oregon Trail · Dark Castle · Beyond Dark Castle ·
Civilization · Indiana Jones: The Last Crusade · Lode Runner · Glider · Bolo ·
RoboSport · 3 in Three · Arkanoid · Star Wars · Sky Shadow · Carmen Sandiego ·
Scarab of RA · TaskMaker · Jewelbox · Flappy Mac · Galactic Frontiers · Diamonds ·
Sparkz · Glypha · Lemmings · A-Train · Loom · Mathematica · Photoshop · Tetris ·
Risk · Kid Pix · Number Munchers · Reader Rabbit · Cosmic Osmo

## Wishlist → recommended, not yet in the library

Add these to the library (a donor source + a record) and they'll surface as
Recommended. Many are commercial titles outside the current shareware-leaning set.

Marathon · Myst · Maelstrom · SimCity 2000 · SimTower · SimFarm · Pathways into
Darkness · Warcraft II · Dark Forces · Quake · Deadlock: Planetary Conquest ·
Mars Rising · Deimos Rising · StarCraft · Unreal · Unreal Tournament · Deus Ex ·
Oni · Jedi Knight II · The Incredible Machine · Escape Velocity (Override / Nova) ·
Doom / Doom II · Snood · Diablo / Diablo II · Fallout · Phrase Craze · Spin Doctor ·
Wolfenstein 3D · Legend of Kyrandia · Indiana Jones and the Fate of Atlantis ·
Day of the Tentacle · Sam & Max Hit the Road · Full Throttle · The Dig ·
Star Wars: Rebel Assault · X-Wing · TIE Fighter · X-Wing vs. TIE Fighter · Heretic ·
Hexen · Duke Nukem 3D · Alone in the Dark · The Journeyman Project (Turbo / II) ·
Power Pete · Mario's Game Gallery · Ares · Titanic: Adventure Out of Time · Apeiron ·
Avara · Barrack · Harry the Handsome Executive · Realmz · Monkey Island · Abuse ·
Flashback · Blackthorne · Master of Orion · The 7th Guest · Zork Nemesis · Bad Mojo ·
I Have No Mouth and I Must Scream · MechWarrior II · Clive Barker's Undying ·
Aliens vs. Predator · No One Lives Forever · American McGee's Alice · Halo · Nanosaur ·
MDK · Myth · Swoop · Bugdom · Cro-Mag Rally · MathCAD · MATLAB · Electronics Workbench

_To promote a wishlist title: add its library record (+ donor), then run
`atrium library categorize` — `taxonomy.json`'s `recommended` list seeds it into
Recommended on the next build._
