//! Remembered speaker (preacher) names, suggested in the "Kaznodzieja i
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

/// Speaker part of a title suffix "Kaznodzieja - RRRR.MM.DD": the text before
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
