//! Job list shared between the UI commands and the worker threads.

use podcast_core::pipeline::{ProcessResult, Stage};
use podcast_core::teaser::TeaserMatch;
use podcast_core::vad::SpeechProposal;
use podcast_core::CancelToken;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// YouTube link: the audio is being downloaded.
    Downloading,
    /// Added, analysis not finished yet.
    Analyzing,
    /// Ready, waiting for "Przetwórz wszystkie".
    Waiting,
    /// In the processing queue.
    Queued,
    Processing,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartSource {
    /// No start marker: the beginning is not cut.
    None,
    /// Set (or confirmed) by the user.
    Manual,
    /// A saved teaser pattern matched with confidence above the threshold.
    Pattern,
    /// Weak teaser match below the threshold (to be checked).
    PatternProposal,
    /// Voice activity detection proposal (to be checked).
    VadProposal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    pub message: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: u64,
    pub path: PathBuf,
    pub file_name: String,
    /// Full ID3 title: "<sermon_title> - <title_suffix>".
    pub title: String,
    /// Sermon title typed by the user (or read from the file name).
    pub sermon_title: String,
    /// Speaker and date from the file name ("Adam Szumorek - 2026.10.11").
    pub title_suffix: String,
    /// Planned output file name (before a possible "-2" for duplicates).
    pub output_name: String,
    /// YouTube link the recording comes from (canonical form).
    pub source_url: Option<String>,
    pub year: Option<i32>,
    pub duration: Option<f64>,
    pub status: Status,
    pub stage: Option<Stage>,
    pub progress: f64,
    pub start: Option<f64>,
    pub start_source: StartSource,
    pub end: Option<f64>,
    pub vad: Option<SpeechProposal>,
    pub teaser: Option<TeaserMatch>,
    /// Problem during analysis (the file can still be processed).
    pub analysis_note: Option<String>,
    pub result: Option<ProcessResult>,
    pub error: Option<ErrorView>,
    /// This recording was processed before (from `history.json`).
    #[serde(default)]
    pub duplicate: Option<DuplicateView>,
}

/// When and as what a recording was processed before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateView {
    /// Unix seconds.
    pub processed_at: i64,
    /// File name of the episode produced then.
    pub output: String,
}

impl From<&podcast_core::history::Entry> for DuplicateView {
    fn from(e: &podcast_core::history::Entry) -> Self {
        Self {
            processed_at: e.processed_at,
            output: e.output.clone(),
        }
    }
}

pub struct Job {
    pub view: JobView,
    pub cancel: CancelToken,
    pub log: String,
    /// Analysis must (re)run: probe + teaser search + VAD.
    pub needs_analysis: bool,
    pub analysis_running: bool,
    pub analysis_cancel: CancelToken,
    /// Incremented whenever the VAD result becomes obsolete (search window
    /// changed). `vad_done` is the generation the current result belongs to;
    /// the result is kept when only teaser patterns change.
    pub vad_generation: u64,
    pub vad_done: Option<u64>,
    /// A YouTube download for this job is running.
    pub download_running: bool,
}

impl Job {
    pub fn new(id: u64, path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parts = podcast_core::naming::title_parts(&path);
        let title = podcast_core::naming::join_title(&parts.sermon, &parts.suffix);
        let year = Some(podcast_core::naming::default_year(&path));
        let mut job = Self {
            view: JobView {
                id,
                path,
                file_name,
                title,
                output_name: String::new(),
                source_url: None,
                sermon_title: parts.sermon,
                title_suffix: parts.suffix,
                year,
                duration: None,
                status: Status::Analyzing,
                stage: None,
                progress: 0.0,
                start: None,
                start_source: StartSource::None,
                end: None,
                vad: None,
                teaser: None,
                analysis_note: None,
                result: None,
                error: None,
                duplicate: None,
            },
            cancel: CancelToken::new(),
            log: String::new(),
            needs_analysis: true,
            analysis_running: false,
            analysis_cancel: CancelToken::new(),
            vad_generation: 0,
            vad_done: None,
            download_running: false,
        };
        job.refresh_output_name();
        job
    }

