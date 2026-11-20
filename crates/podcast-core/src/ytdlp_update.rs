//! Updating yt-dlp after a failed YouTube download.
//!
//! YouTube changes from time to time and an old yt-dlp then stops working.
//! When a download fails, the app offers to fetch the newest yt-dlp release;
//! nothing is checked or downloaded unless the user agrees (see
//! docs/threat-model.md, T14). The update:
//! * asks the GitHub API for the latest yt-dlp release; its tag must look
//!   like a yt-dlp version (`2026.11.15`, `2026.11.15.1`), so it can only
//!   ever name a release of the official `yt-dlp/yt-dlp` repository;
//! * downloads `SHA2-256SUMS` and its detached signature and verifies the
//!   signature with the yt-dlp signing key built into the app
//!   ([`SIGNING_KEY_FINGERPRINT`]); a compromised GitHub account or a
//!   changed file on the way cannot pass this without that private key;
//! * downloads the binary for this system and accepts it only when its
//!   SHA-256 is the one listed in the signed sums;
//! * writes it under a temporary name in the app's data folder (never inside
//!   the app bundle, which would break the macOS signature), runs it once
//!   with `--version` (killed after 90 s) and only then
//!   renames it to `yt-dlp-<version>[.exe]`; a file that does not start or
//!   reports another version is removed and the previous yt-dlp stays in use.
//!
//! The bundled yt-dlp is never changed. A newer installed copy is used
//! instead of it; an app update that bundles an even newer one wins again.

use crate::error::{CoreError, ErrorKind, Result};
use crate::ffmpeg::{run, CancelToken, StdoutMode};
use num_bigint::BigUint;
use sha2::{Digest, Sha256, Sha512};
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Fingerprint of the yt-dlp release signing key ("Simon Sawicki (yt-dlp
/// signing key)", RSA 4096, from `public.key` in the yt-dlp repository).
pub const SIGNING_KEY_FINGERPRINT: &str = "AC0C BBE6 848D 6A87 3464  AF4E 57CF 6593 3B5A 7581";
/// Modulus of that key (big-endian hex) and its public exponent.
const SIGNING_KEY_N: &str = "f4ac5f738c63c0b74b6196de42d5e6f371c0155fb35bd6ff9ea908e4f961197531135428375803399e002b947766fdf66acd480efa9018ac1eb418a7a0edf7d40dff92d1714a11975e44b61e01aaaa2a14a26d56a50e3bce04ecedbb75bad3b1cb113400a80ef04cfb88b765aa9a92fad0d21ccabdbb2f1aa681d37798dbd9da22cef9b3f49ca47f0be3e013d1f8cadcb00dad76cccc7a3e66429d538e3aee97644166754c71750f5f143a9df4b701298d8b0ce712628d6c698d185a26561edcd5a19bb590ece39d2e458eaff204724b2260c7ee93711e6eecfcf9dababa66405b4316bba4b7eda58439e9294986aff23fdc81ea1ff5fef673e84b8ba2a90e47a5b941f012bee167e96a2120ef92e7bee38f7d920f298c976bfd00ed0a2333080eac3f3ca334fe54290a92427e8a35d9ee01cd8e670169474a2bb7c320439823adcbc9ce905a41904e28057df4945d61c308ef5a5cd84fdde8ea15680632d094b2b6e4709916b9663bab9011cbfd391db48cdcae0353d7aab67c66895d5643c9d7b88f49809411be1e4f4e49f2e4bb71ab3726aaf58ef538c705baecd7804f10ef5f717c4a2ff08505bcd70d2bef02002b76aaa53289dc43fca271395f2faab327c10a162a96d7840c4dd027e92bae653a1d5c24e2227c284fb5a38ee14f28a76f7c7d463f86e7a9dcd39f32f10920c812d7f27cfb085fef4623b26493fcd98d";
const SIGNING_KEY_E: u32 = 65537;

const LATEST_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const SUMS: &str = "SHA2-256SUMS";
const MAX_API_BYTES: usize = 2 << 20;
const MAX_SUMS_BYTES: usize = 256 << 10;
const MAX_SIG_BYTES: usize = 16 << 10;
const MAX_BINARY_BYTES: usize = 300 << 20;

fn update_err(details: impl Into<String>) -> CoreError {
    CoreError::new(ErrorKind::ToolUpdate, details)
}

fn rejected(details: impl Into<String>) -> CoreError {
    CoreError::new(ErrorKind::ToolUpdateRejected, details)
}

// ------------------------------------------------------------------ versions

/// A yt-dlp version: `2026.10.26` or `2026.10.26.1`, compared by numbers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(Vec<u32>);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let parts: Vec<&str> = s.trim().split('.').collect();
        if !(3..=4).contains(&parts.len()) || parts[0].len() != 4 {
            return None;
        }
        parts
            .iter()
            .map(|p| {
                (!p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| p.parse().ok())
                    .flatten()
            })
            .collect::<Option<Vec<u32>>>()
            .map(Version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // keep the leading zeros of yt-dlp's month and day ("2026.01.05")
        let s: Vec<String> = self
            .0
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if i == 1 || i == 2 {
                    format!("{n:02}")
                } else {
                    n.to_string()
                }
            })
            .collect();
        f.write_str(&s.join("."))
    }
}

