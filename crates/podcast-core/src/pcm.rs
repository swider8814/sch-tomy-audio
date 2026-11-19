//! Decoding to raw PCM (analysis) and to WAV bytes (preview playback).

use crate::error::{ErrorKind, Result};
use crate::ffmpeg::{run, CancelToken, StdoutMode, Tools};
use crate::filters::decode_pcm_args;
use std::path::Path;

/// Decodes `[start, start + duration]` of the first audio stream to mono
/// `f32` samples at `sample_rate`.
pub fn decode_mono_f32(
    tools: &Tools,
    file: &Path,
    start: f64,
    duration: Option<f64>,
    sample_rate: u32,
    cancel: &CancelToken,
) -> Result<Vec<f32>> {
    let args = decode_pcm_args(file, start, duration, sample_rate, "f32le");
    let out = run(&tools.ffmpeg, &args, cancel, StdoutMode::Capture)
        .map_err(|e| e.with_kind(ErrorKind::InputUnreadable))?;
    Ok(bytes_to_f32(&out.stdout))
}

pub fn bytes_to_f32(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

pub fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for s in v {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Decodes a fragment to a 16-bit mono WAV file in memory (for WebAudio).
pub fn decode_wav(
    tools: &Tools,
    file: &Path,
    start: f64,
    duration: f64,
    sample_rate: u32,
    cancel: &CancelToken,
) -> Result<Vec<u8>> {
    let args = decode_pcm_args(file, start, Some(duration), sample_rate, "s16le");
    let out = run(&tools.ffmpeg, &args, cancel, StdoutMode::Capture)
        .map_err(|e| e.with_kind(ErrorKind::InputUnreadable))?;
    Ok(wav_from_s16le(&out.stdout, sample_rate, 1))
}

/// Wraps raw s16le samples in a canonical 44-byte WAV header.
pub fn wav_from_s16le(data: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (data.len() - data.len() % 2) as u32;
    let byte_rate = sample_rate * channels as u32 * 2;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&byte_rate.to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend_from_slice(&data[..data_len as usize]);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_roundtrip() {
        let v = vec![0.0f32, -1.0, 0.5, 1e-6];
        assert_eq!(bytes_to_f32(&f32_to_bytes(&v)), v);
        assert_eq!(bytes_to_f32(&[1, 2, 3]).len(), 0);
    }

    #[test]
    fn wav_header() {
        let w = wav_from_s16le(&[0, 0, 1, 0, 2], 22050, 1);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 4);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 22050);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 4);
        assert_eq!(w.len(), 48);
    }
}