    /// Chooses the start marker from the analysis results unless the user
    /// has set one by hand.
    /// A job for a YouTube video: downloaded first, then analysed.
    pub fn new_youtube(id: u64, video_id: &str) -> Self {
        let url = podcast_core::youtube::canonical_url(video_id);
        let mut job = Job::new(id, PathBuf::new());
        job.view.file_name = format!("YouTube: {url}");
        job.view.title = String::new();
        job.view.sermon_title = String::new();
        job.view.title_suffix = String::new();
        job.view.year = None;
        job.view.source_url = Some(url);
        job.view.status = Status::Downloading;
        job.view.output_name = String::new();
        job.needs_analysis = false;
        job
    }

    /// The YouTube download finished: switch to the downloaded file and
    /// prefill the title from the video (title; speaker and date from the
    /// description, else the upload date).
    pub fn downloaded(
        &mut self,
        path: PathBuf,
        info: &podcast_core::youtube::VideoInfo,
        known_speakers: &[String],
    ) {
        self.view.file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.view.path = path;
        self.view.sermon_title = info.title.trim().chars().take(200).collect();
        self.view.title_suffix = info.title_suffix(known_speakers);
        self.view.title =
            podcast_core::naming::join_title(&self.view.sermon_title, &self.view.title_suffix);
        self.view.year = info.sermon_date().map(|(y, _, _)| y);
        self.view.status = Status::Analyzing;
        self.view.progress = 0.0;
        self.view.error = None;
        self.needs_analysis = true;
        self.refresh_output_name();
    }

    /// Rebuilds a job from the list saved by the previous session. Work that
    /// was running is put back to "waiting"; files that disappeared become
    /// an error the user can see.
    pub fn restore(mut view: JobView) -> Self {
        let mut job = Job::new(view.id, PathBuf::new());
        let missing = !view.path.as_os_str().is_empty() && !view.path.is_file();
        view.stage = None;
        match view.status {
            Status::Processing | Status::Queued => {
                view.status = Status::Waiting;
                view.progress = 0.0;
            }
            Status::Downloading => view.progress = 0.0,
            _ => {}
        }
        if missing && view.status != Status::Done {
            view.status = Status::Error;
            view.error = Some(ErrorView {
                message: "Nie znaleziono tego nagrania na dysku (zostało przeniesione albo usunięte). Usuń je z listy i dodaj ponownie.".into(),
                details: view.path.display().to_string(),
            });
        }
        let analysed = view.duration.is_some() && view.status != Status::Analyzing;
        job.needs_analysis = !analysed && !missing && !view.path.as_os_str().is_empty();
        if analysed {
            // keep the VAD/teaser results of the previous session
            job.vad_done = Some(job.vad_generation);
        }
        job.view = view;
        job
    }

    /// True for a YouTube job whose audio has not been downloaded yet.
    pub fn needs_download(&self) -> bool {
        self.view.source_url.is_some() && self.view.path.as_os_str().is_empty()
    }

    /// Output file name requested for this job (episode title based).
    pub fn file_name(&self) -> Option<String> {
        podcast_core::naming::episode_file_name(&self.view.sermon_title, &self.view.title_suffix)
    }

    /// Recomputes the planned output file name shown in the list.
    pub fn refresh_output_name(&mut self) {
        self.view.output_name = self
            .file_name()
            .unwrap_or_else(|| podcast_core::naming::output_file_name(&self.view.path));
    }

    pub fn apply_suggestions(&mut self, threshold: f64) {
        if self.view.start_source == StartSource::Manual {
            return;
        }
        let v = &mut self.view;
        match (&v.teaser, &v.vad) {
            (Some(t), _) if t.confidence >= threshold => {
                v.start = Some(t.end);
                v.start_source = StartSource::Pattern;
            }
            (Some(t), _) => {
                v.start = Some(t.end);
                v.start_source = StartSource::PatternProposal;
            }
            (None, Some(p)) if p.start > 0.5 => {
                v.start = Some(p.start);
                v.start_source = StartSource::VadProposal;
            }
            _ => {
                v.start = None;
                v.start_source = StartSource::None;
            }
        }
    }
}

