//! Processing of one recording, in this order:
//!
//! 1. probe the recording (and intro/outro/cover),
//! 2. analyse silence in the kept segment `[start, end]` (trailing silence
//!    only when it lasts until the end; optional leading silence),
//! 3. plan: intro + generated gap + recording (+ generated pad) + outro,
//! 4. loudnorm pass 1 (measure the whole rendered graph),
//! 5. loudnorm pass 2 with measured values + MP3 CBR encode into a temporary
//!    file, ID3 tags, rename to a fresh `<slug>-podcast.mp3`,
//! 6. verify the result with ebur128 (LUFS, true peak) and ffprobe (duration).

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{display_command, run, CancelToken, StdoutMode, Tools};
use crate::filters::{self, RenderPlan};
use crate::naming;
use crate::parse::{self, Ebur128Summary, LoudnormMeasurement};
use crate::probe::{probe, ProbeInfo};
use crate::settings::Settings;
use crate::tags::{self, EpisodeTags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Tolerance when deciding whether a silence reaches the end of the segment.
const END_TOLERANCE: f64 = 0.25;
/// Deviation from the target loudness that is reported as a warning.
pub const LUFS_WARNING_LU: f64 = 0.5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct JobInput {
    pub recording: PathBuf,
    pub cut: Cut,
    pub title: String,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Analysis,
    Measure,
    Encode,
    Verify,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessResult {
    pub output: PathBuf,
    pub loudness: Ebur128Summary,
    pub duration: f64,
    /// Seconds of trailing silence removed; `None` when there was none.
    pub trailing_silence_cut: Option<f64>,
    /// Seconds of leading silence removed (only when enabled).
    pub leading_silence_cut: Option<f64>,
    /// "linear" or "dynamic" (loudnorm mode of pass 2).
    pub loudnorm_mode: String,
    /// Warnings for the user (Polish).
    pub warnings: Vec<String>,
}

/// Result of the silence analysis of the kept segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentPlan {
    pub rec_start: f64,
    pub rec_end: Option<f64>,
    pub pad_after_recording: f64,
    pub trailing_silence_cut: Option<f64>,
    pub leading_silence_cut: Option<f64>,
    /// Length of the kept recording including kept trailing silence.
    pub kept_length: f64,
}

/// Decides how the recording is trimmed and padded, from the silence
/// intervals found in `[start, start + seg_len]` (interval times relative to
/// `start`). Pure function, unit tested.
pub fn plan_segment(
    start: f64,
    end_marker: Option<f64>,
    seg_len: f64,
    intervals: &[parse::SilenceInterval],
    settings: &Settings,
    has_outro: bool,
) -> SegmentPlan {
    let gap = settings.gap_before_outro;
    let mut rec_start = start;
    let mut leading_cut = None;
    let mut seg_end_rel = seg_len;
    let mut trailing_cut = None;
    let mut rec_end = end_marker;
    let mut pad = if has_outro { gap } else { 0.0 };

    if settings.silence.trim_trailing {
        // The decoded length may slightly exceed the progress estimate.
        let true_len = intervals
            .last()
            .and_then(|l| l.end)
            .map_or(seg_len, |e| e.max(seg_len));
        if let Some(ts) = parse::trailing_silence_start(intervals, seg_len, END_TOLERANCE) {
            let silence_len = (true_len - ts).max(0.0);
            let keep = silence_len.min(gap);
            seg_end_rel = ts + keep;
            rec_end = Some(start + seg_end_rel);
            trailing_cut = Some((silence_len - keep).max(0.0));
            pad = if has_outro {
                (gap - keep).max(0.0)
            } else {
                0.0
            };
        } else {
            seg_end_rel = true_len;
        }
    }

    if settings.silence.trim_leading {
        if let Some(le) = parse::leading_silence_end(intervals, 0.05) {
            if le < seg_end_rel - 0.5 {
                rec_start = start + le;
                leading_cut = Some(le);
            }
        }
    }

    SegmentPlan {
        rec_start,
        rec_end,
        pad_after_recording: pad,
        trailing_silence_cut: trailing_cut,
        leading_silence_cut: leading_cut,
        kept_length: (seg_end_rel - (rec_start - start)).max(0.0),
    }
}

/// Validates markers against the file duration; returns (start, end).
pub fn validate_cut(cut: Cut, duration: f64) -> Result<(f64, Option<f64>)> {
    let start = cut.start.unwrap_or(0.0);
    if !start.is_finite() || start < 0.0 || start >= duration - 1.0 {
        return Err(CoreError::new(
            ErrorKind::InvalidCut,
            format!("start {start} outside 0..{duration}"),
        ));
    }
    let end = match cut.end {
        None => None,
        Some(e) if !e.is_finite() || e <= start + 1.0 => {
            return Err(CoreError::new(
                ErrorKind::InvalidCut,
                format!("end {e} not after start {start}"),
            ))
        }
        Some(e) if e >= duration => None,
        Some(e) => Some(e),
    };
    Ok((start, end))
}

struct Progress<'a> {
    cb: &'a mut dyn FnMut(Stage, f64),
}