/// Version of the yt-dlp bundled with this build (pinned in
/// `scripts/ffmpeg-sources.json`).
pub fn bundled_version() -> Option<Version> {
    const SOURCES: &str = include_str!("../../../scripts/ffmpeg-sources.json");
    let marker = "yt-dlp/releases/download/";
    let at = SOURCES.find(marker)? + marker.len();
    Version::parse(SOURCES[at..].split('/').next()?)
}

/// Release file of yt-dlp for this system (None where the app does not run).
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("yt-dlp.exe")
    } else if cfg!(target_os = "macos") {
        Some("yt-dlp_macos")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("yt-dlp_linux")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("yt-dlp_linux_aarch64")
    } else {
        None
    }
}

// ----------------------------------------------------------------- signature

/// Verifies a detached binary OpenPGP signature (v4, RSA, SHA-256/512, as
/// yt-dlp publishes them) of `data` with the built-in yt-dlp signing key.
pub fn verify_signature(data: &[u8], sig: &[u8]) -> Result<()> {
    let body = signature_packet(sig).ok_or_else(|| rejected("not an OpenPGP signature packet"))?;
    // version, type (0 = binary document), public-key algorithm (1 = RSA), hash
    if body.len() < 6 || body[0] != 4 || body[1] != 0 || body[2] != 1 {
        return Err(rejected(
            "unsupported signature (want v4 RSA over binary data)",
        ));
    }
    let hashed_len = u16::from_be_bytes([body[4], body[5]]) as usize;
    let trailer_end = 6 + hashed_len;
    let unhashed_at = trailer_end;
    if body.len() < unhashed_at + 2 {
        return Err(rejected("truncated signature"));
    }
    let unhashed_len = u16::from_be_bytes([body[unhashed_at], body[unhashed_at + 1]]) as usize;
    let mut p = unhashed_at + 2 + unhashed_len;
    if body.len() < p + 4 {
        return Err(rejected("truncated signature"));
    }
    let left16 = [body[p], body[p + 1]];
    p += 2;
    let bits = u16::from_be_bytes([body[p], body[p + 1]]) as usize;
    p += 2;
    let mpi = body
        .get(p..p + bits.div_ceil(8))
        .ok_or_else(|| rejected("truncated signature"))?;

    // hash of the data, then the hashed part of the packet and its trailer
    let mut trailer = body[..trailer_end].to_vec();
    trailer.extend_from_slice(&[4, 0xff]);
    trailer.extend_from_slice(&(trailer_end as u32).to_be_bytes());
    let (digest, prefix): (Vec<u8>, &[u8]) = match body[3] {
        8 => {
            let mut h = Sha256::new();
            h.update(data);
            h.update(&trailer);
            let prefix: &[u8] = &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ];
            (h.finalize().to_vec(), prefix)
        }
        10 => {
            let mut h = Sha512::new();
            h.update(data);
            h.update(&trailer);
            let prefix: &[u8] = &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ];
            (h.finalize().to_vec(), prefix)
        }
        other => return Err(rejected(format!("unsupported hash algorithm {other}"))),
    };
    if digest[..2] != left16 {
        return Err(rejected("signature does not match the file (hash prefix)"));
    }

    // RSA PKCS #1 v1.5: s^e mod n must be 00 01 FF..FF 00 DigestInfo
    let n = BigUint::parse_bytes(SIGNING_KEY_N.as_bytes(), 16).expect("valid key");
    let s = BigUint::from_bytes_be(mpi);
    if s >= n {
        return Err(rejected("signature value out of range"));
    }
    let k = SIGNING_KEY_N.len() / 2;
    let m = s.modpow(&BigUint::from(SIGNING_KEY_E), &n).to_bytes_be();
    let mut em = vec![0u8; k - m.len()];
    em.extend_from_slice(&m);
    let t_len = prefix.len() + digest.len();
    let mut expected = vec![0x00, 0x01];
    expected.resize(k - t_len - 1, 0xff);
    expected.push(0x00);
    expected.extend_from_slice(prefix);
    expected.extend_from_slice(&digest);
    if em != expected {
        return Err(rejected(
            "signature is not valid for the yt-dlp signing key",
        ));
    }
    Ok(())
}

