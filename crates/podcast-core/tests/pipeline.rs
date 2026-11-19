//! End-to-end tests of the processing pipeline with generated audio.

mod common;

use common::*;
use podcast_core::pipeline::{process, Cut, JobInput, ProcessResult};
use podcast_core::settings::Settings;
use podcast_core::{CancelToken, ErrorKind};
use std::path::Path;

const INTRO_HZ: f64 = 440.0;
const OUTRO_HZ: f64 = 1000.0;

fn settings_with_jingles(t: &TestDir) -> Settings {
    let intro = t.path("intro ąę.wav");
    let outro = t.path("outro śż.wav");
    make_tone(&intro, INTRO_HZ, 5.0);
    make_tone(&outro, OUTRO_HZ, 5.0);
    Settings {
        intro: Some(intro),
        outro: Some(outro),
        ..Default::default()
    }
}

fn run_job(settings: &Settings, recording: &Path, cut: Cut) -> ProcessResult {
    let mut log = String::new();
    let job = JobInput {
        recording: recording.to_path_buf(),
        cut,
        title: "Test".into(),
        year: Some(2025),
        file_name: None,
    };
    match process(
        &common::tools(),
        settings,
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut log,
    ) {
        Ok(r) => {
            eprintln!("{r:?}\n{log}");
            r
        }
        Err(e) => panic!("processing failed: {e}\n--- log ---\n{log}"),
    }
}

#[test]
fn two_pass_loudnorm_reaches_target() {
    let t = TestDir::new();
    let rec = t.path("ciche nagranie.wav");
    // quiet speech-like signal (about -30 LUFS)
    make_audio(&rec, &[Piece::Speech(40.0)]);
    common::ffmpeg(&[
        "-i",
        &rec.to_string_lossy(),
        "-af",
        "volume=-12dB",
        &t.path("cichsze.wav").to_string_lossy(),
    ]);
    let s = Settings::default();
    let r = run_job(&s, &t.path("cichsze.wav"), Cut::default());
    assert!(
        approx(r.loudness.integrated, -16.0, 0.5),
        "integrated {}",
        r.loudness.integrated
    );
    let tp = r.loudness.true_peak.expect("true peak measured");
    assert!(tp <= -1.0, "true peak {tp}");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(approx(r.duration, 40.0, 0.1), "duration {}", r.duration);
}

#[test]
fn broadband_noise_is_corrected_after_mp3_encoding() {
    let t = TestDir::new();
    let rec = t.path("szum.wav");
    make_audio(&rec, &[Piece::Noise(30.0)]);
    let r = run_job(&Settings::default(), &rec, Cut::default());
    assert!(
        approx(r.loudness.integrated, -16.0, 0.5),
        "{}",
        r.loudness.integrated
    );
    assert!(
        r.loudness.true_peak.unwrap() <= -1.0,
        "{:?}",
        r.loudness.true_peak
    );
    // no temporary files are left in the folder
    let names: Vec<String> = std::fs::read_dir(t.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains(".part")), "{names:?}");
}

#[test]
fn loud_input_is_limited_to_true_peak() {
    let t = TestDir::new();
    let rec = t.path("głośne.wav");
    make_audio(&rec, &[Piece::Speech(30.0)]);
    common::ffmpeg(&[
        "-i",
        &rec.to_string_lossy(),
        "-af",
        "volume=12dB,alimiter=limit=0.99",
        &t.path("głośniejsze.wav").to_string_lossy(),
    ]);
    let r = run_job(
        &Settings::default(),
        &t.path("głośniejsze.wav"),
        Cut::default(),
    );
    assert!(
        approx(r.loudness.integrated, -16.0, 0.5),
        "{}",
        r.loudness.integrated
    );
    assert!(
        r.loudness.true_peak.unwrap() <= -1.0,
        "{:?}",
        r.loudness.true_peak
    );
}

