//! Application state, analysis and processing workers.

use crate::jobs::{ErrorView, Job, JobView, Status};
use podcast_core::pipeline::{self, Cut, JobInput};
use podcast_core::settings::Settings;
use podcast_core::teaser::{self, PatternMeta, PatternStore};
use podcast_core::youtube::{self, YtTools};
use podcast_core::{vad, CancelToken, CoreError, Tools};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
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
    pub yt: Result<YtTools, CoreError>,
    /// Default folder for YouTube audio (Downloads/SCh Tomy YouTube); the
    /// setting `youtube_dir` overrides it.
    pub default_youtube_dir: PathBuf,
    pub settings_path: PathBuf,
    pub store: PatternStore,
    /// Cancellation tokens of running preview decodes (waveform editor).
    previews: Mutex<Vec<CancelToken>>,
    /// Worker threads, joined on exit so their cleanup guards run.
    workers: Mutex<Vec<JoinHandle<()>>>,
    /// The job list of the previous session is restored from here.
    queue_path: PathBuf,
}

/// How long the app waits on exit for workers to remove temporary files.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(20);

impl AppState {
    pub fn new(config_dir: &Path, data_dir: &Path, download_dir: Option<PathBuf>) -> Self {
        let settings_path = config_dir.join("settings.json");
        let (settings, err) = Settings::load(&settings_path);
        let store = PatternStore::new(data_dir.join("patterns"));
        let patterns = Arc::new(store.load_all());
        let queue_path = data_dir.join("queue.json");
        let jobs: Vec<Job> = std::fs::read(&queue_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<JobView>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .map(Job::restore)
            .collect();
        let next_id = jobs.iter().map(|j| j.view.id).max().unwrap_or(0) + 1;
        Self {
            inner: Mutex::new(Inner {
                jobs,
                settings,
                settings_note: err.map(|e| {
                    format!(
                        "Nie udało się odczytać zapisanych ustawień, przywrócono domyślne. ({})",
                        e.details
                    )
                }),
                next_id,
                patterns,
                shutting_down: false,
            }),
            wake: Condvar::new(),
            tools: Tools::locate(),
            yt: YtTools::locate(),
            default_youtube_dir: download_dir
                .unwrap_or_else(|| data_dir.join("downloads"))
                .join("SCh Tomy YouTube"),
            settings_path,
            store,
            previews: Mutex::new(Vec::new()),
            workers: Mutex::new(Vec::new()),
            queue_path,
        }
    }

    /// Registers a token for a preview decode; cancelled by `cancel_previews`.
    pub fn preview_token(&self) -> CancelToken {
        let token = CancelToken::new();
        let mut p = self.previews.lock().unwrap_or_else(|e| e.into_inner());
        p.retain(|t| !t.is_cancelled());
        p.push(token.clone());
        token
    }

    pub fn finish_preview(&self, token: &CancelToken) {
        token.cancel();
        let mut p = self.previews.lock().unwrap_or_else(|e| e.into_inner());
        p.retain(|t| !t.is_cancelled());
    }

    pub fn cancel_previews(&self) {
        let mut p = self.previews.lock().unwrap_or_else(|e| e.into_inner());
        for t in p.drain(..) {
            t.cancel();
        }
    }

    /// Folder for YouTube downloads: the setting, or the default.
    pub fn youtube_dir(&self, settings: &Settings) -> PathBuf {
        settings
            .youtube_dir
            .clone()
            .unwrap_or_else(|| self.default_youtube_dir.clone())
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

    /// Writes the job list (views only) if it changed since `last`.
    pub fn save_queue(&self, last: &mut String) {
        let json = {
            let g = self.lock();
            let views: Vec<&JobView> = g.jobs.iter().map(|j| &j.view).collect();
            serde_json::to_string(&views).unwrap_or_default()
        };
        if json != *last
            && podcast_core::settings::write_atomic(&self.queue_path, json.as_bytes()).is_ok()
        {
            *last = json;
        }
    }

    pub fn shutdown(&self) {
        let mut g = self.lock();
        g.shutting_down = true;
        for j in &g.jobs {
            j.cancel.cancel();
            j.analysis_cancel.cancel();
        }
        drop(g);
        self.cancel_previews();
        self.wake.notify_all();
        // Wait until every worker has killed its ffmpeg process and its
        // cleanup guard has removed temporary and reserved output files.
        let handles: Vec<JoinHandle<()>> = self
            .workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        for h in handles {
            while !h.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if h.is_finished() {
                let _ = h.join();
            }
        }
        self.save_queue(&mut String::new());
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
    let mut handles = Vec::new();
    for _ in 0..ANALYSIS_WORKERS {
        let (a, s) = (app.clone(), state.clone());
        handles.push(std::thread::spawn(move || analysis_worker(a, s)));
    }
    for slot in 0..MAX_WORKERS {
        let (a, s) = (app.clone(), state.clone());
        handles.push(std::thread::spawn(move || processing_worker(a, s, slot)));
    }
    {
        let (a, s) = (app.clone(), state.clone());
        handles.push(std::thread::spawn(move || download_worker(a, s)));
    }
    {
        let s = state.clone();
        handles.push(std::thread::spawn(move || queue_saver(s)));
    }
    {
        let s = state.clone();
        handles.push(std::thread::spawn(move || keep_awake_worker(s)));
    }
    state
        .workers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(handles);
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
    /// `Some(generation)` when VAD must run for this generation.
    vad_generation: Option<u64>,
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
                && !j.needs_download()
                && !matches!(
                    j.view.status,
                    Status::Processing | Status::Done | Status::Downloading
                )
        }) {
            j.needs_analysis = false;
            j.analysis_running = true;
            j.analysis_cancel = CancelToken::new();
            return Some(AnalysisTask {
                id: j.view.id,
                path: j.view.path.clone(),
                need_probe: j.view.duration.is_none(),
                vad_generation: (j.vad_done != Some(j.vad_generation)).then_some(j.vad_generation),
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
                    // A result computed for an older search window is
                    // discarded; the newer request stays queued.
                    if let Some(gen) = task.vad_generation {
                        if gen == j.vad_generation {
                            j.vad_done = Some(gen);
                            j.view.vad = a.vad;
                        }
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
    if t.vad_generation.is_some() {
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
                        file_name: j.file_name(),
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

// ---------------------------------------------------------------- YouTube

struct DownloadTask {
    id: u64,
    url: String,
    cancel: CancelToken,
    dir: PathBuf,
}

fn next_download(state: &AppState) -> Option<DownloadTask> {
    let mut g = state.lock();
    loop {
        if g.shutting_down {
            return None;
        }
        if let Some(j) = g.jobs.iter_mut().find(|j| {
            j.view.status == Status::Downloading && j.needs_download() && !j.download_running
        }) {
            j.download_running = true;
            j.cancel = CancelToken::new();
            let task = DownloadTask {
                id: j.view.id,
                url: j.view.source_url.clone().unwrap_or_default(),
                cancel: j.cancel.clone(),
                dir: PathBuf::new(),
            };
            let dir = state.youtube_dir(&g.settings);
            return Some(DownloadTask { dir, ..task });
        }
        g = state
            .wake
            .wait_timeout(g, Duration::from_millis(500))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// Downloads YouTube audio one link at a time (YouTube dislikes parallel
/// downloads), then hands the file to the normal analysis.
fn download_worker(app: AppHandle, state: Arc<AppState>) {
    while let Some(task) = next_download(&state) {
        let mut log = format!("YouTube: {}\n", task.url);
        let result = (|| -> Result<(PathBuf, youtube::VideoInfo), CoreError> {
            let tools = state.yt.as_ref().map_err(Clone::clone)?;
            let id = youtube::parse_video_id(&task.url)?;
            let info = youtube::fetch_metadata(tools, &id, &task.cancel)?;
            log.push_str(&format!(
                "Title: {}\nUploaded: {:?}\n",
                info.title, info.upload_date
            ));
            let mut last = Instant::now() - Duration::from_secs(1);
            let mut on_progress = |p: f64| {
                if last.elapsed() < Duration::from_millis(300) {
                    return;
                }
                last = Instant::now();
                let mut g = state.lock();
                if let Some(j) = g.jobs.iter_mut().find(|j| j.view.id == task.id) {
                    j.view.progress = p;
                    emit_job(&app, &j.view);
                }
            };
            let path =
                youtube::download_audio(tools, &info, &task.dir, &task.cancel, &mut on_progress)?;
            Ok((path, info))
        })();
        let mut g = state.lock();
        if let Some(j) = g.jobs.iter_mut().find(|j| j.view.id == task.id) {
            j.download_running = false;
            match result {
                Ok((path, info)) => {
                    log.push_str(&format!("Saved: {}\n", path.display()));
                    j.log = log;
                    j.downloaded(path, &info);
                }
                Err(e) => {
                    j.log = format!("{log}\nERROR: {e}\n");
                    j.view.status = Status::Error;
                    j.view.progress = 0.0;
                    j.view.error = Some(if e.is_cancelled() {
                        ErrorView {
                            message: "Pobieranie przerwane. Kliknij „Spróbuj ponownie”, żeby pobrać jeszcze raz.".into(),
                            details: e.details.clone(),
                        }
                    } else {
                        friendly(&e)
                    });
                }
            }
            emit_job(&app, &j.view);
        }
        drop(g);
        state.wake.notify_all();
    }
}

/// Saves the job list every few seconds when it changed, so titles, markers
/// and results survive closing the app (or a crash).
fn queue_saver(state: Arc<AppState>) {
    let mut last = String::new();
    loop {
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(250));
            if state.lock().shutting_down {
                return;
            }
        }
        state.save_queue(&mut last);
    }
}

/// True while anything is processed, queued or downloaded.
pub fn queue_busy(inner: &Inner) -> bool {
    inner.jobs.iter().any(|j| {
        matches!(
            j.view.status,
            Status::Processing | Status::Queued | Status::Downloading
        )
    })
}

/// Holds the "do not sleep" request while the queue is busy.
fn keep_awake_worker(state: Arc<AppState>) {
    let mut awake = crate::keep_awake::Awake::new();
    let mut on = false;
    loop {
        let (busy, stop) = {
            let g = state.lock();
            (queue_busy(&g), g.shutting_down)
        };
        if stop {
            awake.set(false);
            return;
        }
        if busy != on {
            awake.set(busy);
            on = busy;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}
