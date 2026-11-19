//! History of processed recordings, used to warn when the same recording is
//! added again ("To nagranie przetworzono już ...").
//!
//! A recording is recognised by a fingerprint of its content (size plus a
//! SHA-256 of its first and last MiB, so renamed or moved copies match and
//! reading stays fast even for long videos) and, for YouTube downloads, by
//! the video id. The list lives in a small JSON file in the app data folder.

use crate::settings::write_atomic;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// At most this many recordings are remembered (newest kept).
pub const MAX_ENTRIES: usize = 2000;
const CHUNK: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Fingerprint keys of the recording (`f:...`, `yt:...`).
    pub keys: Vec<String>,
    /// Processing time, Unix seconds (shown as a local date in the UI).
    pub processed_at: i64,
    /// File name of the episode that was produced.
    pub output: String,
}

/// Content fingerprint of a file: `f:<size>:<sha256 of first and last MiB>`.
pub fn file_key(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK as usize];
    let n = read_up_to(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    if size > 2 * CHUNK {
        f.seek(SeekFrom::Start(size - CHUNK))?;
        let n = read_up_to(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    } else if size > CHUNK {
        let n = read_up_to(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    let digest = h.finalize();
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    Ok(format!("f:{size}:{hex}"))
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = f.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

/// Current time in Unix seconds, for [`Entry::processed_at`].
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Key of a YouTube video id (already validated by `youtube::parse_video_id`).
pub fn youtube_key(video_id: &str) -> String {
    format!("yt:{video_id}")
}

/// The newest entry sharing any of `keys`.
pub fn find<'a>(list: &'a [Entry], keys: &[String]) -> Option<&'a Entry> {
    list.iter()
        .find(|e| e.keys.iter().any(|k| keys.contains(k)))
}

/// Adds an entry first; older entries with the same keys are replaced.
pub fn record(list: &mut Vec<Entry>, entry: Entry) {
    if entry.keys.is_empty() {
        return;
    }
    list.retain(|e| !e.keys.iter().any(|k| entry.keys.contains(k)));
    list.insert(0, entry);
    list.truncate(MAX_ENTRIES);
}

/// Reads the history; a missing or broken file gives an empty list.
pub fn load(path: &Path) -> Vec<Entry> {
    let mut list: Vec<Entry> = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    list.retain(|e| !e.keys.is_empty());
    for e in &mut list {
        e.output = e.output.chars().take(260).collect();
        e.keys.truncate(4);
    }
    list.truncate(MAX_ENTRIES);
    list
}

pub fn save(path: &Path, list: &[Entry]) -> io::Result<()> {
    let json = serde_json::to_vec(list).map_err(io::Error::other)?;
    write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, size: usize, seed: u8) {
        let data: Vec<u8> = (0..size)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect();
        std::fs::write(path, data).unwrap();
    }

    #[test]
    fn same_content_same_key_wherever_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("kazanie ąę.mp3");
        let b = dir.path().join("kopia").join("inna nazwa.mp3");
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        for size in [0, 10, 1_500_000, 5_000_000] {
            write(&a, size, 1);
            write(&b, size, 1);
            assert_eq!(file_key(&a).unwrap(), file_key(&b).unwrap(), "size {size}");
        }
        // different content or size: different key
        write(&b, 5_000_000, 2);
        assert_ne!(file_key(&a).unwrap(), file_key(&b).unwrap());
        write(&b, 5_000_001, 1);
        assert_ne!(file_key(&a).unwrap(), file_key(&b).unwrap());
        assert!(file_key(&dir.path().join("brak.mp3")).is_err());
    }

    #[test]
    fn record_find_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Dane ąę").join("history.json");
        let mut list = load(&p);
        assert!(list.is_empty());
        record(
            &mut list,
            Entry {
                keys: vec!["f:1:aa".into(), youtube_key("dQw4w9WgXcQ")],
                processed_at: 100,
                output: "a-podcast.mp3".into(),
            },
        );
        record(
            &mut list,
            Entry {
                keys: vec!["f:2:bb".into()],
                processed_at: 200,
                output: "b-podcast.mp3".into(),
            },
        );
        assert_eq!(
            find(&list, &[youtube_key("dQw4w9WgXcQ")]).unwrap().output,
            "a-podcast.mp3"
        );
        assert!(find(&list, &["f:3:cc".into()]).is_none());
        // processed again: one entry, the newest
        record(
            &mut list,
            Entry {
                keys: vec!["f:1:aa".into()],
                processed_at: 300,
                output: "a-podcast-2.mp3".into(),
            },
        );
        assert_eq!(list.len(), 2);
        assert_eq!(find(&list, &["f:1:aa".into()]).unwrap().processed_at, 300);
        record(
            &mut list,
            Entry {
                keys: vec![],
                processed_at: 1,
                output: String::new(),
            },
        );
        assert_eq!(list.len(), 2);
        save(&p, &list).unwrap();
        assert_eq!(load(&p), list);
        std::fs::write(&p, b"[{broken").unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn history_is_bounded() {
        let mut list = Vec::new();
        for i in 0..(MAX_ENTRIES + 50) {
            record(
                &mut list,
                Entry {
                    keys: vec![format!("f:{i}:x")],
                    processed_at: i as i64,
                    output: String::new(),
                },
            );
        }
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0].processed_at, (MAX_ENTRIES + 49) as i64);
    }
}
