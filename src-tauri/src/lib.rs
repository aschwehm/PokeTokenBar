//! PokeTokenBar — Pokémon companion for your AI coding tokens.
//!
//! Cross-platform port of <https://github.com/chattymin/PokeTokenBar>.
//!
//! Module layout (see docs/architecture.md §3):
//! - [`domain`]       — pure data types and calculations
//! - [`providers`]    — per-AI-CLI usage readers + PokéAPI client
//! - [`companion`]    — companion state machine + aggregation
//! - [`integration`]  — OS-service wrappers (tray, notify, secrets, …)
//! - [`platform`]     — paths, shell env, process spawning

pub mod companion;
pub mod domain;
pub mod integration;
pub mod platform;
pub mod providers;

use std::sync::{Arc, Mutex};

use integration::app::{AppState, StateInner};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: AppState = Arc::new(Mutex::new(StateInner::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(state)
        .setup(|app| {
            integration::tray::setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            integration::app::snapshot,
            integration::app::refresh,
            integration::app::buy_item,
            integration::app::use_rare_candy,
            integration::app::use_mint,
            integration::app::buy_egg,
            integration::app::set_language,
            integration::app::consume_celebration,
            integration::app::consume_feedback,
            integration::app::toggle_pet_window,
            integration::app::minimize_window,
            integration::app::hide_window,
            integration::app::get_sprite,
            integration::app::get_pokedex_details,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
