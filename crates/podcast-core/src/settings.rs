//! Persisted user settings. Unknown or missing fields fall back to defaults,
//! out-of-range values are clamped on load.

use crate::error::{CoreError, ErrorKind, Result};
use crate::filters::{LoudnessTarget, OutputFormat};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SilenceSettings {
    /// Detect and trim silence at the end of the recording.
    pub trim_trailing: bool,
    /// Trim silence at the start (after the start marker). Off by default.
    pub trim_leading: bool,
    /// Level below which audio counts as silence, dBFS.
    pub threshold_db: f64,
    /// Minimum silence length, seconds.
    pub min_duration: f64,
}

impl Default for SilenceSettings {
    fn default() -> Self {
        Self {
            trim_trailing: true,
            trim_leading: false,
            threshold_db: -50.0,
            min_duration: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub intro: Option<PathBuf>,
    pub outro: Option<PathBuf>,
    pub cover: Option<PathBuf>,
    /// `None` = next to the original file.
    pub output_dir: Option<PathBuf>,
    /// Where audio downloaded from YouTube is saved; `None` = the default
    /// "Downloads/SCh Tomy YouTube".
    pub youtube_dir: Option<PathBuf>,
    pub target_lufs: f64,
    pub target_tp: f64,
    pub target_lra: f64,
    pub bitrate_kbps: u32,
    pub channels: u8,
    pub silence: SilenceSettings,
    pub gap_after_intro: f64,
    pub gap_before_outro: f64,
    pub crossfade_ms: u32,
    /// Teaser match confidence (0..1) above which the start marker is set.
    pub pattern_threshold: f64,
    /// How much of the beginning is searched for teasers / speech, seconds.
    pub search_window_s: f64,
    /// Number of files processed at the same time.
    pub concurrency: u8,
    pub artist: String,
    pub album: String,
    /// Ask GitHub for a newer release when the app starts.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            intro: None,
            outro: None,
            cover: None,
            output_dir: None,
            youtube_dir: None,
            target_lufs: -16.0,
            target_tp: -1.0,
            target_lra: 11.0,
            bitrate_kbps: 192,
            channels: 2,
            silence: SilenceSettings::default(),
            gap_after_intro: 1.0,
            gap_before_outro: 1.0,
            crossfade_ms: 0,
            pattern_threshold: 0.6,
            search_window_s: 300.0,
            concurrency: 2,
            artist: crate::tags::DEFAULT_ARTIST.into(),
            album: crate::tags::DEFAULT_ALBUM.into(),
            check_updates: true,
        }
    }
}

/// Upper limit of the teaser/VAD search window, seconds.
pub const MAX_SEARCH_WINDOW_S: f64 = 900.0;

pub const BITRATES: [u32; 6] = [96, 128, 160, 192, 256, 320];

fn clamp_finite(v: f64, lo: f64, hi: f64, default: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        default
    }
}

impl Settings {
    /// Clamps every value into its allowed range.
    pub fn sanitized(mut self) -> Self {
        let d = Settings::default();
        self.target_lufs = clamp_finite(self.target_lufs, -36.0, -6.0, d.target_lufs);
        self.target_tp = clamp_finite(self.target_tp, -9.0, 0.0, d.target_tp);
        self.target_lra = clamp_finite(self.target_lra, 1.0, 50.0, d.target_lra);
        if !BITRATES.contains(&self.bitrate_kbps) {
            self.bitrate_kbps = d.bitrate_kbps;
        }
        if self.channels != 1 {
            self.channels = 2;
        }
        self.silence.threshold_db = clamp_finite(self.silence.threshold_db, -90.0, -20.0, -50.0);
        self.silence.min_duration = clamp_finite(self.silence.min_duration, 0.1, 30.0, 1.0);
        self.gap_after_intro = clamp_finite(self.gap_after_intro, 0.0, 3.0, 1.0);
        self.gap_before_outro = clamp_finite(self.gap_before_outro, 0.0, 3.0, 1.0);
        self.crossfade_ms = self.crossfade_ms.min(5000);
        self.pattern_threshold =
            clamp_finite(self.pattern_threshold, 0.2, 0.99, d.pattern_threshold);
        self.search_window_s = clamp_finite(
            self.search_window_s,
            60.0,
            MAX_SEARCH_WINDOW_S,
            d.search_window_s,
        );
        self.concurrency = self.concurrency.clamp(1, 4);
        if self.artist.trim().is_empty() {
            self.artist = d.artist;
        }
        if self.album.trim().is_empty() {
            self.album = d.album;
        }
        self
    }

