//! Downloading sermon audio from YouTube with the bundled yt-dlp (stage 2).
//!
//! Network access happens only here and only when the user pastes a link.
//! Safety rules (see docs/threat-model.md, T11):
//! * only `https://` YouTube video links are accepted; they are reduced to a
//!   canonical `https://www.youtube.com/watch?v=<id>` before yt-dlp sees them,
//!   and passed after `--`, so a link can never be read as an option;
//! * yt-dlp runs with `--ignore-config` (no user/system configuration files),
//!   `--no-playlist`, no post-processing or `--exec`, and a fixed output
//!   name made of `[a-z0-9-]` in a folder chosen by the app;
//! * the JavaScript runtime it needs for YouTube (Deno) is the bundled,
//!   pinned binary.

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{find_tool_path, run, Args, CancelToken, StdoutMode};
use crate::naming::slugify;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const ENV_YTDLP: &str = "SCHTOMY_YTDLP";
pub const ENV_DENO: &str = "SCHTOMY_DENO";

/// Prefix of our progress lines (see `--progress-template`).
const PROGRESS_PREFIX: &str = "SCHTOMY-PROGRESS ";
/// Prefix of the line with the final file path.
const FILE_PREFIX: &str = "SCHTOMY-FILE ";

#[derive(Debug, Clone)]
pub struct YtTools {
    pub ytdlp: PathBuf,
    pub deno: Option<PathBuf>,
}

impl YtTools {
    /// yt-dlp and Deno next to the executable (sidecars), env override or PATH.
    pub fn locate() -> Result<YtTools> {
        let ytdlp = find_tool_path("yt-dlp", ENV_YTDLP)?;
        let deno = find_tool_path("deno", ENV_DENO).ok();
        Ok(YtTools { ytdlp, deno })
    }
}

fn yt_err(details: impl Into<String>) -> CoreError {
    CoreError::new(ErrorKind::YouTube, details)
}

/// Validates a pasted YouTube link and returns the 11-character video id.
pub fn parse_video_id(input: &str) -> Result<String> {
    let s = input.trim();
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .ok_or_else(|| CoreError::new(ErrorKind::YouTubeLink, "not an http(s) link"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let valid_id = |id: &str| {
        id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    let id: Option<String> = match host.as_str() {
        "youtu.be" | "www.youtu.be" => path.split(['?', '#', '/']).next().map(str::to_string),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            let (p, query) = path.split_once('?').unwrap_or((path, ""));
            let query = query.split('#').next().unwrap_or("");
            if p == "watch" {
                query
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("v="))
                    .map(str::to_string)
            } else if let Some(r) = p
                .strip_prefix("live/")
                .or_else(|| p.strip_prefix("shorts/"))
                .or_else(|| p.strip_prefix("embed/"))
            {
                r.split(['/', '#']).next().map(str::to_string)
            } else {
                None
            }
        }
        _ => None,
    };
    match id {
        Some(id) if valid_id(&id) => Ok(id),
        _ => Err(CoreError::new(
            ErrorKind::YouTubeLink,
            format!("not a YouTube video link: {s}"),
        )),
    }
}

/// Result of reading a list of links (one per line).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkList {
    /// Video ids in list order, without duplicates.
    pub ids: Vec<String>,
    /// Lines that are not YouTube video links: (line number, text).
    pub invalid: Vec<(usize, String)>,
    /// Number of repeated links that were skipped.
    pub duplicates: usize,
}

/// Maximum number of links accepted from one list.
pub const MAX_LINKS: usize = 1000;

/// Parses a pasted text or a .txt file with YouTube links: one link per
/// line, empty lines and lines starting with `#` are ignored. A line may
/// contain other text (e.g. "Kazanie 1: https://youtu.be/..."); the first
/// token that is a YouTube link is used.
pub fn parse_link_list(text: &str) -> LinkList {
    let mut out = LinkList::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let found = line
            .split(|c: char| {
                c.is_whitespace() || c == ',' || c == ';' || c == '"' || c == '<' || c == '>'
            })
            .find_map(|tok| parse_video_id(tok).ok());
        match found {
            Some(id) if out.ids.contains(&id) => out.duplicates += 1,
            Some(id) if out.ids.len() < MAX_LINKS => out.ids.push(id),
            Some(_) => {}
            None => out.invalid.push((n + 1, line.chars().take(120).collect())),
        }
    }
    out
}

