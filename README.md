# SCh Tomy Audio

> **Po polsku:** aplikację pobierzesz z zakładki
> [Releases](https://github.com/swider8814/sch-tomy-audio/releases), a
> instrukcję obsługi znajdziesz w [INSTRUKCJA.md](INSTRUKCJA.md).

Desktop tool (Windows, macOS) that prepares sermon recordings of
"Społeczność Chrześcijańska Tomy" for the church podcast. User guide in
Polish: [INSTRUKCJA.md](INSTRUKCJA.md). Architecture and decisions:
[PLAN.md](PLAN.md). Rules for contributors and agents: [AGENTS.md](AGENTS.md).

## What it does

For every dropped file (MP3, WAV, M4A, MP4, MOV, WEBM, ...):

1. **Start marker**: the series teaser at the beginning is cut. The marker is
   set by hand in a waveform editor (first ~3 min, last ~2 min, playback),
   found automatically from saved **teaser patterns** (FFT cross-correlation,
   gain invariant), or proposed by **Silero VAD** (offline, embedded).
   Optional end marker.
2. **Trailing silence** (ffmpeg `silencedetect`): only silence that lasts
   until the end counts; it is trimmed to the pause before the outro. Pauses
   in the middle are never cut.
3. **Assembly**: intro + 1 s generated silence + recording + 1 s pause +
   outro (pauses 0 to 3 s, optional crossfade).
4. **Loudness**: two-pass `loudnorm` (measure, then apply measured values,
   linear when possible) to -16 LUFS integrated, -1 dBTP, LRA never narrowed
   below the measured value. If MP3 encoding shifts loudness by more than
   0.2 LU, one corrective re-encode is done.
5. **Output**: MP3 CBR 192 kb/s, 44.1 kHz, stereo (configurable), ID3v2.3
   tags (title "Sermon - Speaker - YYYY.MM.DD", artist, album, year,
   optional JPEG cover), saved as `YYYY-MM-DD-sermon-speaker-podcast.mp3`
   (or `<slug-of-original>-podcast.mp3` when the name has no speaker/date)
   next to the original or in a chosen folder. Originals and existing files are never overwritten.
6. **Verification**: `ebur128` measurement of the result (LUFS, true peak,
   duration) shown in the list; deviation > 0.5 LU is a warning.

YouTube links (stage 2): the audio is downloaded with the bundled yt-dlp
(Deno as its JavaScript runtime) into `Downloads/SCh Tomy YouTube` and then
processed like a dropped file (`crates/podcast-core/src/youtube.rs`).

## Repository layout

| Path | |
| --- | --- |
| `crates/podcast-core` | All audio logic (no GUI), unit and integration tests |
| `src-tauri` | Tauri 2 app: commands, job queue, workers |
| `ui` | Frontend: plain HTML/CSS/JS modules, no bundler |
| `ui-tests` | `node --test` tests of UI helpers |
| `scripts` | pinned ffmpeg download (`fetch-ffmpeg.mjs`, `ffmpeg-sources.json`), icon generation |
| `tools/vad` | reproducible derivation of the bundled VAD model |
| `docs/threat-model.md`, `SECURITY.md` | security |

## Development

Requirements: Rust (stable), Node.js 22, and on Linux
`libwebkit2gtk-4.1-dev`. Linux is only a development platform.

```bash
npm ci
node scripts/fetch-ffmpeg.mjs              # pinned sidecars for this platform (Windows/macOS)
node scripts/fetch-ffmpeg.mjs --from-path  # Linux/dev: copy ffmpeg from PATH
cargo test -p podcast-core                 # needs ffmpeg/ffprobe (PATH or SCHTOMY_FFMPEG/SCHTOMY_FFPROBE)
cargo test -p schtomy-audio
npm run test:ui
npx tauri dev                              # run the app
npx tauri build --bundles nsis             # Windows installer (.exe, NSIS)
npx tauri build --bundles dmg              # macOS disk image
```

Tests generate all audio (tones, noise, silence, a music-like "teaser") with
ffmpeg at run time; the only committed fixture is a 15 s synthetic speech
clip made with espeak-ng (`crates/podcast-core/tests/fixtures`) for the VAD
test. Intro/outro in tests are 5 s placeholder tones.

## CI and releases

`.github/workflows/ci.yml` is tuned for the limited Actions minutes of a
private repository (macOS minutes count 10x, Windows 2x):

* **pull requests** (not documentation-only ones): rustfmt and UI tests on
  Linux (no system packages needed); clippy, app unit tests and core tests on
  Windows x64 with the pinned ffmpeg build;
* **tags `v*` and manual runs** (Actions, CI, "Run workflow"): additionally
  the same tests on macOS arm64 and the installers: Windows x64 NSIS `.exe`, macOS
  Apple Silicon `.dmg`, macOS Intel `.dmg` (cross-compiled);
* a **tag `v*`** also creates the GitHub Release with the installers and
  `SHA256SUMS.txt` (or adds them to a release created by hand for that tag).

Bump `version` in `src-tauri/tauri.conf.json` (and the workspace `Cargo.toml`)
before tagging.

The app is **not code-signed**. INSTRUKCJA.md explains how to open it past
macOS Gatekeeper and Windows SmartScreen.

## Bundled third-party components

* **FFmpeg** (`ffmpeg`, `ffprobe`): static builds from the
  `eugeneware/ffmpeg-static` release `b6.1.1` (Windows: gyan.dev essentials
  build of FFmpeg 6.1.1; macOS: evermeet.cx / osxexperts.net builds), pinned
  by URL and SHA-256 in `scripts/ffmpeg-sources.json`. These builds are
  **GPL v3** licensed; the build's licence text is shipped next to the app as
  `FFMPEG_LICENSE.txt`. FFmpeg runs as a separate process.
* **Silero VAD** v5.1.2 (MIT), converted to a 16 kHz graph that tract can
  run, embedded in the binary.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Privacy

No telemetry. Network access happens only in two places: the YouTube
download started by the user, and the update check (one request to
`api.github.com/repos/swider8814/sch-tomy-audio/releases/latest` at start,
which can be turned off in Ustawienia, O programie). Nothing about the user
or their files is sent, and updates are never downloaded or installed by the
app: it only opens the release page in the browser. Everything else runs
locally.