/// ID3 year: the date in the "speaker - date" suffix, else a date in the
/// original file name, else none (never a stale value).
pub fn year_for(suffix: &str, path: &std::path::Path) -> Option<i32> {
    podcast_core::naming::date_from_name(suffix)
        .map(|(y, _, _)| y)
        .or_else(|| {
            path.file_stem()
                .and_then(|n| podcast_core::naming::year_from_name(&n.to_string_lossy()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job::new(1, PathBuf::from("/k/30.11.2025-kazanie_audio.mp3"))
    }

    fn teaser(conf: f64) -> TeaserMatch {
        TeaserMatch {
            pattern_id: "p1".into(),
            name: "Seria".into(),
            offset: 0.0,
            end: 42.0,
            confidence: conf,
        }
    }

    #[test]
    fn restore_after_restart() {
        let dir = std::env::temp_dir().join(format!("schtomy-restore-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4");
        std::fs::write(&file, b"x").unwrap();
        let mut j = Job::new(7, file.clone());
        j.view.duration = Some(3600.0);
        j.view.status = Status::Processing;
        j.view.sermon_title = "Co mi wolno?".into();
        j.view.start = Some(42.0);
        j.view.start_source = StartSource::Manual;
        // through JSON, as written to queue.json
        let json = serde_json::to_string(&vec![j.view.clone()]).unwrap();
        let views: Vec<JobView> = serde_json::from_str(&json).unwrap();
        let r = Job::restore(views[0].clone());
        assert_eq!(r.view.status, Status::Waiting);
        assert_eq!(r.view.sermon_title, "Co mi wolno?");
        assert_eq!(r.view.start, Some(42.0));
        assert_eq!(r.view.start_source, StartSource::Manual);
        assert!(!r.needs_analysis, "analysis results are kept");
        // a file that disappeared
        std::fs::remove_file(&file).unwrap();
        let r = Job::restore(views[0].clone());
        assert_eq!(r.view.status, Status::Error);
        assert!(!r.needs_analysis);
        // not analysed yet: analysis runs again
        let mut v = views[0].clone();
        std::fs::write(&file, b"x").unwrap();
        v.status = Status::Analyzing;
        v.duration = None;
        assert!(Job::restore(v).needs_analysis);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn year_follows_the_suffix() {
        let p = PathBuf::from("/k/TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4");
        assert_eq!(year_for("Adam Szumorek - 2025.12.24", &p), Some(2025));
        // date removed from the suffix: back to the file name, not stale
        assert_eq!(year_for("Adam Szumorek", &p), Some(2026));
        assert_eq!(
            year_for("Adam Szumorek", &PathBuf::from("/d/x-yt-abc.m4a")),
            None
        );
    }

    #[test]
    fn youtube_job_lifecycle() {
        let mut j = Job::new_youtube(3, "dQw4w9WgXcQ");
        assert_eq!(j.view.status, Status::Downloading);
        assert!(j.needs_download());
        assert!(!j.needs_analysis);
        let info = podcast_core::youtube::VideoInfo {
            id: "dQw4w9WgXcQ".into(),
            title: "Co mi wolno?".into(),
            upload_date: Some("20261011".into()),
            uploader: None,
            channel: None,
            duration: None,
            description: Some("Kazanie wygłosił Adam Szumorek".into()),
        };
        j.downloaded(
            PathBuf::from("/d/2026-10-11-co-mi-wolno-yt-dQw4w9WgXcQ.m4a"),
            &info,
            &["Adam Szumorek".to_string()],
        );
        assert!(!j.needs_download());
        assert!(j.needs_analysis);
        assert_eq!(j.view.status, Status::Analyzing);
        assert_eq!(j.view.title, "Co mi wolno? - Adam Szumorek - 2026.10.11");
        assert_eq!(j.view.year, Some(2026));
        assert_eq!(
            j.view.output_name,
            "2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3"
        );
        assert_eq!(
            serde_json::to_value(&j.view).unwrap()["sourceUrl"],
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[test]
    fn title_parts_from_church_file_name() {
        let j = Job::new(
            2,
            PathBuf::from("/k/TOMY Piotrków 2026.10.11 - Adam Szumorek.mp4"),
        );
        assert_eq!(j.view.sermon_title, "");
        assert_eq!(j.view.title_suffix, "Adam Szumorek - 2026.10.11");
        assert_eq!(j.view.title, "Adam Szumorek - 2026.10.11");
        assert_eq!(j.view.year, Some(2026));
        assert_eq!(j.view.output_name, "2026-10-11-adam-szumorek-podcast.mp3");
    }

    #[test]
    fn defaults_from_file_name() {
        let j = job();
        assert_eq!(j.view.title, "30.11.2025 kazanie");
        assert_eq!(j.view.year, Some(2025));
        assert_eq!(j.view.file_name, "30.11.2025-kazanie_audio.mp3");
    }

    #[test]
    fn confident_pattern_wins() {
        let mut j = job();
        j.view.teaser = Some(teaser(0.9));
        j.view.vad = Some(SpeechProposal {
            start: 10.0,
            ratio: 0.9,
        });
        j.apply_suggestions(0.6);
        assert_eq!(j.view.start, Some(42.0));
        assert_eq!(j.view.start_source, StartSource::Pattern);
    }

    #[test]
    fn weak_pattern_is_a_proposal() {
        let mut j = job();
        j.view.teaser = Some(teaser(0.4));
        j.apply_suggestions(0.6);
        assert_eq!(j.view.start_source, StartSource::PatternProposal);
    }

    #[test]
    fn vad_proposal_and_none() {
        let mut j = job();
        j.view.vad = Some(SpeechProposal {
            start: 33.3,
            ratio: 0.8,
        });
        j.apply_suggestions(0.6);
        assert_eq!(j.view.start, Some(33.3));
        assert_eq!(j.view.start_source, StartSource::VadProposal);
        // speech from the very beginning: nothing to cut
        j.view.vad = Some(SpeechProposal {
            start: 0.0,
            ratio: 0.8,
        });
        j.apply_suggestions(0.6);
        assert_eq!(j.view.start_source, StartSource::None);
    }

    /// The UI (ui/js/app.js, editor.js) reads these exact camelCase keys.
    #[test]
    fn json_shape_used_by_the_ui() {
        use podcast_core::parse::Ebur128Summary;
        let mut j = job();
        j.view.teaser = Some(teaser(0.9));
        j.view.vad = Some(SpeechProposal {
            start: 1.0,
            ratio: 0.7,
        });
        j.view.result = Some(ProcessResult {
            output: PathBuf::from("/o/a-podcast.mp3"),
            loudness: Ebur128Summary {
                integrated: -16.0,
                lra: 5.0,
                true_peak: Some(-1.2),
            },
            duration: 60.0,
            trailing_silence_cut: Some(2.0),
            leading_silence_cut: None,
            loudnorm_mode: "linear".into(),
            warnings: vec![],
        });
        j.apply_suggestions(0.6);
        let v = serde_json::to_value(&j.view).unwrap();
        for key in [
            "id",
            "path",
            "fileName",
            "title",
            "duration",
            "status",
            "stage",
            "progress",
            "start",
            "startSource",
            "end",
            "analysisNote",
            "error",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["startSource"], "pattern");
        assert_eq!(v["status"], "analyzing");
        assert_eq!(v["teaser"]["name"], "Seria");
        assert!(
            v["teaser"]["confidence"].is_number()
                && v["teaser"]["end"].is_number()
                && v["teaser"]["offset"].is_number()
        );
        assert!(v["vad"]["start"].is_number());
        let r = &v["result"];
        assert_eq!(r["output"], "/o/a-podcast.mp3");
        assert_eq!(r["trailingSilenceCut"], 2.0);
        assert_eq!(r["loudness"]["truePeak"], -1.2);
        assert_eq!(r["loudness"]["integrated"], -16.0);
        assert!(r["warnings"].is_array());
        assert!(r["duration"].is_number());
    }

    #[test]
    fn manual_marker_is_never_replaced() {
        let mut j = job();
        j.view.start = Some(5.0);
        j.view.start_source = StartSource::Manual;
        j.view.teaser = Some(teaser(0.95));
        j.apply_suggestions(0.6);
        assert_eq!(j.view.start, Some(5.0));
    }
}
