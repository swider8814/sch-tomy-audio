//! Remembered speaker (preacher) names, suggested in the "Mówca i
//! data" field so the same person is always spelled the same way.
//!
//! Names are taken from finished episodes. The list is kept in a small JSON
//! file in the app data folder, most recently used first.

use crate::settings::write_atomic;
use std::path::Path;

/// At most this many names are remembered.
pub const MAX_SPEAKERS: usize = 300;
/// Longest accepted name, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// Speaker part of a title suffix "Mówca - RRRR.MM.DD": the text before
/// a trailing date (separated by " - " or a space). `None` when nothing is
/// left or the text has no letters.
pub fn speaker_of(suffix: &str) -> Option<String> {
    let mut s = suffix.trim();
    // strip a trailing date like 2026.11.08, 2026-11-08 or 08.11.2026
    let date = regex_lite_date_tail(s);
    s = s[..s.len() - date].trim_end();
    s = s.trim_end_matches(['-', ',', '|']).trim_end();
    let name: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || !name.chars().any(char::is_alphabetic) {
        return None;
    }
    Some(name.chars().take(MAX_NAME_CHARS).collect())
}

/// Byte length of a date at the very end of `s` (0 if there is none).
fn regex_lite_date_tail(s: &str) -> usize {
    let b = s.as_bytes();
    // forms: dddd.dd.dd / dddd-dd-dd / dd.dd.dddd / dd-dd-dddd (10 bytes)
    if b.len() < 10 {
        return 0;
    }
    let t = &b[b.len() - 10..];
    let d = |i: usize| t[i].is_ascii_digit();
    let sep = |i: usize| t[i] == b'.' || t[i] == b'-';
    let ymd = d(0) && d(1) && d(2) && d(3) && sep(4) && d(5) && d(6) && sep(7) && d(8) && d(9);
    let dmy = d(0) && d(1) && sep(2) && d(3) && d(4) && sep(5) && d(6) && d(7) && d(8) && d(9);
    let boundary = b.len() == 10 || !b[b.len() - 11].is_ascii_alphanumeric();
    if (ymd || dmy) && boundary {
        10
    } else {
        0
    }
}

/// Key for comparing names: lower case, single spaces.
fn key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Puts `name` first (once, case-insensitively; the new spelling wins) and
/// keeps at most [`MAX_SPEAKERS`] names.
pub fn remember(list: &mut Vec<String>, name: &str) {
    let k = key(name);
    if k.is_empty() {
        return;
    }
    list.retain(|n| key(n) != k);
    list.insert(0, name.split_whitespace().collect::<Vec<_>>().join(" "));
    list.truncate(MAX_SPEAKERS);
}

/// Names from pasted text or a .txt file: one per line. Bullets ("-", "*",
/// "•"), numbering ("1.", "2)"), empty lines and `#` comments are skipped,
/// as are duplicates; at most [`MAX_SPEAKERS`] names.
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        l = l.trim_start_matches(['-', '*', '•', '–', '—']).trim_start();
        let digits = l.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && l[digits..].starts_with(['.', ')']) {
            l = l[digits + 1..].trim_start();
        }
        let name: String = l
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(MAX_NAME_CHARS)
            .collect();
        if name.chars().any(char::is_alphabetic) && !out.iter().any(|o| key(o) == key(&name)) {
            out.push(name);
        }
        if out.len() == MAX_SPEAKERS {
            break;
        }
    }
    out
}

/// Adds imported names after the existing ones (names used in episodes
/// stay first); returns how many were new.
pub fn add_all(list: &mut Vec<String>, names: &[String]) -> usize {
    let mut added = 0;
    for n in names {
        if list.len() >= MAX_SPEAKERS {
            break;
        }
        if !list.iter().any(|o| key(o) == key(n)) {
            list.push(n.clone());
            added += 1;
        }
    }
    added
}

/// Lower case ASCII form for matching names in free text ("Rafał" and
/// "rafal" match).
fn fold(text: &str) -> String {
    // per character: `deunicode::deunicode` would drop line breaks
    text.chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_ascii_lowercase().to_string()
            } else {
                deunicode::deunicode_char(c).unwrap_or(" ").to_lowercase()
            }
        })
        .collect()
}

/// Finds a speaker in a text such as a YouTube description: first a known
/// name (the one mentioned earliest, as whole words), otherwise a capitalised
/// first name and surname after a word that introduces the speaker, as in
/// "Do nowej serii kazań wprowadził nas Jerzy Sikora", "Kazanie wygłosił
/// pastor Jan Nowak" or "Mówca: Ewa Kowalska".
pub fn find_in_text(text: &str, known: &[String]) -> Option<String> {
    let text: String = text.chars().take(20_000).collect();
    known_in_text(&text, known).or_else(|| introduced_in_text(&text))
}

