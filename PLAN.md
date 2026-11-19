# PLAN: SCh Tomy podcast audio tool

Desktop app that turns raw sermon recordings (MP3 or audio/video pulled from
YouTube) into podcast-ready MP3 files: cut the series teaser at the start,
handle trailing silence, add intro/outro with short pauses, normalise loudness
with two-pass `loudnorm`, write ID3 tags, verify the result.

Users are non-technical, on Windows and macOS. About 125 backlog recordings
are waiting, so batch processing is the core use case.

## Technology

* **Tauri 2** (Rust backend, plain HTML/CSS/JS frontend with ES modules, no
  bundler, no framework). Small installers, native file dialogs, native
  drag and drop with real file paths, one codebase for both systems.
* **ffmpeg + ffprobe as Tauri sidecars** (`externalBin`). Static builds from the
  `eugeneware/ffmpeg-static` GitHub release `b6.1.1` (Windows: gyan.dev
  essentials build, macOS: evermeet.cx / osxexperts.net builds), pinned by
  URL and SHA-256 in `scripts/ffmpeg-sources.json`, fetched and verified by
  `scripts/fetch-ffmpeg.mjs` at build time. Nothing to install for the user.
* **Voice activity detection: Silero VAD v5 (MIT)** run with **tract**
  (pure Rust ONNX inference, no native runtime to ship). tract cannot analyse
  the `If` nodes of the upstream model (8 kHz/16 kHz branches), so
  `tools/vad/prepare_silero_vad.py` statically inlines the 16 kHz branches
  and verifies that the result is bit-identical to the original under
  onnxruntime. The derived model (1.2 MB) is committed and embedded into the
  binary with `include_bytes!`, its SHA-256 is checked in a unit test.
* **Teaser matching**: normalised cross-correlation (FFT, `rustfft`) of
  8 kHz mono PCM decoded by ffmpeg. Gain invariant by construction (Pearson
  correlation per lag).
* **ID3 tags**: written with the `id3` crate (ID3v2.3, best podcast client
  compatibility) after encoding; ffmpeg writes no tags of its own.

## Directory layout

```
crates/podcast-core/     Rust library, no Tauri dependency, all audio logic
  src/ffmpeg.rs          locating tools, running processes (args as a list,
                         progress, cancellation, hidden console on Windows)
  src/probe.rs           ffprobe JSON
  src/filters.rs         pure builders for ffmpeg arguments and filter graphs
  src/parse.rs           parsers: silencedetect, loudnorm JSON, ebur128
  src/pipeline.rs        per-file processing (analysis, 2x loudnorm, encode,
                         tags, verification)
  src/pcm.rs             decoding to raw PCM for previews and analysis
  src/vad.rs             Silero VAD + "first longer speech" heuristic
  src/teaser.rs          teaser patterns: storage + cross-correlation search
  src/naming.rs          output file names, titles, years
  src/tags.rs            ID3 tags
  src/settings.rs        persisted settings (serde, defaults, clamping)
  assets/                silero_vad_16k.onnx (+ licence)
  tests/                 integration tests (generate audio with ffmpeg)
src-tauri/               Tauri app: commands, job queue, events
  binaries/              downloaded sidecars (git-ignored)
ui/                      index.html, style.css, js/*.js (served as is)
ui-tests/                node --test tests of ui/js/format.js (mm:ss,d etc.)
scripts/                 fetch-ffmpeg.mjs, ffmpeg-sources.json, make-icon
tools/vad/               model preparation script (one-off, reproducible)
docs/threat-model.md
.github/workflows/ci.yml test + build on Windows and macOS, release on v* tags
```

## Processing order (per file)

1. `ffprobe`: duration, audio stream present (else friendly error).
2. Analysis pass on the recording segment `[start, end]`:
   `silencedetect` (threshold and minimum length from settings).
   Only a silence interval that reaches the end of the segment counts as
   trailing silence; pauses in the middle are never touched. Optional leading
   silence trim uses an interval that starts at the segment start.
3. Segment: `start` marker (or 0, or + leading silence), end =
   `silence_start + min(silence_len, gap_before_outro)` when trailing silence
   exists, else end marker / end of file. If the kept silence is shorter than
   the gap, the rest is generated (`apad`), so the pause before the outro is
   always exactly `gap_before_outro`.
