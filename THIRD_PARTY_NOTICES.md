# Third-party notices

This application bundles the following third-party software.

## FFmpeg (ffmpeg and ffprobe executables)

* Source of the binaries: GitHub release `b6.1.1` of
  <https://github.com/eugeneware/ffmpeg-static>, pinned by SHA-256 in
  `scripts/ffmpeg-sources.json`.
  * Windows x64: static "essentials" build by gyan.dev (<https://www.gyan.dev/ffmpeg/builds/>), FFmpeg 6.1.1.
  * macOS arm64: static build by osxexperts.net (<https://www.osxexperts.net/>).
  * macOS x64: static build by evermeet.cx (<https://evermeet.cx/ffmpeg/>).
* Licence: the Windows build and the macOS builds include GPL components, so
  the executables are distributed under the **GNU General Public License
  version 3** (macOS builds: GPL v2 or later / v3 depending on enabled
  libraries). The exact licence text shipped by the build provider is
  included next to the application as `FFMPEG_LICENSE.txt`, build details as
  `FFMPEG_README.txt`.
* Corresponding source code: FFmpeg sources are available at
  <https://ffmpeg.org/download.html> (release archives at
  <https://ffmpeg.org/releases/>); the build providers publish their build
  configurations at the addresses above. On request we will provide the
  source code of the exact versions shipped with a given release of this
  application.
* FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.
  The ffmpeg and ffprobe programs are run as separate processes; this
  application does not link against FFmpeg libraries.

## Silero VAD

* Model: `silero_vad.onnx` v5.1.2 from the `silero-vad` package
  (<https://github.com/snakers4/silero-vad>), converted to a 16 kHz only
  graph by `tools/vad/prepare_silero_vad.py`. Embedded in the application.
* Licence: MIT. Copyright (c) 2020-present Silero Team. Full text in
  `crates/podcast-core/assets/SILERO_VAD_LICENSE`.

## Rust crates and JavaScript tooling

The application is built with Tauri (MIT/Apache-2.0), tract (MIT/Apache-2.0),
rustfft (MIT/Apache-2.0), id3 (MIT), serde (MIT/Apache-2.0), regex
(MIT/Apache-2.0), chrono (MIT/Apache-2.0), deunicode (BSD-3-Clause) and their
dependencies; see `Cargo.lock` for the full list and versions. Their licences
are permissive and are available in each crate's source distribution.
