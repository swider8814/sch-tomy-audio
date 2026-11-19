//! Pure builders of ffmpeg argument lists and filter graphs.
//!
//! No file paths are ever put inside a filter graph (they would need
//! escaping); all files are regular `-i` inputs.

use crate::ffmpeg::Args;
use crate::parse::LoudnormMeasurement;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Headroom subtracted from the user's true peak target before `loudnorm`,
/// because MP3 encoding adds inter-sample overshoot.
pub const MP3_TP_HEADROOM: f64 = 0.5;
/// Upper bound of `loudnorm`'s LRA option.
pub const LOUDNORM_MAX_LRA: f64 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoudnessTarget {
    /// Integrated loudness, LUFS.
    pub i: f64,
    /// True peak, dBTP.
    pub tp: f64,
    /// Loudness range target, LU.
    pub lra: f64,
}

impl Default for LoudnessTarget {
    fn default() -> Self {
        Self {
            i: -16.0,
            tp: -1.0,
            lra: 11.0,
        }
    }
}

impl LoudnessTarget {
    /// True peak target handed to loudnorm (with MP3 headroom).
    pub fn loudnorm_tp(&self) -> f64 {
        (self.tp - MP3_TP_HEADROOM).max(-9.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OutputFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_kbps: u32,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            channels: 2,
            bitrate_kbps: 192,
        }
    }
}

impl OutputFormat {
    fn layout(&self) -> &'static str {
        if self.channels == 1 {
            "mono"
        } else {
            "stereo"
        }
    }
}

/// Everything needed to render one episode.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    pub recording: PathBuf,
    pub intro: Option<PathBuf>,
    pub outro: Option<PathBuf>,
    /// Kept part of the recording, seconds on the recording timeline.
    pub rec_start: f64,
    /// `None` = until the end of the file.
    pub rec_end: Option<f64>,
    /// Generated silence after the intro (0 when there is no intro).
    pub gap_after_intro: f64,
    /// Generated silence appended to the recording (before the outro).
    pub pad_after_recording: f64,
    /// Crossfade between segments, seconds (0 = plain concatenation).
    pub crossfade: f64,
    /// Clean up the recorded speech ([`VOICE_CHAIN`]); intro and outro are
    /// never touched.
    pub enhance_voice: bool,
    pub format: OutputFormat,
}

/// Optional speech clean-up applied to the recording only: a 4th-order
/// high-pass at 80 Hz (two 2-pole stages, 24 dB/octave) removes rumble and
/// 50 Hz mains hum below the lowest male voice fundamentals, then a gentle
/// FFT denoiser lowers steady background noise by up to 12 dB while tracking
/// how the noise floor changes. Mild on purpose, so voices do not sound
/// processed.
pub const VOICE_CHAIN: &str =
    "highpass=f=80:poles=2,highpass=f=80:poles=2,afftdn=nr=12:nf=-50:tn=1";

/// Formats seconds for filter options (locale independent, no exponent).
pub fn secs(v: f64) -> String {
    let s = format!("{:.6}", v.max(0.0));
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

fn db(v: f64) -> String {
    format!("{v:.2}")
}

fn normalize_chain(fmt: &OutputFormat) -> String {
    format!(
        "aresample={sr},aformat=sample_fmts=fltp:sample_rates={sr}:channel_layouts={layout}",
        sr = fmt.sample_rate,
        layout = fmt.layout()
    )
}

/// Input indices of the plan's files in the ffmpeg command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputIndices {
    pub recording: usize,
    pub intro: Option<usize>,
    pub outro: Option<usize>,
}

/// Adds the `-i` inputs of a plan (recording first) and returns the indices.
pub fn add_inputs(mut args: Args, plan: &RenderPlan) -> (Args, InputIndices) {
    args = args.input(&plan.recording);
    let mut next = 1;
    let mut idx = InputIndices {
        recording: 0,
        intro: None,
        outro: None,
    };
    if let Some(p) = &plan.intro {
        args = args.input(p);
        idx.intro = Some(next);
        next += 1;
    }
    if let Some(p) = &plan.outro {
        args = args.input(p);
        idx.outro = Some(next);
    }
    (args, idx)
}

