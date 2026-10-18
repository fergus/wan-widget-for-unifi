#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod poller;

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    config::save_api_key(&key)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> widget_core::settings::Settings {
    config::load_settings(&app)
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    settings: widget_core::settings::Settings,
) -> Result<(), Vec<String>> {
    config::save_settings(&app, &settings)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![set_api_key, get_settings, save_settings])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(poller::run(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
