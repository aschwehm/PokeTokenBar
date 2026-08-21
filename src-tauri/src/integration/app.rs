//! App state and the Rust↔frontend command bridge.
//!
//! Holds the [`UsageStore`] and [`CompanionStore`] behind a mutex and exposes a
//! serializable [`Snapshot`] to the webview, plus the mutating commands the UI
//! invokes (refresh, buy, use items, hatch, set language).

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

use crate::companion::store::{BurnTier, Celebration, CompanionStore, DexSpecies};
use crate::companion::usage_store::UsageStore;
use crate::domain::companion::{AppLanguage, CompanionState, CompanionStateKind, ItemKind, Rarity};
use crate::domain::models::ProviderSnapshot;

/// The single managed app state, shared with Tauri as `tauri::State`.
pub type AppState = Arc<Mutex<StateInner>>;

pub struct StateInner {
    pub usage: UsageStore,
    pub companion: CompanionStore,
    pub limits: Option<crate::providers::claude_limits::LimitStatus>,
}

impl StateInner {
    pub fn new() -> Self {
        Self {
            usage: UsageStore::default(),
            companion: CompanionStore::new_default(),
            limits: None,
        }
    }

    pub fn create_default_providers() -> Vec<Box<dyn crate::providers::UsageProvider>> {
        vec![
            Box::new(crate::providers::local::LocalClaudeProvider),
            Box::new(crate::providers::local::LocalCodexProvider),
            Box::new(crate::providers::local::LocalGeminiProvider),
            Box::new(crate::providers::local::LocalGrokProvider),
            Box::new(crate::providers::antigravity::LocalAntigravityProvider),
            Box::new(crate::providers::additional::LocalOpenCodeProvider),
            Box::new(crate::providers::additional::LocalHermesProvider),
            Box::new(crate::providers::additional::LocalCursorProvider),
            Box::new(crate::providers::additional::LocalCopilotProvider),
            Box::new(crate::providers::additional::LocalKiroProvider),
        ]
    }
}

impl Default for StateInner {
    fn default() -> Self {
        Self::new()
    }
}

