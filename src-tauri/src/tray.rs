//! System tray: show/hide, click-through, settings, autostart, quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

const ID_TOGGLE: &str = "toggle";
const ID_CLICK_THROUGH: &str = "click_through";
const ID_SETTINGS: &str = "settings";
const ID_AUTOSTART: &str = "autostart";
const ID_QUIT: &str = "quit";

/// Kept in managed state so the settings window can keep the tray checkbox in sync.
pub struct TrayState {
    pub autostart: CheckMenuItem<Wry>,
}

fn toggle_widget(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn build(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle = MenuItem::with_id(app, ID_TOGGLE, "Show / hide widget", true, None::<&str>)?;
    // Click-through is deliberately never persisted: it always starts off, so a
    // widget that ignores the mouse can never be the only way back to the menu.
    let click_through =
        CheckMenuItem::with_id(app, ID_CLICK_THROUGH, "Click-through", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "Settings...", true, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        ID_AUTOSTART,
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &click_through, &settings, &autostart, &sep, &quit])?;

    app.manage(TrayState { autostart: autostart.clone() });

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("no window icon: run `cargo tauri icon` to generate icons/")?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("UniFi Widget")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            ID_TOGGLE => toggle_widget(app),
            ID_CLICK_THROUGH => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_ignore_cursor_events(click_through.is_checked().unwrap_or(false));
                }
            }
            ID_SETTINGS => {
                let _ = crate::show_settings(app);
            }
            ID_AUTOSTART => {
                // The check mark has already flipped; apply it, and flip back on failure.
                let want = autostart.is_checked().unwrap_or(false);
                let al = app.autolaunch();
                let result = if want { al.enable() } else { al.disable() };
                if let Err(e) = result {
                    eprintln!("autostart change failed: {e}");
                    let _ = autostart.set_checked(!want);
                }
            }
            ID_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_widget(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
