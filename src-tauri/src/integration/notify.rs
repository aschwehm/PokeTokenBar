//! Cross-platform desktop notifications.
//!
//! Port of macOS UserNotifications framework in PokeTokenBar to Tauri's
//! notification plugin (Linux DBus/Desktop Notifications, Windows WinRT toast).

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Send a native desktop notification for companion celebrations / events.
pub fn send_notification(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}
