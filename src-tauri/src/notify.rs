//! Windows notifications (tauri-plugin-notification). What to send and when is decided
//! in `widget_core::alerts` and the poller; this only shows them and remembers the last
//! public IP, so a change while the PC was off is still noticed.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Shows a notification. Failures (notifications turned off in Windows, for example) are
/// only logged.
pub fn send(app: &AppHandle, title: &str, body: &str) {
    log::info!("notification: {title}. {body}");
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    last_public_ip: Option<String>,
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("notify-state.json"))
}

/// The public IP seen last time, from any earlier run.
pub fn last_public_ip(app: &AppHandle) -> Option<String> {
    state_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Saved>(&s).ok())
        .and_then(|s| s.last_public_ip)
}

pub fn save_public_ip(app: &AppHandle, ip: &str) {
    let Some(path) = state_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let saved = Saved {
        last_public_ip: Some(ip.to_string()),
    };
    if let Ok(json) = serde_json::to_string(&saved) {
        let _ = fs::write(path, json);
    }
}
