//! Parsers for ffmpeg output: progress, silencedetect, loudnorm, ebur128.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Parses one `-progress pipe:1` line, returning processed seconds.
pub fn parse_progress_line(line: &str) -> Option<f64> {
    let v = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))?;
    // out_time_ms is (despite its name) also in microseconds.
    let us: i64 = v.trim().parse().ok()?;
    if us < 0 {
        return None;
    }
    Some(us as f64 / 1_000_000.0)
}

/// A silence interval reported by `silencedetect`. `end` is `None` when the
/// stream ended in silence and this ffmpeg version printed no final end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceInterval {
    pub start: f64,
    pub end: Option<f64>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

pub fn parse_silencedetect(stderr: &str) -> Vec<SilenceInterval> {
    static START: OnceLock<Regex> = OnceLock::new();
    static END: OnceLock<Regex> = OnceLock::new();
    let start_re = re(&START, r"silence_start:\s*(-?[0-9.]+(?:e-?[0-9]+)?)");
    let end_re = re(&END, r"silence_end:\s*(-?[0-9.]+(?:e-?[0-9]+)?)");
    let mut out: Vec<SilenceInterval> = Vec::new();
    for line in stderr.lines() {
        if let Some(c) = start_re.captures(line) {
            if let Ok(v) = c[1].parse::<f64>() {
                out.push(SilenceInterval {
                    start: v.max(0.0),
                    end: None,
                });
            }
        } else if let Some(c) = end_re.captures(line) {
            if let Ok(v) = c[1].parse::<f64>() {
                if let Some(last) = out.last_mut() {
                    if last.end.is_none() {
                        last.end = Some(v);
                    }
                }
            }
        }
    }
    out
}

/// Trailing silence of a segment of length `duration`: the last interval that
/// lasts until the end (no end reported, or end within `tolerance` of the
/// segment end). Returns the start of that silence.
pub fn trailing_silence_start(
    intervals: &[SilenceInterval],
    duration: f64,
    tolerance: f64,
) -> Option<f64> {
    let last = intervals.last()?;
    let reaches_end = match last.end {
        None => true,
        Some(e) => e >= duration - tolerance,
    };
    if reaches_end && last.start < duration {
        Some(last.start)
    } else {
        None
    }
}

/// Leading silence: an interval starting at the very beginning. Returns its end.
pub fn leading_silence_end(intervals: &[SilenceInterval], tolerance: f64) -> Option<f64> {
    let first = intervals.first()?;
    if first.start <= tolerance {
        first.end
    } else {
        None
    }
}

/// Values measured by `loudnorm` in the first pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoudnormMeasurement {
    pub input_i: f64,
    pub input_tp: f64,
    pub input_lra: f64,
    pub input_thresh: f64,
    pub target_offset: f64,
}

#[derive(Deserialize)]
struct LoudnormJson {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
    #[serde(default)]
    normalization_type: Option<String>,
}

/// Result of the second `loudnorm` pass (only the mode is interesting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoudnormMode {
    Linear,
    Dynamic,
    Unknown,
}

fn last_json_object(stderr: &str) -> Option<&str> {
    let end = stderr.rfind('}')?;
    let start = stderr[..end].rfind('{')?;
    Some(&stderr[start..=end])
}

fn num(s: &str) -> Option<f64> {
    let t = s.trim();
    match t {
        "-inf" => Some(f64::NEG_INFINITY),
        "inf" | "+inf" => Some(f64::INFINITY),
        _ => t.parse().ok(),
    }
}

pub fn parse_loudnorm(stderr: &str) -> Option<(LoudnormMeasurement, LoudnormMode)> {
    let json = last_json_object(stderr)?;
    let j: LoudnormJson = serde_json::from_str(json).ok()?;
    let m = LoudnormMeasurement {
        input_i: num(&j.input_i)?,
        input_tp: num(&j.input_tp)?,
        input_lra: num(&j.input_lra)?,
        input_thresh: num(&j.input_thresh)?,
        target_offset: num(&j.target_offset)?,
    };
    let mode = match j.normalization_type.as_deref() {
        Some("linear") => LoudnormMode::Linear,
        Some("dynamic") => LoudnormMode::Dynamic,
        _ => LoudnormMode::Unknown,
    };
    Some((m, mode))
}

/// Summary printed by the `ebur128` filter at the end of the stream.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ebur128Summary {
    pub integrated: f64,
    pub lra: f64,
    /// True peak in dBTP (when `peak=true` was requested).
    pub true_peak: Option<f64>,
}

