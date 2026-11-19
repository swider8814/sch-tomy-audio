// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod jobs;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let download_dir = app.path().download_dir().ok();
            let state = Arc::new(AppState::new(&config_dir, &data_dir, download_dir));
            state::start_workers(app.handle().clone(), state.clone());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::add_files,
            commands::add_youtube,
            commands::remove_job,
            commands::clear_finished,
            commands::set_title,
            commands::set_markers,
            commands::use_suggestion,
            commands::get_preview,
            commands::cancel_previews,
            commands::get_log,
            commands::save_pattern,
            commands::delete_pattern,
            commands::rename_pattern,
            commands::save_settings,
            commands::start_all,
            commands::retry_job,
            commands::cancel_all,
            commands::cancel_job,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Kill running ffmpeg processes and remove temporary files.
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    state.shutdown();
                }
            }
        });
}

#[cfg(test)]
mod capability_tests {
    /// The UI registers `onCloseRequested` (close guard while processing);
    /// Tauri then closes the window with `destroy()`, which needs this
    /// permission. Without it the X button silently does nothing.
    #[test]
    fn window_can_be_closed() {
        let caps: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let perms: Vec<&str> = caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .collect();
        assert!(perms.contains(&"core:window:allow-destroy"), "{perms:?}");
        let ui = include_str!("../../ui/js/app.js");
        assert!(ui.contains("onCloseRequested"));
    }
}
