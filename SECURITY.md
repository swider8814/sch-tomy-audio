# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub: open the
repository's **Security** tab and choose **Report a vulnerability** (private
vulnerability reporting). If that is not available, contact the repository
owner directly and do not open a public issue with exploit details.

Include the affected version, the platform (Windows/macOS), steps to
reproduce and, if possible, a sample input file. We aim to acknowledge reports
within 7 days. The app has no server side, so fixes are shipped as new
releases.

## Scope

This is a local desktop application without accounts, servers or telemetry.
The threat model is in [docs/threat-model.md](docs/threat-model.md).

## Security review guidelines

When reviewing changes, check in particular:

1. **Process execution.** ffmpeg/ffprobe (and in stage 2, yt-dlp) must be
   started with `std::process::Command` and an argument list, never through a
   shell, `cmd /c` or string concatenation. Input files are passed with
   `podcast_core::ffmpeg::input_arg` (absolute path + `file:` prefix) and
   `-protocol_whitelist file`. No user-controlled text may end up inside a
   filter graph string except validated numbers.
2. **File system writes.** Output names come only from `naming::slugify`
   (`[a-z0-9-]`), are reserved with `create_new` (no overwrite), written to a
   temporary file in the target folder and renamed. Nothing may ever write to
   the input file. Pattern ids are validated (`[A-Za-z0-9]`) before being
   used in paths.
3. **Untrusted media.** Input files may be crafted to exploit decoders. Keep
   decoding inside the ffmpeg child process; do not add in-process media
   parsers for untrusted input. Bound memory use (PCM decoding is limited to
   the search window; stderr capture is capped).
4. **Frontend.** Text from files (names, titles, logs) must be inserted with
   `textContent` (`h()` helper), never `innerHTML`. Keep the CSP in
   `tauri.conf.json` strict and the capability list in
   `src-tauri/capabilities/default.json` minimal.
5. **Supply chain.** Bundled binaries and models are pinned by URL and
   SHA-256 (`scripts/ffmpeg-sources.json`, `vad::MODEL_SHA256`). GitHub Actions
   are pinned by commit SHA, the default token is read-only and only the
   release job gets `contents: write`. `Cargo.lock` and `package-lock.json`
   are committed and CI uses `--locked` / `npm ci`.
6. **Network.** No network access except the explicit YouTube download of
   stage 2. No telemetry, no update checks.