#[test]
fn order_and_gaps_intro_recording_outro() {
    let t = TestDir::new();
    let s = settings_with_jingles(&t);
    let rec = t.path("kazanie bez ciszy.mp3");
    make_audio(&rec, &[Piece::Speech(20.0)]);
    let r = run_job(&s, &rec, Cut::default());

    // 5 intro + 1 gap + 20 recording + 1 gap + 5 outro
    assert!(approx(r.duration, 32.0, 0.1), "duration {}", r.duration);
    let sil = silences(&r.output);
    assert_eq!(sil.len(), 2, "{sil:?}");
    assert!(
        approx(sil[0].start, 5.0, 0.06) && approx(sil[0].end.unwrap(), 6.0, 0.06),
        "{sil:?}"
    );
    assert!(
        approx(sil[1].start, 26.0, 0.06) && approx(sil[1].end.unwrap(), 27.0, 0.06),
        "{sil:?}"
    );
    // intro first, outro last
    assert!(approx(zcr_frequency(&r.output, 1.0, 3.0), INTRO_HZ, 20.0));
    assert!(approx(zcr_frequency(&r.output, 28.0, 3.0), OUTRO_HZ, 40.0));
    assert_eq!(r.trailing_silence_cut, None);
}

#[test]
fn gap_lengths_follow_settings() {
    let t = TestDir::new();
    let mut s = settings_with_jingles(&t);
    s.gap_after_intro = 2.0;
    s.gap_before_outro = 0.5;
    let rec = t.path("nagranie.wav");
    make_audio(&rec, &[Piece::Speech(10.0)]);
    let r = run_job(&s, &rec, Cut::default());
    assert!(
        approx(r.duration, 5.0 + 2.0 + 10.0 + 0.5 + 5.0, 0.1),
        "{}",
        r.duration
    );
    let sil = silences(&r.output);
    assert!(
        approx(sil[0].start, 5.0, 0.06) && approx(sil[0].end.unwrap(), 7.0, 0.06),
        "{sil:?}"
    );
    assert!(
        approx(sil[1].start, 17.0, 0.06) && approx(sil[1].end.unwrap(), 17.5, 0.06),
        "{sil:?}"
    );
}

/// speech 10 s, pause 1.5 s, speech 8 s, then the given trailing silence
fn trailing_case(trailing: f64) -> (ProcessResult, Vec<podcast_core::parse::SilenceInterval>) {
    let t = TestDir::new();
    let s = settings_with_jingles(&t);
    let rec = t.path(&format!("kazanie {trailing} s ciszy.wav"));
    let mut pieces = vec![Piece::Speech(10.0), Piece::Silence(1.5), Piece::Speech(8.0)];
    if trailing > 0.0 {
        pieces.push(Piece::Silence(trailing));
    }
    make_audio(&rec, &pieces);
    let r = run_job(&s, &rec, Cut::default());
    let sil = silences(&r.output);
    // keep the temp dir alive until measured
    drop(t);
    (r, sil)
}

fn assert_trailing_layout(r: &ProcessResult, sil: &[podcast_core::parse::SilenceInterval]) {
    // intro 5, gap 1, recording 19.5, exactly 1 s before outro, outro 5
    assert!(approx(r.duration, 31.5, 0.1), "duration {}", r.duration);
    assert_eq!(sil.len(), 3, "{sil:?}");
    // gap after intro
    assert!(
        approx(sil[0].start, 5.0, 0.06) && approx(sil[0].end.unwrap(), 6.0, 0.06),
        "{sil:?}"
    );
    // the pause in the middle is untouched
    assert!(
        approx(sil[1].start, 16.0, 0.06) && approx(sil[1].end.unwrap(), 17.5, 0.06),
        "{sil:?}"
    );
    // exactly 1 s before the outro
    let before_outro = sil[2].end.unwrap() - sil[2].start;
    assert!(approx(before_outro, 1.0, 0.06), "{sil:?}");
    assert!(approx(sil[2].end.unwrap(), 26.5, 0.06), "{sil:?}");
}

#[test]
fn trailing_silence_3s_is_trimmed_to_1s() {
    let (r, sil) = trailing_case(3.0);
    assert_trailing_layout(&r, &sil);
    let cut = r.trailing_silence_cut.expect("silence found");
    assert!(approx(cut, 2.0, 0.06), "{cut}");
}

