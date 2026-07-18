//! `atrium::variants` — pick the best **edition** of a game for a build target (docs/47).
//!
//! Several dataset records can be editions of one game: a B&W SimCity, a colour
//! SimCity, an LC-tuned one. They share a `group` key. A MacAtrium disk boots on a
//! *range* of machines but carries **one edition per game**, so a build collapses
//! each group to the single edition best suited to the target it is building for —
//! deterministically, from the *same* facets the runtime gate uses
//! ([compatibility.jsonl](../../../data/compatibility.jsonl)).
//!
//! "Best" (the rule the dataset author reasons about):
//!   1. keep only editions whose facet envelope **admits** the target (OS / CPU /
//!      reachable depth / colour need) — [`fits`];
//!   2. among survivors, prefer a **colour match**, then a **richer max depth**, then
//!      the explicit **`prefer`** tiebreak, then the **newest** version — [`score`];
//!   3. a `pin` (target/collection override) skips scoring entirely.
//!
//! Ungrouped records pass through untouched; a group with no admissible edition is
//! dropped for that target (correctly — none of its editions run there).

use crate::catalog::cpu_gen_index;
use crate::selection::os_in_range;
use std::collections::BTreeMap;

/// The build target's machine profile — the yardstick editions are measured against.
/// A build targets a machine *class*; `cpu` is the representative generation it
/// optimises for (per-machine edges within the class are caught by the runtime gate).
#[derive(Debug, Default, Clone)]
pub struct Target {
    pub os: Option<String>,   // dotted System the disk boots ("7.1"); None = unconstrained
    pub cpu: Option<String>,  // representative CPU generation ("68030"); None = unconstrained
    pub max_depth: i64,       // deepest bpp the target shows (from art_depths); 0 = unknown
    pub color: bool,          // colour-capable (art_depths has a tier beyond "1")
}

/// One candidate edition and the facets that decide fit + score. All bounds optional
/// (absent = unconstrained), mirroring the per-title facets.
#[derive(Debug, Default, Clone)]
pub struct Edition {
    pub id: String,
    pub group: Option<String>,
    pub color: Option<bool>,
    pub min_cpu: Option<String>,
    pub max_cpu: Option<String>,
    pub min_os: Option<String>,
    pub max_os: Option<String>,
    pub min_depth: i64,
    pub max_depth: i64,
    pub prefer: i64,
}

/// Why a group resolved the way it did — surfaced by the build so a dropped or
/// swapped edition is never silent.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub group: String,
    pub chosen: Option<String>, // None = no edition admits the target; the group is dropped
    pub dropped: Vec<String>,
    pub pinned: bool,
}

/// Whether an edition's envelope admits the target machine.
fn fits(e: &Edition, t: &Target) -> bool {
    if let Some(os) = &t.os {
        if !os_in_range(os, e.min_os.as_deref(), e.max_os.as_deref()) {
            return false;
        }
    }
    // CPU: the target's generation must sit within [minCPU, maxCPU] on the one scale.
    if let Some(cpu) = t.cpu.as_deref().and_then(cpu_gen_index) {
        if let Some(lo) = e.min_cpu.as_deref().and_then(cpu_gen_index) {
            if cpu < lo {
                return false;
            }
        }
        if let Some(hi) = e.max_cpu.as_deref().and_then(cpu_gen_index) {
            if cpu > hi {
                return false;
            }
        }
    }
    // The target must be able to reach the edition's depth floor…
    if e.min_depth > 0 && t.max_depth > 0 && t.max_depth < e.min_depth {
        return false;
    }
    // …and an edition that needs colour (an explicit colour facet, or a ≥4-bit floor)
    // can't run on a B&W target.
    let needs_color = e.color == Some(true) || e.min_depth >= 4;
    if needs_color && !t.color {
        return false;
    }
    true
}

