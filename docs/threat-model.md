# Threat model

## System overview

SCh Tomy Audio is a local desktop application (Tauri 2: Rust backend,
HTML/JS frontend in the system webview). There is no server, no account, no
telemetry and no network access in stage 1 (stage 2 adds the YouTube
download and the update check, T11 and T12; v0.1.7 the yt-dlp update after
a failed download, T14). The user drops audio/video files;
the app runs bundled `ffmpeg`/`ffprobe` executables as child processes and
writes MP3 files to a folder.

Assets worth protecting:

* the user's original recordings (must never be modified or deleted),
* other files on the user's disk (must not be overwritten, read into output
  or leaked),
* integrity of the produced episodes (correct audio, no injected content),
* the user's machine (no code execution from crafted input).

Trust boundaries:

1. Input media files and their names/paths (untrusted content).
2. Settings and pattern files on disk (written by the app, could be edited).
3. The webview (frontend) calling Rust commands over Tauri IPC.
4. Bundled third-party binaries and model (supply chain).
5. Build pipeline (GitHub Actions) and dependencies (crates, npm).
6. Stage 2: YouTube URLs and the bundled yt-dlp.
7. The update check: the answer of the GitHub API.
8. The yt-dlp update: files from the yt-dlp GitHub releases.

## Threats and mitigations

### T1. Crafted media files exploiting ffmpeg

*Threat:* a malicious MP3/MP4/WebM triggers a memory-safety bug in an
ffmpeg decoder or demuxer.

*Mitigations:*
* All decoding happens in separate `ffmpeg`/`ffprobe` processes
  (`podcast_core::ffmpeg::run`); a crash only fails that file
  (`ErrorKind::InputUnreadable`/`Processing`) and the UI stays responsive.
* No in-process media parsing of untrusted input in Rust (ID3 tags are only
  written to our own output; the cover JPEG is read as bytes and checked for
  a JPEG signature and a 10 MB limit, `tags::read_cover`).
* ffmpeg is pinned to a known release; updating it is a one-line change in
  `scripts/ffmpeg-sources.json` (URL + SHA-256).
* Resource limits: PCM decoding for analysis is limited to the search window
  (max 15 min) and preview (3 + 2 min); stderr capture is capped at 4 MB
  (`ffmpeg::read_tail`); processes are killed on cancel and on app exit.

*Residual risk:* an unknown ffmpeg vulnerability could execute code with the
user's privileges. Accepted for a local tool processing the church's own
recordings; keep ffmpeg updated.

### T2. Crafted playlists / protocol tricks inside media

*Threat:* a file that is really an HLS/concat playlist makes ffmpeg fetch
URLs or read other local files into the output (SSRF / local file disclosure
known from server-side transcoders).

*Mitigations:* every input is opened with `-protocol_whitelist file`
(`Args::input`, `probe::probe`), so network protocols are refused. Output is
written only to the user's own folder, never uploaded.

*Residual risk:* a playlist could still reference another local file and mix
its audio into an episode; the user listens to and publishes the result, and
nothing leaves the machine without them. Accepted.

### T3. Argument injection through file names and paths

*Threat:* a file named `-y.mp3`, `-i x`, `http:evil`, `concat:a|b` or with
spaces/Polish characters changes the ffmpeg command line.

*Mitigations:*
* Processes are spawned with `std::process::Command` and an argument vector;
  no shell is ever involved (`ffmpeg::run`), so quoting and metacharacters
  have no meaning.
* Inputs are passed as absolute paths with a `file:` prefix
  (`ffmpeg::input_arg`), so a leading `-` or a protocol-like prefix is never
  interpreted. Unit test: `input_arg_is_absolute_with_file_prefix`.
* Paths are never embedded in filter graphs; only numbers formatted by
  `filters::secs` go there.
* Windows: Rust passes arguments as UTF-16 and ffmpeg reads the wide command
  line, so Polish characters and spaces survive; integration tests run with
  such paths on Windows and macOS.

### T4. Path traversal and overwriting files when saving results

*Threat:* an input name such as `../../Windows/x` or a name equal to an
existing file makes the app write outside the intended folder or overwrite
something, including the original.

*Mitigations:*
* Output names are built only from `naming::slugify` (`[a-z0-9-]`, max 120
  chars) + `-podcast.mp3`, so they cannot contain separators or `..`
  (tests `slug_examples`, `slug_is_safe_and_bounded`).
* The output path is reserved atomically with `OpenOptions::create_new`
  (`naming::reserve_output_path`); if a file exists, `-2`, `-3`... is used.
  Existing files are never overwritten (test `reserve_never_overwrites`).