/// Body of the first packet when it is a signature packet (tag 2).
fn signature_packet(data: &[u8]) -> Option<&[u8]> {
    let ctb = *data.first()?;
    if ctb & 0x80 == 0 {
        return None;
    }
    let (tag, len, header) = if ctb & 0x40 != 0 {
        let l0 = *data.get(1)? as usize;
        match l0 {
            0..=191 => (ctb & 0x3f, l0, 2),
            192..=223 => (
                ctb & 0x3f,
                ((l0 - 192) << 8) + *data.get(2)? as usize + 192,
                3,
            ),
            255 => (
                ctb & 0x3f,
                u32::from_be_bytes(data.get(2..6)?.try_into().ok()?) as usize,
                6,
            ),
            _ => return None, // partial lengths: not used for signatures
        }
    } else {
        let tag = (ctb >> 2) & 0x0f;
        match ctb & 3 {
            0 => (tag, *data.get(1)? as usize, 2),
            1 => (
                tag,
                u16::from_be_bytes(data.get(1..3)?.try_into().ok()?) as usize,
                3,
            ),
            2 => (
                tag,
                u32::from_be_bytes(data.get(1..5)?.try_into().ok()?) as usize,
                5,
            ),
            _ => return None,
        }
    };
    (tag == 2).then(|| data.get(header..header + len)).flatten()
}

/// SHA-256 of `file` from a `SHA2-256SUMS` text (`<hex>  <name>` lines).
pub fn checksum_for(sums: &str, file: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        if name != file || hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(out)
    })
}

// ------------------------------------------------------------- installation

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// Folder for downloaded yt-dlp copies inside the app's data folder.
pub fn tools_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tools")
}

fn file_name(v: &Version) -> String {
    format!("yt-dlp-{v}{EXE}")
}

/// The newest yt-dlp stored in `dir` and its version.
pub fn installed(dir: &Path) -> Option<(PathBuf, Version)> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            let v = Version::parse(name.strip_prefix("yt-dlp-")?.strip_suffix(EXE)?)?;
            e.path().is_file().then(|| (e.path(), v))
        })
        .max_by(|a, b| a.1.cmp(&b.1))
}

/// The stored yt-dlp to use instead of the bundled one: only when it is
/// newer than the bundled version.
pub fn preferred(dir: &Path, bundled: Option<&Version>) -> Option<(PathBuf, Version)> {
    installed(dir).filter(|(_, v)| bundled.is_none_or(|b| v > b))
}

/// Stores `bytes` as yt-dlp `version` after checking them against the
/// signed SHA-256. The file is written under a temporary name (ignored by
/// [`installed`]), checked with `check` (a test run) and only then renamed,
/// so a broken or unchecked download is never picked up, not even after a
/// crash. The temporary file is removed on every failure.
pub fn install(
    dir: &Path,
    version: &Version,
    bytes: &[u8],
    sha256: &[u8; 32],
    check: &dyn Fn(&Path) -> Result<()>,
) -> Result<PathBuf> {
    if Sha256::digest(bytes).as_slice() != sha256 {
        return Err(rejected(
            "downloaded yt-dlp does not match the signed SHA-256",
        ));
    }
    std::fs::create_dir_all(dir)?;
    let target = dir.join(file_name(version));
    // keeps the system's executable suffix, so Windows can run it for the check
    let part = TempFile(dir.join(format!(".part-{}", file_name(version))));
    std::fs::write(&part.0, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part.0, std::fs::Permissions::from_mode(0o755))?;
    }
    check(&part.0)?;
    std::fs::rename(&part.0, &target)?;
    std::mem::forget(part);
    Ok(target)
}

