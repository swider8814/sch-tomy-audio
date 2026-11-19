//! Moving the setup to another computer: one `.schtomy` file (JSON) with the
//! settings, the intro/outro/cover files themselves, the teaser patterns and
//! the remembered speakers.
//!
//! Folder settings (results, YouTube, watched folder) are machine-specific
//! and never travel. On import the intro, outro and cover are written into the
//! app's own data folder and the settings point there; patterns and speakers
//! are added to the existing ones. Everything read from the file is checked
//! and bounded: it may come from anywhere.

use crate::error::{CoreError, ErrorKind, Result};
use crate::pcm::{bytes_to_f32, f32_to_bytes};
use crate::settings::{write_atomic, Settings};
use crate::teaser::{PatternStore, MAX_PATTERN_S, MIN_PATTERN_S, PATTERN_RATE};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "schtomy-audio-settings";
pub const VERSION: u32 = 1;
/// Largest accepted bundle file.
pub const MAX_BUNDLE_BYTES: u64 = 300 * 1024 * 1024;
/// Largest intro/outro file put into a bundle.
pub const MAX_JINGLE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PATTERNS: usize = 200;
const MAX_SPEAKERS_IN: usize = 1000;

/// Extensions an imported intro/outro may keep (anything else becomes .bin
/// and ffmpeg detects the format from the content).
const AUDIO_EXTENSIONS: [&str; 12] = [
    "mp3", "wav", "m4a", "aac", "ogg", "opus", "flac", "wma", "mp4", "mov", "webm", "mkv",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileData {
    /// Original file name (for the extension and for display).
    pub name: String,
    /// Base64 contents.
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternData {
    pub name: String,
    pub source: String,
    /// Base64 of little-endian f32 samples at `PATTERN_RATE`.
    pub samples: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    pub settings: Settings,
    pub intro: Option<FileData>,
    pub outro: Option<FileData>,
    pub cover: Option<FileData>,
    #[serde(default)]
    pub patterns: Vec<PatternData>,
    #[serde(default)]
    pub speakers: Vec<String>,
}

/// What an import changed, for the message shown to the user.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub intro: bool,
    pub outro: bool,
    pub cover: bool,
    pub patterns_added: usize,
    pub patterns_skipped: usize,
    pub speakers_added: usize,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::new(ErrorKind::Settings, msg.into())
}

fn read_file(path: &Path, limit: u64) -> Result<FileData> {
    let meta = std::fs::metadata(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    if meta.len() > limit {
        return Err(err(format!(
            "{} is larger than {} MB",
            path.display(),
            limit / 1024 / 1024
        )));
    }
    let data = std::fs::read(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    Ok(FileData {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        data: B64.encode(data),
    })
}

/// Builds the bundle bytes for the current setup.
pub fn export(
    settings: &Settings,
    store: &PatternStore,
    speakers: &[String],
    app_version: &str,
) -> Result<Vec<u8>> {
    let mut s = settings.clone();
    s.output_dir = None;
    s.youtube_dir = None;
    s.watch_dir = None;
    s.intro = None;
    s.outro = None;
    s.cover = None;
    let bundle = Bundle {
        format: FORMAT.into(),
        version: VERSION,
        app_version: app_version.into(),
        settings: s,
        intro: settings
            .intro
            .as_deref()
            .map(|p| read_file(p, MAX_JINGLE_BYTES))
            .transpose()?,
        outro: settings
            .outro
            .as_deref()
            .map(|p| read_file(p, MAX_JINGLE_BYTES))
            .transpose()?,
        cover: settings
            .cover
            .as_deref()
            .map(|p| read_file(p, MAX_COVER_BYTES))
            .transpose()?,
        patterns: store
            .load_all()
            .into_iter()
            .map(|(m, samples)| PatternData {
                name: m.name,
                source: m.source,
                samples: B64.encode(f32_to_bytes(&samples)),
            })
            .collect(),
        speakers: speakers.to_vec(),
    };
    serde_json::to_vec(&bundle).map_err(|e| err(e.to_string()))
}

/// Reads and checks a bundle file.
pub fn read(path: &Path) -> Result<Bundle> {
    let meta = std::fs::metadata(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    if meta.len() > MAX_BUNDLE_BYTES {
        return Err(err("bundle file too large"));
    }
    let bytes = std::fs::read(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> Result<Bundle> {
    let b: Bundle =
        serde_json::from_slice(bytes).map_err(|e| err(format!("not a settings file: {e}")))?;
    if b.format != FORMAT {
        return Err(err(format!("unknown format {:?}", b.format)));
    }
    if b.version > VERSION {
        return Err(err(format!(
            "settings file from a newer version ({}), update the app",
            b.app_version
        )));
    }
    Ok(b)
}

fn decode(f: &FileData, limit: u64) -> Result<Vec<u8>> {
    // base64 is 4/3 of the data: refuse oversized text before decoding
    if f.data.len() as u64 > limit / 3 * 4 + 8 {
        return Err(err(format!("{} is too large", f.name)));
    }
    B64.decode(f.data.as_bytes())
        .map_err(|e| err(format!("{}: {e}", f.name)))
}

/// Safe file name for an imported jingle: `<stem>.<known extension>`.
fn jingle_name(stem: &str, original: &str) -> String {
    let ext = Path::new(original)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or_else(|| "bin".into());
    format!("{stem}.{ext}")
}

/// Applies a bundle: writes its files into `files_dir` (the app's data
/// folder), adds patterns and speakers, and returns the new settings.
/// Machine-specific folders keep their current values; a jingle missing from
/// the bundle keeps the current one.
pub fn apply(
    bundle: &Bundle,
    current: &Settings,
    files_dir: &Path,
    store: &PatternStore,
    speakers: &mut Vec<String>,
) -> Result<(Settings, ImportSummary)> {
    let mut summary = ImportSummary::default();
    let mut s = bundle.settings.clone();
    s.output_dir = current.output_dir.clone();
    s.youtube_dir = current.youtube_dir.clone();
    s.watch_dir = current.watch_dir.clone();
    s.intro = current.intro.clone();
    s.outro = current.outro.clone();
    s.cover = current.cover.clone();

    let write = |name: String, data: &[u8]| -> Result<PathBuf> {
        let p = files_dir.join(name);
        write_atomic(&p, data).map_err(|e| err(format!("{}: {e}", p.display())))?;
        Ok(p)
    };
    if let Some(f) = &bundle.intro {
        s.intro = Some(write(
            jingle_name("intro", &f.name),
            &decode(f, MAX_JINGLE_BYTES)?,
        )?);
        summary.intro = true;
    }
    if let Some(f) = &bundle.outro {
        s.outro = Some(write(
            jingle_name("outro", &f.name),
            &decode(f, MAX_JINGLE_BYTES)?,
        )?);
        summary.outro = true;
    }
    if let Some(f) = &bundle.cover {
        let data = decode(f, MAX_COVER_BYTES)?;
        if data.len() < 4 || data[0..3] != [0xFF, 0xD8, 0xFF] {
            return Err(err("cover is not a JPEG file"));
        }
        s.cover = Some(write("okladka.jpg".into(), &data)?);
        summary.cover = true;
    }

    let existing = store.list();
    let max_samples = (MAX_PATTERN_S * PATTERN_RATE as f64) as usize;
    for p in bundle.patterns.iter().take(MAX_PATTERNS) {
        let bytes = decode(
            &FileData {
                name: p.name.clone(),
                data: p.samples.clone(),
            },
            (max_samples * 4) as u64,
        )?;
        let samples = bytes_to_f32(&bytes);
        let duration = samples.len() as f64 / PATTERN_RATE as f64;
        let same = existing
            .iter()
            .any(|m| m.name.trim() == p.name.trim() && (m.duration - duration).abs() < 0.01);
        if same || duration < MIN_PATTERN_S || samples.len() > max_samples {
            summary.patterns_skipped += 1;
            continue;
        }
        store.add(&p.name, &p.source, &samples)?;
        summary.patterns_added += 1;
    }

    for name in bundle.speakers.iter().take(MAX_SPEAKERS_IN).rev() {
        let n: String = name
            .trim()
            .chars()
            .take(crate::speakers::MAX_NAME_CHARS)
            .collect();
        let known = speakers
            .iter()
            .any(|k| k.to_lowercase() == n.to_lowercase());
        if !n.is_empty() && !known {
            // appended at the end: the speakers of this computer stay first
            speakers.push(n);
            summary.speakers_added += 1;
        }
    }
    speakers.truncate(crate::speakers::MAX_SPEAKERS);

    Ok((s.sanitized(), summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]
    }

    #[test]
    fn export_and_import_on_another_computer() {
        let a = tempfile::tempdir().unwrap();
        let intro = a.path().join("Intro ząb.mp3");
        let cover = a.path().join("okładka.jpg");
        std::fs::write(&intro, b"ID3 fake intro").unwrap();
        std::fs::write(&cover, jpeg()).unwrap();
        let store_a = PatternStore::new(a.path().join("patterns"));
        store_a
            .add("Łaska", "a.mp3", &vec![0.25f32; 8000 * 3])
            .unwrap();
        let settings = Settings {
            intro: Some(intro.clone()),
            cover: Some(cover),
            output_dir: Some("/Users/ktoś/Wyniki".into()),
            gap_after_intro: 2.0,
            enhance_voice: true,
            ..Default::default()
        };
        let bytes = export(&settings, &store_a, &["Rafał Piekarski".into()], "0.1.5").unwrap();
        let file = a.path().join("ustawienia.schtomy");
        std::fs::write(&file, &bytes).unwrap();

        // the other computer already has its own folders, a pattern and a speaker
        let b = tempfile::tempdir().unwrap();
        let store_b = PatternStore::new(b.path().join("patterns"));
        store_b
            .add("Inna seria", "b.mp3", &vec![0.1f32; 8000 * 2])
            .unwrap();
        let mut speakers = vec!["Jan Nowak".to_string()];
        let current = Settings {
            output_dir: Some("C:/Podcast".into()),
            outro: Some("C:/Muzyka/outro.mp3".into()),
            ..Default::default()
        };
        let bundle = read(&file).unwrap();
        let files_dir = b.path().join("Dane ąę").join("imported");
        let (s, sum) = apply(&bundle, &current, &files_dir, &store_b, &mut speakers).unwrap();

        assert_eq!(
            std::fs::read(s.intro.as_ref().unwrap()).unwrap(),
            b"ID3 fake intro"
        );
        assert!(s.intro.as_ref().unwrap().starts_with(&files_dir));
        assert_eq!(s.intro.as_ref().unwrap().file_name().unwrap(), "intro.mp3");
        assert_eq!(std::fs::read(s.cover.as_ref().unwrap()).unwrap(), jpeg());
        // no outro in the bundle: the current one stays; folders stay local
        assert_eq!(s.outro, current.outro);
        assert_eq!(s.output_dir, current.output_dir);
        assert_eq!(s.gap_after_intro, 2.0);
        assert!(s.enhance_voice);
        assert_eq!(store_b.list().len(), 2);
        assert_eq!(speakers, ["Jan Nowak", "Rafał Piekarski"]);
        assert_eq!(
            sum,
            ImportSummary {
                intro: true,
                outro: false,
                cover: true,
                patterns_added: 1,
                patterns_skipped: 0,
                speakers_added: 1
            }
        );

        // importing the same file again adds nothing new
        let (_, again) = apply(&bundle, &s, &files_dir, &store_b, &mut speakers).unwrap();
        assert_eq!(
            (
                again.patterns_added,
                again.patterns_skipped,
                again.speakers_added
            ),
            (0, 1, 0)
        );
    }

    #[test]
    fn rejects_foreign_and_broken_files() {
        assert!(parse(b"{}").is_err());
        assert!(parse(b"not json").is_err());
        let mut b = Bundle {
            format: "something-else".into(),
            version: 1,
            app_version: "x".into(),
            settings: Settings::default(),
            intro: None,
            outro: None,
            cover: None,
            patterns: vec![],
            speakers: vec![],
        };
        assert!(parse(&serde_json::to_vec(&b).unwrap()).is_err());
        b.format = FORMAT.into();
        b.version = VERSION + 1;
        assert!(parse(&serde_json::to_vec(&b).unwrap()).is_err());
        b.version = VERSION;
        assert!(parse(&serde_json::to_vec(&b).unwrap()).is_ok());
    }

    #[test]
    fn imported_files_cannot_escape_or_lie() {
        let d = tempfile::tempdir().unwrap();
        let store = PatternStore::new(d.path().join("p"));
        let bundle = Bundle {
            format: FORMAT.into(),
            version: VERSION,
            app_version: "x".into(),
            settings: Settings {
                gap_after_intro: 99.0,
                ..Default::default()
            },
            intro: Some(FileData {
                name: "../../evil.sh".into(),
                data: B64.encode(b"x"),
            }),
            outro: None,
            cover: Some(FileData {
                name: "c.jpg".into(),
                data: B64.encode(b"<svg>"),
            }),
            patterns: vec![],
            speakers: vec![],
        };
        // a cover that is not a JPEG is refused
        assert!(apply(&bundle, &Settings::default(), d.path(), &store, &mut vec![]).is_err());
        let bundle = Bundle {
            cover: None,
            ..bundle
        };
        let (s, _) = apply(&bundle, &Settings::default(), d.path(), &store, &mut vec![]).unwrap();
        // the name never leaves the folder and unknown extensions become .bin
        assert_eq!(s.intro.unwrap(), d.path().join("intro.bin"));
        // out-of-range values are clamped like any settings file
        assert_eq!(s.gap_after_intro, 3.0);
    }
}
