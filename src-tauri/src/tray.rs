//! System tray: show/hide, click-through, settings, autostart, quit.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const ID_TOGGLE: &str = "toggle";
const ID_CLICK_THROUGH: &str = "click_through";
const ID_SETTINGS: &str = "settings";
const ID_AUTOSTART: &str = "autostart";
const ID_PIN: &str = "pin";
const ID_QUIT: &str = "quit";

/// Kept in managed state so the settings window can keep the tray checkboxes in sync.
pub struct TrayState {
    pub autostart: CheckMenuItem<Wry>,
    pub pin: CheckMenuItem<Wry>,
}

fn toggle_widget(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Windows shows the taskbar in light mode when SystemUsesLightTheme is 1.
fn taskbar_is_light() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|v| v == 1)
        .unwrap_or(false)
}

/// Last (light taskbar, 24 px) pair applied, so the icon is only swapped on change.
static APPLIED: Mutex<Option<(bool, bool)>> = Mutex::new(None);

/// Picks the tray icon for the taskbar theme and display scale. Called from the poller
/// loop, so a theme change is picked up within a couple of seconds.
pub fn sync_icon(app: &AppHandle) {
    let light = taskbar_is_light();
    let large = app
        .get_webview_window("main")
        .and_then(|w| w.scale_factor().ok())
        .is_some_and(|s| s > 1.0);
    let want = (light, large);
    let Ok(mut applied) = APPLIED.lock() else {
        return;
    };
    if *applied == Some(want) {
        return;
    }
    let bytes: &[u8] = match want {
        (true, false) => include_bytes!("../icons/tray/tray-light-taskbar-16.png"),
        (true, true) => include_bytes!("../icons/tray/tray-light-taskbar-24.png"),
        (false, false) => include_bytes!("../icons/tray/tray-dark-taskbar-16.png"),
        (false, true) => include_bytes!("../icons/tray/tray-dark-taskbar-24.png"),
    };
    if let (Some(tray), Ok(icon)) = (app.tray_by_id("main"), Image::from_bytes(bytes)) {
        if tray.set_icon(Some(icon)).is_ok() {
            *applied = Some(want);
        }
    }
}

pub fn build(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let toggle = MenuItem::with_id(app, ID_TOGGLE, "Show / hide widget", true, None::<&str>)?;
    // Click-through is deliberately never persisted: it always starts off, so a
    // widget that ignores the mouse can never be the only way back to the menu.
    let click_through = CheckMenuItem::with_id(
        app,
        ID_CLICK_THROUGH,
        "Click-through",
        true,
        false,
        None::<&str>,
    )?;
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
    let pin = CheckMenuItem::with_id(
        app,
        ID_PIN,
        "Pin to desktop",
        true,
        crate::window_state::pinned(app),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &click_through,
            &pin,
            &settings,
            &autostart,
            &sep,
            &quit,
        ],
    )?;

    app.manage(TrayState {
        autostart: autostart.clone(),
        pin: pin.clone(),
    });

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("no window icon: run `cargo tauri icon` to generate icons/")?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("WAN Widget for UniFi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            ID_TOGGLE => toggle_widget(app),
            ID_CLICK_THROUGH => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_ignore_cursor_events(click_through.is_checked().unwrap_or(false));
                }
            }
            ID_PIN => {
                let want = pin.is_checked().unwrap_or(false);
                crate::window_state::set_pinned(app, want);
                if let Some(w) = app.get_webview_window("main") {
                    crate::window_state::apply_pin(&w, want);
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