/// Removes the file when dropped (every early return of [`install`]).
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Removes the other stored copies (best effort: one may still be running).
pub fn remove_others(dir: &Path, keep: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ours = name.starts_with("yt-dlp-") || name.starts_with(".part-yt-dlp-");
            if ours && e.path() != keep {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Longest time `--version` may take (the macOS build unpacks itself first).
const VERSION_TIMEOUT: Duration = Duration::from_secs(90);

/// Runs `path --version`; a program that does not answer in time is killed.
pub fn tool_version(path: &Path) -> Result<Version> {
    let args: Vec<OsString> = vec!["--ignore-config".into(), "--version".into()];
    let cancel = CancelToken::new();
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let watchdog = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            if finished.recv_timeout(VERSION_TIMEOUT)
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                cancel.cancel();
            }
        })
    };
    let out = run(path, &args, &cancel, StdoutMode::Capture);
    drop(done);
    let _ = watchdog.join();
    let out = out.map_err(|e| {
        if e.is_cancelled() {
            update_err(format!(
                "{} --version did not finish in time",
                path.display()
            ))
        } else {
            update_err(format!("{}: {e}", path.display()))
        }
    })?;
    let text = String::from_utf8_lossy(&out.stdout);
    Version::parse(text.trim())
        .ok_or_else(|| update_err(format!("unexpected --version output: {text}")))
}

// ------------------------------------------------------------------ network

/// Downloads `url`, at most `max` bytes.
pub type Fetch<'a> = &'a dyn Fn(&str, usize) -> Result<Vec<u8>>;

/// HTTPS GET with the system's certificates and proxy settings from the
/// environment, a fixed user agent and no cookies.
pub fn http_get(url: &str, max: usize) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("SChTomyAudio/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .try_proxy_from_env(true)
        .build();
    let resp = agent
        .get(url)
        .set(
            "Accept",
            "application/vnd.github+json, application/octet-stream",
        )
        .call()
        .map_err(|e| update_err(format!("{url}: {e}")))?;
    let mut body = Vec::new();
    resp.into_reader()
        .take(max as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| update_err(format!("{url}: {e}")))?;
    if body.len() > max {
        return Err(update_err(format!("{url}: larger than {max} bytes")));
    }
    Ok(body)
}

/// Result of [`update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A newer yt-dlp was installed at `path`.
    Updated { to: Version, path: PathBuf },
    /// `current` is already the newest release.
    UpToDate { version: Version },
}

/// Latest yt-dlp release tag from the GitHub API.
pub fn latest_version(fetch: Fetch) -> Result<Version> {
    let body = fetch(LATEST_API, MAX_API_BYTES)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| update_err(format!("release JSON: {e}")))?;
    let tag = json["tag_name"].as_str().unwrap_or_default();
    Version::parse(tag).ok_or_else(|| update_err(format!("unexpected release tag {tag:?}")))
}

