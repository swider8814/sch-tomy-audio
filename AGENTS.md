# AGENTS.md

Desktop app (Tauri 2, Rust + plain HTML/CSS/JS) that prepares sermon recordings
of "Społeczność Chrześcijańska Tomy" for the church podcast: cuts the series
teaser at the start, handles trailing silence, adds intro/outro with 1 s pauses,
normalises loudness with two-pass ffmpeg `loudnorm` (-16 LUFS, -1 dBTP) and
writes tagged MP3 files. Users are non-technical, on Windows and macOS.

## Layout

| Path | What |
| --- | --- |
| `crates/podcast-core/` | All audio logic, no Tauri dependency. ffmpeg runner, probe, filter builders, parsers, pipeline, VAD (Silero via tract), teaser patterns, naming, ID3 tags, settings. |
| `crates/podcast-core/tests/` | Integration tests; they generate audio with ffmpeg at runtime. |
| `src-tauri/` | Tauri app: commands, job queue, events. `binaries/` holds downloaded sidecars (git-ignored). |
| `ui/` | Frontend (no bundler, served as is). |
| `ui-tests/` | `node --test` tests of the frontend helpers. |
| `scripts/` | `fetch-ffmpeg.mjs` + `ffmpeg-sources.json` (pinned URLs and SHA-256 of ffmpeg, ffprobe, yt-dlp, Deno), icon generation. |
| `tools/vad/` | One-off script that derived the bundled VAD model from upstream Silero. |
| `docs/threat-model.md`, `SECURITY.md` | Security model. |
| `PLAN.md` | Architecture and recorded decisions. |
| `INSTRUKCJA.md` | User guide in Polish. |

## Commands

```bash
# ffmpeg/ffprobe for tests: system ones on PATH, or the pinned sidecars
node scripts/fetch-ffmpeg.mjs            # downloads into src-tauri/binaries/
export SCHTOMY_FFMPEG=/path/to/ffmpeg SCHTOMY_FFPROBE=/path/to/ffprobe   # optional override

cargo test -p podcast-core               # unit + integration tests
npm run test:ui                          # frontend helpers (node --test)
npm ci && npx tauri dev                  # run the app
npx tauri build                          # installers (needs the sidecars)
```

On Linux the app needs `libwebkit2gtk-4.1-dev`; Linux is only a dev platform.

## Rules

* The UI is Polish and addresses the user with "Ty" ("Przeciągnij pliki",
  "Wybierz folder"), never "Wy".
* No em dash (—), en dash (–) or middle dot (·) as separators in human-facing
  text (UI, user guide). Use a period, comma, colon or parentheses.
* Error messages must be understandable for a non-technical user; technical
  details go to the expandable log.
* Code, comments, commits and the technical README are in English;
  `INSTRUKCJA.md` is in Polish.
* Originals are never overwritten. Output files are never overwritten either
  (a `-2`, `-3` suffix is added).
* No telemetry, no network access. Two exceptions only: the YouTube
  download in `youtube.rs`, started explicitly by the user, and the update
  check in `ui/js/update.js` (one GET of the latest release from the GitHub
  API, can be turned off in the settings; it never downloads or installs
  anything, it only opens the releases page in the browser).
* ffmpeg/ffprobe/yt-dlp are always invoked with an argument list
  (`std::process::Command`), never through a shell; ffmpeg inputs are
  absolute paths with `-protocol_whitelist file`; yt-dlp gets only a
  validated canonical YouTube URL after `--` and `--ignore-config`.
* Every change in audio processing comes with a test.

## Review guidelines

Priorities when reviewing a change:

1. **Audio processing order and correctness**: start/end cut, trailing silence
   handling (only silence that lasts to the end of the recording, trimmed to
   the gap length; pauses in the middle are never cut), generated 1 s pause
   after intro and before outro (configurable 0 to 3 s, none when intro/outro
   is not set), two-pass `loudnorm` with measured values to -16 LUFS / -1 dBTP,
   MP3 CBR 192 kb/s 44.1 kHz stereo by default, verification with `ebur128`.
2. **Paths with Polish characters and spaces** on Windows and macOS: no lossy
   conversions of `Path`/`OsStr`, no paths inside filter graphs, output names
   are ASCII slugs.
3. **Safe ffmpeg invocation**: arguments as a list, never via a shell, input
   paths absolute (so they cannot be parsed as options or protocols),
   `-protocol_whitelist file`, output written to a temp file and renamed.
4. **Errors and cancellation**: child processes are killed on cancel, temp
   files removed, the UI never hangs, messages are friendly Polish text with
   technical details available.
5. **Tests** for every change in processing (`crates/podcast-core/tests`).
