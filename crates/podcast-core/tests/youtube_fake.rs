//! yt-dlp integration with a fake yt-dlp (no network in tests). Unix only:
//! the fake is a shell script.
#![cfg(unix)]

use podcast_core::youtube::{download_audio, VideoInfo, YtTools};
use podcast_core::CancelToken;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn fake_ytdlp(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("yt-dlp");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn info() -> VideoInfo {
    VideoInfo {
        id: "dQw4w9WgXcQ".into(),
        title: "Co mi wolno?".into(),
        upload_date: Some("20261011".into()),
        uploader: None,
        channel: None,
        duration: None,
        description: None,
    }
}

#[test]
fn download_reports_progress_and_finds_the_file() {
    let t = tempfile::Builder::new()
        .prefix("YouTube ąę ")
        .tempdir()
        .unwrap();
    let out = t.path().join("Pobrane z YouTube");
    // The fake writes the file named by the -o template (ext m4a) and prints
    // progress and the final path like yt-dlp does.
    let script = r#"
while [ "$1" != "-o" ]; do shift; done
tmpl="$2"
f="${tmpl%.%(ext)s}.m4a"
echo "SCHTOMY-PROGRESS 10 100 NA"
echo "SCHTOMY-PROGRESS 100 100 NA"
printf 'audio' > "$f"
echo "SCHTOMY-FILE $f"
"#;
    let tools = YtTools {
        ytdlp: fake_ytdlp(t.path(), script),
        deno: None,
    };
    let mut seen = Vec::new();
    let f = download_audio(&tools, &info(), &out, &CancelToken::new(), &mut |p| {
        seen.push(p)
    })
    .unwrap();
    assert_eq!(
        f.file_name().unwrap(),
        "2026-10-11-co-mi-wolno-yt-dQw4w9WgXcQ.m4a"
    );
    assert_eq!(std::fs::read(&f).unwrap(), b"audio");
    assert_eq!(seen, vec![0.1, 1.0]);
}

#[test]
fn failure_and_cancel() {
    let t = tempfile::tempdir().unwrap();
    let failing = YtTools {
        ytdlp: fake_ytdlp(
            t.path(),
            "echo 'ERROR: Sign in to confirm you are not a bot' >&2; exit 1",
        ),
        deno: None,
    };
    let e = download_audio(
        &failing,
        &info(),
        t.path(),
        &CancelToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(e.kind, podcast_core::ErrorKind::YouTubeBlocked);

    // the fake leaves a partial file before hanging
    let slow = YtTools {
        ytdlp: fake_ytdlp(
            t.path(),
            r#"while [ "$1" != "-o" ]; do shift; done; f="${2%.%(ext)s}"; printf x > "$f.m4a.part"; printf x > "$f.m4a.part-Frag1"; sleep 30"#,
        ),
        deno: None,
    };
    let c = CancelToken::new();
    let c2 = c.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        c2.cancel();
    });
    let start = std::time::Instant::now();
    let out = t.path().join("pobrane");
    let e = download_audio(&slow, &info(), &out, &c, &mut |_| {}).unwrap_err();
    assert!(e.is_cancelled());
    assert!(start.elapsed().as_secs() < 5);
    let left: Vec<_> = std::fs::read_dir(&out).unwrap().collect();
    assert!(left.is_empty(), "partial files left: {left:?}");
}