// MARK: snapshot DTOs

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub companion: CompanionView,
    pub usage: UsageView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionView {
    /// The full persisted state (active mon / egg, dex, inventory, ledger).
    pub state: CompanionState,
    pub display_state: &'static str,
    pub display_name: String,
    pub is_egg: bool,
    pub has_active: bool,
    pub current_species_id: Option<i64>,
    pub is_shiny: bool,
    pub rarity: Option<&'static str>,
    pub is_final_stage: bool,
    pub stage_text: String,
    pub progress: f64,
    pub tokens_to_next: i64,
    pub egg_progress: f64,
    pub egg_tokens_to_hatch: i64,
    pub available_tokens: i64,
    pub owned_items: Vec<(String, i64)>,
    pub shop: Vec<ShopView>,
    pub dex: Vec<DexSpeciesView>,
    pub celebration: Option<CelebrationView>,
    pub celebration_seq: u64,
    pub candy_feedback: Option<i64>,
    pub mint_feedback: Option<String>,
    pub berry_feedback: Option<String>,
    pub has_golden_aura: bool,
    pub is_mega_overdrive: bool,
    pub mega_overdrive_enabled: bool,
    pub just_evolved_to: Option<String>,
    pub just_graduated: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopView {
    pub kind: String,
    pub tier: Option<&'static str>,
    pub price: i64,
    pub can_buy: bool,
    pub owned: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DexSpeciesView {
    pub id: i64,
    pub name: String,
    pub rarity: &'static str,
    pub is_shiny: bool,
    pub is_raising: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CelebrationView {
    pub kind: &'static str,
    pub shiny: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageView {
    pub today_total_tokens: i64,
    pub today_cost_total: f64,
    pub week_total_tokens: i64,
    pub week_cost_total: f64,
    pub month_total_tokens: i64,
    pub month_cost_total: f64,
    pub burn_tier: &'static str,
    pub menu_lines: Vec<String>,
    pub snapshots: Vec<ProviderView>,
    pub last_updated: Option<i64>,
    pub limits: Option<crate::providers::claude_limits::LimitStatus>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub display_name: String,
    pub today_total_tokens: i64,
    pub week_total_tokens: i64,
    pub month_total_tokens: i64,
}

fn rarity_str(r: Rarity) -> &'static str {
    match r {
        Rarity::Common => "common",
        Rarity::Uncommon => "uncommon",
        Rarity::Rare => "rare",
        Rarity::Legendary => "legendary",
    }
}

fn state_kind_str(k: CompanionStateKind) -> &'static str {
    match k {
        CompanionStateKind::Egg => "egg",
        CompanionStateKind::Idle => "idle",
        CompanionStateKind::Working => "working",
        CompanionStateKind::Focus => "focus",
        CompanionStateKind::Tired => "tired",
        CompanionStateKind::Sleep => "sleep",
        CompanionStateKind::LevelUp => "levelUp",
    }
}

fn burn_tier_str(b: BurnTier) -> &'static str {
    match b {
        BurnTier::Idle => "idle",
        BurnTier::Normal => "normal",
        BurnTier::Fast => "fast",
        BurnTier::Blazing => "blazing",
    }
}

fn celebration_view(c: Celebration) -> CelebrationView {
    match c {
        Celebration::Hatch { shiny } => CelebrationView {
            kind: "hatch",
            shiny,
        },
        Celebration::Evolve => CelebrationView {
            kind: "evolve",
            shiny: false,
        },
        Celebration::DittoReveal { shiny } => CelebrationView {
            kind: "ditto",
            shiny,
        },
    }
}

fn shop_view(kind: ItemKind, price: i64, can_buy: bool, owned: i64) -> ShopView {
    ShopView {
        kind: kind.raw_value().to_string(),
        tier: None,
        price,
        can_buy,
        owned,
    }
}

fn egg_shop_view(tier: Option<Rarity>, price: i64, can_buy: bool) -> ShopView {
    ShopView {
        kind: "egg".to_string(),
        tier: tier.map(rarity_str),
        price,
        can_buy,
        owned: 0,
    }
}

fn provider_view(s: &ProviderSnapshot) -> ProviderView {
    ProviderView {
        id: s.provider_id.clone(),
        display_name: s.display_name.clone(),
        today_total_tokens: s.today.as_ref().map(|t| t.total_tokens).unwrap_or(0),
        week_total_tokens: s.week_total.as_ref().map(|w| w.total_tokens).unwrap_or(0),
        month_total_tokens: s.month_total.as_ref().map(|m| m.total_tokens).unwrap_or(0),
    }
}

fn build_snapshot(inner: &StateInner) -> Snapshot {
    let c = &inner.companion;
    let u = &inner.usage;

    let shop: Vec<ShopView> = c
        .shop_entries()
        .into_iter()
        .map(|entry| match entry {
            crate::domain::companion::ShopEntry::Item(kind) => {
                let owned = c.item_count(kind);
                shop_view(kind, entry.price(), c.can_buy(kind), owned)
            }
            crate::domain::companion::ShopEntry::Egg(tier) => {
                egg_shop_view(tier, entry.price(), c.can_buy_egg(tier))
            }
        })
        .collect();

    let dex: Vec<DexSpeciesView> = c
        .dex_species()
        .into_iter()
        .map(|d: DexSpecies| DexSpeciesView {
            id: d.id,
            name: d.name,
            rarity: rarity_str(d.rarity),
            is_shiny: d.is_shiny,
            is_raising: d.is_raising,
        })
        .collect();

    let owned_items: Vec<(String, i64)> = c
        .owned_items()
        .into_iter()
        .map(|(kind, count)| (kind.raw_value().to_string(), count))
        .collect();

    let companion = CompanionView {
        state: c.state.clone(),
        display_state: state_kind_str(c.display_state),
        display_name: c.display_name(),
        is_egg: c.is_egg(),
        has_active: c.has_active(),
        current_species_id: c.current_species_id(),
        is_shiny: c.current_is_shiny(),
        rarity: c.rarity().map(rarity_str),
        is_final_stage: c.is_final_stage(),
        stage_text: c.stage_text(),
        progress: c.progress(),
        tokens_to_next: c.tokens_to_next(),
        egg_progress: c.egg_progress(),
        egg_tokens_to_hatch: c.egg_tokens_to_hatch(),
        available_tokens: c.available_tokens(),
        owned_items,
        shop,
        dex,
        celebration: c.celebration.map(celebration_view),
        celebration_seq: c.celebration_seq,
        candy_feedback: if c.candy_feedback_amount > 0 {
            Some(c.candy_feedback_amount)
        } else {
            None
        },
        mint_feedback: c
            .mint_feedback_nature
            .map(|n| n.name(c.language()).to_string()),
        berry_feedback: c.berry_feedback_kind.clone(),
        has_golden_aura: c.has_golden_aura(),
        is_mega_overdrive: c.is_mega_overdrive,
        mega_overdrive_enabled: c.mega_overdrive_enabled,
        just_evolved_to: c.just_evolved_to.clone(),
        just_graduated: c.just_graduated.clone(),
    };

    let usage = UsageView {
        today_total_tokens: u.today_total_tokens(),
        today_cost_total: u.today_cost_total(),
        week_total_tokens: u.week_total_tokens(),
        week_cost_total: u.week_cost_total(),
        month_total_tokens: u.month_total_tokens(),
        month_cost_total: u.month_cost_total(),
        burn_tier: burn_tier_str(u.burn_tier()),
        menu_lines: u.menu_lines(),
        snapshots: u.snapshots.iter().map(provider_view).collect(),
        last_updated: u.last_updated.map(|d| d.timestamp_millis()),
        limits: inner.limits.clone(),
    };

    Snapshot { companion, usage }
}

// MARK: commands

#[tauri::command]
pub async fn snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let inner = state.lock().map_err(|e| e.to_string())?;
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 1. Fetch claude limits in background WITHOUT locking state
        let limits =
            if let Some(cred) = crate::providers::claude_limits::read_claude_credentials(None) {
                if !cred.is_expired() {
                    crate::providers::claude_limits::fetch_claude_limits(&cred).ok()
                } else {
                    None
                }
            } else {
                None
            };

        // 2. Heavy filesystem scan across all providers WITHOUT locking state!
        let providers = StateInner::create_default_providers();
        let snapshots = UsageStore::collect_snapshots(&providers);

        // 3. Acquire state lock for < 0.1ms to apply in-memory updates
        let (snap, celebration_to_notify) = {
            let mut inner = state.lock().map_err(|e| e.to_string())?;
            if limits.is_some() {
                inner.limits = limits;
            }
            let prev_seq = inner.companion.celebration_seq;
            let StateInner {
                ref mut usage,
                ref mut companion,
                ..
            } = *inner;
            usage.apply_snapshots(snapshots, companion);

            let celebration = if inner.companion.celebration_seq > prev_seq {
                inner
                    .companion
                    .celebration
                    .map(|c| (c, inner.companion.display_name()))
            } else {
                None
            };
            (build_snapshot(&inner), celebration)
        };

        // 4. Send notification outside the lock
        if let Some((c, name)) = celebration_to_notify {
            let (title, body) = match c {
                Celebration::Hatch { shiny } => (
                    if shiny {
                        "Shiny Pokémon Hatched! ✨"
                    } else {
                        "Pokémon Hatched! 🐣"
                    },
                    format!("Your egg hatched into {}!", name),
                ),
                Celebration::Evolve => (
                    "Pokémon Evolved! 🌟",
                    format!("Your partner evolved into {}!", name),
                ),
                Celebration::DittoReveal { shiny } => (
                    if shiny {
                        "Shiny Ditto Discovered! ✨"
                    } else {
                        "Ditto Discovered! 🟣"
                    },
                    "Your Pokémon transformed back into Ditto!".to_string(),
                ),
            };
            crate::integration::notify::send_notification(&app, title, &body);
        }

        Ok(snap)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn buy_item(state: State<'_, AppState>, kind: String) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = state.lock().map_err(|e| e.to_string())?;
        if let Some(item) = parse_item_kind(&kind) {
            inner.companion.buy(item);
        }
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn use_rare_candy(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = state.lock().map_err(|e| e.to_string())?;
        let prev_seq = inner.companion.celebration_seq;
        inner.companion.use_rare_candy();
        if inner.companion.celebration_seq > prev_seq {
            if let Some(Celebration::Evolve) = inner.companion.celebration {
                crate::integration::notify::send_notification(
                    &app,
                    "Pokémon Evolved! 🌟",
                    &format!(
                        "Your partner evolved into {}!",
                        inner.companion.display_name()
                    ),
                );
            }
        }
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn use_mint(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = state.lock().map_err(|e| e.to_string())?;
        inner.companion.use_mint();
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn use_berry(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = state.lock().map_err(|e| e.to_string())?;
        let prev_seq = inner.companion.celebration_seq;
        match kind.as_str() {
            "oranBerry" => {
                inner.companion.use_oran_berry();
            }
            "sitrusBerry" => {
                inner.companion.use_sitrus_berry();
            }
            _ => return Err("unknown berry kind".to_string()),
        }
        if inner.companion.celebration_seq > prev_seq {
            if let Some(Celebration::Evolve) = inner.companion.celebration {
                crate::integration::notify::send_notification(
                    &app,
                    "Pokémon Evolved! 🌟",
                    &format!(
                        "Your partner evolved into {}!",
                        inner.companion.display_name()
                    ),
                );
            }
        }
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn buy_egg(state: State<'_, AppState>, tier: Option<String>) -> Result<Snapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = state.lock().map_err(|e| e.to_string())?;
        let tier = tier.and_then(|t| parse_rarity(&t));
        inner.companion.buy_egg(tier);
        Ok(build_snapshot(&inner))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn set_language(state: State<'_, AppState>, lang: String) -> Result<Snapshot, String> {
    let mut inner = state.lock().map_err(|e| e.to_string())?;
    if let Some(l) = AppLanguage::from_raw(&lang) {
        inner.companion.set_language(l);
    }
    Ok(build_snapshot(&inner))
}

#[tauri::command]
pub fn set_mega_overdrive_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<Snapshot, String> {
    let mut inner = state.lock().map_err(|e| e.to_string())?;
    inner.companion.set_mega_overdrive_enabled(enabled);
    let burn_tier = inner.usage.burn_tier();
    inner.companion.is_mega_overdrive =
        enabled && (burn_tier == BurnTier::Fast || burn_tier == BurnTier::Blazing);
    Ok(build_snapshot(&inner))
}

#[tauri::command]
pub fn consume_celebration(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let mut inner = state.lock().map_err(|e| e.to_string())?;
    inner.companion.celebration = None;
    Ok(build_snapshot(&inner))
}

#[tauri::command]
pub fn consume_feedback(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let mut inner = state.lock().map_err(|e| e.to_string())?;
    inner.companion.consume_candy_feedback();
    inner.companion.consume_mint_feedback();
    inner.companion.consume_berry_feedback();
    Ok(build_snapshot(&inner))
}

#[tauri::command]
pub fn minimize_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        window.minimize().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn hide_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_pet_window(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("pet") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            Ok(false)
        } else {
            let _ = window.show();
            let _ = window.set_focus();
            Ok(true)
        }
    } else {
        let builder =
            tauri::WebviewWindowBuilder::new(&app, "pet", tauri::WebviewUrl::App("/pet".into()))
                .title("Companion Pet")
                .inner_size(220.0, 220.0)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(true);

        #[cfg(target_os = "windows")]
        let builder = builder.shadow(false);

        let _ = builder.build().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
pub async fn get_sprite(id: i64, shiny: bool) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(crate::providers::pokeapi::get_or_fetch_sprite(id, shiny))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_pokedex_details(
    id: i64,
    lang: Option<String>,
) -> Result<Option<crate::providers::pokeapi::PokedexDetails>, String> {
    let language = lang.unwrap_or_else(|| "en".to_string());
    tauri::async_runtime::spawn_blocking(move || {
        Ok(crate::providers::pokeapi::get_or_fetch_pokedex_details(
            id, &language,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn parse_item_kind(s: &str) -> Option<ItemKind> {
    match s {
        "rareCandy" => Some(ItemKind::RareCandy),
        "mint" => Some(ItemKind::Mint),
        "oranBerry" => Some(ItemKind::OranBerry),
        "sitrusBerry" => Some(ItemKind::SitrusBerry),
        "shinyCharm" => Some(ItemKind::ShinyCharm),
        _ => None,
    }
}

fn parse_rarity(s: &str) -> Option<Rarity> {
    Rarity::from_raw(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_inner_refresh() {
        let mut state = StateInner::new();
        state.usage.refresh(&mut state.companion);
        for snap in &state.usage.snapshots {
            println!(
                "Provider found: {} -> tokens today: {:?}",
                snap.display_name,
                snap.today.as_ref().map(|t| t.total_tokens)
            );
        }
    }
}
