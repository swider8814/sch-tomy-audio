//! Output file names, default titles and years.

use regex::Regex;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_SLUG_LEN: usize = 120;
pub const OUTPUT_SUFFIX: &str = "-podcast";

fn polish_ascii(c: char) -> Option<&'static str> {
    Some(match c {
        'ą' => "a",
        'ć' => "c",
        'ę' => "e",
        'ł' => "l",
        'ń' => "n",
        'ó' => "o",
        'ś' => "s",
        'ź' | 'ż' => "z",
        'Ą' => "A",
        'Ć' => "C",
        'Ę' => "E",
        'Ł' => "L",
        'Ń' => "N",
        'Ó' => "O",
        'Ś' => "S",
        'Ź' | 'Ż' => "Z",
        _ => return None,
    })
}

/// Lowercase ASCII slug: Polish (and other) diacritics removed, every run of
/// other characters replaced by one hyphen. Only `[a-z0-9-]` remains, so the
/// result can never contain path separators or `..`.
pub fn slugify(s: &str) -> String {
    let mut ascii = String::with_capacity(s.len());
    for c in s.chars() {
        if let Some(r) = polish_ascii(c) {
            ascii.push_str(r);
        } else if c.is_ascii() {
            ascii.push(c);
        } else {
            ascii.push_str(deunicode::deunicode_char(c).unwrap_or("-"));
        }
    }
    let mut out = String::with_capacity(ascii.len());
    let mut pending_dash = false;
    for c in ascii.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    if out.len() > MAX_SLUG_LEN {
        out.truncate(MAX_SLUG_LEN);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        "nagranie".into()
    } else {
        out
    }
}

fn stem(input: &Path) -> String {
    input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `<slug of original name>-podcast.mp3`
pub fn output_file_name(input: &Path) -> String {
    format!("{}{}.mp3", slugify(&stem(input)), OUTPUT_SUFFIX)
}

/// Default episode title derived from the file name.
pub fn default_title(input: &Path) -> String {
    static AUDIO: OnceLock<Regex> = OnceLock::new();
    let re = AUDIO.get_or_init(|| Regex::new(r"(?i)[\s_-]*(audio|podcast)$").unwrap());
    let s = stem(input);
    let s = re.replace(&s, "").into_owned();
    let s: String = s
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        stem(input)
    } else {
        s
    }
}

/// Year from a date in the file name (`dd.mm.yyyy`, `yyyy-mm-dd`, `yyyy.mm.dd`).
pub fn year_from_name(name: &str) -> Option<i32> {
    static DMY: OnceLock<Regex> = OnceLock::new();
    static YMD: OnceLock<Regex> = OnceLock::new();
    let dmy = DMY.get_or_init(|| {
        Regex::new(r"(?:^|[^0-9])[0-3]?[0-9][.\-_][01]?[0-9][.\-_]((?:19|20)[0-9]{2})(?:[^0-9]|$)")
            .unwrap()
    });
    let ymd = YMD.get_or_init(|| {
        Regex::new(r"(?:^|[^0-9])((?:19|20)[0-9]{2})[.\-_][01]?[0-9][.\-_][0-3]?[0-9](?:[^0-9]|$)")
            .unwrap()
    });
    dmy.captures(name)
        .or_else(|| ymd.captures(name))
        .and_then(|c| c[1].parse().ok())
}

/// Year for the ID3 tag: from the file name, else the current year.
pub fn default_year(input: &Path) -> i32 {
    use chrono::Datelike;
    year_from_name(&stem(input)).unwrap_or_else(|| chrono::Local::now().year())
}

/// Atomically reserves a free output path in `dir` by creating an empty file
/// with `create_new` (never overwrites anything). Tries `name`, then
/// `stem-2.ext`, `stem-3.ext`, ...
pub fn reserve_output_path(dir: &Path, file_name: &str) -> io::Result<PathBuf> {
    let (base, ext) = match file_name.rsplit_once('.') {
        Some((b, e)) => (b.to_string(), format!(".{e}")),
        None => (file_name.to_string(), String::new()),
    };
    for n in 1..10_000 {
        let name = if n == 1 {
            format!("{base}{ext}")
        } else {
            format!("{base}-{n}{ext}")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free output file name"))
}

/// Checks that two paths do not point to the same file (the original must
/// never be overwritten).
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_examples() {
        assert_eq!(
            output_file_name(Path::new("/k/30.11.2025-mac-johnson-tytul_audio.mp3")),
            "30-11-2025-mac-johnson-tytul-audio-podcast.mp3"
        );
        assert_eq!(
            output_file_name(Path::new("Zażółć GĘŚLĄ jaźń – Łódź (2024).MP4")),
            "zazolc-gesla-jazn-lodz-2024-podcast.mp3"
        );
        assert_eq!(slugify("  ..//\\  "), "nagranie");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("Café Noël"), "cafe-noel");
        assert_eq!(slugify("a___b"), "a-b");
    }

    #[test]
    fn slug_is_safe_and_bounded() {
        let long = "ą".repeat(500);
        let s = slugify(&long);
        assert!(s.len() <= MAX_SLUG_LEN);
        assert!(s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert!(!s.starts_with('-') && !s.ends_with('-'));
    }

    #[test]
    fn titles() {
        assert_eq!(
            default_title(Path::new("30.11.2025-mac-johnson-tytul_audio.mp3")),
            "30.11.2025 mac johnson tytul"
        );
        assert_eq!(
            default_title(Path::new("Kazanie o łasce.m4a")),
            "Kazanie o łasce"
        );
        assert_eq!(default_title(Path::new("audio.mp3")), "audio");
    }

    #[test]
    fn years() {
        assert_eq!(year_from_name("30.11.2025-mac-johnson"), Some(2025));
        assert_eq!(year_from_name("kazanie 2024-03-17 rano"), Some(2024));
        assert_eq!(year_from_name("1.2.2023"), Some(2023));
        assert_eq!(year_from_name("kazanie 12345"), None);
        assert_eq!(year_from_name("bez daty"), None);
    }

    #[test]
    fn reserve_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a-podcast.mp3"), b"existing").unwrap();
        let p = reserve_output_path(dir.path(), "a-podcast.mp3").unwrap();
        assert_eq!(p.file_name().unwrap(), "a-podcast-2.mp3");
        let p3 = reserve_output_path(dir.path(), "a-podcast.mp3").unwrap();
        assert_eq!(p3.file_name().unwrap(), "a-podcast-3.mp3");
        assert_eq!(
            std::fs::read(dir.path().join("a-podcast.mp3")).unwrap(),
            b"existing"
        );
    }
}
