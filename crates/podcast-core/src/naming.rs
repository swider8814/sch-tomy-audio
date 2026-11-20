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

/// Episode title split into the part the user types (sermon title) and the
/// part derived from the file name (speaker and date), joined as
/// `"<sermon> - <suffix>"` like the existing podcast episodes
/// ("Kiedy masz dość - Adam Szumorek - 2025.11.23").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleParts {
    pub sermon: String,
    pub suffix: String,
}

/// Joins the parts with " - ", skipping empty ones.
pub fn join_title(sermon: &str, suffix: &str) -> String {
    [sermon.trim(), suffix.trim()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" - ")
}

/// Date in the file name (`dd.mm.yyyy`, `yyyy.mm.dd`, `yyyy-mm-dd`, also with
/// `_`), validated, as (year, month, day).
pub fn date_from_name(name: &str) -> Option<(i32, u32, u32)> {
    static DMY: OnceLock<Regex> = OnceLock::new();
    static YMD: OnceLock<Regex> = OnceLock::new();
    let ymd = YMD.get_or_init(|| {
        Regex::new(
            r"(?:^|[^0-9])((?:19|20)[0-9]{2})[.\-_]([01]?[0-9])[.\-_]([0-3]?[0-9])(?:[^0-9]|$)",
        )
        .unwrap()
    });
    let dmy = DMY.get_or_init(|| {
        Regex::new(
            r"(?:^|[^0-9])([0-3]?[0-9])[.\-_]([01]?[0-9])[.\-_]((?:19|20)[0-9]{2})(?:[^0-9]|$)",
        )
        .unwrap()
    });
    let valid = |y: i32, m: u32, d: u32| {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        (1..=days).contains(&d).then_some((y, m, d))
    };
    if let Some(c) = ymd.captures(name) {
        if let Some(v) = valid(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?) {
            return Some(v);
        }
    }
    let c = dmy.captures(name)?;
    valid(c[3].parse().ok()?, c[2].parse().ok()?, c[1].parse().ok()?)
}

