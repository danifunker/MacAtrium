//! `atrium enrich` — fill the curated dataset from the **LaunchBox Games
//! Database** (docs/06 "Build-time").
//!
//! Streams LaunchBox's `Metadata.xml` (~500 MB) with a SAX-style parser, keeps
//! only `Platform == "Apple Mac OS"` games, matches our dataset titles by
//! normalised name, and fills the facet fields LaunchBox knows — `year`
//! (ReleaseYear/Date), `vendor` (Publisher), `genre[]` (Genres, `;`-delimited) —
//! WITHOUT clobbering hand-curated values (unless `--overwrite`). Box-Front art
//! URLs (joined by DatabaseID) can be written to a manifest for a later art pass.
//!
//! LaunchBox has no colour or mouse data, so those two facets stay curated.
//! (Approach adapted from megatron-uk/x68klauncher's tools/metadata.py.)

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;

const IMAGE_URL: &str = "https://images.launchbox-app.com/";

struct LbGame {
    name: String,
    year: Option<i64>,
    publisher: Option<String>,
    genres: Vec<String>,
    database_id: String,
}

/// Normalise a title for matching: lowercase, fold common Latin accents, keep
/// alphanumerics + spaces, collapse runs of whitespace.
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut sp = false;
    for c in s.chars() {
        let c = fold(c).to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            if sp && !out.is_empty() {
                out.push(' ');
            }
            out.push(c);
            sp = false;
        } else {
            sp = true;
        }
    }
    out
}

fn fold(c: char) -> char {
    match c {
        'à'|'á'|'â'|'ä'|'ã'|'å'|'À'|'Á'|'Â'|'Ä'|'Ã'|'Å' => 'a',
        'è'|'é'|'ê'|'ë'|'È'|'É'|'Ê'|'Ë' => 'e',
        'ì'|'í'|'î'|'ï'|'Ì'|'Í'|'Î'|'Ï' => 'i',
        'ò'|'ó'|'ô'|'ö'|'õ'|'Ò'|'Ó'|'Ô'|'Ö'|'Õ' => 'o',
        'ù'|'ú'|'û'|'ü'|'Ù'|'Ú'|'Û'|'Ü' => 'u',
        'ñ'|'Ñ' => 'n',
        'ç'|'Ç' => 'c',
        other => other,
    }
}