    pub fn loudness(&self) -> LoudnessTarget {
        LoudnessTarget {
            i: self.target_lufs,
            tp: self.target_tp,
            lra: self.target_lra,
        }
    }

    pub fn output_format(&self) -> OutputFormat {
        OutputFormat {
            sample_rate: 44_100,
            channels: self.channels,
            bitrate_kbps: self.bitrate_kbps,
        }
    }

    /// Loads settings; a missing file gives defaults, a broken file gives
    /// defaults too (and the error, so the UI can mention it).
    pub fn load(path: &Path) -> (Settings, Option<CoreError>) {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(s) => (s.sanitized(), None),
                Err(e) => (
                    Settings::default(),
                    Some(CoreError::new(
                        ErrorKind::Settings,
                        format!("{}: {e}", path.display()),
                    )),
                ),
            },
            Err(_) => (Settings::default(), None),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| CoreError::new(ErrorKind::Settings, e.to_string()))?;
        write_atomic(path, &json).map_err(|e| CoreError::new(ErrorKind::Settings, e.to_string()))
    }
}

/// Writes a file via a temporary sibling and rename, so a crash never leaves
/// a half-written settings file.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.target_lufs, -16.0);
        assert_eq!(s.target_tp, -1.0);
        assert_eq!(s.bitrate_kbps, 192);
        assert_eq!(s.channels, 2);
        assert_eq!(s.gap_after_intro, 1.0);
        assert_eq!(s.gap_before_outro, 1.0);
        assert_eq!(s.crossfade_ms, 0);
        assert!(s.silence.trim_trailing);
        assert!(!s.silence.trim_leading);
        assert!(s.check_updates);
    }

    #[test]
    fn roundtrip_with_polish_paths() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Ustawienia ąę").join("settings.json");
        let s = Settings {
            intro: Some(PathBuf::from("/Users/łucja/Muzyka/intro zespołu.mp3")),
            gap_before_outro: 2.5,
            youtube_dir: Some(PathBuf::from("C:/Users/Łucja/Kazania z YouTube")),
            ..Default::default()
        };
        s.save(&p).unwrap();
        let (loaded, err) = Settings::load(&p);
        assert!(err.is_none());
        assert_eq!(loaded, s);
    }

    #[test]
    fn partial_and_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        std::fs::write(
            &p,
            br#"{"gap_after_intro": 10, "bitrate_kbps": 191, "channels": 7, "unknown": 1}"#,
        )
        .unwrap();
        let (s, err) = Settings::load(&p);
        assert!(err.is_none());
        assert_eq!(s.gap_after_intro, 3.0);
        assert_eq!(s.bitrate_kbps, 192);
        assert_eq!(s.channels, 2);
        assert_eq!(s.target_lufs, -16.0);
        // files from older versions have no `check_updates`: on by default
        assert!(s.check_updates);
    }

    /// `write_atomic` must replace an existing file on every platform
    /// (`std::fs::rename` replaces the destination on Windows too); the
    /// settings, patterns and the job list are saved this way repeatedly.
    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Kolejka ąę").join("queue.json");
        for i in 0..3 {
            let data = format!("[{i}]");
            write_atomic(&p, data.as_bytes()).unwrap();
            assert_eq!(std::fs::read_to_string(&p).unwrap(), data);
        }
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        std::fs::write(&p, b"{not json").unwrap();
        let (s, err) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(err.is_some());
    }
}
