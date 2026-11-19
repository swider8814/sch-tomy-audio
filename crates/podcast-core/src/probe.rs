//! ffprobe wrapper.

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{run, Args, CancelToken, StdoutMode, Tools};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeInfo {
    /// Duration in seconds (container duration, or audio stream duration).
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub codec: String,
    pub has_video: bool,
}

#[derive(Deserialize)]
struct Json {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    duration: Option<String>,
    disposition: Option<Disposition>,
}

#[derive(Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

pub fn parse_probe_json(json: &[u8]) -> Result<ProbeInfo> {
    let j: Json = serde_json::from_slice(json)
        .map_err(|e| CoreError::new(ErrorKind::InputUnreadable, format!("ffprobe JSON: {e}")))?;
    let audio = j
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .ok_or_else(|| CoreError::new(ErrorKind::NoAudio, "no audio stream"))?;
    let has_video = j.streams.iter().any(|s| {
        s.codec_type.as_deref() == Some("video")
            && s.disposition.as_ref().map(|d| d.attached_pic).unwrap_or(0) == 0
    });
    let duration = j
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| audio.duration.as_deref().and_then(|d| d.parse().ok()))
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or_else(|| CoreError::new(ErrorKind::InputUnreadable, "unknown duration"))?;
    Ok(ProbeInfo {
        duration,
        sample_rate: audio
            .sample_rate
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        channels: audio.channels.unwrap_or(0),
        codec: audio.codec_name.clone().unwrap_or_default(),
        has_video,
    })
}

pub fn probe(tools: &Tools, path: &Path, cancel: &CancelToken) -> Result<ProbeInfo> {
    if !path.is_file() {
        return Err(CoreError::new(
            ErrorKind::InputUnreadable,
            format!("not a file: {}", path.display()),
        ));
    }
    let args = Args::new()
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .args(["-protocol_whitelist", "file"])
        .arg(crate::ffmpeg::input_arg(path))
        .into_vec();
    let out = run(&tools.ffprobe, &args, cancel, StdoutMode::Capture).map_err(|e| {
        if e.kind == ErrorKind::Processing {
            CoreError::new(ErrorKind::InputUnreadable, e.details)
        } else {
            e
        }
    })?;
    parse_probe_json(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_audio_and_video() {
        let j = br#"{"streams":[
            {"codec_type":"video","codec_name":"h264"},
            {"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2,"duration":"61.2"}],
            "format":{"duration":"61.25"}}"#;
        let p = parse_probe_json(j).unwrap();
        assert_eq!(p.duration, 61.25);
        assert_eq!(p.sample_rate, 48000);
        assert_eq!(p.channels, 2);
        assert!(p.has_video);
    }

    #[test]
    fn cover_art_is_not_video() {
        let j = br#"{"streams":[
            {"codec_type":"audio","codec_name":"mp3","sample_rate":"44100","channels":2},
            {"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}],
            "format":{"duration":"10.0"}}"#;
        assert!(!parse_probe_json(j).unwrap().has_video);
    }

    #[test]
    fn no_audio() {
        let j = br#"{"streams":[{"codec_type":"video"}],"format":{"duration":"3"}}"#;
        assert_eq!(parse_probe_json(j).unwrap_err().kind, ErrorKind::NoAudio);
    }
}
