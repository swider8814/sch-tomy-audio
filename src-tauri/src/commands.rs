//! Commands invoked from the UI.

use crate::jobs::{Job, JobView, StartSource, Status};
use crate::state::{emit_job, emit_jobs, AppState};
use podcast_core::settings::Settings;
use podcast_core::teaser::{self, PatternMeta};
use podcast_core::CancelToken;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, State};

type St<'a> = State<'a, Arc<AppState>>;

/// Extensions picked up when a whole folder is dropped.
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "mp4", "m4a", "webm", "mov", "wav", "aac", "ogg", "opus", "flac", "mkv", "wma", "m4v",
];

/// Preview lengths shown in the waveform editor, seconds.
const PREVIEW_HEAD_S: f64 = 180.0;
const PREVIEW_TAIL_S: f64 = 120.0;
const PREVIEW_RATE: u32 = 22_050;
/// Context played after the latest marker or proposal in the head preview.
const PREVIEW_MARGIN_S: f64 = 30.0;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    jobs: Vec<JobView>,
    settings: Settings,
    patterns: Vec<PatternMeta>,
    tools_error: Option<String>,
    settings_note: Option<String>,
    preview_head: f64,
    preview_tail: f64,
    /// Default folder for YouTube downloads (shown in the settings).
    default_youtube_dir: String,
}

fn not_found() -> String {
    "Nie znaleziono pliku na liście.".into()
}

fn with_job<R>(
    app: &AppHandle,
    state: &AppState,
    id: u64,
    f: impl FnOnce(&mut Job) -> Result<R, String>,
) -> Result<R, String> {
    let mut g = state.lock();
    let job = g
        .jobs
        .iter_mut()
        .find(|j| j.view.id == id)
        .ok_or_else(not_found)?;
    let r = f(job)?;
    emit_job(app, &job.view);
    drop(g);
    state.wake.notify_all();
    Ok(r)
}

fn editable(job: &Job) -> Result<(), String> {
    match job.view.status {
        Status::Processing | Status::Queued => {
            Err("Tego pliku nie można zmienić podczas obróbki.".into())
        }
        Status::Downloading => Err("Poczekaj, aż nagranie pobierze się z YouTube.".into()),
        _ if job.needs_download() => {
            Err("To nagranie nie zostało jeszcze pobrane z YouTube.".into())
        }
        _ => Ok(()),
    }
}

#[tauri::command]
pub fn get_state(state: St<'_>) -> StateView {
    let g = state.lock();
    StateView {
        jobs: g.jobs.iter().map(|j| j.view.clone()).collect(),
        settings: g.settings.clone(),
        patterns: state.store.list(),
        tools_error: state
            .tools
            .as_ref()
            .err()
            .map(|e| format!("{} ({})", e.user_message(), e.details)),
        settings_note: g.settings_note.clone(),
        preview_head: PREVIEW_HEAD_S,
        preview_tail: PREVIEW_TAIL_S,
        default_youtube_dir: state.default_youtube_dir.display().to_string(),
    }
}

fn expand(paths: Vec<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        let p = PathBuf::from(p);
        if p.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|f| f.is_file() && is_media(f) && !is_our_output(f))
                .collect();
            files.sort();
            out.extend(files);
        } else if p.is_file() {
            out.push(p);
        }
    }
    out
}

fn is_media(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.as_str()))
}

/// Text files that may hold a list of YouTube links.
pub fn is_link_list(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "txt" || e == "csv")
}

fn is_our_output(p: &Path) -> bool {
    p.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|n| {
            n.starts_with('.') || n.contains("-podcast.mp3") || n.contains("-podcast-")
        })
}

