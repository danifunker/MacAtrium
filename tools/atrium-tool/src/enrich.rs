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
    overview: Option<String>,
    database_id: String,
}

/// Tidy a LaunchBox <Overview> into a one-paragraph `desc`: collapse whitespace
/// and cap the length (the on-device buffer is small), backing off to a sentence
/// or word boundary so it doesn't cut mid-word.
pub fn clamp_desc(s: &str) -> String {
    const MAX: usize = 240;
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        return flat;
    }
    let mut t: String = flat.chars().take(MAX).collect();
    if let Some(i) = t.rfind(['.', '!', '?']) {
        if i > MAX / 2 {
            t.truncate(i + 1);
            return t;
        }
    }
    if let Some(i) = t.rfind(' ') {
        t.truncate(i);
    }
    t.push_str("...");
    t
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

/// Drop a trailing dotted version token ("Glider 4.0" -> "Glider") but NOT a
/// bare sequel number ("Prince of Persia 2" stays distinct).
fn strip_version(s: &str) -> String {
    let t = s.trim_end();
    if let Some(pos) = t.rfind(' ') {
        let last = &t[pos + 1..];
        if last.contains('.')
            && last.chars().any(|c| c.is_ascii_digit())
            && last.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return t[..pos].to_string();
        }
    }
    s.to_string()
}

/// Drop a leading or trailing article from an already-normalised key, so
/// "the hobbit" and "hobbit the" (from "Hobbit, The") both reduce to "hobbit".
fn strip_articles(k: &str) -> String {
    let mut s = k;
    for a in ["the ", "a ", "an "] {
        if let Some(rest) = s.strip_prefix(a) {
            s = rest;
            break;
        }
    }
    for a in [" the", " a", " an"] {
        if let Some(rest) = s.strip_suffix(a) {
            s = rest;
            break;
        }
    }
    s.to_string()
}

/// Normalised match keys for a title: the full name, with the parenthetical
/// qualifier removed, with any ":" subtitle dropped, and each of those with
/// articles stripped — so our clean titles match LaunchBox's disambiguated ones
/// ("Deja Vu: A Nightmare Comes True!!", "Hobbit, The", "The Ancient Art of War").
pub fn candidate_keys(name: &str) -> Vec<String> {
    let stripped = strip_groups(name);
    // Split on ':' (subtitle) and '/' (compound bundles, e.g. Macintosh Garden's
    // "Lemmings/Oh No! More Lemmings") so the head segment matches our clean title.
    let before_colon = name.split([':', '/']).next().unwrap_or(name);
    let stripped_before_colon = stripped.split([':', '/']).next().unwrap_or(&stripped);
    let raw = [
        name.to_string(),
        stripped.clone(),
        before_colon.to_string(),
        stripped_before_colon.to_string(),
    ];
    let mut v: Vec<String> = Vec::new();
    for r in &raw {
        for cand in [r.clone(), strip_version(r)] {
            let k = normalize(&cand);
            let a = strip_articles(&k);
            for kk in [k, a] {
                if !kk.is_empty() && !v.contains(&kk) {
                    v.push(kk);
                }
            }
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
                                overview: acc
                                    .get("Overview")
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty()),
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
        b"Overview" => Some("Overview"),
        b"DatabaseID" => Some("DatabaseID"),
        _ => None,
    }
}

/// The images we care about per game.
#[derive(Default, Clone)]
pub struct ImageSet {
    pub box_front: Option<String>,  // FileName for "Box - Front"
    pub screenshot: Option<String>, // FileName for a gameplay screenshot
}

