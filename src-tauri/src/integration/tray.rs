//! System-tray integration: a tray icon whose left click toggles the popover
//! window, plus a small menu (Refresh / Quit).

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide Popover", true, None::<&str>)?;
    let pet = MenuItem::with_id(app, "pet", "Toggle Desktop Pet", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &pet, &refresh, &quit])?;
    let icon = if let Some(icon) = app.default_window_icon() {
        icon.clone()
    } else {
        tauri::include_image!("icons/32x32.png")
    };

    TrayIconBuilder::new()
        .tooltip("PokeTokenBar")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_window(app),
            "pet" => {
                let _ = crate::integration::app::toggle_pet_window(app.clone());
            }
            "refresh" => {
                let _ = app.emit("tray-refresh", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
