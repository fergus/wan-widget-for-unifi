#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod poller;

use tauri::Manager;
use widget_core::settings::Settings;

#[tauri::command]
fn has_api_key() -> bool {
    config::has_api_key()
}

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    config::save_api_key(&key)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    config::load_settings(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), Vec<String>> {
    config::save_settings(&app, &settings)
}

/// `key` is optional: blank or missing means "use the stored key".
#[tauri::command]
async fn test_connection(settings: Settings, key: Option<String>) -> Result<String, String> {
    let key = key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(config::load_api_key)
        .ok_or("No API key entered or stored")?;
    poller::test_connection(&settings, &key).await
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("settings")
        .ok_or("settings window is missing")?;
    w.show().map_err(|e| e.to_string())?;
    w.set_focus().map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            has_api_key,
            set_api_key,
            get_settings,
            save_settings,
            test_connection,
            open_settings
        ])
        // Closing the settings window hides it so it can be reopened from the widget.
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(poller::run(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
