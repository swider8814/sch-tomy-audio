//! Watched folder: recordings that appear in a chosen folder are added to the
//! list automatically.
//!
//! Files already in the folder when it is chosen are not added (they are the
//! "seen" baseline). A new file is added only once its size has stayed the
//! same between two polls, so a recording that is still being copied or
//! recorded is not picked up half-written. The state is kept in a small JSON
//! file, so files that arrive while the app is closed are added at the next
//! start.

use crate::settings::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchState {
    /// Folder the `seen` set belongs to.
    pub dir: Option<PathBuf>,
    /// Files already handled (baseline or added).
    pub seen: BTreeSet<PathBuf>,
    /// Size of each new file at the previous poll (not saved).
    #[serde(skip)]
    pending: HashMap<PathBuf, u64>,
}

impl WatchState {
    /// Switches to `dir`. When it differs from the current folder, every file
    /// in `current` becomes the baseline (not added). Returns true if changed.
    pub fn set_dir(&mut self, dir: Option<PathBuf>, current: &[PathBuf]) -> bool {
        if self.dir == dir {
            return false;
        }
        self.dir = dir;
        self.seen = current.iter().cloned().collect();
        self.pending.clear();
        true
    }

    /// One poll with the folder's media files and their sizes. Returns the
    /// files that are new and finished (size unchanged since the last poll),
    /// in name order; they are marked as seen. Files that disappeared are
    /// forgotten, so the saved state stays small.
    pub fn poll(&mut self, current: &[(PathBuf, u64)]) -> Vec<PathBuf> {
        let present: BTreeSet<&PathBuf> = current.iter().map(|(p, _)| p).collect();
        self.seen.retain(|p| present.contains(p));
        self.pending.retain(|p, _| present.contains(p));
        let mut ready = Vec::new();
        for (path, size) in current {
            if self.seen.contains(path) {
                continue;
            }
            match self.pending.get(path) {
                Some(prev) if *prev == *size && *size > 0 => {
                    self.pending.remove(path);
                    self.seen.insert(path.clone());
                    ready.push(path.clone());
                }
                _ => {
                    self.pending.insert(path.clone(), *size);
                }
            }
        }
        ready.sort();
        ready
    }

    pub fn load(path: &Path) -> WatchState {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        write_atomic(path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(format!("/Nagrania ąę/{s}"))
    }

    #[test]
    fn baseline_is_not_added() {
        let mut w = WatchState::default();
        assert!(w.set_dir(Some(p("")), &[p("stare.mp3")]));
        assert!(
            !w.set_dir(Some(p("")), &[]),
            "same folder keeps the baseline"
        );
        for _ in 0..3 {
            assert!(w.poll(&[(p("stare.mp3"), 100)]).is_empty());
        }
    }

    #[test]
    fn new_file_is_added_once_its_size_is_stable() {
        let mut w = WatchState::default();
        w.set_dir(Some(p("")), &[]);
        assert!(
            w.poll(&[(p("nowe kazanie.mp3"), 10)]).is_empty(),
            "first sight"
        );
        assert!(
            w.poll(&[(p("nowe kazanie.mp3"), 500)]).is_empty(),
            "still growing"
        );
        assert_eq!(
            w.poll(&[(p("nowe kazanie.mp3"), 500)]),
            [p("nowe kazanie.mp3")]
        );
        assert!(
            w.poll(&[(p("nowe kazanie.mp3"), 500)]).is_empty(),
            "added only once"
        );
        // an empty file is never added
        assert!(w.poll(&[(p("pusty.mp3"), 0)]).is_empty());
        assert!(w.poll(&[(p("pusty.mp3"), 0)]).is_empty());
    }

    #[test]
    fn several_files_come_in_name_order_and_vanished_ones_are_forgotten() {
        let mut w = WatchState::default();
        w.set_dir(Some(p("")), &[]);
        let files = [(p("b.mp3"), 5), (p("a.mp3"), 5)];
        w.poll(&files);
        assert_eq!(w.poll(&files), [p("a.mp3"), p("b.mp3")]);
        w.poll(&[(p("a.mp3"), 5)]);
        assert!(!w.seen.contains(&p("b.mp3")));
    }

    #[test]
    fn changing_the_folder_takes_a_new_baseline_and_state_persists() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Dane ąę").join("watch.json");
        let mut w = WatchState::default();
        w.set_dir(Some(p("A")), &[p("A/x.mp3")]);
        w.save(&f).unwrap();
        let mut back = WatchState::load(&f);
        assert_eq!(back.dir, Some(p("A")));
        assert!(back.seen.contains(&p("A/x.mp3")));
        assert!(back.set_dir(Some(p("B")), &[p("B/y.mp3")]));
        assert!(!back.seen.contains(&p("A/x.mp3")));
        assert!(back.set_dir(None, &[]));
        std::fs::write(&f, b"{broken").unwrap();
        assert_eq!(WatchState::load(&f), WatchState::default());
    }
}