/// Builds the graph `intro + gap, recording + pad, outro` ending in `[cat]`.
pub fn build_concat_graph(plan: &RenderPlan, idx: InputIndices) -> String {
    let norm = normalize_chain(&plan.format);
    let mut parts: Vec<String> = Vec::new();
    let mut labels: Vec<&str> = Vec::new();

    if let Some(i) = idx.intro {
        let mut chain = format!("[{i}:a:0]asetpts=PTS-STARTPTS,{norm}");
        if plan.gap_after_intro > 0.0 {
            chain.push_str(&format!(",apad=pad_dur={}", secs(plan.gap_after_intro)));
        }
        chain.push_str("[intro]");
        parts.push(chain);
        labels.push("[intro]");
    }

    let only_recording = idx.intro.is_none() && idx.outro.is_none();
    let rec_label = if only_recording { "[cat]" } else { "[rec]" };
    let trim = match plan.rec_end {
        Some(e) => format!("atrim=start={}:end={}", secs(plan.rec_start), secs(e)),
        None => format!("atrim=start={}", secs(plan.rec_start)),
    };
    let mut rec = format!(
        "[{r}:a:0]asetpts=PTS-STARTPTS,{trim},asetpts=PTS-STARTPTS,{norm}",
        r = idx.recording
    );
    if plan.enhance_voice {
        rec.push(',');
        rec.push_str(VOICE_CHAIN);
    }
    if plan.pad_after_recording > 0.0 {
        rec.push_str(&format!(",apad=pad_dur={}", secs(plan.pad_after_recording)));
    }
    rec.push_str(rec_label);
    parts.push(rec);
    labels.push("[rec]");

    if let Some(o) = idx.outro {
        parts.push(format!("[{o}:a:0]asetpts=PTS-STARTPTS,{norm}[outro]"));
        labels.push("[outro]");
    }

    if !only_recording {
        if plan.crossfade > 0.0 {
            let d = secs(plan.crossfade);
            let mut current = labels[0].to_string();
            for (n, next) in labels.iter().enumerate().skip(1) {
                let out = if n == labels.len() - 1 {
                    "[cat]".to_string()
                } else {
                    format!("[x{n}]")
                };
                parts.push(format!(
                    "{current}{next}acrossfade=d={d}:c1=tri:c2=tri{out}"
                ));
                current = out;
            }
        } else {
            parts.push(format!(
                "{}concat=n={}:v=0:a=1[cat]",
                labels.concat(),
                labels.len()
            ));
        }
    }
    parts.join(";")
}

pub fn loudnorm_measure_filter(t: &LoudnessTarget) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={}:print_format=json",
        db(t.i),
        db(t.loudnorm_tp()),
        db(t.lra.clamp(1.0, LOUDNORM_MAX_LRA))
    )
}

/// Second pass. The LRA target is never below the measured LRA, so loudnorm
/// does not compress the dynamic range; it stays linear unless the true peak
/// limit forces its dynamic mode.
pub fn loudnorm_apply_filter(t: &LoudnessTarget, m: &LoudnormMeasurement) -> String {
    let lra = t.lra.max(m.input_lra + 0.1).clamp(1.0, LOUDNORM_MAX_LRA);
    format!(
        "loudnorm=I={}:TP={}:LRA={}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=json",
        db(t.i),
        db(t.loudnorm_tp()),
        db(lra),
        db(m.input_i),
        db(m.input_tp),
        db(m.input_lra),
        db(m.input_thresh),
        db(m.target_offset)
    )
}

fn common() -> Args {
    Args::new().args(["-hide_banner", "-nostdin", "-nostats", "-loglevel", "info"])
}

