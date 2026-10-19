//! Remembers the widget window position and pin state in window.json (app config dir).
//! Kept apart from settings.json so the settings form cannot overwrite it.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    x: Option<i32>,
    y: Option<i32>,
    /// Pinned to the desktop: kept behind other windows instead of always on top.
    #[serde(default)]
    pinned: bool,
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("window.json"))
}

fn read(app: &AppHandle) -> Saved {
    state_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(app: &AppHandle, saved: &Saved) {
    let Some(path) = state_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(saved) {
        let _ = fs::write(path, json);
    }
}

pub fn pinned(app: &AppHandle) -> bool {
    read(app).pinned
}

pub fn set_pinned(app: &AppHandle, pinned: bool) {
    let mut saved = read(app);
    saved.pinned = pinned;
    write(app, &saved);
}

/// Applies the saved pin state and moves the window to the saved spot if that spot
/// is still on a connected monitor.
pub fn restore(window: &WebviewWindow) {
    let app = window.app_handle();
    let saved = read(app);
    apply_pin(window, saved.pinned);
    if let (Some(x), Some(y)) = (saved.x, saved.y) {
        let on_screen = app
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .any(|m| {
                let (p, s) = (m.position(), m.size());
                x >= p.x && x < p.x + s.width as i32 && y >= p.y && y < p.y + s.height as i32
            });
        if on_screen {
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// Pinned: below other windows. Unpinned: always on top.
pub fn apply_pin(window: &WebviewWindow, pinned: bool) {
    let _ = window.set_always_on_top(!pinned);
    let _ = window.set_always_on_bottom(pinned);
}

pub fn save(app: &AppHandle, pos: PhysicalPosition<i32>) {
    // Minimized windows are reported far off-screen (about -32000); never remember that.
    if pos.x <= -10000 || pos.y <= -10000 {
        return;
    }
    let mut saved = read(app);
    saved.x = Some(pos.x);
    saved.y = Some(pos.y);
    write(app, &saved);
}