pub fn canonical_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// Options shared by every yt-dlp call.
fn common_args(tools: &YtTools) -> Args {
    let mut a = Args::new()
        .args(["--ignore-config", "--no-playlist", "--no-exec"])
        .args(["--no-warnings", "--encoding", "utf-8"]);
    if let Some(deno) = &tools.deno {
        let mut spec = std::ffi::OsString::from("deno:");
        spec.push(deno.as_os_str());
        a = a.arg("--js-runtimes").arg(spec);
    }
    a
}

pub fn metadata_args(tools: &YtTools, id: &str) -> Vec<std::ffi::OsString> {
    common_args(tools)
        .args(["--skip-download", "--print"])
        .arg("%(.{id,title,upload_date,uploader,channel,duration})j")
        .args(["--", &canonical_url(id)])
        .into_vec()
}

/// File stem of the downloaded audio: date, title and id, ASCII only.
pub fn download_stem(info: &VideoInfo) -> String {
    let date = info
        .date()
        .map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}"))
        .unwrap_or_default();
    let mut slug = slugify(&format!("{date} {}", info.title));
    slug.truncate(90);
    while slug.ends_with('-') {
        slug.pop();
    }
    // the id keeps the name unique; it may contain '_' and uppercase letters
    let id: String = info
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("{slug}-yt-{id}")
}

pub fn download_args(tools: &YtTools, info: &VideoInfo, dir: &Path) -> Vec<std::ffi::OsString> {
    let mut template = dir.as_os_str().to_os_string();
    template.push(std::path::MAIN_SEPARATOR_STR);
    template.push(format!("{}.%(ext)s", download_stem(info)));
    common_args(tools)
        // audio only, prefer AAC in MP4 (no conversion needed)
        .args(["-f", "bestaudio[ext=m4a]/bestaudio"])
        .args(["--no-mtime", "--newline", "--progress-template"])
        .arg(format!(
            "download:{PROGRESS_PREFIX}%(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"
        ))
        .args(["--print", &format!("after_move:{FILE_PREFIX}%(filepath)s")])
        .arg("-o")
        .arg(template)
        .args(["--", &canonical_url(&info.id)])
        .into_vec()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub upload_date: Option<String>,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
}

impl VideoInfo {
    /// Upload date (YYYYMMDD from yt-dlp) as (year, month, day).
    pub fn date(&self) -> Option<(i32, u32, u32)> {
        let d = self.upload_date.as_deref()?;
        if d.len() != 8 || !d.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some((
            d[0..4].parse().ok()?,
            d[4..6].parse().ok()?,
            d[6..8].parse().ok()?,
        ))
    }

    /// "2026.10.11" for the title suffix.
    pub fn dotted_date(&self) -> Option<String> {
        self.date().map(|(y, m, d)| format!("{y:04}.{m:02}.{d:02}"))
    }
}

pub fn parse_metadata(stdout: &str) -> Result<VideoInfo> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| yt_err(format!("no metadata in yt-dlp output: {stdout}")))?;
    serde_json::from_str(line).map_err(|e| yt_err(format!("metadata JSON: {e}")))
}

/// Progress (0..1) from one of our progress lines.
pub fn parse_progress(line: &str) -> Option<f64> {
    let rest = line.strip_prefix(PROGRESS_PREFIX)?;
    let mut it = rest.split_whitespace();
    let done: f64 = it.next()?.parse().ok()?;
    let total = it.next().and_then(|t| t.parse::<f64>().ok());
    let estimate = it.next().and_then(|t| t.parse::<f64>().ok());
    let total = total.or(estimate).filter(|t| *t > 0.0)?;
    Some((done / total).clamp(0.0, 1.0))
}

