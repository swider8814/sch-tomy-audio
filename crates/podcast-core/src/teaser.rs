//! Series teaser patterns: storage and search by normalised cross-correlation.
//!
//! A pattern is a fragment of a recording (usually 0:00 to the start of the
//! sermon) stored as 8 kHz mono `f32` PCM. To find it in a new recording, the
//! beginning of the recording is decoded the same way and the Pearson
//! correlation between the pattern and every window of the recording is
//! computed with FFTs. Pearson correlation is invariant to gain and DC offset,
//! so a teaser that is louder or quieter than the stored pattern still
//! matches with the same confidence.

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{CancelToken, Tools};
use crate::pcm::{bytes_to_f32, decode_mono_f32, f32_to_bytes};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Sample rate used for patterns and the searched signal.
pub const PATTERN_RATE: u32 = 8000;
pub const MIN_PATTERN_S: f64 = 2.0;
pub const MAX_PATTERN_S: f64 = 180.0;
/// Correlation below this value is noise and not even shown as a proposal.
pub const PROPOSAL_FLOOR: f64 = 0.3;

const INDEX_FILE: &str = "patterns.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternMeta {
    pub id: String,
    pub name: String,
    /// Length in seconds.
    pub duration: f64,
    /// Name of the file the pattern was taken from (for display only).
    pub source: String,
    /// RFC 3339 creation time.
    pub created: String,
}

/// Patterns stored in a directory: `patterns.json` + `<id>.f32` files.
#[derive(Debug, Clone)]
pub struct PatternStore {
    dir: PathBuf,
}

fn pattern_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::new(ErrorKind::Pattern, e.to_string())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

impl PatternStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn list(&self) -> Vec<PatternMeta> {
        std::fs::read(self.dir.join(INDEX_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<PatternMeta>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| valid_id(&p.id))
            .collect()
    }

    fn save_index(&self, list: &[PatternMeta]) -> Result<()> {
        let json = serde_json::to_vec_pretty(list).map_err(pattern_err)?;
        crate::settings::write_atomic(&self.dir.join(INDEX_FILE), &json).map_err(pattern_err)
    }

