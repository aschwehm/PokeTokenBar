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

/// Placeholder command (replaced by real commands in Phase 1).
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