/// Rank an admissible edition for the target; higher tuple wins. Compared before the
/// version + id tiebreaks in [`collapse`].
fn score(e: &Edition, t: &Target) -> (i64, i64, i64) {
    // 1. colour-match: on a colour target prefer colour editions; on B&W prefer B&W.
    //    An unknown colour facet is neutral (0), never beating a definite match.
    let color_match = match e.color {
        Some(c) if c == t.color => 1,
        _ => 0,
    };
    // 2. depth richness: the edition's ceiling, capped at what the target can show —
    //    a colour edition beats a 1-bit one on a colour screen.
    let cap = if e.max_depth > 0 { e.max_depth } else { t.max_depth };
    let depth = if t.max_depth > 0 { cap.min(t.max_depth) } else { cap };
    (color_match, depth, e.prefer)
}

/// Trailing dash-separated numeric segments of an id, as a comparable version key:
/// `simcity-1-4` → `[1, 4]`, `simcity` → `[]`. Higher = newer; the last tiebreak.
fn version_key(id: &str) -> Vec<u32> {
    let mut parts = Vec::new();
    for seg in id.rsplit('-') {
        match seg.parse::<u32>() {
            Ok(n) => parts.push(n),
            Err(_) => break,
        }
    }
    parts.reverse();
    parts
}

