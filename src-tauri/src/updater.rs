//! Update checks against the latest GitHub release (endpoint and public key in
//! tauri.conf.json). Updates are signed in the release workflow and the signature is
//! checked before anything is installed.
//!
//! The app checks shortly after start and then daily. A found update is kept here and
//! offered in the tray and in Settings; nothing installs without the user choosing it.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The newest update found, waiting for the user to install it.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

/// Checks for an update. Returns its version, or None when this is the newest.
pub async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let version = update.as_ref().map(|u| u.version.clone());
    match &version {
        Some(v) => log::info!("update check: {v} is available"),
        None => log::info!("update check: up to date"),
    }
    if let Ok(mut g) = app.state::<Pending>().0.lock() {
        *g = update;
    }
    crate::tray::show_update(app, version.as_deref());
    if let Some(v) = &version {
        let _ = app.emit("update-available", v.clone());
    }
    Ok(version)
}

/// Downloads, verifies and installs the pending update. On Windows the installer
/// takes over and the app exits, then restarts on the new version.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let pending = app.state::<Pending>().0.lock().ok().and_then(|g| g.clone());
    let update = match pending {
        Some(u) => u,
        None => {
            check(app).await?;
            app.state::<Pending>()
                .0
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .ok_or("You already have the latest version")?
        }
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())
}

/// Background loop: one check soon after start, then one a day. Failures (offline,
/// GitHub unreachable) are only logged; the next check tries again.
pub async fn run(app: AppHandle) {
    tokio::time::sleep(FIRST_CHECK_AFTER).await;
    loop {
        if let Err(e) = check(&app).await {
            log::warn!("update check: {e}");
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

fn has_pending(app: &AppHandle) -> bool {
    app.state::<Pending>()
        .0
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

/// Tray item: installs a found update, or checks and says what it found.
pub async fn tray_clicked(app: AppHandle) {
    if has_pending(&app) {
        if let Err(e) = install(&app).await {
            log::warn!("update install: {e}");
            crate::tray::set_update_text(&app, "Update failed. Try again from Settings");
        }
        return;
    }
    match check(&app).await {
        Ok(Some(_)) => {}
        Ok(None) => crate::tray::set_update_text(&app, "No update available"),
        Err(e) => {
            log::warn!("update check: {e}");
            crate::tray::set_update_text(&app, "Update check failed");
        }
    }
}
