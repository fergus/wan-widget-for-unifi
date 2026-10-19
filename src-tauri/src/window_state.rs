//! Remembers the widget window position in window.json (app config dir).
//! Kept apart from settings.json so the settings form cannot overwrite it.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

#[derive(Serialize, Deserialize)]
struct Saved {
    x: i32,
    y: i32,
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("window.json"))
}

/// Moves the window to the saved spot if that spot is still on a connected monitor.
pub fn restore(window: &WebviewWindow) {
    let app = window.app_handle();
    let saved: Saved = match state_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(s) => s,
        None => return,
    };
    let on_screen = app.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        saved.x >= p.x
            && saved.x < p.x + s.width as i32
            && saved.y >= p.y
            && saved.y < p.y + s.height as i32
    });
    if on_screen {
        let _ = window.set_position(PhysicalPosition::new(saved.x, saved.y));
    }
}

pub fn save(app: &AppHandle, pos: PhysicalPosition<i32>) {
    // Minimized windows are reported far off-screen (about -32000); never remember that.
    if pos.x <= -10000 || pos.y <= -10000 {
        return;
    }
    let Some(path) = state_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&Saved { x: pos.x, y: pos.y }) {
        let _ = fs::write(path, json);
    }
}