    fn samples_path(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(pattern_err(format!("invalid pattern id {id:?}")));
        }
        Ok(self.dir.join(format!("{id}.f32")))
    }

    pub fn add(&self, name: &str, source: &str, samples: &[f32]) -> Result<PatternMeta> {
        let duration = samples.len() as f64 / PATTERN_RATE as f64;
        if duration < MIN_PATTERN_S {
            return Err(pattern_err(format!("pattern too short: {duration:.1} s")));
        }
        std::fs::create_dir_all(&self.dir).map_err(pattern_err)?;
        let id = new_id();
        let meta = PatternMeta {
            id: id.clone(),
            name: name.trim().chars().take(100).collect(),
            duration,
            source: source.chars().take(200).collect(),
            created: chrono::Local::now().to_rfc3339(),
        };
        std::fs::write(self.samples_path(&id)?, f32_to_bytes(samples)).map_err(pattern_err)?;
        let mut list = self.list();
        list.push(meta.clone());
        self.save_index(&list)?;
        Ok(meta)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let path = self.samples_path(id)?;
        let list: Vec<PatternMeta> = self.list().into_iter().filter(|p| p.id != id).collect();
        self.save_index(&list)?;
        let _ = std::fs::remove_file(path);
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<()> {
        let mut list = self.list();
        for p in list.iter_mut().filter(|p| p.id == id) {
            p.name = name.trim().chars().take(100).collect();
        }
        self.save_index(&list)
    }

    pub fn load_samples(&self, id: &str) -> Result<Vec<f32>> {
        let bytes = std::fs::read(self.samples_path(id)?).map_err(pattern_err)?;
        Ok(bytes_to_f32(&bytes))
    }

    /// All patterns with their samples (unreadable ones are skipped).
    pub fn load_all(&self) -> Vec<(PatternMeta, Vec<f32>)> {
        self.list()
            .into_iter()
            .filter_map(|m| self.load_samples(&m.id).ok().map(|s| (m, s)))
            .collect()
    }
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("p{:x}{:x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Decodes `[from, to]` of a file as pattern samples.
pub fn extract_pattern(
    tools: &Tools,
    file: &Path,
    from: f64,
    to: f64,
    cancel: &CancelToken,
) -> Result<Vec<f32>> {
    let len = to - from;
    if !(MIN_PATTERN_S..=MAX_PATTERN_S).contains(&len) || from < 0.0 {
        return Err(pattern_err(format!(
            "pattern length {len:.1} s outside {MIN_PATTERN_S}..{MAX_PATTERN_S} s"
        )));
    }
    decode_mono_f32(tools, file, from, Some(len), PATTERN_RATE, cancel)
}

/// Decodes the searched beginning of a recording.
pub fn decode_search_window(
    tools: &Tools,
    file: &Path,
    window_s: f64,
    cancel: &CancelToken,
) -> Result<Vec<f32>> {
    decode_mono_f32(tools, file, 0.0, Some(window_s), PATTERN_RATE, cancel)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeaserMatch {
    pub pattern_id: String,
    pub name: String,
    /// Where the teaser starts in the recording, seconds.
    pub offset: f64,
    /// Where it ends (= proposed start marker), seconds.
    pub end: f64,
    /// Pearson correlation at the best lag, 0..1.
    pub confidence: f64,
}

/// Cross-correlation search over one signal, reusing its spectrum for every
/// pattern.
pub struct Correlator {
    signal: Vec<f32>,
    fft_len: usize,
    spectrum: Vec<Complex32>,
    prefix: Vec<f64>,
    prefix_sq: Vec<f64>,
    planner: FftPlanner<f32>,
}

impl Correlator {
    pub fn new(signal: Vec<f32>, max_pattern_len: usize) -> Self {
        let n = signal.len();
        let fft_len = (n + max_pattern_len).max(2).next_power_of_two();
        let mut planner = FftPlanner::<f32>::new();
        let mut spectrum: Vec<Complex32> = signal
            .iter()
            .map(|&x| Complex32::new(x, 0.0))
            .chain(std::iter::repeat(Complex32::new(0.0, 0.0)))
            .take(fft_len)
            .collect();
        planner.plan_fft_forward(fft_len).process(&mut spectrum);
        let mut prefix = Vec::with_capacity(n + 1);
        let mut prefix_sq = Vec::with_capacity(n + 1);
        let (mut a, mut b) = (0.0f64, 0.0f64);
        prefix.push(0.0);
        prefix_sq.push(0.0);
        for &x in &signal {
            a += x as f64;
            b += (x as f64) * (x as f64);
            prefix.push(a);
            prefix_sq.push(b);
        }
        Self {
            signal,
            fft_len,
            spectrum,
            prefix,
            prefix_sq,
            planner,
        }
    }

    /// Best lag (in samples) and its Pearson correlation.
    pub fn search(&mut self, pattern: &[f32]) -> Option<(usize, f64)> {
        let n = self.signal.len();
        let m = pattern.len();
        if m < 2 || m > n || n + m > self.fft_len {
            return None;
        }
        let mean = pattern.iter().map(|&x| x as f64).sum::<f64>() / m as f64;
        let norm = pattern
            .iter()
            .map(|&x| (x as f64 - mean).powi(2))
            .sum::<f64>()
            .sqrt();
        if norm < 1e-9 {
            return None; // silent pattern
        }
        // reversed, zero-mean, unit-norm pattern
        let mut buf: Vec<Complex32> = vec![Complex32::new(0.0, 0.0); self.fft_len];
        for (i, &x) in pattern.iter().enumerate() {
            buf[m - 1 - i] = Complex32::new(((x as f64 - mean) / norm) as f32, 0.0);
        }
        self.planner
            .plan_fft_forward(self.fft_len)
            .process(&mut buf);
        for (b, s) in buf.iter_mut().zip(&self.spectrum) {
            *b *= *s;
        }
        self.planner
            .plan_fft_inverse(self.fft_len)
            .process(&mut buf);
        let scale = 1.0 / self.fft_len as f64;

        // Ignore windows that are (nearly) silent: their correlation is noise.
        let total_energy = self.prefix_sq[n] / n as f64;
        let min_var = (total_energy * 1e-4).max(1e-12) * m as f64;
        let mut best = (0usize, f64::MIN);
        for k in 0..=(n - m) {
            let sum = self.prefix[k + m] - self.prefix[k];
            let sum_sq = self.prefix_sq[k + m] - self.prefix_sq[k];
            let var = sum_sq - sum * sum / m as f64;
            if var <= min_var {
                continue;
            }
            let corr = buf[k + m - 1].re as f64 * scale;
            let r = corr / var.sqrt();
            if r > best.1 {
                best = (k, r);
            }
        }
        (best.1 > f64::MIN).then(|| (best.0, best.1.clamp(-1.0, 1.0)))
    }
}

/// Finds the best matching pattern in `signal` (8 kHz mono).
pub fn find_best_match(
    signal: Vec<f32>,
    patterns: &[(PatternMeta, Vec<f32>)],
    cancel: &CancelToken,
) -> Result<Option<TeaserMatch>> {
    let max_len = patterns.iter().map(|(_, s)| s.len()).max().unwrap_or(0);
    if max_len == 0 || signal.is_empty() {
        return Ok(None);
    }
    let mut c = Correlator::new(signal, max_len);
    let mut best: Option<TeaserMatch> = None;
    for (meta, samples) in patterns {
        cancel.check()?;
        if let Some((lag, r)) = c.search(samples) {
            let offset = lag as f64 / PATTERN_RATE as f64;
            let m = TeaserMatch {
                pattern_id: meta.id.clone(),
                name: meta.name.clone(),
                offset,
                end: offset + samples.len() as f64 / PATTERN_RATE as f64,
                confidence: r.max(0.0),
            };
            if best.as_ref().is_none_or(|b| m.confidence > b.confidence) {
                best = Some(m);
            }
        }
    }
    Ok(best.filter(|m| m.confidence >= PROPOSAL_FLOOR))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random noise.
    fn noise(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / (1u64 << 31) as f32) - 0.5
            })
            .collect()
    }

    #[test]
    fn finds_scaled_pattern_at_exact_lag() {
        let pattern = noise(8000 * 3, 1);
        let mut signal = noise(8000 * 20, 2)
            .iter()
            .map(|x| x * 0.3)
            .collect::<Vec<_>>();
        let lag = 8000 * 5 + 123;
        for (i, &p) in pattern.iter().enumerate() {
            signal[lag + i] = p * 0.25; // 12 dB quieter than the pattern
        }
        let mut c = Correlator::new(signal, pattern.len());
        let (k, r) = c.search(&pattern).unwrap();
        assert_eq!(k, lag);
        assert!(r > 0.99, "{r}");
    }

    #[test]
    fn unrelated_signal_has_low_confidence() {
        let pattern = noise(8000 * 3, 3);
        let signal = noise(8000 * 30, 4);
        let mut c = Correlator::new(signal, pattern.len());
        let (_, r) = c.search(&pattern).unwrap();
        assert!(r < PROPOSAL_FLOOR, "{r}");
    }

    #[test]
    fn silent_pattern_or_too_long_is_none() {
        let mut c = Correlator::new(noise(1000, 5), 2000);
        assert!(c.search(&[0.0; 100]).is_none());
        assert!(c.search(&noise(1500, 6)).is_none());
    }

    #[test]
    fn store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = PatternStore::new(dir.path().join("wzorce ąę"));
        assert!(store.list().is_empty());
        let s = noise(8000 * 3, 7);
        let m = store.add("Seria: Łaska", "plik.mp3", &s).unwrap();
        assert_eq!(store.list(), vec![m.clone()]);
        assert_eq!(store.load_samples(&m.id).unwrap(), s);
        store.rename(&m.id, "Nowa nazwa").unwrap();
        assert_eq!(store.list()[0].name, "Nowa nazwa");
        store.remove(&m.id).unwrap();
        assert!(store.list().is_empty());
        assert!(store.add("krótki", "x", &noise(100, 8)).is_err());
        assert!(store.load_samples("../../etc").is_err());
    }
}
