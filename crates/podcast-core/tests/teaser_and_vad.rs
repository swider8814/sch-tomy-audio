//! Teaser pattern search and VAD start proposals on generated recordings.

mod common;

use common::*;
use podcast_core::teaser::{decode_search_window, extract_pattern, find_best_match, PatternStore};
use podcast_core::vad::propose_start;
use podcast_core::CancelToken;

const THRESHOLD: f64 = 0.6; // default confidence threshold in settings

#[test]
fn saved_teaser_is_found_in_another_episode() {
    let t = TestDir::new();
    let c = CancelToken::new();
    // episode 1: 25 s teaser + "speech"
    let ep1 = t.path("odcinek 1 ąę.mp3");
    make_audio(&ep1, &[Piece::Teaser(25.0), Piece::Speech(40.0)]);
    let store = PatternStore::new(t.path("wzorce"));
    let samples = extract_pattern(&tools(), &ep1, 0.0, 25.0, &c).unwrap();
    store
        .add("Seria testowa", "odcinek 1 ąę.mp3", &samples)
        .unwrap();

    // episode 2: 3 s of other audio, the same teaser 9 dB quieter, other speech
    let ep2 = t.path("odcinek 2 śż.mp3");
    make_audio(
        &ep2,
        &[
            Piece::Speech(3.0),
            Piece::Gain(Box::new(Piece::Teaser(25.0)), -9.0),
            Piece::Speech(50.0),
        ],
    );
    let window = decode_search_window(&tools(), &ep2, 300.0, &c).unwrap();
    let m = find_best_match(window, &store.load_all(), &c)
        .unwrap()
        .expect("teaser found");
    eprintln!("{m:?}");
    assert_eq!(m.name, "Seria testowa");
    assert!(approx(m.offset, 3.0, 0.5), "{m:?}");
    assert!(approx(m.end, 28.0, 0.5), "{m:?}");
    assert!(m.confidence >= THRESHOLD, "{m:?}");

    // the same teaser 8 dB louder (still no clipping) is found as well
    let ep3 = t.path("odcinek 3.wav");
    make_audio(
        &ep3,
        &[
            Piece::Gain(Box::new(Piece::Teaser(25.0)), 4.0),
            Piece::Speech(30.0),
        ],
    );
    let window = decode_search_window(&tools(), &ep3, 300.0, &c).unwrap();
    let m = find_best_match(window, &store.load_all(), &c)
        .unwrap()
        .unwrap();
    assert!(
        approx(m.end, 25.0, 0.5) && m.confidence >= THRESHOLD,
        "{m:?}"
    );
}

#[test]
fn recording_without_teaser_gives_no_confident_match() {
    let t = TestDir::new();
    let c = CancelToken::new();
    let ep1 = t.path("z zajawką.wav");
    make_audio(&ep1, &[Piece::Teaser(20.0), Piece::Speech(10.0)]);
    let store = PatternStore::new(t.path("wzorce"));
    store
        .add(
            "Seria",
            "x",
            &extract_pattern(&tools(), &ep1, 0.0, 20.0, &c).unwrap(),
        )
        .unwrap();
    let other = t.path("bez zajawki.wav");
    make_audio(&other, &[Piece::Speech(60.0), Piece::Tone(523.0, 20.0)]);
    let window = decode_search_window(&tools(), &other, 300.0, &c).unwrap();
    let m = find_best_match(window, &store.load_all(), &c).unwrap();
    assert!(m.as_ref().is_none_or(|m| m.confidence < THRESHOLD), "{m:?}");
}

#[test]
fn vad_proposes_start_of_speech_after_music() {
    let t = TestDir::new();
    let rec = t.path("muzyka i mowa.wav");
    let speech = speech_fixture();
    make_audio(
        &rec,
        &[
            Piece::Teaser(20.0),
            Piece::File(speech.clone()),
            Piece::File(speech),
        ],
    );
    let p = propose_start(&tools(), &rec, 300.0, &CancelToken::new())
        .unwrap()
        .expect("speech found");
    eprintln!("{p:?}");
    assert!(approx(p.start, 20.0, 1.0), "{p:?}");
}

#[test]
fn vad_on_music_only_finds_nothing() {
    let t = TestDir::new();
    let rec = t.path("sama muzyka.wav");
    make_audio(&rec, &[Piece::Teaser(40.0)]);
    let p = propose_start(&tools(), &rec, 300.0, &CancelToken::new()).unwrap();
    assert_eq!(p, None);
}
