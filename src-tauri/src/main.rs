#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod pin;
mod poller;
mod tray;
mod window_state;

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
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
fn get_settings(app: AppHandle) -> Settings {
    config::load_settings(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), Vec<String>> {
    config::save_settings(&app, &settings)
}

/// `key` is optional: blank or missing means "use the stored key".
#[tauri::command]
async fn test_connection(
    app: AppHandle,
    settings: Settings,
    key: Option<String>,
) -> Result<String, String> {
    let key = key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(config::load_api_key)
        .ok_or("No API key entered or stored")?;
    let pin_path = pin::path(&app).unwrap_or_default();
    poller::test_connection(&settings, &key, &pin_path).await
}

/// Forgets the stored gateway certificate pin, so the next certificate is trusted.
#[tauri::command]
fn forget_pin(app: AppHandle) {
    pin::forget(&app);
}

pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("settings")
        .ok_or("settings window is missing")?;
    w.show().map_err(|e| e.to_string())?;
    w.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

/// Reads the real OS state (registry Run key) rather than a saved flag.
/// Latest monthly and IP values, so the widget can show them straight after loading.
#[tauri::command]
fn latest_snapshot(latest: tauri::State<Arc<poller::Latest>>) -> poller::Snapshot {
    latest.snapshot()
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    tray: tauri::State<tray::TrayState>,
    enabled: bool,
) -> Result<(), String> {
    let al = app.autolaunch();
    let result = if enabled { al.enable() } else { al.disable() };
    result.map_err(|e| e.to_string())?;
    let _ = tray.autostart.set_checked(enabled);
    Ok(())
}

#[tauri::command]
fn get_pinned(app: AppHandle) -> bool {
    window_state::pinned(&app)
}

#[tauri::command]
fn set_pinned(app: AppHandle, tray: tauri::State<tray::TrayState>, pinned: bool) {
    window_state::set_pinned(&app, pinned);
    if let Some(w) = app.get_webview_window("main") {
        window_state::apply_pin(&w, pinned);
    }
    let _ = tray.pin.set_checked(pinned);
}

fn main() {
    tauri::Builder::default()
        // Must be the first plugin. A second launch exits and brings this widget forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            has_api_key,
            set_api_key,
            get_settings,
            save_settings,
            test_connection,
            open_settings,
            get_autostart,
            set_autostart,
            get_pinned,
            set_pinned,
            latest_snapshot,
            forget_pin
        ])
        // Closing the settings window hides it so it can be reopened from the widget.
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            if window.label() == "main" {
                if let tauri::WindowEvent::Moved(pos) = event {
                    window_state::save(window.app_handle(), *pos);
                }
            }
        })
        .setup(|app| {
            // The main window is created hidden (tauri.conf.json) so it appears
            // at the restored spot instead of jumping there after it is shown.
            if let Some(main) = app.get_webview_window("main") {
                window_state::restore(&main);
                main.show()?;
            }
            tray::build(app.handle())?;
            app.manage(Arc::new(poller::Latest::default()));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(poller::run(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