fn known_in_text(text: &str, known: &[String]) -> Option<String> {
    let folded = fold(text);
    let bytes = folded.as_bytes();
    let mut best: Option<(usize, &String)> = None;
    for name in known {
        let needle = fold(name.trim());
        if needle.len() < 3 {
            continue;
        }
        let mut from = 0;
        while let Some(pos) = folded[from..].find(&needle) {
            let at = from + pos;
            let end = at + needle.len();
            let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
            let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                if best.is_none_or(|(b, _)| at < b) {
                    best = Some((at, name));
                }
                break;
            }
            from = at + 1;
        }
    }
    best.map(|(_, name)| name.clone())
}

/// Words (lower case, without Polish letters) after which the speaker's name
/// follows.
const CUES: [&str; 32] = [
    "nas",
    "was",
    "wam",
    "sie",
    "mowca",
    "mowi",
    "mowil",
    "mowila",
    "kaznodzieja",
    "glosi",
    "glosil",
    "glosila",
    "wyglosil",
    "wyglosila",
    "prowadzi",
    "prowadzil",
    "prowadzila",
    "poprowadzil",
    "poprowadzila",
    "wprowadzil",
    "wprowadzila",
    "usluguje",
    "uslugiwal",
    "uslugiwala",
    "przemawia",
    "przemawial",
    "przemawiala",
    "naucza",
    "nauczal",
    "nauczala",
    "speaker",
    "preacher",
];
/// Titles between the cue and the name ("wygłosił pastor Jan Nowak").
const TITLES: [&str; 9] = [
    "pastor", "pastorka", "brat", "br.", "siostra", "s.", "ks.", "dr", "dr.",
];

fn introduced_in_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            // "Mówca:" ends with a colon; "nas," does not introduce anyone
            let cue = w.strip_suffix(':').unwrap_or(w);
            if !CUES.contains(&fold(cue).as_str()) {
                continue;
            }
            let mut rest = words[i + 1..]
                .iter()
                .copied()
                .skip_while(|w| matches!(*w, ":" | "-"))
                .peekable();
            while rest
                .peek()
                .is_some_and(|w| TITLES.contains(&fold(w).as_str()))
            {
                rest.next();
            }
            let mut name: Vec<&str> = Vec::new();
            for w in rest {
                let bare = w.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\u{201d}']);
                if !is_name_word(bare) || name.len() == 3 {
                    break;
                }
                name.push(bare);
                if bare.len() != w.len() {
                    break; // punctuation ends the name
                }
            }
            if name.len() >= 2 {
                return Some(name.join(" "));
            }
        }
    }
    None
}

/// "Jerzy", "Kowalska-Nowak", "O'Brien": each part a capital letter, then
/// lower case letters.
fn is_name_word(w: &str) -> bool {
    w.split(['-', '\'']).all(|part| {
        let mut chars = part.chars();
        chars.next().is_some_and(char::is_uppercase)
            && chars.clone().next().is_some()
            && chars.all(char::is_lowercase)
    })
}

/// Removes `name` (case-insensitively).
pub fn forget(list: &mut Vec<String>, name: &str) {
    let k = key(name);
    list.retain(|n| key(n) != k);
}

/// Reads the list; a missing or broken file gives an empty list. Entries
/// are cleaned up (trimmed, length-limited, duplicates dropped).
pub fn load(path: &Path) -> Vec<String> {
    let raw: Vec<String> = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for n in raw {
        let n: String = n.trim().chars().take(MAX_NAME_CHARS).collect();
        if !n.is_empty() && !out.iter().any(|o| key(o) == key(&n)) {
            out.push(n);
        }
        if out.len() == MAX_SPEAKERS {
            break;
        }
    }
    out
}

