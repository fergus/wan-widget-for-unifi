//! Settings file in the app config dir; API key in the OS credential store.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use widget_core::settings::Settings;

const KEYRING_SERVICE: &str = "unifi-widget";
const KEYRING_USER: &str = "api-key";

/// Set when Settings saves the settings or the key, so the poller reloads them.
/// Starts set so the first poll loads both.
static CHANGED: AtomicBool = AtomicBool::new(true);

/// True once after each save. The poller reloads settings and key only then.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

/// Falls back to defaults when the file is missing or unreadable.
pub fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), Vec<String>> {
    settings.validate()?;
    let path = settings_path(app).ok_or_else(|| vec!["no config directory".to_string()])?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| vec![e.to_string()])?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| vec![e.to_string()])?;
    fs::write(path, json).map_err(|e| vec![e.to_string()])?;
    CHANGED.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn save_api_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("API key is empty".into());
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .and_then(|e| e.set_password(key))
        .map_err(|e| e.to_string())?;
    CHANGED.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn has_api_key() -> bool {
    load_api_key().is_some()
}

pub fn load_api_key() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
}