/// Drop "(...)" / "[...]" qualifier groups, e.g.
/// "Prince of Persia (Brøderbund Software)" -> "Prince of Persia ".
fn strip_groups(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Normalised match keys for a title: the full name, with the parenthetical
/// qualifier removed, and with any ":" subtitle dropped — so our clean titles
/// match LaunchBox's disambiguated ones ("Deja Vu: A Nightmare Comes True!!").
fn candidate_keys(name: &str) -> Vec<String> {
    let stripped = strip_groups(name);
    let mut v: Vec<String> = Vec::new();
    for cand in [
        name,
        &stripped,
        name.split(':').next().unwrap_or(name),
        stripped.split(':').next().unwrap_or(&stripped),
    ] {
        let k = normalize(cand);
        if !k.is_empty() && !v.contains(&k) {
            v.push(k);
        }
    }
    v
}

fn year_of(acc: &HashMap<&str, String>) -> Option<i64> {
    if let Some(y) = acc.get("ReleaseYear").and_then(|s| s.trim().parse().ok()) {
        return Some(y);
    }
    // ReleaseDate is ISO ("1993-05-22T..."); take the leading year.
    acc.get("ReleaseDate")
        .and_then(|s| s.get(0..4))
        .and_then(|s| s.parse().ok())
}

/// Stream pass 1: collect games on the target platform.
fn parse_games(path: &Path, platform: &str) -> Result<Vec<LbGame>> {
    let mut reader = Reader::from_reader(BufReader::new(
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?,
    ));
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut games = Vec::new();
    let mut in_game = false;
    let mut cur: Option<&'static str> = None;
    let mut acc: HashMap<&str, String> = HashMap::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let n = e.name();
                match n.as_ref() {
                    b"Game" => {
                        in_game = true;
                        acc.clear();
                        cur = None;
                    }
                    other if in_game => {
                        cur = field_tag(other);
                    }
                    _ => {}
                }
            }
            Event::Text(e) if in_game => {
                if let Some(f) = cur {
                    acc.entry(f).or_default().push_str(&e.xml_content()?);
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == b"Game" {
                    in_game = false;
                    cur = None;
                    if acc.get("Platform").map(String::as_str) == Some(platform) {
                        if let (Some(name), Some(id)) = (acc.get("Name"), acc.get("DatabaseID")) {
                            games.push(LbGame {
                                name: name.clone(),
                                year: year_of(&acc),
                                publisher: acc.get("Publisher").map(|s| s.trim().to_string()),
                                genres: acc
                                    .get("Genres")
                                    .map(|g| {
                                        g.split(';')
                                            .map(|s| s.trim().to_string())
                                            .filter(|s| !s.is_empty())
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                database_id: id.trim().to_string(),
                            });
                        }
                    }
                } else {
                    cur = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(games)
}

fn field_tag(n: &[u8]) -> Option<&'static str> {
    match n {
        b"Name" => Some("Name"),
        b"Platform" => Some("Platform"),
        b"ReleaseYear" => Some("ReleaseYear"),
        b"ReleaseDate" => Some("ReleaseDate"),
        b"Publisher" => Some("Publisher"),
        b"Genres" => Some("Genres"),
        b"DatabaseID" => Some("DatabaseID"),
        _ => None,
    }
}

/// Stream pass 2: Box-Front (preferred) image filename per wanted DatabaseID.
fn parse_box_art(path: &Path, wanted: &HashSet<String>) -> Result<HashMap<String, String>> {
    let mut reader = Reader::from_reader(BufReader::new(std::fs::File::open(path)?));
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_img = false;
    let mut cur: Option<&'static str> = None;
    let mut acc: HashMap<&str, String> = HashMap::new();
    // database_id -> (is_box_front, filename)
    let mut found: HashMap<String, (bool, String)> = HashMap::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => match e.name().as_ref() {
                b"GameImage" => {
                    in_img = true;
                    acc.clear();
                    cur = None;
                }
                b"DatabaseID" if in_img => cur = Some("DatabaseID"),
                b"FileName" if in_img => cur = Some("FileName"),
                b"Type" if in_img => cur = Some("Type"),
                _ => {}
            },
            Event::Text(e) if in_img => {
                if let Some(f) = cur {
                    acc.entry(f).or_default().push_str(&e.xml_content()?);
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == b"GameImage" {
                    in_img = false;
                    cur = None;
                    if let (Some(id), Some(file)) = (acc.get("DatabaseID"), acc.get("FileName")) {
                        if wanted.contains(id.trim()) {
                            let is_box = acc.get("Type").map(|t| t == "Box - Front").unwrap_or(false);
                            let entry = found.entry(id.trim().to_string());
                            match entry {
                                std::collections::hash_map::Entry::Vacant(v) => {
                                    v.insert((is_box, file.trim().to_string()));
                                }
                                std::collections::hash_map::Entry::Occupied(mut o) => {
                                    if is_box && !o.get().0 {
                                        o.insert((true, file.trim().to_string()));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    cur = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(found.into_iter().map(|(k, (_, f))| (k, f)).collect())
}

fn is_blank_or_comment(t: &str) -> bool {
    t.is_empty() || t.starts_with('#') || t.starts_with("//")
}

fn missing(obj: &Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

pub fn run(
    src: &Path,
    metadata: &Path,
    out: &Path,
    platform: &str,
    overwrite: bool,
    art_manifest: Option<&Path>,
) -> Result<()> {
    let games = parse_games(metadata, platform)?;
    eprintln!("LaunchBox: {} games on platform \"{platform}\"", games.len());

    // Index every candidate key -> the most complete game (prefer one carrying a
    // year and publisher, so "Prince of Persia (Brøderbund Software)" wins over
    // "Prince of Persia (2008)").
    let score = |g: &LbGame| g.year.is_some() as u8 + g.publisher.is_some() as u8;
    let mut idx: HashMap<String, usize> = HashMap::new();
    for (i, g) in games.iter().enumerate() {
        for k in candidate_keys(&g.name) {
            match idx.get(&k) {
                Some(&j) if score(&games[j]) >= score(g) => {}
                _ => {
                    idx.insert(k, i);
                }
            }
        }
    }

    let text = std::fs::read_to_string(src)
        .with_context(|| format!("reading {}", src.display()))?;
    let mut out_text = String::new();
    let mut total = 0usize;
    let mut matched = 0usize;
    let mut unmatched: Vec<String> = Vec::new();
    let mut wanted_ids: Vec<(String, String)> = Vec::new(); // (item id, database id)

    for line in text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            out_text.push_str(line);
            out_text.push('\n');
            continue;
        }
        total += 1;
        let mut obj: Map<String, Value> =
            serde_json::from_str(t).with_context(|| format!("parsing dataset line: {t}"))?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("").to_string();

        let hit = candidate_keys(&name).into_iter().find_map(|k| idx.get(&k).copied());
        if let Some(gi) = hit {
            let g = &games[gi];
            matched += 1;
            if let Some(y) = g.year {
                if overwrite || missing(&obj, "year") {
                    obj.insert("year".into(), Value::from(y));
                }
            }
            if let Some(p) = &g.publisher {
                if !p.is_empty() && (overwrite || missing(&obj, "vendor")) {
                    obj.insert("vendor".into(), Value::from(p.clone()));
                }
            }
            if !g.genres.is_empty() && (overwrite || missing(&obj, "genre")) {
                obj.insert("genre".into(), Value::from(g.genres.clone()));
            }
            wanted_ids.push((id.clone(), g.database_id.clone()));
        } else {
            unmatched.push(format!("{name} ({id})"));
        }

        out_text.push_str(&serde_json::to_string(&Value::Object(obj))?);
        out_text.push('\n');
    }

    std::fs::write(out, &out_text).with_context(|| format!("writing {}", out.display()))?;

    eprintln!(
        "enriched {matched}/{total} item(s) -> {} ({} unmatched)",
        out.display(),
        unmatched.len()
    );
    for u in &unmatched {
        eprintln!("  unmatched: {u}");
    }

    if let Some(mpath) = art_manifest {
        let ids: HashSet<String> = wanted_ids.iter().map(|(_, d)| d.clone()).collect();
        let art = parse_box_art(metadata, &ids)?;
        let mut m = String::new();
        let mut n = 0;
        for (item_id, db_id) in &wanted_ids {
            if let Some(file) = art.get(db_id) {
                m.push_str(&format!(
                    "{{\"id\":{:?},\"databaseID\":{:?},\"art\":{:?}}}\n",
                    item_id,
                    db_id,
                    format!("{IMAGE_URL}{file}")
                ));
                n += 1;
            }
        }
        std::fs::write(mpath, m)?;
        eprintln!("art manifest: {n} Box-Front image(s) -> {}", mpath.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize("Prince of Persia"), "prince of persia");
        assert_eq!(normalize("SimCity"), "simcity");
        assert_eq!(normalize("Maze Wars+"), "maze wars");
        assert_eq!(normalize("Déjà Vu"), "deja vu");
        assert_eq!(normalize("Shufflepuck Café"), "shufflepuck cafe");
        assert_eq!(normalize("  The   Hobbit  "), "the hobbit");
    }

    #[test]
    fn candidate_keys_handle_qualifiers_and_subtitles() {
        // LaunchBox disambiguated names reduce to our clean titles.
        assert!(candidate_keys("Prince of Persia (Brøderbund Software)")
            .contains(&"prince of persia".to_string()));
        assert!(candidate_keys("Deja Vu: A Nightmare Comes True!!")
            .contains(&"deja vu".to_string()));
        // a sequel must NOT reduce to the base title
        assert!(!candidate_keys("Prince of Persia 2").contains(&"prince of persia".to_string()));
    }

    #[test]
    fn year_prefers_releaseyear_then_date() {
        let mut a = HashMap::new();
        a.insert("ReleaseYear", "1989".to_string());
        assert_eq!(year_of(&a), Some(1989));
        a.clear();
        a.insert("ReleaseDate", "1991-05-22T00:00:00+00:00".to_string());
        assert_eq!(year_of(&a), Some(1991));
    }

    #[test]
    fn missing_detects_absent_null_empty() {
        let obj: Map<String, Value> =
            serde_json::from_str(r#"{"a":"x","b":"","c":[],"d":null,"e":["g"]}"#).unwrap();
        assert!(missing(&obj, "z")); // absent
        assert!(missing(&obj, "b")); // empty string
        assert!(missing(&obj, "c")); // empty array
        assert!(missing(&obj, "d")); // null
        assert!(!missing(&obj, "a"));
        assert!(!missing(&obj, "e"));
    }
}