pub fn save(path: &Path, list: &[String]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?;
    write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_from_suffix() {
        assert_eq!(
            speaker_of("Adam Szumorek - 2026.10.11").as_deref(),
            Some("Adam Szumorek")
        );
        assert_eq!(
            speaker_of("Rafał  Piekarski - 06.09.2026").as_deref(),
            Some("Rafał Piekarski")
        );
        assert_eq!(
            speaker_of("Ewa Kowalska 2026-11-15").as_deref(),
            Some("Ewa Kowalska")
        );
        assert_eq!(speaker_of("Jan Nowak").as_deref(), Some("Jan Nowak"));
        assert_eq!(speaker_of(" - 2026.10.11"), None);
        assert_eq!(speaker_of(""), None);
        assert_eq!(speaker_of("2026.10.11"), None);
        // a date glued to a word is not a date
        assert_eq!(
            speaker_of("Kod X2026.10.11").as_deref(),
            Some("Kod X2026.10.11")
        );
        let long = "A".repeat(500);
        assert_eq!(speaker_of(&long).unwrap().chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn remember_moves_to_front_without_duplicates() {
        let mut l = vec!["Jan Nowak".to_string(), "Ewa Kowalska".to_string()];
        remember(&mut l, "ewa  kowalska");
        assert_eq!(l, ["ewa kowalska", "Jan Nowak"]);
        remember(&mut l, "Adam Szumorek");
        assert_eq!(l[0], "Adam Szumorek");
        remember(&mut l, "   ");
        assert_eq!(l.len(), 3);
        forget(&mut l, "JAN NOWAK");
        assert_eq!(l, ["Adam Szumorek", "ewa kowalska"]);
        for i in 0..400 {
            remember(&mut l, &format!("Osoba {i}"));
        }
        assert_eq!(l.len(), MAX_SPEAKERS);
        assert_eq!(l[0], "Osoba 399");
    }

    #[test]
    fn list_from_pasted_text() {
        let text = "Pastorzy\n# komentarz\n\n- Adam Szumorek\n* Rafał  Piekarski\n• Ewa Kowalska\n1. Jan Nowak\n2) Piotr Zieliński\n  adam szumorek  \n2026\n";
        assert_eq!(
            parse_list(text),
            [
                "Pastorzy",
                "Adam Szumorek",
                "Rafał Piekarski",
                "Ewa Kowalska",
                "Jan Nowak",
                "Piotr Zieliński"
            ]
        );
        let mut l = vec!["Jan Nowak".to_string()];
        let added = add_all(&mut l, &parse_list("Ewa Kowalska\njan nowak\nAnna Lis"));
        assert_eq!(added, 2);
        assert_eq!(l, ["Jan Nowak", "Ewa Kowalska", "Anna Lis"]);
    }

    #[test]
    fn speaker_from_a_youtube_description() {
        let known = vec![
            "Adam Szumorek".to_string(),
            "Leonid Matseyka".to_string(),
            "Rafał Piekarski".to_string(),
        ];
        // the church's description format, speaker not remembered yet
        let ours = "ŁDZ // Do nowej serii kazań \"Wiara pomimo\" wprowadził nas Jerzy Sikora\n\n\
                    Fragment: Księga Daniela 1:1-21\nData: 01.11.2026";
        assert_eq!(find_in_text(ours, &known).as_deref(), Some("Jerzy Sikora"));
        // a known name anywhere in the text, whole words, ignoring case and Polish letters
        assert_eq!(
            find_in_text(
                "Kazanie z niedzieli.\nZaprasza pastor rafal piekarski oraz cały zbór.",
                &known
            )
            .as_deref(),
            Some("Rafał Piekarski")
        );
        // the one mentioned first wins
        assert_eq!(
            find_in_text("Mówi Leonid Matseyka. Modlitwa: Adam Szumorek", &known).as_deref(),
            Some("Leonid Matseyka")
        );
        // not part of another word
        assert_eq!(find_in_text("Adam Szumorekowski", &known), None);
        // other ways of introducing an unknown speaker
        for (text, who) in [
            ("Seria: Babilon\nMówca: Jan Nowak\nZapraszamy!", "Jan Nowak"),
            ("Kaznodzieja - Ewa Kowalska (Łódź)", "Ewa Kowalska"),
            (
                "Kazanie wygłosił pastor Piotr Zieliński, zapraszamy",
                "Piotr Zieliński",
            ),
            (
                "Słowem podzielił się br. Anna Kowalska-Nowak.",
                "Anna Kowalska-Nowak",
            ),
            (
                "PTK // Poprowadził nas Jan Maria Rokita Nowak",
                "Jan Maria Rokita",
            ),
        ] {
            assert_eq!(find_in_text(text, &known).as_deref(), Some(who), "{text}");
        }
        // places, titles and references are not speakers
        for text in [
            "Kazanie: W Babilonie\nTomaszów Mazowiecki",
            "Fragment: Księga Daniela 1:1-21\nData: 01.11.2026",
            "Zaprasza nas Kościół w Łodzi",
            "wprowadził nas Jerzy",
            "wprowadził nas ŁDZ TOMY",
            "",
        ] {
            assert_eq!(find_in_text(text, &known), None, "{text}");
        }
    }

    #[test]
    fn load_and_save_with_polish_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Dane ąę").join("speakers.json");
        assert!(load(&p).is_empty());
        save(&p, &["Rafał Piekarski".into(), "Łukasz Żak".into()]).unwrap();
        assert_eq!(load(&p), ["Rafał Piekarski", "Łukasz Żak"]);
        std::fs::write(&p, br#"["  A  ", "a", "", "B"]"#).unwrap();
        assert_eq!(load(&p), ["A", "B"]);
        std::fs::write(&p, b"{broken").unwrap();
        assert!(load(&p).is_empty());
    }
}
