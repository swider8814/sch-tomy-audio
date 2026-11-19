//! Job list shared between the UI commands and the worker threads.

use podcast_core::pipeline::{ProcessResult, Stage};
use podcast_core::teaser::TeaserMatch;
use podcast_core::vad::SpeechProposal;
use podcast_core::CancelToken;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    pub message: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: u64,
    pub path: PathBuf,
    pub file_name: String,
    pub title: String,
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
}

impl Job {
    pub fn new(id: u64, path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = podcast_core::naming::default_title(&path);
        let year = Some(podcast_core::naming::default_year(&path));
        Self {
            view: JobView {
                id,
                path,
                file_name,
                title,
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
            },
            cancel: CancelToken::new(),
            log: String::new(),
            needs_analysis: true,
            analysis_running: false,
            analysis_cancel: CancelToken::new(),
            vad_generation: 0,
            vad_done: None,
        }
    }

    /// Chooses the start marker from the analysis results unless the user
    /// has set one by hand.
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
