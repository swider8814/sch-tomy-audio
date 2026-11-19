//! Voice activity detection with Silero VAD v5 (MIT licence), run offline
//! with tract (pure Rust). The model is embedded in the binary; see
//! `tools/vad/prepare_silero_vad.py` for how it was derived from upstream.
//!
//! The detector only proposes a start marker: music with vocals can look like
//! speech, so the UI always lets the user check it.

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{CancelToken, Tools};
use crate::pcm::decode_mono_f32;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tract_onnx::prelude::*;

pub const MODEL_BYTES: &[u8] = include_bytes!("../assets/silero_vad_16k.onnx");
pub const MODEL_SHA256: &str = "861718705f2a3f36edc720f969263fcf465464a0cb5fc11571acecc4efce5524";

pub const VAD_RATE: u32 = 16_000;
const CHUNK: usize = 512;
const CONTEXT: usize = 64;
/// Duration of one probability frame, seconds.
pub const FRAME_S: f64 = CHUNK as f64 / VAD_RATE as f64;

/// Speech onset/offset thresholds (hysteresis, as in upstream Silero utils).
const ON: f32 = 0.5;
const OFF: f32 = 0.35;
/// "Longer speech": speech covers this share of the following window.
const WINDOW_S: f64 = 20.0;
const MIN_RATIO: f64 = 0.6;
/// The proposal is placed slightly before the detected onset.
const PRE_ROLL_S: f64 = 0.3;
/// Before the onset, quieter speech (a short greeting the model is less sure
/// about) still belongs to the start: frames at or above this probability,
/// separated by pauses of at most `BACK_GAP_S`, within `BACK_MAX_S`.
const BACK_LOW: f32 = 0.2;
const BACK_GAP_S: f64 = 0.8;
const BACK_MAX_S: f64 = 4.0;

type Model = TypedRunnableModel;

fn model() -> Result<Arc<Model>> {
    static MODEL: OnceLock<std::result::Result<Arc<Model>, String>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let load = || -> TractResult<Arc<Model>> {
                let m = tract_onnx::onnx()
                    .model_for_read(&mut std::io::Cursor::new(MODEL_BYTES))?
                    .with_input_fact(0, f32::fact([1, CHUNK + CONTEXT]).into())?
                    .with_input_fact(1, f32::fact([2, 1, 128]).into())?
                    .into_optimized()?
                    .into_runnable()?;
                Ok(m)
            };
            load().map_err(|e| format!("{e:#}"))
        })
        .clone()
        .map_err(|e| CoreError::new(ErrorKind::Processing, format!("VAD model: {e}")))
}

/// Speech probability for every 32 ms frame of 16 kHz mono audio.
pub fn speech_probabilities(samples: &[f32], cancel: &CancelToken) -> Result<Vec<f32>> {
    let model = model()?;
    let err = |e: TractError| CoreError::new(ErrorKind::Processing, format!("VAD: {e:#}"));
    let mut state: Tensor = tract_ndarray::Array3::<f32>::zeros((2, 1, 128)).into();
    let mut context = [0.0f32; CONTEXT];
    let mut out = Vec::with_capacity(samples.len() / CHUNK + 1);
    for (n, chunk) in samples.chunks(CHUNK).enumerate() {
        if n % 256 == 0 {
            cancel.check()?;
        }
        let mut input = Vec::with_capacity(CHUNK + CONTEXT);
        input.extend_from_slice(&context);
        input.extend_from_slice(chunk);
        input.resize(CHUNK + CONTEXT, 0.0);
        context.copy_from_slice(&input[CHUNK..]);
        let x: Tensor = tract_ndarray::Array2::from_shape_vec((1, CHUNK + CONTEXT), input)
            .map_err(|e| CoreError::new(ErrorKind::Processing, e.to_string()))?
            .into();
        let res = model
            .run(tvec!(x.into_tvalue(), state.clone().into_tvalue()))
            .map_err(err)?;
        let p = res[0]
            .to_plain_array_view::<f32>()
            .map_err(err)?
            .iter()
            .next()
            .copied()
            .unwrap_or(0.0);
        out.push(p);
        state = res[1].clone().into_tensor();
    }
    Ok(out)
}