/// Turns yt-dlp's error output into a friendly error kind.
fn classify(e: CoreError) -> CoreError {
    if e.kind != ErrorKind::Processing {
        return e;
    }
    let d = e.details.to_ascii_lowercase();
    let kind = if d.contains("sign in to confirm") || d.contains("not a bot") {
        ErrorKind::YouTubeBlocked
    } else if d.contains("private video")
        || d.contains("video unavailable")
        || d.contains("members-only")
        || d.contains("removed")
    {
        ErrorKind::YouTubeUnavailable
    } else {
        ErrorKind::YouTube
    };
    CoreError::new(kind, e.details)
}

pub fn fetch_metadata(tools: &YtTools, id: &str, cancel: &CancelToken) -> Result<VideoInfo> {
    let args = metadata_args(tools, id);
    let out = run(&tools.ytdlp, &args, cancel, StdoutMode::Capture).map_err(classify)?;
    parse_metadata(&String::from_utf8_lossy(&out.stdout))
}

/// Downloads the audio into `dir` and returns the file path.
pub fn download_audio(
    tools: &YtTools,
    info: &VideoInfo,
    dir: &Path,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(f64),
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| {
        CoreError::new(
            ErrorKind::OutputNotWritable,
            format!("{}: {e}", dir.display()),
        )
    })?;
    let args = download_args(tools, info, dir);
    let mut file: Option<PathBuf> = None;
    {
        let mut on_line = |line: &str| {
            if let Some(p) = parse_progress(line) {
                on_progress(p);
            } else if let Some(f) = line.strip_prefix(FILE_PREFIX) {
                file = Some(PathBuf::from(f.trim()));
            }
        };
        run(&tools.ytdlp, &args, cancel, StdoutMode::Lines(&mut on_line)).map_err(classify)?;
    }
    let stem = download_stem(info);
    // Trust only a file in our folder whose name starts with our stem.
    let found = file.filter(|f| f.is_file()).or_else(|| {
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(&format!("{stem}.")))
                    .unwrap_or(false)
                    && !p.to_string_lossy().ends_with(".part")
            })
    });
    match found {
        Some(f) if f.parent() == Some(dir) || f.starts_with(dir) => Ok(f),
        Some(f) => Err(yt_err(format!(
            "unexpected download location {}",
            f.display()
        ))),
        None => Err(yt_err("yt-dlp finished but the file was not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> YtTools {
        YtTools {
            ytdlp: PathBuf::from("/app/yt-dlp"),
            deno: Some(PathBuf::from("/app/deno")),
        }
    }

    #[test]
    fn accepts_common_youtube_links() {
        for (link, id) in [
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            (
                "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42s",
                "dQw4w9WgXcQ",
            ),
            ("  https://youtu.be/dQw4w9WgXcQ?si=abc  ", "dQw4w9WgXcQ"),
            (
                "https://m.youtube.com/watch?v=dQw4w9WgXcQ#t=1",
                "dQw4w9WgXcQ",
            ),
            (
                "https://www.youtube.com/live/dQw4w9WgXcQ?si=x",
                "dQw4w9WgXcQ",
            ),
            ("https://www.youtube.com/shorts/dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            ("http://www.youtube.com/watch?v=a-b_c1234XY", "a-b_c1234XY"),
        ] {
            assert_eq!(parse_video_id(link).unwrap(), id, "{link}");
        }
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "dQw4w9WgXcQ",
            "ftp://youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/playlist?list=PL123",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/watch?v=--exec=rm-rf",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ%20--exec",
            "file:///etc/passwd",
            "-o/tmp/x https://youtu.be/dQw4w9WgXcQ",
        ] {
            assert!(parse_video_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn args_are_safe() {
        let a: Vec<String> = metadata_args(&tools(), "dQw4w9WgXcQ")
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(a[0], "--ignore-config");
        assert!(a.contains(&"--no-playlist".to_string()));
        let dd = a.iter().position(|s| s == "--").unwrap();
        assert_eq!(a[dd + 1], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(dd + 2, a.len(), "URL must be the last argument after --");
        let js = a.iter().position(|s| s == "--js-runtimes").unwrap();
        assert_eq!(a[js + 1], "deno:/app/deno");

        let info = VideoInfo {
            id: "dQw4w9WgXcQ".into(),
            title: "Co mi wolno? | Adam Szumorek / ../../x".into(),
            upload_date: Some("20261011".into()),
            uploader: None,
            channel: None,
            duration: None,
        };
        let d: Vec<String> = download_args(
            &tools(),
            &info,
            Path::new("/home/u/Pobrane/SCh Tomy YouTube"),
        )
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
        let o = d.iter().position(|s| s == "-o").unwrap();
        assert_eq!(
            d[o + 1],
            format!(
                "/home/u/Pobrane/SCh Tomy YouTube{}2026-10-11-co-mi-wolno-adam-szumorek-x-yt-dQw4w9WgXcQ.%(ext)s",
                std::path::MAIN_SEPARATOR
            )
        );
        assert_eq!(
            d.last().unwrap(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert!(!d.iter().any(|s| s == "--exec" || s == "-x"));
    }

    #[test]
    fn metadata_and_progress_parsing() {
        let out = "WARNING: x\n{\"id\": \"dQw4w9WgXcQ\", \"title\": \"Co mi wolno?\", \"upload_date\": \"20261011\", \"uploader\": \"SCh Tomy\", \"channel\": \"SCh Tomy\", \"duration\": 3600.5}\n";
        let info = parse_metadata(out).unwrap();
        assert_eq!(info.title, "Co mi wolno?");
        assert_eq!(info.dotted_date().as_deref(), Some("2026.10.11"));
        assert!(parse_metadata("nothing").is_err());
        assert_eq!(parse_progress("SCHTOMY-PROGRESS 50 200 NA"), Some(0.25));
        assert_eq!(parse_progress("SCHTOMY-PROGRESS 50 NA 100"), Some(0.5));
        assert_eq!(parse_progress("SCHTOMY-PROGRESS 50 NA NA"), None);
        assert_eq!(parse_progress("[download] 5%"), None);
    }

    #[test]
    fn link_lists() {
        let text = "\u{feff}# Kazania Piotrków\n\nhttps://youtu.be/dQw4w9WgXcQ\n  https://www.youtube.com/watch?v=a-b_c1234XY&t=1  \nKazanie 3: https://www.youtube.com/live/abcdefghijk?si=1\nhttps://youtu.be/dQw4w9WgXcQ?si=x\nto nie link\nhttps://evil.com/watch?v=dQw4w9WgXcQ\n";
        let l = parse_link_list(text);
        assert_eq!(l.ids, vec!["dQw4w9WgXcQ", "a-b_c1234XY", "abcdefghijk"]);
        assert_eq!(l.duplicates, 1);
        assert_eq!(
            l.invalid,
            vec![
                (7, "to nie link".to_string()),
                (8, "https://evil.com/watch?v=dQw4w9WgXcQ".to_string())
            ]
        );
        assert_eq!(parse_link_list(""), LinkList::default());
        // CSV-like lines with quotes and separators
        let l = parse_link_list("\"Tytuł\";\"https://youtu.be/dQw4w9WgXcQ\"\r\n");
        assert_eq!(l.ids, vec!["dQw4w9WgXcQ"]);
    }

    #[test]
    fn errors_are_classified() {
        let e = classify(CoreError::new(
            ErrorKind::Processing,
            "ERROR: [youtube] x: Sign in to confirm you're not a bot",
        ));
        assert_eq!(e.kind, ErrorKind::YouTubeBlocked);
        let e = classify(CoreError::new(
            ErrorKind::Processing,
            "ERROR: Private video",
        ));
        assert_eq!(e.kind, ErrorKind::YouTubeUnavailable);
        let e = classify(CoreError::new(ErrorKind::Processing, "HTTP Error 500"));
        assert_eq!(e.kind, ErrorKind::YouTube);
        assert!(classify(CoreError::cancelled()).is_cancelled());
    }
}