pub fn parse_ebur128_summary(stderr: &str) -> Option<Ebur128Summary> {
    static I: OnceLock<Regex> = OnceLock::new();
    static LRA: OnceLock<Regex> = OnceLock::new();
    static PEAK: OnceLock<Regex> = OnceLock::new();
    let idx = stderr.rfind("Summary:")?;
    let summary = &stderr[idx..];
    let i_re = re(&I, r"(?m)^\s*I:\s*(-?[0-9.]+|-inf)\s*LUFS");
    let lra_re = re(&LRA, r"(?m)^\s*LRA:\s*(-?[0-9.]+)\s*LU");
    let peak_re = re(&PEAK, r"(?m)^\s*Peak:\s*(-?[0-9.]+|-inf)\s*dBFS");
    let integrated = num(&i_re.captures(summary)?[1])?;
    let lra = lra_re
        .captures(summary)
        .and_then(|c| num(&c[1]))
        .unwrap_or(0.0);
    let true_peak = summary
        .find("True peak:")
        .and_then(|p| peak_re.captures(&summary[p..]))
        .and_then(|c| num(&c[1]));
    Some(Ebur128Summary {
        integrated,
        lra,
        true_peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=1500000"), Some(1.5));
        assert_eq!(parse_progress_line("out_time_ms=2000000"), Some(2.0));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("out_time_us=-5"), None);
        assert_eq!(parse_progress_line("progress=end"), None);
    }

    #[test]
    fn silencedetect_with_final_end() {
        let log = "\
[silencedetect @ 0x1] silence_start: 12.5
[silencedetect @ 0x1] silence_end: 13.75 | silence_duration: 1.25
[silencedetect @ 0x1] silence_start: 100.02
[silencedetect @ 0x1] silence_end: 115 | silence_duration: 14.98
";
        let v = parse_silencedetect(log);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].start, 12.5);
        assert_eq!(v[0].end, Some(13.75));
        assert_eq!(trailing_silence_start(&v, 115.0, 0.05), Some(100.02));
        // A pause in the middle is never treated as trailing silence.
        assert_eq!(trailing_silence_start(&v[..1], 115.0, 0.05), None);
    }

    #[test]
    fn silencedetect_without_final_end() {
        let log = "[silencedetect @ 0x1] silence_start: 3.2\nsize=N/A time=00:00:10.00\n";
        let v = parse_silencedetect(log);
        assert_eq!(
            v,
            vec![SilenceInterval {
                start: 3.2,
                end: None
            }]
        );
        assert_eq!(trailing_silence_start(&v, 10.0, 0.05), Some(3.2));
    }

    #[test]
    fn silencedetect_negative_start_clamped_and_leading() {
        let log = "silence_start: -0.0213\nsilence_end: 2.5 | silence_duration: 2.52\n";
        let v = parse_silencedetect(log);
        assert_eq!(v[0].start, 0.0);
        assert_eq!(leading_silence_end(&v, 0.05), Some(2.5));
        assert_eq!(trailing_silence_start(&v, 60.0, 0.05), None);
    }

    #[test]
    fn loudnorm_json() {
        let log = r#"[Parsed_loudnorm_0 @ 0x5]
{
	"input_i" : "-27.61",
	"input_tp" : "-4.47",
	"input_lra" : "18.06",
	"input_thresh" : "-39.20",
	"output_i" : "-16.58",
	"output_tp" : "-1.50",
	"output_lra" : "14.78",
	"output_thresh" : "-27.71",
	"normalization_type" : "dynamic",
	"target_offset" : "0.58"
}
"#;
        let (m, mode) = parse_loudnorm(log).unwrap();
        assert_eq!(m.input_i, -27.61);
        assert_eq!(m.input_tp, -4.47);
        assert_eq!(m.input_lra, 18.06);
        assert_eq!(m.input_thresh, -39.20);
        assert_eq!(m.target_offset, 0.58);
        assert_eq!(mode, LoudnormMode::Dynamic);
    }

    #[test]
    fn loudnorm_silent_input() {
        let log = r#"{ "input_i" : "-inf", "input_tp" : "-inf", "input_lra" : "0.00",
 "input_thresh" : "-inf", "target_offset" : "inf" }"#;
        let (m, _) = parse_loudnorm(log).unwrap();
        assert!(m.input_i.is_infinite());
    }

    #[test]
    fn ebur128_summary() {
        let log = "\
[Parsed_ebur128_0 @ 0x55] t: 9.9 TARGET:-23 LUFS M: -16.0 S: -16.1 I: -16.0 LUFS LRA: 0.0 LU FTPK: -2.1 dBFS TPK: -1.9 dBFS
[Parsed_ebur128_0 @ 0x55] Summary:

  Integrated loudness:
    I:         -16.1 LUFS
    Threshold: -26.2 LUFS

  Loudness range:
    LRA:         3.4 LU
    Threshold: -36.1 LUFS
    LRA low:   -18.0 LUFS
    LRA high:  -14.6 LUFS

  True peak:
    Peak:       -1.3 dBFS
";
        let s = parse_ebur128_summary(log).unwrap();
        assert_eq!(s.integrated, -16.1);
        assert_eq!(s.lra, 3.4);
        assert_eq!(s.true_peak, Some(-1.3));
    }
}