* An explicit `same_file` check guards against the output being the input.
* ffmpeg writes to a hidden temporary file in the target folder; it is
  renamed onto the reserved name only after success, and temporary files are
  removed on error or cancel (`pipeline::Cleanup`, test
  `cancel_stops_and_leaves_no_files`).
* Pattern files use generated ids validated as `[A-Za-z0-9]` before being
  joined to a path (`teaser::valid_id`).
* `get_result_preview` reads only the output file recorded for a finished
  job (never a path from the frontend) and decodes it in ffmpeg like any
  other input. Remembered speaker names (`speakers.json`) are plain text,
  length-limited and shown with `textContent`.

### T5. Tampered settings or pattern files

*Threat:* `settings.json` or `patterns.json` in the app data folder is edited
(by malware or by hand) to contain extreme values or odd paths.

*Mitigations:* settings are deserialised with defaults for missing fields and
`Settings::sanitized` clamps every number (loudness, gaps, concurrency...);
a broken file falls back to defaults with a visible note. Intro/outro/cover
paths are only ever read (as ffmpeg inputs with the same protections as T1 to
T3), never written. Files are written atomically (temp + rename). An attacker
who can write the user's app data folder already has the user's privileges,
so this is defence in depth only.

### T6. Frontend injection (XSS) and IPC abuse

*Threat:* a file name or title containing HTML/JS executes in the webview and
calls privileged commands.

*Mitigations:*
* The UI builds DOM nodes with `textContent` only (`ui/js/util.js` `h()`),
  never `innerHTML`.
* Strict CSP in `tauri.conf.json` (`script-src 'self'`, no remote origins),
  `freezePrototype`, no remote content is loaded.
* Capabilities are minimal (`core:default`, window destroy, dialog
  open/ask/message, opener reveal-in-folder and open-url limited to this
  project's releases page and a `mailto:` link to the author, window
  set-theme, notifications); there is no generic shell/fs plugin. Commands validate
  their inputs (markers within duration, titles length-limited, pattern
  lengths bounded) and only operate on files the user added.

### T7. Denial of service / hangs

*Threat:* a huge or malformed file makes the app hang or exhaust memory.

*Mitigations:* work runs in background threads; the UI thread never waits on
ffmpeg. Every child process is cancellable (`CancelToken`, polled every
100 ms, child killed). Bounded buffers as in T1. Concurrency is limited to
1 to 4 files.

### T8. Supply chain: bundled binaries and model

*Threat:* a compromised download replaces ffmpeg/ffprobe or the VAD model.

*Mitigations:*
* ffmpeg/ffprobe: fixed release URLs (GitHub release `b6.1.1` of
  eugeneware/ffmpeg-static, which repackages gyan.dev / evermeet.cx /
  osxexperts.net builds) with SHA-256 pinned in
  `scripts/ffmpeg-sources.json`; `scripts/fetch-ffmpeg.mjs` refuses
  mismatching files. Licence and readme files are pinned too.
* VAD model: derived from the PyPI wheel `silero_vad-5.1.2` (wheel and model
  hashes recorded in `tools/vad/prepare_silero_vad.py`), committed to the
  repository, embedded with `include_bytes!` and checked against
  `vad::MODEL_SHA256` in a unit test. The derivation script verifies the
  converted model is bit-identical in output to the original.
* The speech test fixture is generated audio (espeak-ng), not executable.

*Residual risk:* we trust the original build providers. Accepted; the
sources are long-standing and widely used.

### T9. Supply chain: Rust and JS dependencies

*Mitigations:* `Cargo.lock` and `package-lock.json` are committed; CI uses
`--locked` and `npm ci`. The frontend has **no runtime npm dependencies**
(plain JS); npm is only used for the Tauri CLI at build time. Dependencies
are mainstream crates (tauri, tract, rustfft, id3, serde, regex).

### T10. Build pipeline (GitHub Actions)

*Threat:* a compromised action or an over-privileged token tampers with
releases.

*Mitigations:*
* All third-party actions are pinned by full commit SHA
  (`.github/workflows/ci.yml`), with the version in a comment.
* Workflow-level `permissions: contents: read`; only the `release` job (on
  `v*` tags, after lint, tests and builds pass) gets `contents: write`, and it
  uses the built-in `gh` CLI rather than a third-party release action.
* `actions/checkout` runs with `persist-credentials: false`.
* Release assets are published with `SHA256SUMS.txt`.
* The existing Claude workflows (`claude.yml`, `claude-code-review.yml`) are
  managed separately and not changed by this project.