/// Title parts from the file names used by the church:
/// * "06.09.2026 - Rafał Piekarski - Wspólniej - blisko_audio.mp3"
///   (date - speaker - title): sermon "Wspólniej - blisko", suffix
///   "Rafał Piekarski - 2026.09.06";
/// * "TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4" (no title): sermon
///   empty (the user types it), suffix "Adam Szumorek - 2026.10.11";
/// * "Tytuł - Speaker - 2026.10.11.mp3" (date last) is understood too.
///
/// Names without a speaker and a date fall back to [`default_title`] as the
/// sermon title and no suffix.
pub fn title_parts(input: &Path) -> TitleParts {
    static SEP: OnceLock<Regex> = OnceLock::new();
    static AUDIO: OnceLock<Regex> = OnceLock::new();
    let sep = SEP.get_or_init(|| Regex::new(r"\s+[-\u{2013}\u{2014}]\s+").unwrap());
    let audio = AUDIO.get_or_init(|| Regex::new(r"(?i)[\s_-]*(audio|podcast)$").unwrap());
    let name = stem(input);
    let name = audio.replace(&name, "").into_owned();
    let segs: Vec<String> = sep
        .split(&name)
        .map(|seg| {
            seg.replace('_', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|seg| !seg.is_empty())
        .collect();
    let is_person =
        |seg: &String| date_from_name(seg).is_none() && seg.chars().any(char::is_alphabetic);
    let fallback = || TitleParts {
        sermon: default_title(input),
        suffix: String::new(),
    };
    let Some(di) = segs.iter().position(|seg| date_from_name(seg).is_some()) else {
        return fallback();
    };
    let Some((y, m, d)) = date_from_name(&segs[di]) else {
        return fallback();
    };
    let (speaker, sermon) = if di + 1 < segs.len() && is_person(&segs[di + 1]) {
        // date - speaker [- title...]
        (segs[di + 1].clone(), segs[di + 2..].join(" - "))
    } else if di == segs.len() - 1 && di >= 2 && is_person(&segs[di - 1]) {
        // title... - speaker - date
        (segs[di - 1].clone(), segs[..di - 1].join(" - "))
    } else {
        return fallback();
    };
    TitleParts {
        sermon,
        suffix: format!("{speaker} - {y:04}.{m:02}.{d:02}"),
    }
}

/// Output file name built from the episode title, date first so finished
/// files sort chronologically:
/// "2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3".
/// `suffix` is "Speaker - YYYY.MM.DD" as produced by [`title_parts`] (the
/// user may have edited it). Returns `None` when there is nothing to name
/// the file after; the caller then uses [`output_file_name`].
pub fn episode_file_name(sermon: &str, suffix: &str) -> Option<String> {
    static SEP: OnceLock<Regex> = OnceLock::new();
    let sep = SEP.get_or_init(|| Regex::new(r"\s+[-\u{2013}\u{2014}]\s+").unwrap());
    let date = date_from_name(suffix);
    let speaker = sep
        .split(suffix.trim())
        .filter(|seg| date_from_name(seg).is_none())
        .collect::<Vec<_>>()
        .join(" ");
    let mut parts: Vec<String> = Vec::new();
    if let Some((y, m, d)) = date {
        parts.push(format!("{y:04}-{m:02}-{d:02}"));
    }
    parts.push(sermon.trim().to_string());
    parts.push(speaker.trim().to_string());
    let text = parts.join(" ");
    if sermon.trim().is_empty() && speaker.trim().is_empty() {
        return None;
    }
    let mut slug = slugify(&text);
    // keep room for "-podcast-NN.mp3" within common file name limits
    slug.truncate(100);
    while slug.ends_with('-') {
        slug.pop();
    }
    Some(format!("{slug}{OUTPUT_SUFFIX}.mp3"))
}

/// A file name the pipeline accepts as given: `[a-z0-9-]` stem plus ".mp3",
/// so it can never contain path separators, `..` or reserved characters.
pub fn is_safe_output_name(name: &str) -> bool {
    match name.strip_suffix(".mp3") {
        Some(stem) => {
            !stem.is_empty()
                && stem.len() <= 140
                && !stem.starts_with('-')
                && stem
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        None => false,
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
    fn title_parts_from_church_file_names() {
        let p = title_parts(Path::new(
            "C:/Nagrania/TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4",
        ));
        assert_eq!(p.sermon, "");
        assert_eq!(p.suffix, "Adam Szumorek - 2026.10.11");
        assert_eq!(
            join_title("Co mi wolno?", &p.suffix),
            "Co mi wolno? - Adam Szumorek - 2026.10.11"
        );
        let p = title_parts(Path::new("23.11.2025 - Adam Szumorek_audio.mp3"));
        assert_eq!(p.suffix, "Adam Szumorek - 2025.11.23");
        let p = title_parts(Path::new("Kazanie 2025-3-9 – Jan Kowalski.m4a"));
        assert_eq!(p.suffix, "Jan Kowalski - 2025.03.09");
    }

    #[test]
    fn title_parts_with_title_in_the_name() {
        // Tomaszów and Łódź: date - speaker - title(_audio)
        let p = title_parts(Path::new(
            "06.09.2026 - Rafał Piekarski - Wspólniej - blisko siebie_audio.mp3",
        ));
        assert_eq!(p.sermon, "Wspólniej - blisko siebie");
        assert_eq!(p.suffix, "Rafał Piekarski - 2026.09.06");
        let p = title_parts(Path::new(
            "27.09.2026 - Gary Johnson - Razem Najlepiej_audio.mp3",
        ));
        assert_eq!(p.sermon, "Razem Najlepiej");
        assert_eq!(p.suffix, "Gary Johnson - 2026.09.27");
        assert_eq!(
            join_title(&p.sermon, &p.suffix),
            "Razem Najlepiej - Gary Johnson - 2026.09.27"
        );
        let p = title_parts(Path::new(
            "13.09.2026 - Adam Pańczak - Gościnność - drzwi otwarte.mp3",
        ));
        assert_eq!(p.sermon, "Gościnność - drzwi otwarte");
        assert_eq!(p.suffix, "Adam Pańczak - 2026.09.13");
        // title - speaker - date
        let p = title_parts(Path::new(
            "Kiedy masz dość - Adam Szumorek - 2025.11.23.mp3",
        ));
        assert_eq!(p.sermon, "Kiedy masz dość");
        assert_eq!(p.suffix, "Adam Szumorek - 2025.11.23");
    }

    #[test]
    fn title_parts_fallback() {
        // no " - Speaker" segment: the old behaviour (title from the name)
        let p = title_parts(Path::new("30.11.2025-mac-johnson-tytul_audio.mp3"));
        assert_eq!(p.sermon, "30.11.2025 mac johnson tytul");
        assert_eq!(p.suffix, "");
        // speaker but no date
        let p = title_parts(Path::new("Kazanie - Adam Szumorek.mp3"));
        assert_eq!(p.suffix, "");
        assert_eq!(join_title("", ""), "");
        assert_eq!(join_title(" a ", ""), "a");
    }

    #[test]
    fn episode_file_names() {
        assert_eq!(
            episode_file_name("Co mi wolno?", "Adam Szumorek - 2026.10.11").as_deref(),
            Some("2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3")
        );
        assert_eq!(
            episode_file_name("Wspólniej - blisko siebie", "Rafał Piekarski - 2026.09.06")
                .as_deref(),
            Some("2026-09-06-wspolniej-blisko-siebie-rafal-piekarski-podcast.mp3")
        );
        // no sermon title yet: date and speaker only
        assert_eq!(
            episode_file_name("", "Adam Szumorek - 2026.10.11").as_deref(),
            Some("2026-10-11-adam-szumorek-podcast.mp3")
        );
        // title only (file name without speaker/date)
        assert_eq!(
            episode_file_name("Kazanie o łasce", "").as_deref(),
            Some("kazanie-o-lasce-podcast.mp3")
        );
        assert_eq!(episode_file_name("  ", ""), None);
        assert_eq!(episode_file_name("", " 2026.10.11 "), None);
        let long = episode_file_name(&"ą".repeat(300), "X - 2026.01.01").unwrap();
        assert!(is_safe_output_name(&long), "{long}");
        for n in [
            "2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3",
            "kazanie-o-lasce-podcast.mp3",
        ] {
            assert!(is_safe_output_name(n), "{n}");
        }
        for bad in [
            "../x.mp3", "a b.mp3", "Ą.mp3", "x.wav", ".mp3", "-x.mp3", "a/b.mp3", "a\\b.mp3",
        ] {
            assert!(!is_safe_output_name(bad), "{bad}");
        }
    }

    #[test]
    fn dates() {
        assert_eq!(date_from_name("TOMY 2026.10.11 x"), Some((2026, 10, 11)));
        assert_eq!(date_from_name("30.11.2025-mac"), Some((2025, 11, 30)));
        assert_eq!(date_from_name("2025_1_5"), Some((2025, 1, 5)));
        assert_eq!(date_from_name("2025.13.40"), None);
        // real calendar days only
        assert_eq!(date_from_name("31.02.2026"), None);
        assert_eq!(date_from_name("2026.04.31"), None);
        assert_eq!(date_from_name("29.02.2026"), None);
        assert_eq!(date_from_name("29.02.2028"), Some((2028, 2, 29)));
        assert_eq!(date_from_name("2026.12.31"), Some((2026, 12, 31)));
        assert_eq!(date_from_name("bez daty"), None);
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
