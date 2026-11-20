# PLAN: SCh Tomy Audio

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
* **Title (v0.1.2):** two fields, "sermon title" and "speaker - date",
  joined as "Tytuł - Mówca - RRRR.MM.DD" like the existing Spotify
  episodes. Both are read from file names such as "06.09.2026 - Speaker -
  Title_audio.mp3" or "TOMY Piotrków 2026.10.11 - Speaker.mp4" (title then
  empty and highlighted); other names fall back to the file name as title.
* **Output name (v0.1.2):** `RRRR-MM-DD-tytul-mowca-podcast.mp3`
  (ASCII slug, date first for sorting), falling back to the slug of the
  original name. The spec asked for `<original>-podcast.mp3`; changed at the
  user's request.
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
* **Queue persistence (v0.1.3):** the job list is saved to `queue.json` in
  the app data folder (every few seconds when changed and on exit) and
  restored on start. Jobs that were processing come back as waiting,
  interrupted YouTube downloads resume, files no longer on disk become an
  error; analysis results are kept, so nothing is re-analysed needlessly.
* **Keep awake (v0.1.3):** while anything is downloading, queued or
  processing the system must not sleep (`SetThreadExecutionState` on
  Windows, `caffeinate -i -w <pid>` on macOS). The display may still turn off.
* **Review flow (v0.1.3):** "Do sprawdzenia" means a proposed start (VAD or
  weak teaser match) on a file not processed yet; the editor offers
  "Zatwierdź i następny", which saves and opens the next such file (or one
  without a sermon title) in list order. Confirming a proposal turns it into
  a manual marker. List rules live in `ui/js/logic.js` (DOM-free, tested in
  `ui-tests/logic.test.mjs`).
* **Editor zoom (v0.1.3):** only the view changes (mouse wheel, +/-
  buttons and keys); the decoded preview stays the same, so zooming needs no
  new ffmpeg call. Shortcuts: space, arrows (0.1 s, Shift 1 s), S, E, +, -.
* **Update check (v0.1.3):** at the user's request (relaxes the original
  "no network except YouTube" rule). At start (setting `check_updates`, on by
  default) and on "Sprawdź teraz" the webview fetches the latest release from
  the GitHub API (no own server needed; drafts and pre-releases are ignored by
  that endpoint) and compares versions. A newer one shows a banner that opens
  the release page; nothing is downloaded or installed by the app (an
  in-app updater would need signed update bundles and is not worth it for
  an unsigned app). Threat model T12.
* **Name (v0.1.3):** the app is "SCh Tomy Audio" (was "SCh Tomy Podcast"),
  like the repository. The bundle identifier `pl.schtomy.podcast-audio` is
  unchanged on purpose: it names the data folder, so settings, patterns and
  the job list carry over. Because the product name changed, Windows/macOS
  install it next to the old app; the guide tells users to remove the old
  one. The ID3 album "SCh Tomy podcast" is the podcast's name and stays.
* **Dark mode (v0.1.4):** setting `theme` = system (default) / light /
  dark, in Podstawowe and as one icon in the top bar (moon: switch to dark,
  sun: switch to light, based on what is on screen; both save the setting;
  "system" is chosen in the settings only). Colours are CSS variables on `:root`; dark values apply
  under `prefers-color-scheme: dark` unless `data-theme="light"`, and always
  with `data-theme="dark"`. The native window theme follows via
  `setTheme` (capability `core:window:allow-set-theme`), so the title bar
  matches. The waveform canvas is dark in both themes.
* **ETA and notification (v0.1.4):** a "run" lasts from the first queued
  file until nothing is queued or processing; the remaining time is
  extrapolated from its elapsed time and finished share (`logic.js`
  `runFraction`, `etaSeconds`), shown after 15 s. At the end a system
  notification (tauri-plugin-notification, setting `notify_done`) is sent
  only when the window is not focused; permission is asked when processing
  starts.
* **Speaker suggestions (v0.1.4):** the speaker of every finished episode
  (`speakers::speaker_of` on the "speaker - date" field) is remembered in
  `speakers.json` in the data folder (max 300, most recent first). The
  field offers them via a `<datalist>` ("Name - <this file's date>"), and a
  name within one or two typos of a known one (ignoring case and Polish
  letters) shows "Czy chodziło o...?" with a fix button. Names can be
  removed in the settings, and (v0.1.5) a list can be pasted or loaded
  from a .txt file (one name per line, bullets and numbering stripped,
  `speakers::parse_list`), appended after the names used in episodes.
* **Listening to the result (v0.1.4):** `get_result_preview` decodes the
  first 90 s or last 60 s of the job's own output file to WAV (same path as
  the editor previews) and the UI plays it with WebAudio.
* **VAD onset (v0.1.4 fix):** the "longer speech" onset is moved back over
  weaker speech just before it (probability >= 0.2, pauses <= 0.8 s, at most
  4 s), so a quiet greeting before a pause is not cut off; a longer pause
  (e.g. after the teaser music) stops it. Tests in `vad.rs`.
* **macOS signing (v0.1.4 fix):** ad hoc, without the hardened runtime.
  With it (Tauri's default), library validation refused the Python that the
  standalone yt-dlp unpacks at start ("different Team IDs"), so YouTube
  downloads failed on macOS in 0.1.3. CI now starts yt-dlp, Deno and ffmpeg
  from inside the built .app on Apple Silicon.
* **Speech clean-up (v0.1.5):** optional, off by default (`enhance_voice`):
  `filters::VOICE_CHAIN` = 4th-order 80 Hz high-pass (rumble, 50 Hz hum)
  plus `afftdn` (12 dB, noise tracking) on the recording only, before the
  generated pause. Tested on speech over 50 Hz hum and pink noise.
* **Already processed (v0.1.5):** `history.json` keeps a content fingerprint
  (size + SHA-256 of the first and last MiB, so renamed copies match) and the
  YouTube id of each finished recording; adding it again shows a note. Never
  blocks processing.
* **Watched folder (v0.1.5):** polled every 5 s (no OS watcher, so network
  drives work). Files present when the folder is chosen are the baseline; a
  new file is added once its size and modification time have not changed
  for 6 polls (30 s), so stalled network copies are not picked up early. State in
  `watch.json`, so files that arrive while the app is closed are added at
  the next start.
* **Moving the setup (v0.1.5):** one `.schtomy` JSON file (`bundle.rs`) with
  settings, intro/outro/cover bytes (base64), patterns and speakers; folder
  settings never travel. Import writes the files into `<data>/imported`,
  keeps local folders, adds patterns/speakers and validates everything.
* **Settings tabs (v0.1.3):** Podstawowe (files, folders, tags), Dźwięk
  (gaps, silence, loudness, format), Zajawki i zaawansowane (patterns,
  search window, concurrency), Skróty klawiszowe (read-only list: shortcuts
are few and conventional, remapping would only make support harder), O
programie (version, update check). A validation error switches to its tab.

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
10. Stage 2 (v0.1.2): YouTube link via bundled yt-dlp 2026.10.26 + Deno 2.9.7
    (yt-dlp needs a JavaScript runtime for YouTube; Deno is its default).
    Audio only (`bestaudio[ext=m4a]/bestaudio`, no conversion), saved to
    `Downloads/SCh Tomy YouTube`, one download at a time; title and date
    prefilled from the video, the speaker is typed by the user.