### T11. YouTube downloads (yt-dlp + Deno)

*Threat:* malicious or unexpected links, argument injection into yt-dlp,
downloaded files landing outside the intended folder, yt-dlp reading
configuration files or running commands, a compromised yt-dlp/Deno binary,
hanging downloads.

*Mitigations (`crates/podcast-core/src/youtube.rs`):*
* Network access happens only when the user pastes links (or loads a .txt
  list) and clicks "Pobierz" (`add_youtube` command). There is no other
  network code in the app. Link lists are parsed by `parse_link_list`
  (one link per line, max 1000, comments and duplicates skipped); the
  `read_link_file` command reads only `.txt`/`.csv` files up to 1 MB.
* `parse_video_id` accepts only `http(s)` links to `youtube.com`,
  `www./m./music.youtube.com` or `youtu.be` with an 11-character id
  (`[A-Za-z0-9_-]`); everything else (playlists, other hosts, look-alike
  domains, option-like text) is rejected (tests `rejects_everything_else`).
  yt-dlp only ever receives the canonical
  `https://www.youtube.com/watch?v=<id>`, after `--`, as the last argument.
* yt-dlp runs with `--ignore-config` (no user/system config files),
  `--no-playlist`, `--no-exec`, no post-processors, audio-only format
  selection, and an output template built from `[a-z0-9-]` plus the id in
  an app-chosen folder (`Downloads/SCh Tomy YouTube`). The reported file
  path is accepted only inside that folder.
* The binaries are pinned like ffmpeg: yt-dlp 2026.10.26 official release
  binaries and Deno 2.9.7 official release zips, URL + SHA-256 in
  `scripts/ffmpeg-sources.json` (sums equal upstream `SHA2-256SUMS` /
  `.sha256sum`). yt-dlp gets the bundled Deno via `--js-runtimes deno:<path>`;
  no remote components are enabled and yt-dlp never updates itself (its own
  `-U` is not used); a newer yt-dlp only comes through T14.
* Downloads run in a single worker, are cancellable, and cancelling kills
  the whole process tree (`ffmpeg::kill_tree`: own process group on Unix,
  `taskkill /T` on Windows) because the standalone yt-dlp builds start a
  child process (test `failure_and_cancel`).
* The downloaded file then goes through the same pipeline as T1 to T4.

*Residual risk:* yt-dlp parses data from YouTube; a vulnerability there runs
with the user's privileges, like T1. YouTube changes regularly break older
yt-dlp versions; T14 lets the user fetch a newer one after a failure.

### T12. Update check (GitHub API)

*Threat:* the check leaks information about the user, a tampered or spoofed
answer makes the app open a malicious page or run code, or the check is
abused to download and run an "update".

*Mitigations (`ui/js/update.js`, `ui/js/logic.js`):*
* One HTTPS GET of
  `https://api.github.com/repos/swider8814/sch-tomy-audio/releases/latest`
  at start (only when "Sprawdzaj przy uruchomieniu" is on, the default) or
  when the user clicks "Sprawdź teraz". No body, no cookies
  (`credentials: "omit"`), no referrer, no identifiers; GitHub sees an
  ordinary request from the user's IP address.
* The CSP `connect-src` allows only `https://api.github.com` as a remote
  host, so the webview cannot contact anything else (test
  `links_and_network_are_narrow` in `src-tauri/src/main.rs`).
* From the answer only `tag_name` (must match `v?N.N.N`) and `html_url` are
  used. A link that is not under
  `https://github.com/swider8814/sch-tomy-audio/releases/` (plain
  characters only) is replaced by the fixed releases page
  (`releaseInfo`, tests in `ui-tests/logic.test.mjs`). Text is shown with
  `textContent` only.
* The opener capability allows `openUrl` only for
  `https://github.com/swider8814/sch-tomy-audio/releases*`, so even a
  compromised frontend cannot open other URLs through it.
* The app never downloads or installs updates; the user downloads the
  installer from the release page. Failures are silent at start and shown
  as a short message on manual checks.

*Residual risk:* someone controlling the GitHub repository could announce a
"new version" leading to a malicious installer on the real releases page.
That is the same trust as installing the app in the first place (see Out of
scope, code signing).

### T13. Settings files from another computer, watched folder

*Threat:* a crafted `.schtomy` file (import) writes files outside the app's
folder, smuggles a non-audio file in as intro/outro, exhausts memory, or
sets extreme values; a watched folder picks up files the user did not mean
to add.