/// Collapse editions to one per group for `target`. Returns the winning ids (grouped
/// winners + every ungrouped record, in a stable order) and a [`Decision`] per group.
/// `pins` maps a group to a forced edition id (a `pin` present among the group's
/// members wins outright); an unknown/absent pin falls through to scoring.
pub fn collapse(
    editions: &[Edition],
    target: &Target,
    pins: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<Decision>) {
    let mut grouped: BTreeMap<String, Vec<&Edition>> = BTreeMap::new();
    let mut out: Vec<String> = Vec::new();
    for e in editions {
        match &e.group {
            Some(g) => grouped.entry(g.clone()).or_default().push(e),
            None => out.push(e.id.clone()), // ungrouped: not a variant, always kept
        }
    }

    // Every member id except `keep` — the "dropped" list for a Decision.
    fn others(members: &[&Edition], keep: &str) -> Vec<String> {
        members.iter().map(|e| e.id.clone()).filter(|i| i != keep).collect()
    }

    let mut decisions = Vec::new();
    for (group, members) in grouped {
        // A pin present among the members wins outright — no scoring.
        if let Some(pin) = pins.get(&group) {
            if members.iter().any(|e| &e.id == pin) {
                out.push(pin.clone());
                decisions.push(Decision {
                    group,
                    chosen: Some(pin.clone()),
                    dropped: others(&members, pin),
                    pinned: true,
                });
                continue;
            }
            // pin names an id not in this build → ignore it, fall through to scoring.
        }

        let mut fit: Vec<&Edition> = members.iter().copied().filter(|e| fits(e, target)).collect();
        if fit.is_empty() {
            decisions.push(Decision {
                group,
                chosen: None,
                dropped: members.iter().map(|e| e.id.clone()).collect(),
                pinned: false,
            });
            continue;
        }
        fit.sort_by(|a, b| {
            score(b, target)
                .cmp(&score(a, target))
                .then_with(|| version_key(&b.id).cmp(&version_key(&a.id)))
                .then_with(|| a.id.cmp(&b.id)) // final: stable, deterministic
        });
        let chosen = fit[0].id.clone();
        out.push(chosen.clone());
        decisions.push(Decision {
            group,
            dropped: others(&members, &chosen),
            chosen: Some(chosen),
            pinned: false,
        });
    }

    (out, decisions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(id: &str, group: &str) -> Edition {
        Edition { id: id.into(), group: Some(group.into()), ..Default::default() }
    }
    fn colour_target() -> Target {
        Target { os: Some("7.1".into()), cpu: Some("68030".into()), max_depth: 8, color: true }
    }
    fn bw_target() -> Target {
        Target { os: Some("6.0.8".into()), cpu: Some("68000".into()), max_depth: 1, color: false }
    }
    fn chosen(dec: &[Decision], group: &str) -> Option<String> {
        dec.iter().find(|d| d.group == group).and_then(|d| d.chosen.clone())
    }

    #[test]
    fn colour_target_picks_colour_edition_bw_target_picks_bw() {
        let eds = vec![
            Edition { color: Some(false), max_depth: 1, ..ed("simcity-bw", "simcity") },
            Edition { color: Some(true), min_depth: 4, max_depth: 8, ..ed("simcity-color", "simcity") },
        ];
        let (win, dec) = collapse(&eds, &colour_target(), &BTreeMap::new());
        assert_eq!(win, vec!["simcity-color"]);
        assert_eq!(chosen(&dec, "simcity").as_deref(), Some("simcity-color"));
        // On a B&W compact the colour edition is filtered (needs colour) → B&W wins.
        let (win, _) = collapse(&eds, &bw_target(), &BTreeMap::new());
        assert_eq!(win, vec!["simcity-bw"]);
    }

    #[test]
    fn cpu_range_filters_then_prefer_breaks_the_tie() {
        // A generic colour edition and an LC-tuned one both admit a 68030 colour
        // target; identical on colour+depth, so `prefer` decides.
        let eds = vec![
            Edition { color: Some(true), max_depth: 8, ..ed("game-generic", "game") },
            Edition { color: Some(true), max_depth: 8, max_cpu: Some("68030".into()), prefer: 1,
                      ..ed("game-lc", "game") },
        ];
        assert_eq!(collapse(&eds, &colour_target(), &BTreeMap::new()).0, vec!["game-lc"]);
        // On a 68040 target the LC-tuned edition (maxCPU 68030) is filtered out.
        let quadra = Target { cpu: Some("68040".into()), ..colour_target() };
        assert_eq!(collapse(&eds, &quadra, &BTreeMap::new()).0, vec!["game-generic"]);
    }

    #[test]
    fn newest_version_is_the_final_tiebreak() {
        let eds = vec![
            Edition { color: Some(true), max_depth: 8, ..ed("simcity-1-1", "simcity") },
            Edition { color: Some(true), max_depth: 8, ..ed("simcity-1-4", "simcity") },
            Edition { color: Some(true), max_depth: 8, ..ed("simcity-1-11", "simcity") },
        ];
        // 1.11 > 1.4 > 1.1 — numeric, not lexical.
        assert_eq!(collapse(&eds, &colour_target(), &BTreeMap::new()).0, vec!["simcity-1-11"]);
    }

    #[test]
    fn pin_overrides_scoring_and_no_fit_drops_the_group() {
        let eds = vec![
            Edition { color: Some(false), max_depth: 1, ..ed("simcity-bw", "simcity") },
            Edition { color: Some(true), min_depth: 4, max_depth: 8, ..ed("simcity-color", "simcity") },
        ];
        // Pin forces the B&W edition even on a colour target.
        let mut pins = BTreeMap::new();
        pins.insert("simcity".to_string(), "simcity-bw".to_string());
        let (win, dec) = collapse(&eds, &colour_target(), &pins);
        assert_eq!(win, vec!["simcity-bw"]);
        assert!(dec.iter().find(|d| d.group == "simcity").unwrap().pinned);
        // A group where nothing admits the target is dropped (no colour edition, B&W target,
        // and here even the B&W edition requires System 7 the compact can't reach).
        let eds = vec![Edition {
            color: Some(true), min_depth: 8, min_os: Some("7.5".into()),
            ..ed("only-color", "loner")
        }];
        let (win, dec) = collapse(&eds, &bw_target(), &BTreeMap::new());
        assert!(win.is_empty());
        assert_eq!(chosen(&dec, "loner"), None);
    }

    #[test]
    fn ungrouped_records_pass_through() {
        let eds = vec![
            Edition { id: "loom".into(), ..Default::default() },
            Edition { color: Some(true), max_depth: 8, ..ed("simcity-color", "simcity") },
            Edition { color: Some(false), max_depth: 1, ..ed("simcity-bw", "simcity") },
        ];
        let (mut win, _) = collapse(&eds, &colour_target(), &BTreeMap::new());
        win.sort();
        assert_eq!(win, vec!["loom", "simcity-color"]); // loom kept as-is; one SimCity
    }
}