#[test]
fn trailing_silence_15s_is_trimmed_to_1s() {
    let (r, sil) = trailing_case(15.0);
    assert_trailing_layout(&r, &sil);
    assert!(approx(r.trailing_silence_cut.unwrap(), 14.0, 0.06));
}

#[test]
fn no_trailing_silence_gets_generated_1s() {
    let (r, sil) = trailing_case(0.0);
    assert_trailing_layout(&r, &sil);
    assert_eq!(r.trailing_silence_cut, None);
}

#[test]
fn start_and_end_markers_cut_the_recording() {
    let t = TestDir::new();
    let s = Settings::default();
    let rec = t.path("zajawka i kazanie.wav");
    // 6 s "teaser" tone, 20 s speech, 4 s tone at the end
    make_audio(
        &rec,
        &[
            Piece::Tone(300.0, 6.0),
            Piece::Speech(20.0),
            Piece::Tone(300.0, 4.0),
        ],
    );
    let r = run_job(
        &s,
        &rec,
        Cut {
            start: Some(6.0),
            end: Some(26.0),
        },
    );
    assert!(approx(r.duration, 20.0, 0.1), "{}", r.duration);
    assert_eq!(r.trailing_silence_cut, None);
}

#[test]
fn crossfade_shortens_the_result() {
    let t = TestDir::new();
    let mut s = settings_with_jingles(&t);
    s.crossfade_ms = 500;
    let rec = t.path("nagranie.wav");
    make_audio(&rec, &[Piece::Speech(10.0)]);
    let r = run_job(&s, &rec, Cut::default());
    assert!(approx(r.duration, 22.0 - 1.0, 0.1), "{}", r.duration);
}

#[test]
fn missing_intro_reports_friendly_error() {
    let t = TestDir::new();
    let rec = t.path("a.wav");
    make_audio(&rec, &[Piece::Speech(5.0)]);
    let s = Settings {
        intro: Some(t.path("nie ma mnie.mp3")),
        ..Default::default()
    };
    let job = JobInput {
        recording: rec,
        cut: Cut::default(),
        title: "x".into(),
        year: None,
        file_name: None,
    };
    let err = process(
        &common::tools(),
        &s,
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut String::new(),
    )
    .unwrap_err();
    assert_eq!(err.kind, ErrorKind::IntroUnreadable);
    assert!(err.user_message().contains("intro"));
}

#[test]
fn unreadable_input_reports_friendly_error() {
    let t = TestDir::new();
    let rec = t.path("to nie jest audio.mp3");
    std::fs::write(&rec, b"hello, definitely not audio").unwrap();
    let job = JobInput {
        recording: rec,
        cut: Cut::default(),
        title: "x".into(),
        year: None,
        file_name: None,
    };
    let err = process(
        &common::tools(),
        &Settings::default(),
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut String::new(),
    )
    .unwrap_err();
    assert!(
        matches!(err.kind, ErrorKind::InputUnreadable | ErrorKind::NoAudio),
        "{err}"
    );
    assert_eq!(
        ErrorKind::InputUnreadable,
        podcast_core::CoreError::new(ErrorKind::InputUnreadable, "").kind
    );
}

#[test]
fn cancel_stops_and_leaves_no_files() {
    let t = TestDir::new();
    let rec = t.path("długie.wav");
    make_audio(&rec, &[Piece::Speech(600.0)]);
    let before: Vec<_> = std::fs::read_dir(t.dir.path()).unwrap().collect();
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    let rec2 = rec.clone();
    let h = std::thread::spawn(move || {
        let job = JobInput {
            recording: rec2,
            cut: Cut::default(),
            title: "x".into(),
            year: None,
            file_name: None,
        };
        process(
            &common::tools(),
            &Settings::default(),
            &job,
            &c2,
            &mut |_, _| {},
            &mut String::new(),
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(700));
    cancel.cancel();
    let res = h.join().unwrap();
    assert!(res.unwrap_err().is_cancelled());
    let after: Vec<_> = std::fs::read_dir(t.dir.path()).unwrap().collect();
    assert_eq!(before.len(), after.len(), "temporary files left behind");
}