#[tauri::command]
pub fn add_files(app: AppHandle, state: St<'_>, paths: Vec<String>) -> usize {
    let files = expand(paths);
    let mut g = state.lock();
    let mut added = 0;
    for f in files {
        if g.jobs.iter().any(|j| j.view.path == f) {
            continue;
        }
        let id = g.next_id;
        g.next_id += 1;
        g.jobs.push(Job::new(id, f));
        added += 1;
    }
    emit_jobs(&app, &g);
    drop(g);
    state.wake.notify_all();
    added
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeAdded {
    added: usize,
    /// Repeated in the list or already on the job list.
    skipped: usize,
    /// "line N: text" of lines that are not YouTube video links.
    invalid: Vec<String>,
    /// Folder the audio is saved to.
    dir: String,
}

/// Adds YouTube links (one per line; a single link works too). Each video
/// is downloaded in list order, then processed like a dropped file.
#[tauri::command]
pub fn add_youtube(app: AppHandle, state: St<'_>, text: String) -> Result<YoutubeAdded, String> {
    if let Err(e) = &state.yt {
        // friendly text only; the technical cause is in e.details
        return Err(e.user_message());
    }
    let list = podcast_core::youtube::parse_link_list(&text);
    let invalid: Vec<String> = list
        .invalid
        .iter()
        .map(|(n, t)| format!("linia {n}: {t}"))
        .collect();
    if list.ids.is_empty() {
        return Err(if invalid.is_empty() {
            "Wklej link do filmu na YouTube.".into()
        } else {
            podcast_core::CoreError::new(podcast_core::ErrorKind::YouTubeLink, "").user_message()
        });
    }
    let mut g = state.lock();
    let mut added = 0;
    let mut skipped = list.duplicates;
    for id in &list.ids {
        let canonical = podcast_core::youtube::canonical_url(id);
        if g.jobs
            .iter()
            .any(|j| j.view.source_url.as_deref() == Some(canonical.as_str()))
        {
            skipped += 1;
            continue;
        }
        let job_id = g.next_id;
        g.next_id += 1;
        g.jobs.push(Job::new_youtube(job_id, id));
        added += 1;
    }
    emit_jobs(&app, &g);
    drop(g);
    state.wake.notify_all();
    Ok(YoutubeAdded {
        added,
        skipped,
        invalid,
        dir: state
            .youtube_dir(&state.lock().settings)
            .display()
            .to_string(),
    })
}

/// Reads a text file with links (for the YouTube dialog). Size limited.
#[tauri::command(async)]
pub fn read_link_file(path: String) -> Result<String, String> {
    const MAX: u64 = 1024 * 1024;
    let p = PathBuf::from(path);
    if !is_link_list(&p) {
        return Err("Wybierz plik tekstowy z linkami (.txt albo .csv).".into());
    }
    let meta =
        std::fs::metadata(&p).map_err(|_| "Nie udało się odczytać pliku z linkami.".to_string())?;
    if meta.len() > MAX {
        return Err("Plik z linkami jest za duży (maksymalnie 1 MB).".into());
    }
    let bytes =
        std::fs::read(&p).map_err(|_| "Nie udało się odczytać pliku z linkami.".to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[tauri::command]
pub fn remove_job(app: AppHandle, state: St<'_>, id: u64) {
    let mut g = state.lock();
    if let Some(j) = g.jobs.iter().find(|j| j.view.id == id) {
        j.cancel.cancel();
        j.analysis_cancel.cancel();
    }
    g.jobs.retain(|j| j.view.id != id);
    emit_jobs(&app, &g);
}

#[tauri::command]
pub fn clear_finished(app: AppHandle, state: St<'_>) {
    let mut g = state.lock();
    g.jobs.retain(|j| j.view.status != Status::Done);
    emit_jobs(&app, &g);
}

#[tauri::command]
pub fn set_title(
    app: AppHandle,
    state: St<'_>,
    id: u64,
    sermon: String,
    suffix: String,
) -> Result<(), String> {
    with_job(&app, &state, id, |j| {
        editable(j)?;
        let sermon: String = sermon.trim().chars().take(200).collect();
        let suffix: String = suffix.trim().chars().take(200).collect();
        let title = podcast_core::naming::join_title(&sermon, &suffix);
        // The ID3 year follows the date in the suffix; without one, the date
        // in the original file name (if any) is used, never a stale value.
        j.view.year = crate::jobs::year_for(&suffix, &j.view.path);
        j.view.sermon_title = sermon;
        j.view.title_suffix = suffix;
        j.refresh_output_name();
        if title != j.view.title {
            j.view.title = title;
            // The finished MP3 carries the old title: process it again.
            requeue_finished(j);
        }
        Ok(())
    })
}

/// A finished (or cancelled) job whose title or markers change goes back to
/// "waiting", so "Przetwórz wszystkie" writes a new file with the change.
fn requeue_finished(j: &mut Job) {
    if matches!(j.view.status, Status::Done | Status::Cancelled) {
        j.view.status = Status::Waiting;
        j.view.result = None;
        j.view.progress = 0.0;
    }
}

/// Checks markers against the recording; the same limits as
/// `pipeline::validate_cut`, so an accepted marker never fails later.
fn check_markers(
    start: Option<f64>,
    end: Option<f64>,
    duration: Option<f64>,
) -> Result<(), String> {
    if let (Some(s), Some(d)) = (start, duration) {
        if s >= d - 1.0 {
            return Err("Start musi być co najmniej sekundę przed końcem nagrania.".into());
        }
    }
    if let Some(e) = end {
        if e <= start.unwrap_or(0.0) + 1.0 {
            return Err("Koniec musi być co najmniej sekundę po starcie.".into());
        }
    }
    Ok(())
}

fn valid_time(t: Option<f64>, duration: Option<f64>) -> Result<Option<f64>, String> {
    match t {
        None => Ok(None),
        Some(v) if !v.is_finite() || v < 0.0 => Err("Czas musi być liczbą dodatnią.".into()),
        Some(v) if duration.is_some_and(|d| v > d) => Err("Czas jest dłuższy niż nagranie.".into()),
        Some(v) => Ok(Some(v)),
    }
}

/// Sets start/end markers by hand (`None` clears them).
#[tauri::command]
pub fn set_markers(
    app: AppHandle,
    state: St<'_>,
    id: u64,
    start: Option<f64>,
    end: Option<f64>,
) -> Result<(), String> {
    with_job(&app, &state, id, |j| {
        editable(j)?;
        let start = valid_time(start, j.view.duration)?;
        let end = valid_time(end, j.view.duration)?;
        check_markers(start, end, j.view.duration)?;
        j.view.start = start;
        j.view.start_source = if start.is_some() {
            StartSource::Manual
        } else {
            StartSource::None
        };
        j.view.end = end;
        requeue_finished(j);
        Ok(())
    })
}

/// Goes back to the automatic suggestion ("auto") for the start marker.
#[tauri::command]
pub fn use_suggestion(app: AppHandle, state: St<'_>, id: u64) -> Result<JobView, String> {
    let threshold = state.lock().settings.pattern_threshold;
    with_job(&app, &state, id, |j| {
        editable(j)?;
        j.view.start_source = StartSource::None;
        j.apply_suggestions(threshold);
        Ok(j.view.clone())
    })
}

fn job_path_and_duration(state: &AppState, id: u64) -> Result<(PathBuf, Option<f64>), String> {
    let g = state.lock();
    let j = g
        .jobs
        .iter()
        .find(|j| j.view.id == id)
        .ok_or_else(not_found)?;
    Ok((j.view.path.clone(), j.view.duration))
}

/// Length of the "head" preview: at least 3 minutes, longer when a marker or
/// an automatic proposal lies later (up to the search window), so every
/// proposal can be checked by listening.
pub fn head_preview_length(
    duration: f64,
    search_window: f64,
    points: impl IntoIterator<Item = f64>,
) -> f64 {
    let latest = points.into_iter().fold(0.0f64, f64::max);
    let limit = PREVIEW_HEAD_S.max(search_window + PREVIEW_MARGIN_S);
    PREVIEW_HEAD_S
        .max(latest + PREVIEW_MARGIN_S)
        .min(limit)
        .min(duration)
}

/// Lower sample rate for long previews keeps the WAV small.
fn preview_rate(len: f64) -> u32 {
    if len > 300.0 {
        11_025
    } else {
        PREVIEW_RATE
    }
}

/// WAV bytes of the beginning ("head") or the end ("tail") of a recording.
/// `length` (head only) comes from the editor, see `head_preview_length`.
#[tauri::command(async)]
pub fn get_preview(
    state: St<'_>,
    id: u64,
    part: String,
    length: Option<f64>,
) -> Result<tauri::ipc::Response, String> {
    let (path, duration, default_head) = {
        let g = state.lock();
        let j = g
            .jobs
            .iter()
            .find(|j| j.view.id == id)
            .ok_or_else(not_found)?;
        let points = [
            j.view.start,
            j.view.teaser.as_ref().map(|t| t.end),
            j.view.vad.map(|v| v.start),
        ];
        let default_head = j.view.duration.map(|d| {
            head_preview_length(d, g.settings.search_window_s, points.into_iter().flatten())
        });
        (j.view.path.clone(), j.view.duration, default_head)
    };
    let tools = state.tools.as_ref().map_err(|e| e.user_message())?;
    let duration = duration.ok_or("Plik jest jeszcze analizowany. Spróbuj za chwilę.")?;
    let (start, len) = if part == "tail" {
        let start = (duration - PREVIEW_TAIL_S).max(0.0);
        (start, duration - start)
    } else {
        let max =
            PREVIEW_HEAD_S.max(podcast_core::settings::MAX_SEARCH_WINDOW_S + PREVIEW_MARGIN_S);
        let len = length
            .filter(|l| l.is_finite())
            .or(default_head)
            .unwrap_or(PREVIEW_HEAD_S)
            .clamp(1.0, max);
        (0.0, len.min(duration))
    };
    let token = state.preview_token();
    let res = podcast_core::pcm::decode_wav(tools, &path, start, len, preview_rate(len), &token);
    state.finish_preview(&token);
    res.map(tauri::ipc::Response::new).map_err(|e| {
        if e.is_cancelled() {
            "Anulowano.".to_string()
        } else {
            format!(
                "{} ({})",
                e.user_message(),
                e.details.lines().next().unwrap_or("")
            )
        }
    })
}

/// Stops preview decodes (called when the editor closes).
#[tauri::command]
pub fn cancel_previews(state: St<'_>) {
    state.cancel_previews();
}

#[tauri::command]
pub fn get_log(state: St<'_>, id: u64) -> String {
    state
        .lock()
        .jobs
        .iter()
        .find(|j| j.view.id == id)
        .map(|j| j.log.clone())
        .unwrap_or_default()
}

/// Saves `[from, to]` of a job's recording as a named series teaser.
#[tauri::command(async)]
pub fn save_pattern(
    state: St<'_>,
    id: u64,
    name: String,
    from: f64,
    to: f64,
) -> Result<PatternMeta, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Podaj nazwę zajawki (np. nazwę serii).".into());
    }
    let (path, _) = job_path_and_duration(&state, id)?;
    let tools = state.tools.as_ref().map_err(|e| e.user_message())?;
    let len = to - from;
    if !(teaser::MIN_PATTERN_S..=teaser::MAX_PATTERN_S).contains(&len) {
        return Err(format!(
            "Zajawka musi mieć od {} do {} sekund (zaznaczono {} s).",
            teaser::MIN_PATTERN_S,
            teaser::MAX_PATTERN_S,
            podcast_core::pipeline::pl1(len)
        ));
    }
    let samples = teaser::extract_pattern(tools, &path, from, to, &CancelToken::new())
        .map_err(|e| e.user_message())?;
    let source = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = state
        .store
        .add(&name, &source, &samples)
        .map_err(|e| format!("{} ({})", e.user_message(), e.details))?;
    state.reload_patterns();
    Ok(meta)
}

#[tauri::command]
pub fn delete_pattern(state: St<'_>, id: String) -> Result<Vec<PatternMeta>, String> {
    state.store.remove(&id).map_err(|e| e.user_message())?;
    state.reload_patterns();
    Ok(state.store.list())
}

#[tauri::command]
pub fn rename_pattern(state: St<'_>, id: String, name: String) -> Result<Vec<PatternMeta>, String> {
    if name.trim().is_empty() {
        return Err("Nazwa nie może być pusta.".into());
    }
    state
        .store
        .rename(&id, &name)
        .map_err(|e| e.user_message())?;
    state.reload_patterns();
    Ok(state.store.list())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: St<'_>,
    settings: Settings,
) -> Result<Settings, String> {
    let s = settings.sanitized();
    s.save(&state.settings_path)
        .map_err(|e| format!("{} ({})", e.user_message(), e.details))?;
    let mut g = state.lock();
    let window_changed = g.settings.search_window_s != s.search_window_s;
    g.settings = s.clone();
    g.settings_note = None;
    let threshold = s.pattern_threshold;
    for j in g.jobs.iter_mut() {
        if matches!(
            j.view.status,
            Status::Processing | Status::Queued | Status::Done | Status::Downloading
        ) || j.needs_download()
        {
            continue;
        }
        if window_changed {
            j.vad_generation += 1;
            j.needs_analysis = true;
        } else {
            j.apply_suggestions(threshold);
        }
    }
    emit_jobs(&app, &g);
    drop(g);
    state.wake.notify_all();
    Ok(s)
}

/// Queues every waiting or cancelled file.
#[tauri::command]
pub fn start_all(app: AppHandle, state: St<'_>) -> usize {
    let mut g = state.lock();
    let mut n = 0;
    for j in g.jobs.iter_mut() {
        if matches!(j.view.status, Status::Waiting | Status::Cancelled) && !j.needs_download() {
            j.view.status = Status::Queued;
            j.view.progress = 0.0;
            n += 1;
        }
    }
    emit_jobs(&app, &g);
    drop(g);
    state.wake.notify_all();
    n
}

#[tauri::command]
pub fn retry_job(app: AppHandle, state: St<'_>, id: u64) -> Result<(), String> {
    with_job(&app, &state, id, |j| {
        if matches!(j.view.status, Status::Processing | Status::Queued) {
            return Ok(());
        }
        j.view.error = None;
        if j.needs_download() {
            j.view.status = Status::Downloading;
            j.view.progress = 0.0;
        } else if j.view.duration.is_none() {
            // reading the file failed: analyse again
            j.view.status = Status::Analyzing;
            j.needs_analysis = true;
        } else {
            j.view.status = Status::Queued;
        }
        Ok(())
    })
}

#[tauri::command]
pub fn cancel_all(app: AppHandle, state: St<'_>) {
    let mut g = state.lock();
    for j in g.jobs.iter_mut() {
        match j.view.status {
            Status::Queued => j.view.status = Status::Waiting,
            Status::Processing => j.cancel.cancel(),
            _ => {}
        }
    }
    emit_jobs(&app, &g);
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, state: St<'_>, id: u64) -> Result<(), String> {
    with_job(&app, &state, id, |j| {
        match j.view.status {
            Status::Queued => j.view.status = Status::Waiting,
            Status::Processing | Status::Downloading => j.cancel.cancel(),
            _ => {}
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_expansion_skips_outputs_and_other_files() {
        let dir = std::env::temp_dir().join(format!("schtomy-expand-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in [
            "b kazanie.mp3",
            "a film.MP4",
            "notatki.txt",
            "a-podcast.mp3",
            ".a.part.mp3",
        ] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let got: Vec<String> = expand(vec![dir.to_string_lossy().into_owned()])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, vec!["a film.MP4", "b kazanie.mp3"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn markers_follow_pipeline_limits() {
        assert!(check_markers(Some(99.5), None, Some(100.0)).is_err());
        assert!(check_markers(Some(100.0), None, Some(100.0)).is_err());
        assert!(check_markers(Some(98.9), None, Some(100.0)).is_ok());
        assert!(check_markers(Some(10.0), Some(10.5), Some(100.0)).is_err());
        assert!(check_markers(None, Some(0.5), Some(100.0)).is_err());
        assert!(check_markers(None, None, None).is_ok());
        // every accepted combination is also accepted by the pipeline
        for (s, e) in [
            (Some(98.9), None),
            (Some(10.0), Some(11.5)),
            (None, Some(2.0)),
        ] {
            check_markers(s, e, Some(100.0)).unwrap();
            podcast_core::pipeline::validate_cut(
                podcast_core::pipeline::Cut { start: s, end: e },
                100.0,
            )
            .unwrap();
        }
    }

    #[test]
    fn head_preview_covers_late_proposals() {
        assert_eq!(head_preview_length(3600.0, 300.0, []), 180.0);
        assert_eq!(head_preview_length(3600.0, 300.0, [42.0]), 180.0);
        assert_eq!(head_preview_length(3600.0, 300.0, [250.0]), 280.0);
        assert_eq!(head_preview_length(3600.0, 900.0, [880.0]), 910.0);
        // a manual marker beyond the window is capped
        assert_eq!(head_preview_length(3600.0, 300.0, [2000.0]), 330.0);
        assert_eq!(head_preview_length(100.0, 300.0, [90.0]), 100.0);
    }

    #[test]
    fn time_validation() {
        assert_eq!(valid_time(None, Some(10.0)), Ok(None));
        assert_eq!(valid_time(Some(5.0), Some(10.0)), Ok(Some(5.0)));
        assert!(valid_time(Some(-1.0), None).is_err());
        assert!(valid_time(Some(11.0), Some(10.0)).is_err());
        assert!(valid_time(Some(f64::NAN), None).is_err());
    }
}