/// Installs the newest yt-dlp into `dir` unless `current` is already the
/// newest. Every check is described in the module documentation.
pub fn update(dir: &Path, current: Option<&Version>, fetch: Fetch) -> Result<Outcome> {
    let asset = asset_name().ok_or_else(|| update_err("no yt-dlp build for this system"))?;
    let latest = latest_version(fetch)?;
    if current.is_some_and(|c| *c >= latest) {
        return Ok(Outcome::UpToDate { version: latest });
    }
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{latest}");
    let sums = fetch(&format!("{base}/{SUMS}"), MAX_SUMS_BYTES)?;
    let sig = fetch(&format!("{base}/{SUMS}.sig"), MAX_SIG_BYTES)?;
    verify_signature(&sums, &sig)?;
    let sums = String::from_utf8_lossy(&sums);
    let sha = checksum_for(&sums, asset)
        .ok_or_else(|| rejected(format!("{asset} is not listed in the signed {SUMS}")))?;
    let bytes = fetch(&format!("{base}/{asset}"), MAX_BINARY_BYTES)?;
    let check = |p: &Path| match tool_version(p) {
        Ok(v) if v == latest => Ok(()),
        Ok(v) => Err(update_err(format!(
            "the new yt-dlp reports version {v}, expected {latest}"
        ))),
        Err(e) => Err(update_err(format!(
            "the new yt-dlp does not run: {}",
            e.details
        ))),
    };
    let path = install(dir, &latest, &bytes, &sha, &check)?;
    remove_others(dir, &path);
    Ok(Outcome::Updated { to: latest, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ytdlp");

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(Path::new(FIXTURES).join(name)).unwrap()
    }

    #[test]
    fn versions() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("2026.11.15") > v("2026.10.26"));
        assert!(v("2026.10.26.1") > v("2026.10.26"));
        assert!(v("2027.01.02") > v("2026.12.31"));
        assert_eq!(v("2026.01.05").to_string(), "2026.01.05");
        assert_eq!(v("2026.10.26.1").to_string(), "2026.10.26.1");
        for bad in [
            "",
            "nightly",
            "2026.10",
            "2026.10.26/../x",
            "v2026.10.26",
            "26.10.26",
            "2026.10.26.1.2",
            "2026.-1.26",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(bundled_version(), Some(v("2026.10.26")));
    }

    #[test]
    fn real_release_signature_is_accepted_and_tampering_is_not() {
        let sums = fixture("SHA2-256SUMS");
        let sig = fixture("SHA2-256SUMS.sig");
        verify_signature(&sums, &sig).unwrap();
        // one changed byte in the sums
        let mut bad = sums.clone();
        bad[3] ^= 1;
        assert_eq!(
            verify_signature(&bad, &sig).unwrap_err().kind,
            ErrorKind::ToolUpdateRejected
        );
        // one changed byte in the signature value
        let mut bad_sig = sig.clone();
        let last = bad_sig.len() - 1;
        bad_sig[last] ^= 1;
        assert_eq!(
            verify_signature(&sums, &bad_sig).unwrap_err().kind,
            ErrorKind::ToolUpdateRejected
        );
        // garbage and empty input
        assert!(verify_signature(&sums, b"").is_err());
        assert!(verify_signature(&sums, b"-----BEGIN PGP SIGNATURE-----").is_err());
        assert!(verify_signature(&sums, &sig[..40]).is_err());
    }

    #[test]
    fn checksums_from_the_sums_file() {
        let sums = String::from_utf8(fixture("SHA2-256SUMS")).unwrap();
        let hex = |b: [u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(
            hex(checksum_for(&sums, "yt-dlp.exe").unwrap()),
            "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d"
        );
        assert_eq!(
            hex(checksum_for(&sums, "yt-dlp_macos").unwrap()),
            "0f922c15ccc0b83eefa1c8bc9104e434318c4bf16e408e1ee93b7745a6efe0aa"
        );
        assert!(checksum_for(&sums, "yt-dlp_macos.zip").is_some());
        assert_eq!(checksum_for(&sums, "yt-dlp_mac"), None);
        assert_eq!(checksum_for(&sums, ""), None);
    }

    #[test]
    fn install_checks_the_hash_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("Dane ąę").join("tools");
        let v1 = Version::parse("2026.11.01").unwrap();
        let v2 = Version::parse("2026.11.15").unwrap();
        let bytes = b"#!/bin/sh\necho 2026.11.15\n".to_vec();
        let sha: [u8; 32] = Sha256::digest(&bytes).into();
        // wrong hash: nothing stored
        let ok = |_: &Path| -> Result<()> { Ok(()) };
        let err = install(&tools, &v2, b"something else", &sha, &ok).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ToolUpdateRejected);
        assert_eq!(installed(&tools), None);
        // a copy that fails the test run is never stored, nor left behind
        let broken = |p: &Path| -> Result<()> {
            assert!(
                p.is_file() && installed(p.parent().unwrap()).is_none(),
                "not visible yet"
            );
            Err(update_err("does not run"))
        };
        let err = install(&tools, &v2, &bytes, &sha, &broken).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ToolUpdate);
        assert_eq!(
            std::fs::read_dir(&tools).unwrap().count(),
            0,
            "temporary file removed"
        );
        let old = install(&tools, &v1, b"old", &Sha256::digest(b"old").into(), &ok).unwrap();
        let new = install(&tools, &v2, &bytes, &sha, &ok).unwrap();
        assert_eq!(std::fs::read(&new).unwrap(), bytes);
        assert_eq!(installed(&tools), Some((new.clone(), v2.clone())));
        // used only when newer than the bundled yt-dlp
        let bundled = Version::parse("2026.10.26").unwrap();
        assert_eq!(
            preferred(&tools, Some(&bundled)).map(|p| p.1),
            Some(v2.clone())
        );
        let newer_bundle = Version::parse("2026.12.01").unwrap();
        assert_eq!(preferred(&tools, Some(&newer_bundle)), None);
        remove_others(&tools, &new);
        assert!(!old.exists());
        assert!(new.exists());
        // unrelated files are ignored
        std::fs::write(tools.join("notes.txt"), b"x").unwrap();
        std::fs::write(tools.join(format!("yt-dlp-latest{EXE}")), b"x").unwrap();
        assert_eq!(installed(&tools).map(|p| p.1), Some(v2));
    }

    #[cfg(unix)]
    #[test]
    fn version_check_of_a_program_that_does_not_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("yt-dlp-fake");
        std::fs::write(&fake, "#!/bin/sh\necho not-a-version\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(tool_version(&fake).unwrap_err().kind, ErrorKind::ToolUpdate);
        std::fs::write(&fake, "#!/bin/sh\necho 2026.11.15\n").unwrap();
        assert_eq!(
            tool_version(&fake).unwrap(),
            Version::parse("2026.11.15").unwrap()
        );
        assert!(tool_version(&dir.path().join("missing")).is_err());
    }

    /// Serves the real signed sums of 2026.10.26 and a fake binary.
    fn fake_fetch<'a>(
        binary: &'a [u8],
        sums: &'a [u8],
    ) -> impl Fn(&str, usize) -> Result<Vec<u8>> + 'a {
        move |url: &str, max: usize| {
            let body = if url == LATEST_API {
                br#"{"tag_name":"2026.10.26","name":"yt-dlp 2026.10.26"}"#.to_vec()
            } else if url.ends_with("/SHA2-256SUMS") {
                sums.to_vec()
            } else if url.ends_with("/SHA2-256SUMS.sig") {
                fixture("SHA2-256SUMS.sig")
            } else {
                assert!(
                    url.starts_with(
                        "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/"
                    ),
                    "{url}"
                );
                binary.to_vec()
            };
            assert!(body.len() <= max);
            Ok(body)
        }
    }

    #[test]
    fn update_refuses_files_that_do_not_match_the_signed_sums() {
        if asset_name().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let sums = fixture("SHA2-256SUMS");
        // up to date: nothing downloaded besides the release info
        let current = Version::parse("2026.10.26").unwrap();
        let fetch = fake_fetch(b"", &sums);
        assert_eq!(
            update(dir.path(), Some(&current), &fetch).unwrap(),
            Outcome::UpToDate { version: current }
        );
        // a binary that is not the released one
        let older = Version::parse("2026.09.01").unwrap();
        let err = update(dir.path(), Some(&older), &fetch).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ToolUpdateRejected, "{err}");
        assert_eq!(installed(dir.path()), None);
        // sums changed on the way (e.g. the binary's hash replaced)
        let mut forged = sums.clone();
        forged[0] = if forged[0] == b'0' { b'1' } else { b'0' };
        let fetch = fake_fetch(b"", &forged);
        let err = update(dir.path(), Some(&older), &fetch).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ToolUpdateRejected, "{err}");
        assert_eq!(installed(dir.path()), None);
        // a release tag that is not a version never becomes a URL
        let bad_tag = |url: &str, _: usize| -> Result<Vec<u8>> {
            assert_eq!(url, LATEST_API);
            Ok(br#"{"tag_name":"2026.10.26/../../evil"}"#.to_vec())
        };
        assert_eq!(
            update(dir.path(), None, &bad_tag).unwrap_err().kind,
            ErrorKind::ToolUpdate
        );
    }

    /// Real download from GitHub; run with
    /// `SCHTOMY_NET_TESTS=1 cargo test -p podcast-core real_update -- --nocapture`.
    #[test]
    fn real_update_from_github() {
        if std::env::var_os("SCHTOMY_NET_TESTS").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let old = Version::parse("2020.01.01").unwrap();
        // SCHTOMY_NET_TAG=2026.10.26 answers the release lookup locally
        // (for networks that block api.github.com); the rest is real
        let tag = std::env::var("SCHTOMY_NET_TAG").ok();
        let fetch = |url: &str, max: usize| match (&tag, url == LATEST_API) {
            (Some(t), true) => Ok(format!(r#"{{"tag_name":"{t}"}}"#).into_bytes()),
            _ => http_get(url, max),
        };
        let out = update(dir.path(), Some(&old), &fetch).unwrap();
        println!("{out:?}");
        let Outcome::Updated { to, path } = out else {
            panic!()
        };
        assert_eq!(tool_version(&path).unwrap(), to);
    }
}