*Mitigations (`podcast_core::bundle`, `podcast_core::watch`):*
* The import reads at most 300 MB, refuses unknown formats and newer
  versions, and bounds every embedded file before decoding it (intro/outro
  100 MB, cover 10 MB, at most 200 patterns of at most 180 s).
* Files are written only to `<app data>/imported` under fixed names
  (`intro.<ext>`, `outro.<ext>`, `okladka.jpg`); the original name only
  contributes a known audio extension, anything else becomes `.bin`, so
  `../` or odd names cannot escape. The cover must start with a JPEG
  signature. Imported intro/outro are later ffmpeg inputs with the same
  protections as T1 to T3. Settings go through `Settings::sanitized`.
* Machine-specific folders (results, YouTube, watched) are never exported
  or imported. The user confirms the import first.
* The watched folder only adds media files directly in the chosen folder
  (no subfolders, no hidden files, not our own outputs) that appeared after
  it was chosen; they go through the same pipeline as dropped files.
  Tests: `imported_files_cannot_escape_or_lie`, `rejects_foreign_and_broken_files`,
  `baseline_is_not_added`.

### T14. yt-dlp update after a failed download

*Threat:* the app downloads and runs a program from the internet, so a
compromised yt-dlp GitHub account or release, a forged answer of the GitHub
API, a man in the middle or a broken download could make it run malicious
or corrupt code; the update could also be triggered without the user
knowing, or leave the YouTube download broken.

*Mitigations (`crates/podcast-core/src/ytdlp_update.rs`,
`update_youtube_tool` in `src-tauri/src/commands.rs`):*
* Nothing is checked at start. The button "Zaktualizuj narzędzie" appears
  only next to a failed YouTube download (errors `YouTube`,
  `YouTubeBlocked`), and the update runs only after the user confirms a
  dialog that says what will be downloaded from where.
* The release tag from `api.github.com/repos/yt-dlp/yt-dlp/releases/latest`
  must be a yt-dlp version (`NNNN.N.N[.N]`, digits only), so the download
  URLs always point to `https://github.com/yt-dlp/yt-dlp/releases/download/`
  (HTTPS only; sizes capped: 256 KB sums, 16 KB signature, 300 MB binary).
* `SHA2-256SUMS` must carry a valid OpenPGP signature (v4, RSA, SHA-256/512)
  by the yt-dlp signing key built into the app (fingerprint
  `AC0C BBE6 848D 6A87 3464 AF4E 57CF 6593 3B5A 7581`, RSA 4096,
  `public.key` of the yt-dlp repository). The verification is implemented
  with `num-bigint` (PKCS #1 v1.5) and tested against the real signature of
  2026.10.26 and with changed bytes (`real_release_signature_is_accepted_and_tampering_is_not`).
  Taking over the GitHub account is not enough to pass; the private key is.
* The binary for this system (`yt-dlp.exe`, `yt-dlp_macos`) is accepted only
  with the SHA-256 listed in the signed sums, written to a temporary file in
  `<app data>/tools` and renamed to `yt-dlp-<version>[.exe]`; the app bundle
  is never changed (macOS signature). It must run and report the expected
  version (`--ignore-config --version`), else it is deleted and the previous
  yt-dlp stays in use (`update_refuses_files_that_do_not_match_the_signed_sums`).
* A stored copy is used only when it is newer than the bundled yt-dlp, so
  an app update with a newer pinned yt-dlp takes over again. It runs with
  exactly the same arguments and restrictions as T11. Older copies are
  removed after a successful update.
* The HTTP client (`ureq`, rustls) uses the system certificates and sends a
  fixed user agent (`SChTomyAudio/<version>`), no cookies and no data about
  the user.

*Residual risk:* we trust the yt-dlp maintainers' signing key, as we trust
their releases pinned at build time (T8). A new yt-dlp release could have a
bug that breaks downloads; the app then shows the error again and a later
yt-dlp release or app update fixes it. Downloaded copies are not
re-verified at every start; someone who can write to the user's app data
folder can already change far more.

## Out of scope

* **Code signing / notarisation**: deliberately not done (no paid
  certificates). The macOS app is signed ad hoc without the hardened runtime
  (`hardenedRuntime: false`): the hardened runtime only matters for
  notarisation, and its library validation stops the bundled yt-dlp, which
  unpacks its own Python signed by the yt-dlp team. Users are told how to open the app past Gatekeeper and
  SmartScreen (INSTRUKCJA.md). This means users cannot verify the publisher
  from the OS; they should download only from this repository's Releases and
  may compare `SHA256SUMS.txt`.
* An attacker who already controls the user's account or machine.
* Content correctness of sermons (theology is not our threat model).
