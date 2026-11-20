// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod jobs;
mod keep_awake;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Window position, size and maximised state are saved on exit and
        // restored at the next start (a position on a monitor that is gone
        // is not restored, so the window cannot open off screen).
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
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
            commands::read_link_file,
            commands::remove_job,
            commands::clear_finished,
            commands::set_title,
            commands::set_markers,
            commands::use_suggestion,
            commands::get_preview,
            commands::cancel_previews,
            commands::get_result_preview,
            commands::remove_speaker,
            commands::add_speakers,
            commands::export_settings,
            commands::import_settings,
            commands::get_log,
            commands::save_pattern,
            commands::delete_pattern,
            commands::rename_pattern,
            commands::save_settings,
            commands::start_all,
            commands::start_job,
            commands::retry_job,
            commands::update_youtube_tool,
            commands::youtube_tool_info,
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

    /// Opening links is allowed only for this project's releases page and an
    /// e-mail to the author, and
    /// the only remote host the webview may contact is the GitHub API.
    #[test]
    fn links_and_network_are_narrow() {
        let caps: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let open: Vec<&serde_json::Value> = caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["identifier"] == "opener:allow-open-url")
            .collect();
        assert_eq!(open.len(), 1);
        let urls: Vec<&str> = open[0]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["url"].as_str().unwrap())
            .collect();
        assert_eq!(
            urls,
            [
                "https://github.com/swider8814/sch-tomy-audio/releases*",
                "mailto:daniel.swiderski@proem.pl*"
            ]
        );
        assert!(!caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "opener:default" || p == "opener:allow-open-url"));

        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        let connect = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("connect-src"))
            .unwrap();
        let remote: Vec<&str> = connect
            .split_whitespace()
            .filter(|s| s.starts_with("https:") || s.starts_with("http:"))
            .filter(|s| *s != "http://ipc.localhost")
            .collect();
        assert_eq!(remote, ["https://api.github.com"]);
    }
}
