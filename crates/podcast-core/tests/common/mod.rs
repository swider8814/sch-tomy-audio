//! Helpers for integration tests: test audio is generated with ffmpeg at run
//! time (nothing binary is committed). Intro/outro are 5 s placeholder tones.
#![allow(dead_code)]

use podcast_core::ffmpeg::{run, Args, CancelToken, StdoutMode};
use podcast_core::filters::secs;
use podcast_core::parse::{parse_silencedetect, SilenceInterval};
use podcast_core::pcm::decode_mono_f32;
use podcast_core::Tools;
use std::path::{Path, PathBuf};

pub fn tools() -> Tools {
    Tools::locate()
        .expect("ffmpeg/ffprobe must be available (PATH or SCHTOMY_FFMPEG/SCHTOMY_FFPROBE)")
}

/// Runs ffmpeg with lavfi inputs and an optional filter graph.
pub fn ffmpeg(args: &[&str]) {
    let t = tools();
    let a = Args::new()
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"])
        .args(args.iter())
        .into_vec();
    run(&t.ffmpeg, &a, &CancelToken::new(), StdoutMode::Capture).expect("ffmpeg generation failed");
}

/// One piece of a synthetic recording.
#[derive(Clone, Debug)]
pub enum Piece {
    /// Music-like "series teaser": deterministic chords, vibrato, noise hits.
    Teaser(f64),
    /// An existing audio file (e.g. the speech fixture), whole.
    File(PathBuf),
    /// Another piece with a gain change in dB.
    Gain(Box<Piece>, f64),
    /// Speech-like signal: pink noise with a syllable-rate amplitude envelope.
    Speech(f64),
    /// Digital silence.
    Silence(f64),
    /// Tone at a frequency.
    Tone(f64, f64),
    /// Full-band white noise (worst case for MP3 loudness).
    Noise(f64),
}

fn piece_source(p: &Piece, seed: usize) -> (String, f64) {
    match *p {
        Piece::Teaser(d) => (
            format!(
                "aevalsrc='0.18*sin(2*PI*(220+15*sin(2*PI*0.7*t))*t)+0.15*sin(2*PI*330*t)*(0.5+0.5*sin(2*PI*1.5*t))+0.12*sin(2*PI*(440+110*floor(mod(t*2,4)))*t)+0.08*(random(0)-0.5)*lt(mod(t,0.5),0.05)':s=44100:d={}",
                secs(d)
            ),
            d,
        ),
        Piece::File(_) | Piece::Gain(..) => unreachable!(),
        Piece::Speech(d) => (
            format!(
                "anoisesrc=color=pink:amplitude=0.25:seed={}:sample_rate=44100:duration={},lowpass=f=6000,volume='0.55+0.45*sin(2*PI*4*t)':eval=frame",
                seed + 1,
                secs(d)
            ),
            d,
        ),
        Piece::Silence(d) => (format!("anullsrc=r=44100:cl=mono,atrim=duration={}", secs(d)), d),
        Piece::Noise(d) => (
            format!(
                "anoisesrc=color=white:amplitude=0.1:seed={}:sample_rate=44100:duration={}",
                seed + 7,
                secs(d)
            ),
            d,
        ),
        Piece::Tone(f, d) => (format!("sine=f={}:sample_rate=44100:duration={}", secs(f), secs(d)), d),
    }
}

/// Builds an audio file from pieces (mono 44.1 kHz), encoded by extension.
pub fn make_audio(path: &Path, pieces: &[Piece]) {
    let mut graph = String::new();
    let mut inputs: Vec<String> = Vec::new();
    fn chain(p: &Piece, i: usize, inputs: &mut Vec<String>) -> String {
        match p {
            Piece::File(f) => {
                inputs.push(f.to_string_lossy().into_owned());
                format!("[{}:a]aresample=44100", inputs.len() - 1)
            }
            Piece::Gain(inner, g) => format!("{},volume={}dB", chain(inner, i, inputs), g),
            other => piece_source(other, i).0,
        }
    }
    for (i, p) in pieces.iter().enumerate() {
        let src = chain(p, i, &mut inputs);
        graph.push_str(&format!(
            "{src},aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=mono[p{i}];"
        ));
    }
    for i in 0..pieces.len() {
        graph.push_str(&format!("[p{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=0:a=1[out]", pieces.len()));
    let p = path.to_string_lossy().into_owned();
    let mut args: Vec<&str> = Vec::new();
    for i in &inputs {
        args.extend(["-i", i.as_str()]);
    }
    args.extend(["-filter_complex", &graph, "-map", "[out]"]);
    if p.ends_with(".mp3") {
        args.extend(["-c:a", "libmp3lame", "-b:a", "192k"]);
    }
    args.push(&p);
    ffmpeg(&args);
}

pub fn make_tone(path: &Path, freq: f64, duration: f64) {
    make_audio(path, &[Piece::Tone(freq, duration)]);
}

pub fn make_jpeg(path: &Path) {
    let p = path.to_string_lossy().into_owned();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=0x224488:s=64x64",
        "-frames:v",
        "1",
        &p,
    ]);
}

/// Silence intervals of a file (threshold -50 dB, minimum 0.3 s).
pub fn silences(file: &Path) -> Vec<SilenceInterval> {
    let t = tools();
    let a = Args::new()
        .args(["-hide_banner", "-nostdin", "-nostats"])
        .input(file)
        .args(["-af", "silencedetect=noise=-50dB:d=0.3", "-f", "null", "-"])
        .into_vec();
    let out = run(&t.ffmpeg, &a, &CancelToken::new(), StdoutMode::Capture).unwrap();
    parse_silencedetect(&out.stderr)
}

/// Approximate dominant frequency of a fragment from its zero-crossing rate.
pub fn zcr_frequency(file: &Path, start: f64, duration: f64) -> f64 {
    let v = decode_mono_f32(
        &tools(),
        file,
        start,
        Some(duration),
        44100,
        &CancelToken::new(),
    )
    .unwrap();
    let crossings = v
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    crossings as f64 / 2.0 / (v.len() as f64 / 44100.0)
}

pub struct TestDir {
    pub dir: tempfile::TempDir,
}

impl TestDir {
    /// Temporary directory whose name contains Polish characters and spaces.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("Kazania Łódź ząb ")
            .tempdir()
            .unwrap();
        Self { dir }
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

pub fn speech_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mowa-espeak.opus")
}

pub fn approx(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