/// Pass 1: render the whole graph and measure it with loudnorm.
pub fn measure_args(plan: &RenderPlan, target: &LoudnessTarget) -> Vec<OsString> {
    let (args, idx) = add_inputs(common(), plan);
    let graph = format!(
        "{};[cat]{}[out]",
        build_concat_graph(plan, idx),
        loudnorm_measure_filter(target)
    );
    args.args(["-filter_complex", &graph])
        .args(["-map", "[out]", "-progress", "pipe:1", "-f", "null", "-"])
        .into_vec()
}

/// Pass 2: apply loudnorm with the measured values and encode MP3 CBR.
pub fn encode_args(
    plan: &RenderPlan,
    target: &LoudnessTarget,
    m: &LoudnormMeasurement,
    output: &Path,
) -> Vec<OsString> {
    let (args, idx) = add_inputs(common(), plan);
    let graph = format!(
        "{};[cat]{},{}[out]",
        build_concat_graph(plan, idx),
        loudnorm_apply_filter(target, m),
        normalize_chain(&plan.format)
    );
    args.args(["-filter_complex", &graph])
        .args([
            "-map",
            "[out]",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
        ])
        .args(["-c:a", "libmp3lame"])
        .args(["-b:a", &format!("{}k", plan.format.bitrate_kbps)])
        .args(["-ar", &plan.format.sample_rate.to_string()])
        .args(["-ac", &plan.format.channels.to_string()])
        .args([
            "-id3v2_version",
            "0",
            "-write_id3v1",
            "0",
            "-write_xing",
            "1",
        ])
        .args(["-progress", "pipe:1", "-f", "mp3", "-y"])
        .arg(crate::ffmpeg::output_arg(output))
        .into_vec()
}

/// Silence analysis of the recording segment `[start, end]`. Times reported by
/// silencedetect are relative to `start`.
pub fn silence_analysis_args(
    recording: &Path,
    start: f64,
    end: Option<f64>,
    threshold_db: f64,
    min_duration: f64,
) -> Vec<OsString> {
    let trim = match end {
        Some(e) => format!("atrim=start={}:end={}", secs(start), secs(e)),
        None => format!("atrim=start={}", secs(start)),
    };
    let filter = format!(
        "asetpts=PTS-STARTPTS,{trim},asetpts=PTS-STARTPTS,silencedetect=noise={}dB:d={}",
        db(threshold_db),
        secs(min_duration.max(0.01))
    );
    common()
        .input(recording)
        .args(["-vn", "-sn", "-dn", "-map", "0:a:0", "-af", &filter])
        .args(["-progress", "pipe:1", "-f", "null", "-"])
        .into_vec()
}

/// Loudness verification of a finished file.
pub fn verify_args(file: &Path) -> Vec<OsString> {
    common()
        .input(file)
        .args([
            "-vn",
            "-map",
            "0:a:0",
            "-af",
            "ebur128=peak=true:framelog=quiet",
        ])
        .args(["-progress", "pipe:1", "-f", "null", "-"])
        .into_vec()
}

