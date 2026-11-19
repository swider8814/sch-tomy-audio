//! Output naming, ID3 tags and MP3 format of finished episodes.

mod common;

use common::*;
use id3::{Tag, TagLike};
use podcast_core::ffmpeg::{run, Args, StdoutMode};
use podcast_core::pipeline::{process, Cut, JobInput};
use podcast_core::settings::Settings;
use podcast_core::CancelToken;
use std::path::Path;

fn process_ok(
    s: &Settings,
    rec: &Path,
    title: &str,
    year: Option<i32>,
) -> podcast_core::pipeline::ProcessResult {
    let job = JobInput {
        recording: rec.to_path_buf(),
        cut: Cut::default(),
        title: title.into(),
        year,
        file_name: None,
    };
    let mut log = String::new();
    process(
        &tools(),
        s,
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut log,
    )
    .unwrap_or_else(|e| panic!("{e}\n{log}"))
}

fn stream_info(file: &Path) -> serde_json::Value {
    let a = Args::new()
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(podcast_core::ffmpeg::input_arg(file))
        .into_vec();
    let out = run(
        &tools().ffprobe,
        &a,
        &CancelToken::new(),
        StdoutMode::Capture,
    )
    .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn name_tags_cover_and_format() {
    let t = TestDir::new();
    let rec = t.path("30.11.2025 Kazanie o łasce – Żółć_audio.mp3");
    make_audio(&rec, &[Piece::Speech(12.0)]);
    let original = std::fs::read(&rec).unwrap();
    let cover = t.path("okładka zespołu.jpg");
    make_jpeg(&cover);
    let s = Settings {
        cover: Some(cover.clone()),
        ..Default::default()
    };

    let r = process_ok(&s, &rec, "Kazanie o łasce (część 1)", Some(2025));
    assert_eq!(
        r.output.file_name().unwrap().to_string_lossy(),
        "30-11-2025-kazanie-o-lasce-zolc-audio-podcast.mp3"
    );
    assert_eq!(
        r.output.parent(),
        rec.parent(),
        "default output is next to the original"
    );
    assert_eq!(
        std::fs::read(&rec).unwrap(),
        original,
        "original must not change"
    );

    let tag = Tag::read_from_path(&r.output).unwrap();
    assert_eq!(tag.version(), id3::Version::Id3v23);
    assert_eq!(tag.title(), Some("Kazanie o łasce (część 1)"));
    assert_eq!(tag.artist(), Some("Społeczność Chrześcijańska Tomy"));
    assert_eq!(tag.album(), Some("SCh Tomy podcast"));
    assert_eq!(tag.year(), Some(2025));
    let pic = tag.pictures().next().expect("cover embedded");
    assert_eq!(pic.mime_type, "image/jpeg");
    assert_eq!(pic.data, std::fs::read(&cover).unwrap());

    // MP3 CBR 192 kb/s, 44.1 kHz, stereo
    let info = stream_info(&r.output);
    let a = &info["streams"][0];
    assert_eq!(a["codec_name"], "mp3");
    assert_eq!(a["sample_rate"], "44100");
    assert_eq!(a["channels"], 2);
    assert_eq!(a["bit_rate"], "192000");

    // a second run never overwrites: -2 suffix
    let r2 = process_ok(&s, &rec, "x", None);
    assert_eq!(
        r2.output.file_name().unwrap().to_string_lossy(),
        "30-11-2025-kazanie-o-lasce-zolc-audio-podcast-2.mp3"
    );
    assert!(r.output.exists());
}

#[test]
fn custom_output_folder_bitrate_and_mono() {
    let t = TestDir::new();
    let rec = t.path("Nagranie z YouTube.webm");
    // a "YouTube" style file: opus audio in webm
    let wav = t.path("src.wav");
    make_audio(&wav, &[Piece::Speech(8.0)]);
    common::ffmpeg(&[
        "-i",
        &wav.to_string_lossy(),
        "-c:a",
        "libopus",
        "-b:a",
        "64k",
        &rec.to_string_lossy(),
    ]);
    let out_dir = t.path("Gotowe odcinki ś");
    std::fs::create_dir(&out_dir).unwrap();
    let s = Settings {
        output_dir: Some(out_dir.clone()),
        bitrate_kbps: 128,
        channels: 1,
        ..Default::default()
    };
    let r = process_ok(&s, &rec, "Tytuł", None);
    assert_eq!(r.output.parent().unwrap(), out_dir);
    assert_eq!(
        r.output.file_name().unwrap(),
        "nagranie-z-youtube-podcast.mp3"
    );
    let info = stream_info(&r.output);
    assert_eq!(info["streams"][0]["channels"], 1);
    assert_eq!(info["streams"][0]["bit_rate"], "128000");
    let tag = Tag::read_from_path(&r.output).unwrap();
    assert!(tag.pictures().next().is_none());
    assert!(
        approx(r.loudness.integrated, -16.0, 0.5),
        "{}",
        r.loudness.integrated
    );
}

#[test]
fn file_name_from_episode_title() {
    let t = TestDir::new();
    let rec = t.path("TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4");
    make_audio(&t.path("src.wav"), &[Piece::Speech(5.0)]);
    common::ffmpeg(&[
        "-i",
        &t.path("src.wav").to_string_lossy(),
        "-c:a",
        "aac",
        &rec.to_string_lossy(),
    ]);
    let parts = podcast_core::naming::title_parts(&rec);
    let title = podcast_core::naming::join_title("Co mi wolno?", &parts.suffix);
    let job = JobInput {
        recording: rec.clone(),
        cut: Cut::default(),
        title: title.clone(),
        year: Some(2026),
        file_name: podcast_core::naming::episode_file_name("Co mi wolno?", &parts.suffix),
    };
    let mut log = String::new();
    let r = process(
        &tools(),
        &Settings::default(),
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut log,
    )
    .unwrap_or_else(|e| panic!("{e}\n{log}"));
    assert_eq!(
        r.output.file_name().unwrap().to_string_lossy(),
        "2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3"
    );
    let tag = Tag::read_from_path(&r.output).unwrap();
    assert_eq!(
        tag.title(),
        Some("Co mi wolno? - Adam Szumorek - 2026.10.11")
    );
}

#[test]
fn missing_output_folder_is_a_friendly_error() {
    let t = TestDir::new();
    let rec = t.path("a.wav");
    make_audio(&rec, &[Piece::Speech(4.0)]);
    let s = Settings {
        output_dir: Some(t.path("nie istnieje")),
        ..Default::default()
    };
    let job = JobInput {
        recording: rec,
        cut: Cut::default(),
        title: "x".into(),
        year: None,
        file_name: None,
    };
    let e = process(
        &tools(),
        &s,
        &job,
        &CancelToken::new(),
        &mut |_, _| {},
        &mut String::new(),
    )
    .unwrap_err();
    assert_eq!(e.kind, podcast_core::ErrorKind::OutputNotWritable);
}
