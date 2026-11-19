//! Watched folder: recordings that appear in a chosen folder are added to the
//! list automatically.
//!
//! Files already in the folder when it is chosen are not added (they are the
//! "seen" baseline). A new file is added only once its size and modification
//! time have not changed for [`STABLE_POLLS`] polls in a row (30 s at the
//! app's 5 s interval), so a recording that is still being copied or
//! recorded, even with short stalls on a network drive, is not picked up
//! half-written. The state is kept in a small JSON
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
    /// Each new file's (size, modification time) at the previous poll and
    /// how many polls in a row it has stayed the same (not saved).
    #[serde(skip)]
    pending: HashMap<PathBuf, ((u64, u64), u32)>,
}

/// Polls a new file must stay unchanged before it is added.
pub const STABLE_POLLS: u32 = 6;

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

    /// One poll with the folder's media files, their sizes and modification
    /// times (any monotonic number). Returns the files that are new and
    /// finished (unchanged for [`STABLE_POLLS`] polls), in name order; they
    /// are marked as seen. Files that disappeared are forgotten, so the saved
    /// state stays small.
    pub fn poll(&mut self, current: &[(PathBuf, u64, u64)]) -> Vec<PathBuf> {
        let present: BTreeSet<&PathBuf> = current.iter().map(|(p, _, _)| p).collect();
        self.seen.retain(|p| present.contains(p));
        self.pending.retain(|p, _| present.contains(p));
        let mut ready = Vec::new();
        for (path, size, mtime) in current {
            if self.seen.contains(path) {
                continue;
            }
            let stamp = (*size, *mtime);
            let stable = match self.pending.get(path) {
                Some((prev, n)) if *prev == stamp => n + 1,
                _ => 0,
            };
            if stable >= STABLE_POLLS && *size > 0 {
                self.pending.remove(path);
                self.seen.insert(path.clone());
                ready.push(path.clone());
            } else {
                self.pending.insert(path.clone(), (stamp, stable));
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

    /// Polls until something is ready (at most `max` polls).
    fn settle(w: &mut WatchState, files: &[(PathBuf, u64, u64)], max: u32) -> (u32, Vec<PathBuf>) {
        for n in 1..=max {
            let r = w.poll(files);
            if !r.is_empty() {
                return (n, r);
            }
        }
        (max, vec![])
    }

    #[test]
    fn baseline_is_not_added() {
        let mut w = WatchState::default();
        assert!(w.set_dir(Some(p("")), &[p("stare.mp3")]));
        assert!(
            !w.set_dir(Some(p("")), &[]),
            "same folder keeps the baseline"
        );
        for _ in 0..10 {
            assert!(w.poll(&[(p("stare.mp3"), 100, 1)]).is_empty());
        }
    }

    #[test]
    fn new_file_is_added_once_it_stops_changing() {
        let mut w = WatchState::default();
        w.set_dir(Some(p("")), &[]);
        let f = p("nowe kazanie.mp3");
        assert!(w.poll(&[(f.clone(), 10, 1)]).is_empty(), "first sight");
        assert!(w.poll(&[(f.clone(), 500, 2)]).is_empty(), "still growing");
        // a stall shorter than the window, then more data: not added
        for _ in 0..(STABLE_POLLS - 1) {
            assert!(w.poll(&[(f.clone(), 500, 2)]).is_empty(), "short stall");
        }
        assert!(w.poll(&[(f.clone(), 800, 3)]).is_empty(), "growing again");
        // unchanged for the whole window: added exactly once
        let (n, ready) = settle(&mut w, &[(f.clone(), 800, 3)], 20);
        assert_eq!(ready, std::slice::from_ref(&f));
        assert_eq!(n, STABLE_POLLS);
        assert!(w.poll(&[(f, 800, 3)]).is_empty(), "added only once");
        // the modification time alone also counts as a change
        let g = p("drugie.mp3");
        for _ in 0..STABLE_POLLS {
            assert!(w.poll(&[(g.clone(), 5, 1)]).is_empty());
        }
        assert!(w.poll(&[(g.clone(), 5, 2)]).is_empty(), "touched");
        // an empty file is never added
        let (_, ready) = settle(&mut w, &[(p("pusty.mp3"), 0, 1)], 20);
        assert!(ready.is_empty());
    }

    #[test]
    fn several_files_come_in_name_order_and_vanished_ones_are_forgotten() {
        let mut w = WatchState::default();
        w.set_dir(Some(p("")), &[]);
        let files = [(p("b.mp3"), 5, 1), (p("a.mp3"), 5, 1)];
        let (_, ready) = settle(&mut w, &files, 20);
        assert_eq!(ready, [p("a.mp3"), p("b.mp3")]);
        w.poll(&[(p("a.mp3"), 5, 1)]);
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