/// Decodes `[start, start + duration]` to raw mono little-endian samples
/// (`f32le` or `s16le`) at `sample_rate` on stdout.
pub fn decode_pcm_args(
    file: &Path,
    start: f64,
    duration: Option<f64>,
    sample_rate: u32,
    sample_format: &str,
) -> Vec<OsString> {
    let mut a = common();
    if start > 0.0 {
        a = a.args(["-ss", &secs(start)]);
    }
    if let Some(d) = duration {
        a = a.args(["-t", &secs(d)]);
    }
    a.input(file)
        .args(["-vn", "-sn", "-dn", "-map", "0:a:0", "-ac", "1"])
        .args(["-ar", &sample_rate.to_string()])
        .args(["-f", sample_format, "-"])
        .into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(intro: bool, outro: bool) -> RenderPlan {
        RenderPlan {
            recording: "/x/nagranie ąę.mp3".into(),
            intro: intro.then(|| "/x/intro.mp3".into()),
            outro: outro.then(|| "/x/outro.mp3".into()),
            rec_start: 42.5,
            rec_end: Some(3600.25),
            gap_after_intro: 1.0,
            pad_after_recording: 1.0,
            crossfade: 0.0,
            enhance_voice: false,
            format: OutputFormat::default(),
        }
    }

    #[test]
    fn voice_chain_only_on_the_recording_when_enabled() {
        let idx = InputIndices {
            intro: Some(0),
            recording: 1,
            outro: Some(2),
        };
        let off = build_concat_graph(&plan(true, true), idx);
        assert!(
            !off.contains("afftdn") && !off.contains("highpass"),
            "{off}"
        );
        let mut p = plan(true, true);
        p.enhance_voice = true;
        let on = build_concat_graph(&p, idx);
        let rec = on.split(';').find(|c| c.starts_with("[1:a:0]")).unwrap();
        assert!(rec.contains(VOICE_CHAIN), "{rec}");
        // before the generated pause, so the pause stays digital silence
        assert!(
            rec.find(VOICE_CHAIN).unwrap() < rec.find("apad").unwrap(),
            "{rec}"
        );
        for c in on.split(';').filter(|c| !c.starts_with("[1:a:0]")) {
            assert!(!c.contains("afftdn"), "{c}");
        }
    }

    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn secs_format() {
        assert_eq!(secs(1.0), "1");
        assert_eq!(secs(0.0), "0");
        assert_eq!(secs(42.5), "42.5");
        assert_eq!(secs(1.0 / 3.0), "0.333333");
        assert_eq!(secs(-3.0), "0");
    }

    #[test]
    fn graph_full_order() {
        let p = plan(true, true);
        let (_, idx) = add_inputs(Args::new(), &p);
        assert_eq!(idx.recording, 0);
        assert_eq!(idx.intro, Some(1));
        assert_eq!(idx.outro, Some(2));
        let g = build_concat_graph(&p, idx);
        assert!(g.contains("[1:a:0]asetpts=PTS-STARTPTS,aresample=44100"));
        assert!(g.contains("apad=pad_dur=1[intro]"));
        assert!(g.contains(
            "[0:a:0]asetpts=PTS-STARTPTS,atrim=start=42.5:end=3600.25,asetpts=PTS-STARTPTS"
        ));
        assert!(g.contains("channel_layouts=stereo,apad=pad_dur=1[rec]"));
        assert!(g.contains("[2:a:0]asetpts=PTS-STARTPTS"));
        assert!(g.ends_with("[intro][rec][outro]concat=n=3:v=0:a=1[cat]"));
    }

    #[test]
    fn graph_without_intro_and_outro() {
        let mut p = plan(false, false);
        p.pad_after_recording = 0.0;
        p.rec_end = None;
        let (_, idx) = add_inputs(Args::new(), &p);
        let g = build_concat_graph(&p, idx);
        assert!(g.contains("atrim=start=42.5,asetpts"));
        assert!(!g.contains("concat"));
        assert!(!g.contains("apad"));
        assert!(g.ends_with("[cat]"));
    }

    #[test]
    fn graph_only_outro() {
        let p = plan(false, true);
        let (_, idx) = add_inputs(Args::new(), &p);
        assert_eq!(idx.outro, Some(1));
        let g = build_concat_graph(&p, idx);
        assert!(g.ends_with("[rec][outro]concat=n=2:v=0:a=1[cat]"));
        assert!(!g.contains("[intro]"));
    }

    #[test]
    fn graph_crossfade_chain() {
        let mut p = plan(true, true);
        p.crossfade = 0.25;
        let (_, idx) = add_inputs(Args::new(), &p);
        let g = build_concat_graph(&p, idx);
        assert!(g.contains("[intro][rec]acrossfade=d=0.25:c1=tri:c2=tri[x1]"));
        assert!(g.contains("[x1][outro]acrossfade=d=0.25:c1=tri:c2=tri[cat]"));
        assert!(!g.contains("concat="));
    }

    #[test]
    fn mono_layout() {
        let mut p = plan(true, false);
        p.format.channels = 1;
        let (_, idx) = add_inputs(Args::new(), &p);
        assert!(build_concat_graph(&p, idx).contains("channel_layouts=mono"));
    }

    #[test]
    fn measure_command() {
        let p = plan(true, true);
        let a = strs(&measure_args(&p, &LoudnessTarget::default()));
        // three inputs, each protected by the protocol whitelist
        assert_eq!(a.iter().filter(|s| *s == "-protocol_whitelist").count(), 3);
        let fc = a.iter().position(|s| s == "-filter_complex").unwrap();
        assert!(
            a[fc + 1].ends_with("[cat]loudnorm=I=-16.00:TP=-1.50:LRA=11.00:print_format=json[out]")
        );
        assert!(a.ends_with(&["-f".into(), "null".into(), "-".into()]));
        // the recording path is passed verbatim (absolute, file: prefix)
        assert!(a
            .iter()
            .any(|s| s.starts_with("file:") && s.ends_with("nagranie ąę.mp3")));
    }

    #[test]
    fn encode_command() {
        let p = plan(true, true);
        let m = LoudnormMeasurement {
            input_i: -24.3,
            input_tp: -3.2,
            input_lra: 14.0,
            input_thresh: -35.0,
            target_offset: 0.4,
        };
        let a = strs(&encode_args(
            &p,
            &LoudnessTarget::default(),
            &m,
            Path::new("/out/x.mp3.tmp"),
        ));
        let fc = a.iter().position(|s| s == "-filter_complex").unwrap();
        let g = &a[fc + 1];
        assert!(g.contains("measured_I=-24.30:measured_TP=-3.20:measured_LRA=14.00:measured_thresh=-35.00:offset=0.40:linear=true"));
        // LRA target raised to the measured LRA (no compression of dynamics)
        assert!(g.contains("LRA=14.10"));
        assert!(g.ends_with("channel_layouts=stereo[out]"));
        for w in [
            ["-c:a", "libmp3lame"],
            ["-b:a", "192k"],
            ["-ar", "44100"],
            ["-ac", "2"],
            ["-f", "mp3"],
        ] {
            assert!(a.windows(2).any(|x| x[0] == w[0] && x[1] == w[1]), "{w:?}");
        }
        assert!(a.last().unwrap().starts_with("file:"));
    }

    #[test]
    fn apply_filter_keeps_target_lra_when_larger() {
        let m = LoudnormMeasurement {
            input_i: -20.0,
            input_tp: -6.0,
            input_lra: 5.0,
            input_thresh: -30.0,
            target_offset: 0.0,
        };
        assert!(loudnorm_apply_filter(&LoudnessTarget::default(), &m).contains(":LRA=11.00:"));
    }

    #[test]
    fn analysis_command() {
        let a = strs(&silence_analysis_args(
            Path::new("/a/b c.mp4"),
            10.0,
            Some(70.0),
            -50.0,
            1.5,
        ));
        let af = a.iter().position(|s| s == "-af").unwrap();
        assert_eq!(
            a[af + 1],
            "asetpts=PTS-STARTPTS,atrim=start=10:end=70,asetpts=PTS-STARTPTS,silencedetect=noise=-50.00dB:d=1.5"
        );
        assert!(a.contains(&"0:a:0".to_string()));
    }

    #[test]
    fn decode_command() {
        let a = strs(&decode_pcm_args(
            Path::new("/a.mp3"),
            5.0,
            Some(10.0),
            8000,
            "f32le",
        ));
        assert_eq!(&a[5..9], ["-ss", "5", "-t", "10"]);
        assert!(a.ends_with(&[
            "-ar".into(),
            "8000".into(),
            "-f".into(),
            "f32le".into(),
            "-".into()
        ]));
    }
}
