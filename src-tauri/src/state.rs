//! Application state, analysis and processing workers.

use crate::jobs::{ErrorView, Job, JobView, Status};
use podcast_core::pipeline::{self, Cut, JobInput};
use podcast_core::settings::Settings;
use podcast_core::teaser::{self, PatternMeta, PatternStore};
use podcast_core::{vad, CancelToken, CoreError, Tools};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Maximum number of processing threads (the setting picks 1 to 4).
const MAX_WORKERS: usize = 4;
const ANALYSIS_WORKERS: usize = 2;

pub type Patterns = Arc<Vec<(PatternMeta, Vec<f32>)>>;

pub struct Inner {
    pub jobs: Vec<Job>,
    pub settings: Settings,
    pub settings_note: Option<String>,
    pub next_id: u64,
    pub patterns: Patterns,
    pub shutting_down: bool,
}

pub struct AppState {
    inner: Mutex<Inner>,
    pub wake: Condvar,
    pub tools: Result<Tools, CoreError>,
    pub settings_path: PathBuf,
    pub store: PatternStore,
}

impl AppState {
    pub fn new(config_dir: &Path, data_dir: &Path) -> Self {
        let settings_path = config_dir.join("settings.json");
        let (settings, err) = Settings::load(&settings_path);
        let store = PatternStore::new(data_dir.join("patterns"));
        let patterns = Arc::new(store.load_all());
        Self {
            inner: Mutex::new(Inner {
                jobs: Vec::new(),
                settings,
                settings_note: err.map(|e| {
                    format!(
                        "Nie udało się odczytać zapisanych ustawień, przywrócono domyślne. ({})",
                        e.details
                    )
                }),
                next_id: 1,
                patterns,
                shutting_down: false,
            }),
            wake: Condvar::new(),
            tools: Tools::locate(),
            settings_path,
            store,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic in a worker must not take the whole UI down.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn reload_patterns(&self) {
        let patterns = Arc::new(self.store.load_all());
        let mut g = self.lock();
        g.patterns = patterns;
        // Search again in files whose start marker is not manual.
        for j in g.jobs.iter_mut() {
            let editable = matches!(
                j.view.status,
                Status::Analyzing | Status::Waiting | Status::Error | Status::Cancelled
            );
            if editable && j.view.start_source != crate::jobs::StartSource::Manual {
                j.needs_analysis = true;
            }
        }
        drop(g);
        self.wake.notify_all();
    }

    pub fn shutdown(&self) {
        let mut g = self.lock();
        g.shutting_down = true;
        for j in &g.jobs {
            j.cancel.cancel();
            j.analysis_cancel.cancel();
        }
        drop(g);
        self.wake.notify_all();
        // Give workers a moment to kill ffmpeg and remove temporary files.
        std::thread::sleep(Duration::from_millis(400));
    }
}

pub fn emit_job(app: &AppHandle, view: &JobView) {
    let _ = app.emit("job", view);
}

pub fn emit_jobs(app: &AppHandle, inner: &Inner) {
    let views: Vec<&JobView> = inner.jobs.iter().map(|j| &j.view).collect();
    let _ = app.emit("jobs", views);
}

pub fn start_workers(app: AppHandle, state: Arc<AppState>) {
    for _ in 0..ANALYSIS_WORKERS {
        let (a, s) = (app.clone(), state.clone());
        std::thread::spawn(move || analysis_worker(a, s));
    }
    for slot in 0..MAX_WORKERS {
        let (a, s) = (app.clone(), state.clone());
        std::thread::spawn(move || processing_worker(a, s, slot));
    }
}

fn friendly(e: &CoreError) -> ErrorView {
    ErrorView {
        message: e.user_message(),
        details: e.details.clone(),
    }
}

// ---------------------------------------------------------------- analysis

struct AnalysisTask {
    id: u64,
    path: PathBuf,
    need_probe: bool,
    need_vad: bool,
    window: f64,
    patterns: Patterns,
    cancel: CancelToken,
}

fn next_analysis(state: &AppState) -> Option<AnalysisTask> {
    let mut g = state.lock();
    loop {
        if g.shutting_down {
            return None;
        }
        let window = g.settings.search_window_s;
        let patterns = g.patterns.clone();
        if let Some(j) = g.jobs.iter_mut().find(|j| {
            j.needs_analysis
                && !j.analysis_running
                && !matches!(j.view.status, Status::Processing | Status::Done)
        }) {
            j.needs_analysis = false;
            j.analysis_running = true;
            j.analysis_cancel = CancelToken::new();
            return Some(AnalysisTask {
                id: j.view.id,
                path: j.view.path.clone(),
                need_probe: j.view.duration.is_none(),
                need_vad: !j.vad_attempted,
                window,
                patterns,
                cancel: j.analysis_cancel.clone(),
            });
        }
        g = state
            .wake
            .wait_timeout(g, Duration::from_millis(500))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

fn analysis_worker(app: AppHandle, state: Arc<AppState>) {
    while let Some(task) = next_analysis(&state) {
        let result = analyse(&state, &task);
        let mut g = state.lock();
        let threshold = g.settings.pattern_threshold;
        if let Some(j) = g.jobs.iter_mut().find(|j| j.view.id == task.id) {
            j.analysis_running = false;
            match result {
                Ok(a) => {
                    if let Some(d) = a.duration {
                        j.view.duration = Some(d);
                    }
                    if task.need_vad {
                        j.vad_attempted = true;
                        j.view.vad = a.vad;
                    }
                    j.view.teaser = a.teaser;
                    j.view.analysis_note = a.note;
                    j.apply_suggestions(threshold);
                    if j.view.status == Status::Analyzing {
                        j.view.status = Status::Waiting;
                    }
                }
                Err(e) if e.is_cancelled() => {}
                Err(e) => {
                    // The file cannot be read at all.
                    j.log.push_str(&format!("Analysis failed: {e}\n"));
                    j.view.status = Status::Error;
                    j.view.error = Some(friendly(&e));
                }
            }
            emit_job(&app, &j.view);
        }
        drop(g);
        state.wake.notify_all();
    }
}

struct Analysis {
    duration: Option<f64>,
    teaser: Option<teaser::TeaserMatch>,
    vad: Option<vad::SpeechProposal>,
    note: Option<String>,
}

fn analyse(state: &AppState, t: &AnalysisTask) -> Result<Analysis, CoreError> {
    let tools = state.tools.as_ref().map_err(Clone::clone)?;
    let mut out = Analysis {
        duration: None,
        teaser: None,
        vad: None,
        note: None,
    };
    if t.need_probe {
        out.duration = Some(podcast_core::probe::probe(tools, &t.path, &t.cancel)?.duration);
    }
    if !t.patterns.is_empty() {
        let signal = teaser::decode_search_window(tools, &t.path, t.window, &t.cancel)?;
        out.teaser = teaser::find_best_match(signal, &t.patterns, &t.cancel)?;
    }
    if t.need_vad {
        match vad::propose_start(tools, &t.path, t.window, &t.cancel) {
            Ok(p) => out.vad = p,
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => {
                out.note = Some(format!(
                    "Nie udało się wykryć początku mowy ({}).",
                    e.details.lines().next().unwrap_or_default()
                ))
            }
        }
    }
    Ok(out)
}

// -------------------------------------------------------------- processing

struct ProcessingTask {
    id: u64,
    input: JobInput,
    cancel: CancelToken,
    settings: Settings,
}

fn next_processing(app: &AppHandle, state: &AppState, slot: usize) -> Option<ProcessingTask> {
    let mut g = state.lock();
    loop {
        if g.shutting_down {
            return None;
        }
        let limit = g.settings.concurrency as usize;
        let active = g
            .jobs
            .iter()
            .filter(|j| j.view.status == Status::Processing)
            .count();
        if slot < limit && active < limit {
            let settings = g.settings.clone();
            if let Some(j) = g.jobs.iter_mut().find(|j| {
                j.view.status == Status::Queued && !j.needs_analysis && !j.analysis_running
            }) {
                j.view.status = Status::Processing;
                j.view.progress = 0.0;
                j.view.stage = None;
                j.view.error = None;
                j.view.result = None;
                j.cancel = CancelToken::new();
                j.log.clear();
                emit_job(app, &j.view);
                return Some(ProcessingTask {
                    id: j.view.id,
                    input: JobInput {
                        recording: j.view.path.clone(),
                        cut: Cut {
                            start: j.view.start,
                            end: j.view.end,
                        },
                        title: j.view.title.clone(),
                        year: j.view.year,
                    },
                    cancel: j.cancel.clone(),
                    settings,
                });
            }
        }
        g = state
            .wake
            .wait_timeout(g, Duration::from_millis(500))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

fn processing_worker(app: AppHandle, state: Arc<AppState>, slot: usize) {
    while let Some(task) = next_processing(&app, &state, slot) {
        let mut log = String::new();
        let result = match &state.tools {
            Err(e) => Err(e.clone()),
            Ok(tools) => {
                let mut last = Instant::now() - Duration::from_secs(1);
                let mut on_progress = |stage, p: f64| {
                    if last.elapsed() < Duration::from_millis(200) {
                        return;
                    }
                    last = Instant::now();
                    let mut g = state.lock();
                    if let Some(j) = g.jobs.iter_mut().find(|j| j.view.id == task.id) {
                        j.view.stage = Some(stage);
                        j.view.progress = p;
                        emit_job(&app, &j.view);
                    }
                };
                pipeline::process(
                    tools,
                    &task.settings,
                    &task.input,
                    &task.cancel,
                    &mut on_progress,
                    &mut log,
                )
            }
        };
        let mut g = state.lock();
        if let Some(j) = g.jobs.iter_mut().find(|j| j.view.id == task.id) {
            j.log = log;
            j.view.stage = None;
            match result {
                Ok(r) => {
                    j.view.status = Status::Done;
                    j.view.progress = 1.0;
                    j.view.result = Some(r);
                }
                Err(e) if e.is_cancelled() => {
                    j.view.status = Status::Cancelled;
                    j.view.progress = 0.0;
                }
                Err(e) => {
                    j.log.push_str(&format!("\nERROR: {e}\n"));
                    j.view.status = Status::Error;
                    j.view.error = Some(friendly(&e));
                }
            }
            emit_job(&app, &j.view);
        }
        drop(g);
        state.wake.notify_all();
    }
}