impl Progress<'_> {
    fn report(&mut self, stage: Stage, within: f64) {
        let (a, b) = match stage {
            Stage::Analysis => (0.0, 0.1),
            Stage::Measure => (0.1, 0.45),
            Stage::Encode => (0.45, 0.9),
            Stage::Verify => (0.9, 1.0),
        };
        (self.cb)(stage, a + (b - a) * within.clamp(0.0, 1.0));
    }
}

fn probe_optional(
    tools: &Tools,
    path: &Option<PathBuf>,
    kind: ErrorKind,
    cancel: &CancelToken,
) -> Result<Option<ProbeInfo>> {
    match path {
        None => Ok(None),
        Some(p) => probe(tools, p, cancel)
            .map(Some)
            .map_err(|e| e.with_kind(kind)),
    }
}

/// Removes temporary files when dropped (error or cancel).
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn process(
    tools: &Tools,
    settings: &Settings,
    job: &JobInput,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(Stage, f64),
    log: &mut String,
) -> Result<ProcessResult> {
    let mut progress = Progress { cb: on_progress };
    progress.report(Stage::Analysis, 0.0);

    let rec_info = probe(tools, &job.recording, cancel)?;
    let intro_info = probe_optional(tools, &settings.intro, ErrorKind::IntroUnreadable, cancel)?;
    let outro_info = probe_optional(tools, &settings.outro, ErrorKind::OutroUnreadable, cancel)?;
    if let Some(c) = &settings.cover {
        tags::read_cover(c)?;
    }
    let (start, end_marker) = validate_cut(job.cut, rec_info.duration)?;
    log.push_str(&format!(
        "Input: {} ({:.2} s, {}, {} Hz, {} ch)\nStart: {start:.2} s, end: {}\n",
        job.recording.display(),
        rec_info.duration,
        rec_info.codec,
        rec_info.sample_rate,
        rec_info.channels,
        end_marker.map_or("end of file".to_string(), |e| format!("{e:.2} s"))
    ));

    // --- 2. silence analysis --------------------------------------------
    let estimated_len = end_marker.unwrap_or(rec_info.duration) - start;
    let mut intervals = Vec::new();
    let mut seg_len = estimated_len;
    if settings.silence.trim_trailing || settings.silence.trim_leading {
        let args = filters::silence_analysis_args(
            &job.recording,
            start,
            end_marker,
            settings.silence.threshold_db,
            settings.silence.min_duration,
        );
        log.push_str(&format!("$ {}\n", display_command(&tools.ffmpeg, &args)));
        let mut last = 0.0f64;
        let out = {
            let mut cb = |t: f64| {
                last = last.max(t);
                progress.report(Stage::Analysis, t / estimated_len.max(1.0));
            };
            run(&tools.ffmpeg, &args, cancel, StdoutMode::Progress(&mut cb))
                .map_err(|e| e.with_kind(ErrorKind::InputUnreadable))?
        };
        intervals = parse::parse_silencedetect(&out.stderr);
        if last > 0.0 {
            seg_len = last;
        }
    }
    let has_outro = outro_info.is_some();
    let seg = plan_segment(start, end_marker, seg_len, &intervals, settings, has_outro);
    log.push_str(&format!(
        "Silence intervals: {intervals:?}\nSegment plan: {seg:?}\n"
    ));
    if seg.kept_length < 0.5 {
        return Err(CoreError::new(
            ErrorKind::InputSilent,
            "nothing left after trimming",
        ));
    }

    let gap_after_intro = if intro_info.is_some() {
        settings.gap_after_intro
    } else {
        0.0
    };
    let intro_len = intro_info
        .as_ref()
        .map_or(0.0, |i| i.duration + gap_after_intro);
    let outro_len = outro_info.as_ref().map_or(0.0, |i| i.duration);
    let rec_len = seg.kept_length + seg.pad_after_recording;
    let mut crossfade = settings.crossfade_ms as f64 / 1000.0;
    if crossfade > 0.0 {
        let mut limit = rec_len / 2.0;
        if intro_info.is_some() {
            limit = limit.min(intro_len / 2.0);
        }
        if outro_info.is_some() {
            limit = limit.min(outro_len / 2.0);
        }
        if intro_info.is_none() && outro_info.is_none() {
            limit = 0.0;
        }
        crossfade = crossfade.min(limit);
    }
    let segments = 1 + intro_info.is_some() as usize + outro_info.is_some() as usize;
    let expected_len = intro_len + rec_len + outro_len - crossfade * (segments - 1) as f64;

    let plan = RenderPlan {
        recording: job.recording.clone(),
        intro: settings.intro.clone(),
        outro: settings.outro.clone(),
        rec_start: seg.rec_start,
        rec_end: seg.rec_end,
        gap_after_intro,
        pad_after_recording: seg.pad_after_recording,
        crossfade,
        format: settings.output_format(),
    };
    let target = settings.loudness();

    // --- 4. loudnorm pass 1 --------------------------------------------
    progress.report(Stage::Measure, 0.0);
    let args = filters::measure_args(&plan, &target);
    log.push_str(&format!("$ {}\n", display_command(&tools.ffmpeg, &args)));
    let out = {
        let mut cb = |t: f64| progress.report(Stage::Measure, t / expected_len.max(1.0));
        run(&tools.ffmpeg, &args, cancel, StdoutMode::Progress(&mut cb))?
    };
    let (measured, _) = parse::parse_loudnorm(&out.stderr).ok_or_else(|| {
        CoreError::new(
            ErrorKind::Processing,
            format!(
                "loudnorm measurement missing\n{}",
                crate::ffmpeg::tail_lines(&out.stderr, 30)
            ),
        )
    })?;
    log.push_str(&format!("Measured: {measured:?}\n"));
    check_measurement(&measured)?;

    // --- 5. encode ------------------------------------------------------
    let out_dir = match &settings.output_dir {
        Some(d) => d.clone(),
        None => job
            .recording
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    if !out_dir.is_dir() {
        return Err(CoreError::new(
            ErrorKind::OutputNotWritable,
            format!("output folder does not exist: {}", out_dir.display()),
        ));
    }
    let final_path =
        naming::reserve_output_path(&out_dir, &naming::output_file_name(&job.recording)).map_err(
            |e| {
                CoreError::new(
                    ErrorKind::OutputNotWritable,
                    format!("{}: {e}", out_dir.display()),
                )
            },
        )?;
    let file_name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = out_dir.join(format!(".{file_name}.part"));
    let mut cleanup = Cleanup(vec![final_path.clone(), tmp_path.clone()]);
    if naming::same_file(&final_path, &job.recording) {
        return Err(CoreError::new(
            ErrorKind::OutputNotWritable,
            "output equals input",
        ));
    }

    progress.report(Stage::Encode, 0.0);
    let args = filters::encode_args(&plan, &target, &measured, &tmp_path);
    log.push_str(&format!("$ {}\n", display_command(&tools.ffmpeg, &args)));
    let out = {
        let mut cb = |t: f64| progress.report(Stage::Encode, t / expected_len.max(1.0));
        run(&tools.ffmpeg, &args, cancel, StdoutMode::Progress(&mut cb))?
    };
    let mode = match parse::parse_loudnorm(&out.stderr) {
        Some((_, parse::LoudnormMode::Linear)) => "linear",
        Some((_, parse::LoudnormMode::Dynamic)) => "dynamic",
        _ => "unknown",
    };
    log.push_str(&format!("loudnorm mode: {mode}\n"));
    cancel.check()?;

    tags::write_tags(
        &tmp_path,
        &EpisodeTags {
            title: job.title.clone(),
            artist: settings.artist.clone(),
            album: settings.album.clone(),
            year: job.year,
            cover: settings.cover.clone(),
        },
    )?;
    cancel.check()?;
    std::fs::rename(&tmp_path, &final_path).map_err(|e| {
        CoreError::new(
            ErrorKind::OutputNotWritable,
            format!("rename to {}: {e}", final_path.display()),
        )
    })?;

    // --- 6. verify ------------------------------------------------------
    progress.report(Stage::Verify, 0.0);
    let (loudness, duration) = measure_file(tools, &final_path, cancel, &mut |t| {
        progress.report(Stage::Verify, t / expected_len.max(1.0))
    })?;
    log.push_str(&format!("Result: {loudness:?}, {duration:.2} s\n"));
    let warnings = loudness_warnings(&loudness, settings);
    progress.report(Stage::Verify, 1.0);
    cleanup.0.clear(); // success: keep the output
    Ok(ProcessResult {
        output: final_path,
        loudness,
        duration,
        trailing_silence_cut: seg.trailing_silence_cut,
        leading_silence_cut: seg.leading_silence_cut,
        loudnorm_mode: mode.to_string(),
        warnings,
    })
}

fn check_measurement(m: &LoudnormMeasurement) -> Result<()> {
    if !m.input_i.is_finite() || m.input_i < -70.0 || !m.input_thresh.is_finite() {
        return Err(CoreError::new(
            ErrorKind::InputSilent,
            format!("input loudness {} LUFS", m.input_i),
        ));
    }
    Ok(())
}

/// ebur128 + ffprobe of a finished file.
pub fn measure_file(
    tools: &Tools,
    file: &Path,
    cancel: &CancelToken,
    on_seconds: &mut dyn FnMut(f64),
) -> Result<(Ebur128Summary, f64)> {
    let args = filters::verify_args(file);
    let out = run(
        &tools.ffmpeg,
        &args,
        cancel,
        StdoutMode::Progress(on_seconds),
    )?;
    let loudness = parse::parse_ebur128_summary(&out.stderr).ok_or_else(|| {
        CoreError::new(
            ErrorKind::Processing,
            format!(
                "ebur128 summary missing\n{}",
                crate::ffmpeg::tail_lines(&out.stderr, 30)
            ),
        )
    })?;
    let info = probe(tools, file, cancel)?;
    Ok((loudness, info.duration))
}

/// Polish warnings when the result misses the targets.
pub fn loudness_warnings(l: &Ebur128Summary, s: &Settings) -> Vec<String> {
    let mut w = Vec::new();
    if (l.integrated - s.target_lufs).abs() > LUFS_WARNING_LU {
        w.push(format!(
            "Głośność {} LUFS odbiega od celu ({} LUFS) o więcej niż {} LU.",
            pl1(l.integrated),
            pl1(s.target_lufs),
            pl1(LUFS_WARNING_LU)
        ));
    }
    if let Some(tp) = l.true_peak {
        if tp > s.target_tp + 0.05 {
            w.push(format!(
                "Szczyt (true peak) {} dBTP jest powyżej celu ({} dBTP).",
                pl1(tp),
                pl1(s.target_tp)
            ));
        }
    }
    w
}

/// One decimal with a Polish decimal comma.
pub fn pl1(v: f64) -> String {
    format!("{v:.1}").replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::SilenceInterval;

    fn s() -> Settings {
        Settings::default()
    }

    fn iv(start: f64, end: Option<f64>) -> SilenceInterval {
        SilenceInterval { start, end }
    }

    #[test]
    fn trailing_long_silence_is_trimmed_to_gap() {
        // segment 100 s from start 20; silence 85..100
        let p = plan_segment(
            20.0,
            None,
            100.0,
            &[iv(30.0, Some(31.5)), iv(85.0, Some(100.0))],
            &s(),
            true,
        );
        assert_eq!(p.rec_start, 20.0);
        assert_eq!(p.rec_end, Some(20.0 + 86.0));
        assert_eq!(p.pad_after_recording, 0.0);
        assert_eq!(p.trailing_silence_cut, Some(14.0));
        assert_eq!(p.kept_length, 86.0);
    }

    #[test]
    fn short_trailing_silence_is_padded_to_gap() {
        let mut st = s();
        st.silence.min_duration = 0.3;
        let p = plan_segment(0.0, None, 50.0, &[iv(49.6, None)], &st, true);
        assert!((p.pad_after_recording - 0.6).abs() < 1e-9);
        assert_eq!(p.trailing_silence_cut, Some(0.0));
        assert_eq!(p.rec_end, Some(50.0));
    }

    #[test]
    fn no_trailing_silence_adds_generated_gap() {
        let p = plan_segment(0.0, None, 50.0, &[iv(10.0, Some(12.0))], &s(), true);
        assert_eq!(p.pad_after_recording, 1.0);
        assert_eq!(p.trailing_silence_cut, None);
        assert_eq!(p.rec_end, None);
        assert_eq!(p.kept_length, 50.0);
    }

    #[test]
    fn without_outro_nothing_is_generated() {
        let p = plan_segment(0.0, None, 50.0, &[], &s(), false);
        assert_eq!(p.pad_after_recording, 0.0);
        let p = plan_segment(0.0, None, 50.0, &[iv(40.0, None)], &s(), false);
        assert_eq!(p.pad_after_recording, 0.0);
        assert_eq!(p.rec_end, Some(41.0));
        assert_eq!(p.trailing_silence_cut, Some(9.0));
    }

    #[test]
    fn trailing_disabled_keeps_everything() {
        let mut st = s();
        st.silence.trim_trailing = false;
        let p = plan_segment(5.0, Some(60.0), 55.0, &[iv(40.0, None)], &st, true);
        assert_eq!(p.rec_end, Some(60.0));
        assert_eq!(p.pad_after_recording, 1.0);
        assert_eq!(p.trailing_silence_cut, None);
    }

    #[test]
    fn gap_setting_is_respected() {
        let mut st = s();
        st.gap_before_outro = 2.5;
        let p = plan_segment(0.0, None, 30.0, &[iv(20.0, Some(30.0))], &st, true);
        assert_eq!(p.rec_end, Some(22.5));
        assert_eq!(p.pad_after_recording, 0.0);
        st.gap_before_outro = 0.0;
        let p = plan_segment(0.0, None, 30.0, &[iv(20.0, Some(30.0))], &st, true);
        assert_eq!(p.rec_end, Some(20.0));
        assert_eq!(p.trailing_silence_cut, Some(10.0));
    }

    #[test]
    fn leading_silence_only_when_enabled() {
        let ivs = [iv(0.0, Some(2.0)), iv(20.0, Some(21.0))];
        let p = plan_segment(10.0, None, 40.0, &ivs, &s(), true);
        assert_eq!(p.rec_start, 10.0);
        let mut st = s();
        st.silence.trim_leading = true;
        let p = plan_segment(10.0, None, 40.0, &ivs, &st, true);
        assert_eq!(p.rec_start, 12.0);
        assert_eq!(p.leading_silence_cut, Some(2.0));
        assert_eq!(p.kept_length, 38.0);
    }

    #[test]
    fn cut_validation() {
        assert_eq!(validate_cut(Cut::default(), 100.0).unwrap(), (0.0, None));
        assert_eq!(
            validate_cut(
                Cut {
                    start: Some(10.0),
                    end: Some(90.0)
                },
                100.0
            )
            .unwrap(),
            (10.0, Some(90.0))
        );
        assert_eq!(
            validate_cut(
                Cut {
                    start: None,
                    end: Some(150.0)
                },
                100.0
            )
            .unwrap(),
            (0.0, None)
        );
        assert!(validate_cut(
            Cut {
                start: Some(99.5),
                end: None
            },
            100.0
        )
        .is_err());
        assert!(validate_cut(
            Cut {
                start: Some(50.0),
                end: Some(40.0)
            },
            100.0
        )
        .is_err());
        assert!(validate_cut(
            Cut {
                start: Some(-1.0),
                end: None
            },
            100.0
        )
        .is_err());
    }

    #[test]
    fn warnings() {
        let st = s();
        let ok = Ebur128Summary {
            integrated: -16.3,
            lra: 5.0,
            true_peak: Some(-1.2),
        };
        assert!(loudness_warnings(&ok, &st).is_empty());
        let bad = Ebur128Summary {
            integrated: -15.2,
            lra: 5.0,
            true_peak: Some(-0.5),
        };
        let w = loudness_warnings(&bad, &st);
        assert_eq!(w.len(), 2);
        assert!(w[0].contains("-15,2 LUFS"));
    }
}