/// Stream pass 2: per wanted DatabaseID, the best Box-Front (for art) and the
/// best gameplay Screenshot (for colour detection). Box art is colourful even
/// for B&W games, so the *screenshot* is what classifies colour.
fn parse_images(path: &Path, wanted: &HashSet<String>) -> Result<HashMap<String, ImageSet>> {
    let mut reader = Reader::from_reader(BufReader::new(std::fs::File::open(path)?));
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_img = false;
    let mut cur: Option<&'static str> = None;
    let mut acc: HashMap<&str, String> = HashMap::new();
    let mut out: HashMap<String, ImageSet> = HashMap::new();
    // track whether the chosen screenshot was the preferred "Gameplay" one
    let mut shot_is_gameplay: HashMap<String, bool> = HashMap::new();

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
                        let id = id.trim().to_string();
                        if wanted.contains(&id) {
                            let ty = acc.get("Type").map(String::as_str).unwrap_or("");
                            let file = file.trim().to_string();
                            let set = out.entry(id.clone()).or_default();
                            if ty == "Box - Front" {
                                set.box_front = Some(file.clone());
                            } else if ty.starts_with("Screenshot") {
                                let gameplay = ty == "Screenshot - Gameplay";
                                let had_gameplay = *shot_is_gameplay.get(&id).unwrap_or(&false);
                                if set.screenshot.is_none() || (gameplay && !had_gameplay) {
                                    set.screenshot = Some(file);
                                    shot_is_gameplay.insert(id, gameplay);
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
    Ok(out)
}

/// Download a URL to `out` via curl (no Rust HTTP dependency). Returns Ok only if
/// the file ends up non-empty.
pub fn download(url: &str, out: &Path, curl: &str) -> Result<()> {
    let dst = out.to_string_lossy();
    let status = std::process::Command::new(curl)
        .args(["-sL", "--max-time", "30", "-o", &dst, url])
        .status()
        .with_context(|| format!("running {curl}"))?;
    anyhow::ensure!(status.success(), "curl failed for {url}");
    let len = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    anyhow::ensure!(len > 0, "empty download for {url}");
    Ok(())
}

/// Classify an image as colour (true) or B&W (false) by the fraction of clearly
/// saturated pixels — robust to JPEG chroma noise on grayscale shots.
pub fn is_color_image(path: &Path) -> Result<bool> {
    let img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?
        .to_rgb8();
    let (w, h) = img.dimensions();
    let data = img.as_raw();
    let n = (w as usize) * (h as usize);
    if n == 0 {
        return Ok(false);
    }
    let step = (n / 5000).max(1);
    let (mut colored, mut total) = (0usize, 0usize);
    let mut i = 0;
    while i < n {
        let o = i * 3;
        let (r, g, b) = (data[o] as i32, data[o + 1] as i32, data[o + 2] as i32);
        let sat = r.max(g).max(b) - r.min(g).min(b);
        if sat > 40 {
            colored += 1;
        }
        total += 1;
        i += step;
    }
    Ok(total > 0 && (colored as f64 / total as f64) >= 0.03)
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

/// A dataset line: a comment/blank to pass through, or a parsed record.
enum Item {
    Pass(String),
    Rec(Map<String, Value>),
}

pub fn run(
    src: &Path,
    metadata: &Path,
    out: &Path,
    platform: &str,
    overwrite: bool,
    art_manifest: Option<&Path>,
    detect_color: bool,
    curl_bin: &str,
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

    let text =
        std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
    let mut items: Vec<Item> = Vec::new();
    let mut total = 0usize;
    let mut matched = 0usize;
    let mut unmatched: Vec<String> = Vec::new();
    // (index into `items`, database id) for the matched records
    let mut wanted: Vec<(usize, String)> = Vec::new();

    for line in text.lines() {
        let t = line.trim();
        if is_blank_or_comment(t) {
            items.push(Item::Pass(line.to_string()));
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
            if let Some(ov) = &g.overview {
                if !ov.is_empty() && (overwrite || missing(&obj, "desc")) {
                    obj.insert("desc".into(), Value::from(clamp_desc(ov)));
                }
            }
            wanted.push((items.len(), g.database_id.clone()));
        } else {
            unmatched.push(format!("{name} ({id})"));
        }
        items.push(Item::Rec(obj));
    }

    // Image lookup (shared by colour detection + the art manifest).
    let images = if detect_color || art_manifest.is_some() {
        let ids: HashSet<String> = wanted.iter().map(|(_, d)| d.clone()).collect();
        parse_images(metadata, &ids)?
    } else {
        HashMap::new()
    };

    // Colour detection from a gameplay screenshot (box art is always colourful).
    let (mut col_n, mut bw_n) = (0usize, 0usize);
    if detect_color {
        let tmp = std::env::temp_dir().join("atrium-shots");
        std::fs::create_dir_all(&tmp)?;
        for (item_idx, dbid) in &wanted {
            let need = match &items[*item_idx] {
                // Pre-1987 titles are B&W by the year prior (the Mac was 1-bit
                // until the Mac II), so don't let a colourful LaunchBox shot
                // mislabel them — leave `color` unset for the catalog to fill.
                Item::Rec(o) => {
                    let pre_color_mac = o.get("year").and_then(Value::as_i64).is_some_and(|y| y < 1987);
                    !pre_color_mac && (overwrite || missing(o, "color"))
                }
                _ => false,
            };
            if !need {
                continue;
            }
            let Some(shot) = images.get(dbid).and_then(|s| s.screenshot.as_ref()) else {
                continue;
            };
            let dst = tmp.join(dbid);
            if download(&format!("{IMAGE_URL}{shot}"), &dst, curl_bin).is_ok() {
                if let Ok(is_col) = is_color_image(&dst) {
                    if let Item::Rec(o) = &mut items[*item_idx] {
                        o.insert("color".into(), Value::Bool(is_col));
                    }
                    if is_col {
                        col_n += 1;
                    } else {
                        bw_n += 1;
                    }
                }
            }
        }
    }

    let mut out_text = String::new();
    for it in &items {
        match it {
            Item::Pass(s) => {
                out_text.push_str(s);
                out_text.push('\n');
            }
            Item::Rec(o) => {
                out_text.push_str(&serde_json::to_string(&Value::Object(o.clone()))?);
                out_text.push('\n');
            }
        }
    }
    std::fs::write(out, &out_text).with_context(|| format!("writing {}", out.display()))?;

    eprintln!(
        "enriched {matched}/{total} item(s) -> {} ({} unmatched)",
        out.display(),
        unmatched.len()
    );
    if detect_color {
        eprintln!("colour-detect: {col_n} colour, {bw_n} B&W (from screenshots)");
    }
    for u in &unmatched {
        eprintln!("  unmatched: {u}");
    }

    if let Some(mpath) = art_manifest {
        let mut m = String::new();
        let (mut n, mut n_shot) = (0, 0);
        for (item_idx, db_id) in &wanted {
            let item_id = match &items[*item_idx] {
                Item::Rec(o) => o.get("id").and_then(Value::as_str).unwrap_or(""),
                _ => "",
            };
            let set = images.get(db_id);
            let box_front = set.and_then(|s| s.box_front.as_ref());
            let shot = set.and_then(|s| s.screenshot.as_ref());
            if box_front.is_none() && shot.is_none() {
                continue;
            }
            // One line per item: Box-Front as "art", gameplay Screenshot as "shot"
            // (either may be absent). The image pass bakes both depth variants.
            let mut fields = format!("\"id\":{item_id:?},\"databaseID\":{db_id:?}");
            if let Some(file) = box_front {
                fields.push_str(&format!(",\"art\":{:?}", format!("{IMAGE_URL}{file}")));
                n += 1;
            }
            if let Some(file) = shot {
                fields.push_str(&format!(",\"shot\":{:?}", format!("{IMAGE_URL}{file}")));
                n_shot += 1;
            }
            m.push_str(&format!("{{{fields}}}\n"));
        }
        std::fs::write(mpath, m)?;
        eprintln!(
            "art manifest: {n} Box-Front + {n_shot} Screenshot image(s) -> {}",
            mpath.display()
        );
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
    fn candidate_keys_handle_articles() {
        // "The Ancient Art of War" and our "Ancient Art of War" share a key
        assert!(candidate_keys("The Ancient Art of War").contains(&"ancient art of war".to_string()));
        assert!(candidate_keys("Ancient Art of War").contains(&"ancient art of war".to_string()));
        // comma-article: "Hobbit, The" -> "hobbit"
        assert!(candidate_keys("Hobbit, The").contains(&"hobbit".to_string()));
        assert!(candidate_keys("The Hobbit").contains(&"hobbit".to_string()));
    }

    #[test]
    fn candidate_keys_handle_versions() {
        // dotted version stripped: "Glider 4.0" matches our "Glider"
        assert!(candidate_keys("Glider 4.0").contains(&"glider".to_string()));
        assert_eq!(strip_version("Glider 4.0"), "Glider");
        // bare sequel number NOT stripped
        assert_eq!(strip_version("Prince of Persia 2"), "Prince of Persia 2");
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