/// Speech/non-speech decision per frame with hysteresis.
pub fn speech_frames(probs: &[f32]) -> Vec<bool> {
    let mut on = false;
    probs
        .iter()
        .map(|&p| {
            on = if on { p >= OFF } else { p >= ON };
            on
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeechProposal {
    /// Proposed start marker, seconds.
    pub start: f64,
    /// Share of speech in the window after the onset (0..1).
    pub ratio: f64,
}

/// First speech onset followed by a longer, mostly continuous fragment of
/// speech (>= 60% of the next 20 s; at least 5 s of window must remain).
pub fn first_long_speech(probs: &[f32]) -> Option<SpeechProposal> {
    let frames = speech_frames(probs);
    let w = (WINDOW_S / FRAME_S).round() as usize;
    let min_w = (5.0 / FRAME_S).round() as usize;
    let mut prefix = Vec::with_capacity(frames.len() + 1);
    prefix.push(0usize);
    for &f in &frames {
        prefix.push(prefix.last().unwrap() + f as usize);
    }
    for i in 0..frames.len() {
        let onset = frames[i] && (i == 0 || !frames[i - 1]);
        if !onset {
            continue;
        }
        let end = (i + w).min(frames.len());
        if end - i < min_w {
            break;
        }
        let ratio = (prefix[end] - prefix[i]) as f64 / (end - i) as f64;
        if ratio >= MIN_RATIO {
            let first = earlier_onset(probs, i);
            return Some(SpeechProposal {
                start: (first as f64 * FRAME_S - PRE_ROLL_S).max(0.0),
                ratio,
            });
        }
    }
    None
}

/// Moves an onset back over weaker speech just before it (see `BACK_LOW`):
/// "Dzień dobry", a pause, then the sermon should start at "Dzień".
fn earlier_onset(probs: &[f32], onset: usize) -> usize {
    let max_gap = (BACK_GAP_S / FRAME_S).round() as usize;
    let max_back = (BACK_MAX_S / FRAME_S).round() as usize;
    let mut first = onset;
    let mut gap = 0;
    let mut j = onset;
    while j > 0 && onset - (j - 1) <= max_back {
        j -= 1;
        if probs[j] >= BACK_LOW {
            first = j;
            gap = 0;
        } else {
            gap += 1;
            if gap > max_gap {
                break;
            }
        }
    }
    first
}

/// Decodes the beginning of a file and proposes where the sermon starts.
pub fn propose_start(
    tools: &Tools,
    file: &Path,
    window_s: f64,
    cancel: &CancelToken,
) -> Result<Option<SpeechProposal>> {
    let samples = decode_mono_f32(tools, file, 0.0, Some(window_s), VAD_RATE, cancel)?;
    let probs = speech_probabilities(&samples, cancel)?;
    Ok(first_long_speech(&probs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn embedded_model_checksum() {
        let digest = Sha256::digest(MODEL_BYTES);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, MODEL_SHA256);
    }

    #[test]
    fn model_runs_and_ignores_silence() {
        let probs =
            speech_probabilities(&vec![0.0; VAD_RATE as usize * 2], &CancelToken::new()).unwrap();
        assert_eq!(probs.len(), 63);
        assert!(probs.iter().all(|&p| p < 0.2), "{probs:?}");
    }

    fn probs_from(pattern: &[(f64, f32)]) -> Vec<f32> {
        let mut v = Vec::new();
        for &(secs, p) in pattern {
            v.extend(std::iter::repeat_n(p, (secs / FRAME_S).round() as usize));
        }
        v
    }

    #[test]
    fn short_speech_bursts_are_skipped() {
        // teaser: music with two short vocal bursts, then the sermon at 40 s
        let probs = probs_from(&[
            (5.0, 0.05),
            (1.5, 0.9),
            (10.0, 0.1),
            (2.0, 0.8),
            (21.5, 0.05),
            (8.0, 0.9),
            (0.5, 0.2),
            (30.0, 0.9),
        ]);
        let p = first_long_speech(&probs).unwrap();
        assert!((p.start - (40.0 - PRE_ROLL_S)).abs() < 0.1, "{p:?}");
        assert!(p.ratio > 0.9);
    }

    #[test]
    fn quiet_greeting_before_a_pause_is_included() {
        // music, a quiet "Dzień dobry" the model is unsure about (0.4), a short
        // pause, then the sermon: the start belongs before the greeting
        let probs = probs_from(&[(35.7, 0.05), (0.5, 0.4), (0.4, 0.05), (25.0, 0.9)]);
        let p = first_long_speech(&probs).unwrap();
        assert!((p.start - (35.7 - PRE_ROLL_S)).abs() < 0.1, "{p:?}");
        // a clear greeting, a 0.6 s pause, then speech: also from the greeting
        let probs = probs_from(&[(20.0, 0.05), (0.6, 0.9), (0.6, 0.1), (25.0, 0.9)]);
        let p = first_long_speech(&probs).unwrap();
        assert!((p.start - (20.0 - PRE_ROLL_S)).abs() < 0.1, "{p:?}");
    }

    #[test]
    fn music_ending_before_a_longer_pause_is_not_included() {
        // singing (0.3) until 2 s before the sermon: the pause stops the search
        let probs = probs_from(&[(10.0, 0.3), (2.0, 0.05), (25.0, 0.9)]);
        let p = first_long_speech(&probs).unwrap();
        assert!((p.start - (12.0 - PRE_ROLL_S)).abs() < 0.1, "{p:?}");
        // never more than 4 s back, even through continuous weak speech
        let probs = probs_from(&[(10.0, 0.05), (10.0, 0.25), (25.0, 0.9)]);
        let p = first_long_speech(&probs).unwrap();
        assert!(p.start >= 20.0 - 4.0 - PRE_ROLL_S - 0.1, "{p:?}");
    }

    #[test]
    fn no_speech_gives_none() {
        assert_eq!(first_long_speech(&probs_from(&[(60.0, 0.1)])), None);
        assert_eq!(first_long_speech(&[]), None);
    }

    #[test]
    fn hysteresis() {
        assert_eq!(
            speech_frames(&[0.4, 0.6, 0.4, 0.3, 0.5]),
            vec![false, true, true, false, true]
        );
    }
}