4. Filter graph: `intro + apad(gap_after_intro)` → recording (+ `apad`) →
   outro, joined by `concat` (or `acrossfade` when crossfade > 0 ms). All
   inputs resampled to 44.1 kHz and the target channel layout first.
5. Loudnorm pass 1 (measure) on the whole graph, pass 2 with the measured
   values and `linear=true`. Target TP passed to loudnorm is 0.5 dB below the
   user target to leave headroom for MP3 encoding overshoot. LRA target in
   pass 2 is `max(setting, measured LRA)` so loudnorm never narrows the
   dynamic range (no aggressive compression); it falls back to its dynamic
   mode only when a linear gain would break the true peak limit.
6. Encode: libmp3lame CBR (default 192 kb/s), 44.1 kHz, stereo (settings),
   to a temporary file in the output folder, then ID3 tags, then rename to
   the final name (`<slug>-podcast.mp3`, `-2`, `-3`... if taken; existing
   files are never overwritten, the original never).
7. Verify with `ebur128=peak=true`: integrated LUFS, true peak, duration.
   MP3 encoding (and loudnorm's own gating on programmes with silent gaps)
   can leave the file a few tenths of a LU off target; if the error is
   > 0.2 LU, the encode is repeated once with the target shifted by the
   error and the better file is kept.
   |I - target| > 0.5 LU or TP above target → warning in the list.

## Teaser start marker

* Manual: waveform of the first ~3 min and last ~2 min with playback from a
  clicked position, start/end markers, time fields `mm:ss,d`.
* VAD proposal: first point where speech covers >= 60% of the following 20 s
  window within the first 5 minutes (configurable search window). Clearly
  labelled "propozycja VAD, sprawdź".
* Patterns: a selected fragment (default 0:00 to start marker) can be saved
  under a name. New files are searched (first 5 minutes) for every pattern;
  the best match >= confidence threshold sets the start marker right after
  the matched teaser ("wzorzec zajawki: <name>, 87%"). Matches below the
  threshold (but above a noise floor) only become a proposal. Saving a new
  pattern re-runs matching for files that have no manual marker yet.
* Marker sources shown in the list: ręczny, wzorzec zajawki, propozycja
  (VAD or weak pattern match), brak. Files without a marker are processed
  without cutting the start.

## Decisions on unclear points

* **Proposals are used in batch processing.** The spec lists "propozycja VAD"
  as a marker source shown before batch start, so a proposal is applied
  unless the user changes it; it is highlighted in amber and the toolbar
  shows how many files still have only a proposal.
* **No outro:** trailing silence is still trimmed to the configured length,
  but no silence is generated. **No intro:** no gap after intro.
* **Crossfade** overlaps neighbouring segments (including the generated
  pause), so the pause becomes shorter by the crossfade length. Default 0.
* **Existing output file:** never overwritten; `-2`, `-3` suffix is added.
* **Year tag:** from a date in the file name (`dd.mm.yyyy` or `yyyy-mm-dd`),
  else the current year.
* **Title:** file name without extension, `_audio` suffix removed,
  underscores and hyphens turned into spaces; editable before start.
* **Mono output:** loudness is measured as mono (no `dual_mono`), so the
  target applies to what ebur128 reports for the file.
* **Concurrency:** 2 files at a time by default (setting 1 to 4).
* **macOS builds:** separate arm64 and x64 DMGs (a universal build would
  double the size of the bundled ffmpeg binaries). x64 is cross-compiled on
  the arm64 runner; tests run natively on arm64 and Windows x64.
* **Preview playback** decodes the head/tail to WAV via ffmpeg and plays it
  with WebAudio, so every input format plays the same on both systems.
* **ffmpeg hardening:** inputs are passed as absolute paths with
  `-protocol_whitelist file`, so crafted playlists cannot reach the network.

## Steps

1. Scaffolding, docs (AGENTS.md, CLAUDE.md, SECURITY.md, threat model).
2. Core: ffmpeg runner, probe, parsers, filter builders + unit tests.
3. Core: pipeline + integration tests (loudnorm, gaps, trailing silence).
4. Core: naming, tags, settings + tests.
5. Core: PCM decoding, VAD, teaser patterns + tests.
6. Tauri app: state, queue, commands, events.
7. UI: list, drop zone, settings, waveform editor, patterns.
8. CI: tests + bundles on Windows/macOS, release on tags.
9. README (technical), INSTRUKCJA.md (Polish user guide).
10. Stage 2 (after stage 1 is merged/tested): YouTube link via bundled yt-dlp.
