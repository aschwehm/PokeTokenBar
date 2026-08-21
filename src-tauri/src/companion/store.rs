//! The game-state machine: egg → hatch → evolve → graduate lifecycle, shiny /
//! nature / Ditto-disguise rolls, Pokédex + catch log, and the Shop/Bag economy.
//!
//! Port of the original `Core/CompanionStore.swift`. The Swift `@MainActor
//! @Observable final class` becomes a plain synchronous struct with `&mut self`
//! methods; `async` methods become blocking calls through the
//! [`PokeProvider`] trait and the fire-and-forget `Task { … }` launches become
//! direct synchronous calls in the same method. No async / tokio.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Timelike, Utc};
use rand::RngCore;
use rand::SeedableRng;
use uuid::Uuid;

use crate::domain::companion::{
    AppLanguage, CandyGrant, CandyWindow, CompanionState, CompanionStateKind, DexEntry, EvoLine,
    EvoLineItem, EvoLineItemContent, EvoLineItemState, EvoNode, FreshEgg, ItemKind, JournalEntry,
    MonState, OranBerry, PokemonAssets, PokemonBalance, PokemonNature, PokemonOdds, RareCandy,
    Rarity, ShinyCharm, ShopEntry, SitrusBerry, WindowClass,
};
use crate::domain::save::{SaveEnvelope, SaveSummary, SaveTransfer, SaveTransferError};
use crate::platform::{self, app_env};
use crate::providers::pokeapi::{BaseSpecies, PokeAPIClient, PokeProvider};

/// Burn-rate tier — drives the companion display state (working/focused).
/// Mirrors Swift's `UsageStore.BurnTier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BurnTier {
    Idle,
    Normal,
    Fast,
    Blazing,
}

/// One-shot animation trigger — seq increments so the UI detects it, and a
/// popover that was closed still replays on the next open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Celebration {
    Hatch { shiny: bool },
    Evolve,
    DittoReveal { shiny: bool },
}

/// Candy-use outcome — the UI feedback branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandyUseResult {
    Evolved,
    Graduated,
    Progressed,
    Unavailable,
}

/// The RNG seam used by every roll in the state machine. Deterministic in tests
/// via [`SequenceRng`], cryptographically-ish seeded in production via
/// [`StdRng`].
pub trait Rng: Send {
    fn next_u64(&mut self) -> u64;
}

/// Production RNG — wraps `rand::rngs::StdRng`, seeded from the OS.
pub struct StdRng(rand::rngs::StdRng);

impl StdRng {
    pub fn new() -> Self {
        Self(rand::rngs::StdRng::from_os_rng())
    }
}

impl Default for StdRng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng for StdRng {
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
}

/// Test RNG — pops the next canned value, or returns `0` once exhausted.
pub struct SequenceRng(VecDeque<u64>);

impl SequenceRng {
    pub fn new(values: Vec<u64>) -> Self {
        Self(values.into_iter().collect())
    }
}

impl Rng for SequenceRng {
    fn next_u64(&mut self) -> u64 {
        self.0.pop_front().unwrap_or(0)
    }
}

/// One dex cell — a species folded into a single collection record. Same line
/// raised multiple times still occupies one cell (the catch log stays
/// individual-level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexSpecies {
    /// speciesID = dex number (sort key).
    pub id: i64,
    pub name: String,
    pub rarity: Rarity,
    /// Whether this species has ever been owned shiny.
    pub is_shiny: bool,
    /// This cell's only basis is the currently-raised mon — not yet secured by
    /// a graduation record.
    pub is_raising: bool,
    /// Earned ribbons for this species.
    pub ribbons: Vec<String>,
}

/// Per-species accumulator for `dexSpecies` (species-level aggregation).
struct DexAccumulator {
    /// Fixed on first sight — the same species always comes from the same base
    /// line, so there is nothing to update.
    rarity: Rarity,
    names: Option<HashMap<String, String>>,
    is_shiny: bool,
    /// Has it ever come from a graduation record — once true the species is
    /// permanently preserved and never vanishes.
    is_graduated: bool,
    ribbons: HashSet<String>,
}

/// The game-state machine. All methods are synchronous; network calls block
/// through the injected [`PokeProvider`].
pub struct CompanionStore {
    pub state: CompanionState,
    pub display_state: CompanionStateKind,
    pub current_line: Option<EvoLine>,
    pub is_hatching: bool,
    is_revealing_ditto: bool,
    pub just_evolved_to: Option<String>,
    pub just_graduated: Option<String>,
    event_until: Option<DateTime<Utc>>,
    pub celebration: Option<Celebration>,
    pub celebration_seq: u64,
    pub candy_feedback_seq: u64,
    pub candy_feedback_amount: i64,
    pub mint_feedback_seq: u64,
    pub mint_feedback_nature: Option<PokemonNature>,
    pub berry_feedback_seq: u64,
    pub berry_feedback_kind: Option<String>,
    pub golden_aura_until: Option<DateTime<Utc>>,
    pub mega_overdrive_enabled: bool,
    pub is_mega_overdrive: bool,
    provider: Box<dyn PokeProvider>,
    clock: fn() -> DateTime<Utc>,
    file_url: PathBuf,
    rng: Box<dyn Rng>,
    ditto_disguise_rolling_enabled: bool,
    /// Session-wide subject-replacement guard — stale async results never
    /// overwrite a newer subject.
    active_generation: u64,
    prefetch_in_flight: bool,
    prefetched_line_id: Option<i64>,
    notif_seq: u64,
}

impl CompanionStore {
    /// Constructs a store, then loads the persisted state.
    ///
    /// Defaults (the caller may supply any of them explicitly):
    /// - provider = `PokeAPIClient::shared()` (boxed)
    /// - clock = `Utc::now`
    /// - file_url = `platform::data_dir()/companion-state.json`, honoring a
    ///   non-empty `PTB_STATE_DIR` env var as the *directory*
    /// - rng = [`StdRng`]
    /// - ditto_disguise_rolling_enabled = `app_env::is_bundled_app()`
    pub fn new(
        provider: Box<dyn PokeProvider>,
        clock: fn() -> DateTime<Utc>,
        file_url: Option<PathBuf>,
        rng: Box<dyn Rng>,
        ditto_disguise_rolling_enabled: bool,
    ) -> Self {
        let file_url = file_url.unwrap_or_else(default_file_url);
        let mut store = Self {
            state: CompanionState::default(),
            display_state: CompanionStateKind::Egg,
            current_line: None,
            is_hatching: false,
            is_revealing_ditto: false,
            just_evolved_to: None,
            just_graduated: None,
            event_until: None,
            celebration: None,
            celebration_seq: 0,
            candy_feedback_seq: 0,
            candy_feedback_amount: 0,
            mint_feedback_seq: 0,
            mint_feedback_nature: None,
            berry_feedback_seq: 0,
            berry_feedback_kind: None,
            golden_aura_until: None,
            mega_overdrive_enabled: false,
            is_mega_overdrive: false,
            provider,
            clock,
            file_url,
            rng,
            ditto_disguise_rolling_enabled,
            active_generation: 0,
            prefetch_in_flight: false,
            prefetched_line_id: None,
            notif_seq: 0,
        };
        store.load();
        if store.state.active.is_some() {
            store.display_state = CompanionStateKind::Idle;
        }
        store
    }

    /// A store with production defaults (shared PokéAPI client, wall clock,
    /// default state file, OS-seeded RNG, packaged-app gate).
    pub fn new_default() -> Self {
        Self::new(
            Box::new(PokeAPIClient::shared().clone()),
            Utc::now,
            None,
            Box::new(StdRng::new()),
            app_env::is_bundled_app(),
        )
    }

    fn fire_celebration(&mut self, c: Celebration) {
        self.celebration = Some(c);
        self.celebration_seq += 1;
    }

    // MARK: derived values (UI)

    pub fn language(&self) -> AppLanguage {
        self.state.language
    }

    pub fn set_language(&mut self, lang: AppLanguage) {
        self.state.language = lang;
        self.save();
    }

    pub fn has_active(&self) -> bool {
        self.state.active.is_some()
    }

    pub fn rarity(&self) -> Option<Rarity> {
        self.state.active.as_ref().map(|a| a.rarity)
    }

    pub fn current_is_shiny(&self) -> bool {
        let Some(a) = self.state.active.as_ref() else {
            return false;
        };
        // While disguised the shiny stays hidden (revealed only at reveal time).
        if a.ditto_disguise.is_some() && !a.ditto_revealed {
            return false;
        }
        a.is_shiny
    }

    pub fn current_nature(&self) -> Option<PokemonNature> {
        self.state.active.as_ref().and_then(|a| a.nature)
    }

    /// Egg incubation (no active mon).
    pub fn is_egg(&self) -> bool {
        self.state.active.is_none()
    }

    pub fn egg_started(&self) -> bool {
        self.state.egg_usage > 0
    }

    pub fn egg_progress(&self) -> f64 {
        (self.state.egg_usage as f64 / PokemonBalance::EGG_HATCH_THRESHOLD as f64).clamp(0.0, 1.0)
    }

    pub fn egg_tokens_to_hatch(&self) -> i64 {
        (PokemonBalance::EGG_HATCH_THRESHOLD - self.state.egg_usage).max(0)
    }

    pub fn display_name(&self) -> String {
        let (Some(active), Some(line)) = (&self.state.active, &self.current_line) else {
            return "Token Egg".to_string();
        };
        line.localized_name(active.current_id(), self.state.language)
    }

    pub fn current_species_id(&self) -> Option<i64> {
        self.state.active.as_ref().map(MonState::current_id)
    }

    pub fn is_final_stage(&self) -> bool {
        let (Some(active), Some(line)) = (&self.state.active, &self.current_line) else {
            return false;
        };
        line.tree
            .node_with_id(active.current_id())
            .map(|n| n.children.is_empty())
            .unwrap_or(true)
    }

    pub fn stage_text(&self) -> String {
        let Some(active) = self.state.active.as_ref() else {
            return String::new();
        };
        if self.is_final_stage() {
            "Final form".to_string() // TODO: localize
        } else {
            format!("Stage {}/{}", active.stage_index + 1, active.total_forms) // TODO: localize
        }
    }

    pub fn threshold(&self) -> i64 {
        match self.state.active.as_ref() {
            Some(active) => PokemonBalance::phase_threshold(
                active.rarity,
                active.total_forms,
                active.stage_index,
            ),
            None => 1,
        }
    }

    pub fn progress(&self) -> f64 {
        let (Some(active), thr) = (self.state.active.as_ref(), self.threshold()) else {
            return 0.0;
        };
        if thr <= 0 {
            return 0.0;
        }
        (active.used_at_stage as f64 / thr as f64).clamp(0.0, 1.0)
    }

    pub fn tokens_to_next(&self) -> i64 {
        let Some(active) = self.state.active.as_ref() else {
            return 0;
        };
        (self.threshold() - active.used_at_stage).max(0)
    }

    /// Evolution-line display: the realized path + a preview of the next stage.
    /// Only a uniquely-following stage is shown; downstream branches collapse
    /// into a single mystery item until the branch is actually reached.
    pub fn line_nodes(&self) -> Vec<EvoLineItem> {
        let (Some(active), Some(line)) = (&self.state.active, &self.current_line) else {
            return Vec::new();
        };
        let mut out = Self::realized_line_items(&active.path_ids, active.stage_index);
        if let Some(current) = line.tree.node_with_id(active.current_id()) {
            let mut node = current;
            let mut guaranteed_prefix: Vec<EvoNode> = Vec::new();
            while node.children.len() == 1 {
                let child = &node.children[0];
                guaranteed_prefix.push(child.clone());
                node = child;
            }
            let future = guaranteed_prefix.iter().map(|n| {
                EvoLineItem::new(
                    EvoLineItemContent::Species(n.species_id),
                    EvoLineItemState::Future,
                )
            });
            if node.children.len() > 1 {
                out.extend(future);
                out.push(EvoLineItem::new(
                    EvoLineItemContent::Mystery,
                    EvoLineItemState::Future,
                ));
            } else {
                out.extend(future);
            }
        }
        out
    }

    pub fn realized_line_items(path_ids: &[i64], stage_index: i64) -> Vec<EvoLineItem> {
        path_ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                EvoLineItem::new(
                    EvoLineItemContent::Species(*id),
                    if i as i64 == stage_index {
                        EvoLineItemState::Current
                    } else {
                        EvoLineItemState::Done
                    },
                )
            })
            .collect()
    }

    /// The dex shows both permanently-graduated entries and the currently-
    /// raised mon. The current mon is not stored in the persistent dex — it is
    /// synthesized for display only, so the list length stays constant across
    /// graduation.
    pub fn active_dex_entry(&self) -> Option<DexEntry> {
        let active = self.state.active.as_ref()?;
        let names = self.current_line.as_ref().map(|line| {
            let mut m = HashMap::new();
            for id in &active.path_ids {
                if let Some(n) = line.names.get(id) {
                    m.insert(*id, n.clone());
                }
            }
            m
        });
        Some(
            DexEntry::new(
                active_entry_id(active.base_id, active.current_id()),
                active.base_id,
                active.current_id(),
                active.path_ids.clone(),
                active.rarity,
                None,
                self.current_is_shiny(), // disguised Ditto hides shiny before reveal
                active.nature,
                names,
            )
            .with_ribbons(active.ribbons.clone()),
        )
    }

    pub fn dex_entries(&self) -> Vec<DexEntry> {
        let mut entries = self.state.dex.clone();
        if let Some(active) = self.active_dex_entry() {
            entries.push(active);
        }
        entries
    }

    /// Distinguishes the synthesized current-mon entry — without confusing it
    /// with legacy graduated entries that have no `caughtAt`.
    pub fn is_active_dex_entry(&self, entry: &DexEntry) -> bool {
        self.active_dex_entry().map(|e| e.id) == Some(entry.id)
    }

    /// Catch-log display order — current mon pinned first, then graduated
    /// entries newest-first (recency regardless of rarity). Legacy entries
    /// without `caughtAt` sink to the back.
    pub fn dex_entries_sorted(&self) -> Vec<DexEntry> {
        let mut graduated = self.state.dex.clone();
        graduated.sort_by(|a, b| {
            let a_time = a.caught_at.unwrap_or(DateTime::<Utc>::MIN_UTC);
            let b_time = b.caught_at.unwrap_or(DateTime::<Utc>::MIN_UTC);
            b_time.cmp(&a_time)
        });
        if let Some(active) = self.active_dex_entry() {
            let mut out = vec![active];
            out.extend(graduated);
            out
        } else {
            graduated
        }
    }

    /// Per-rarity catch-log count (individual-based — the summary header).
    pub fn dex_count(&self, rarity: Rarity) -> usize {
        self.dex_entries()
            .iter()
            .filter(|e| e.rarity == rarity)
            .count()
    }

    /// The dex list — only owned species, dex-number ascending.
    ///
    /// Included species = graduated `chainOrder` ∪ the current mon's **reached**
    /// `path_ids[0…stage_index]`. The pre-selected `planned_path_ids` is never
    /// used — it contains unreached stages that would otherwise show as owned.
    pub fn dex_species(&self) -> Vec<DexSpecies> {
        let mut acc: HashMap<i64, DexAccumulator> = HashMap::new();
        for entry in &self.state.dex {
            for id in &entry.chain_order {
                let a = acc.entry(*id).or_insert_with(|| DexAccumulator {
                    rarity: entry.rarity,
                    names: None,
                    is_shiny: false,
                    is_graduated: false,
                    ribbons: HashSet::new(),
                });
                if let Some(n) = entry.names.as_ref().and_then(|m| m.get(id)) {
                    a.names = Some(n.clone()); // name-less legacy entries never overwrite
                }
                if entry.is_shiny {
                    a.is_shiny = true;
                }
                for r in &entry.ribbons {
                    a.ribbons.insert(r.clone());
                }
                a.is_graduated = true;
            }
        }
        if let Some(active) = self.state.active.as_ref() {
            let reached = (active.stage_index as usize + 1).min(active.path_ids.len());
            for id in active.path_ids.iter().take(reached) {
                let a = acc.entry(*id).or_insert_with(|| DexAccumulator {
                    rarity: active.rarity,
                    names: None,
                    is_shiny: false,
                    is_graduated: false,
                    ribbons: HashSet::new(),
                });
                if let Some(n) = self
                    .current_line
                    .as_ref()
                    .and_then(|line| line.names.get(id))
                {
                    a.names = Some(n.clone());
                }
                if self.current_is_shiny() {
                    a.is_shiny = true; // reuse the disguise-hiding rule
                }
                for r in &active.ribbons {
                    a.ribbons.insert(r.clone());
                }
            }
        }
        let mut species: Vec<DexSpecies> = acc
            .into_iter()
            .map(|(id, a)| {
                let name = a
                    .names
                    .as_ref()
                    .and_then(|m| self.state.language.resolve_name(m))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("#{id}"));
                let mut ribbons: Vec<String> = a.ribbons.into_iter().collect();
                ribbons.sort();
                DexSpecies {
                    id,
                    name,
                    rarity: a.rarity,
                    is_shiny: a.is_shiny,
                    is_raising: !a.is_graduated,
                    ribbons,
                }
            })
            .collect();
        species.sort_by_key(|s| s.id);
        species
    }

    /// Stored chain names for a dex entry, resolved into the current language.
    /// `None` when nothing is stored (the view backfills via a line fetch).
    pub fn dex_stored_chain_names(&self, entry: &DexEntry) -> Option<HashMap<i64, String>> {
        let names = entry.names.as_ref()?;
        if names.is_empty() {
            return None;
        }
        let mut out = HashMap::new();
        for (id, by_lang) in names {
            if let Some(name) = self.state.language.resolve_name(by_lang) {
                out.insert(*id, name.to_string());
            }
        }
        Some(out)
    }

    /// Resolves + backfills the chain names of a legacy (name-less) entry with a
    /// one-time line fetch. Offline → `#id` fallback, **not stored** (retried on
    /// the next grid entry).
    pub fn dex_resolve_chain_names(&mut self, entry: &DexEntry) -> HashMap<i64, String> {
        let entry = entry.clone();
        if let Some(stored) = self.dex_stored_chain_names(&entry) {
            return stored;
        }
        let line = match self.provider.line(entry.base_id) {
            Ok(line) => line,
            Err(_) => {
                return entry
                    .chain_order
                    .iter()
                    .map(|id| (*id, format!("#{id}")))
                    .collect();
            }
        };
        let mut chain_names: HashMap<i64, HashMap<String, String>> = HashMap::new();
        for id in &entry.chain_order {
            if let Some(n) = line.names.get(id) {
                chain_names.insert(*id, n.clone());
            }
        }
        if !chain_names.is_empty() {
            if let Some(idx) = self.state.dex.iter().position(|e| e.id == entry.id) {
                self.state.dex[idx].names = Some(chain_names.clone());
                self.save();
            }
        }
        entry
            .chain_order
            .iter()
            .map(|id| {
                let name = chain_names
                    .get(id)
                    .and_then(|m| self.state.language.resolve_name(m))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("#{id}"));
                (*id, name)
            })
            .collect()
    }

    /// One-time backfill for name-less legacy entries (the dex grid reads only
    /// stored names, so without this they stay `#id` forever).
    pub fn backfill_missing_dex_names(&mut self) {
        let entries: Vec<DexEntry> = self
            .state
            .dex
            .iter()
            .filter(|e| e.names.is_none())
            .cloned()
            .collect();
        for entry in entries {
            self.dex_resolve_chain_names(&entry);
        }
    }

    // MARK: inventory / rare candy

    pub fn rare_candy_count(&self) -> i64 {
        self.item_count(ItemKind::RareCandy)
    }

    pub fn item_count(&self, kind: ItemKind) -> i64 {
        self.state
            .inventory
            .get(kind.raw_value())
            .copied()
            .unwrap_or(0)
    }

    /// Held-item ownership — count > 0 = owned (lowers the shiny denominator).
    pub fn owns_shiny_charm(&self) -> bool {
        self.item_count(ItemKind::ShinyCharm) > 0
    }

    /// Owned items (count > 0) in `ItemKind::ALL` order.
    pub fn owned_items(&self) -> Vec<(ItemKind, i64)> {
        ItemKind::ALL
            .iter()
            .copied()
            .filter_map(|k| {
                let c = self.item_count(k);
                (c > 0).then_some((k, c))
            })
            .collect()
    }

    /// Candy usable only with an active mon + loaded line + stock — a candy
    /// must never accrue XP without the line (restart/offline).
    pub fn can_use_rare_candy(&self) -> bool {
        self.has_active() && self.current_line.is_some() && self.rare_candy_count() > 0
    }

    /// Uses one Rare Candy: +`RareCandy::XP` into the current mon. `apply_usage`
    /// handles carryover / evolution / graduation / celebration automatically.
    /// Candy XP only lands on `used_at_stage` — never on the real-usage stats.
    pub fn use_rare_candy(&mut self) -> CandyUseResult {
        if !self.can_use_rare_candy() {
            return CandyUseResult::Unavailable;
        }
        *self
            .state
            .inventory
            .entry(ItemKind::RareCandy.raw_value().to_string())
            .or_insert(0) -= 1;
        let before_stage = self
            .state
            .active
            .as_ref()
            .map(|a| a.stage_index)
            .unwrap_or(0);
        // Partial progress also shows the "+XP" feedback immediately.
        self.candy_feedback_amount = RareCandy::XP;
        self.candy_feedback_seq += 1;
        self.apply_usage(RareCandy::XP);
        if self.state.active.is_none() {
            return CandyUseResult::Graduated;
        }
        if self.state.active.as_ref().unwrap().stage_index > before_stage {
            return CandyUseResult::Evolved;
        }
        CandyUseResult::Progressed
    }

    pub fn consume_candy_feedback(&mut self) {
        self.candy_feedback_amount = 0;
    }

    // MARK: berries (Oran & Sitrus feeding)

    pub fn can_use_oran_berry(&self) -> bool {
        self.has_active() && self.current_line.is_some() && self.item_count(ItemKind::OranBerry) > 0
    }

    /// Feeds one Oran Berry (+15M XP). Boosts mood and happiness!
    pub fn use_oran_berry(&mut self) -> CandyUseResult {
        if !self.can_use_oran_berry() {
            return CandyUseResult::Unavailable;
        }
        *self
            .state
            .inventory
            .entry(ItemKind::OranBerry.raw_value().to_string())
            .or_insert(0) -= 1;
        let before_stage = self
            .state
            .active
            .as_ref()
            .map(|a| a.stage_index)
            .unwrap_or(0);
        self.candy_feedback_amount = OranBerry::XP;
        self.candy_feedback_seq += 1;
        self.berry_feedback_kind = Some("oranBerry".to_string());
        self.berry_feedback_seq += 1;
        let mon_id = self.state.active.as_ref().map(|a| a.current_id());
        let is_shiny = self
            .state
            .active
            .as_ref()
            .map(|a| a.is_shiny)
            .unwrap_or(false);
        if let Some(active) = self.state.active.as_mut() {
            active.add_ribbon("gourmet");
        }
        self.add_journal_entry(
            "berry",
            "Fed Oran Berry 🍊",
            "Boosted XP (+15M) and delighted your companion!",
            "🍊",
            mon_id,
            is_shiny,
        );
        self.apply_usage(OranBerry::XP);
        if self.state.active.is_none() {
            return CandyUseResult::Graduated;
        }
        if self.state.active.as_ref().unwrap().stage_index > before_stage {
            return CandyUseResult::Evolved;
        }
        CandyUseResult::Progressed
    }

    pub fn can_use_sitrus_berry(&self) -> bool {
        self.has_active()
            && self.current_line.is_some()
            && self.item_count(ItemKind::SitrusBerry) > 0
    }

    /// Feeds one Sitrus Berry (+50M XP + 1-hour Golden Sparkle Aura!).
    pub fn use_sitrus_berry(&mut self) -> CandyUseResult {
        if !self.can_use_sitrus_berry() {
            return CandyUseResult::Unavailable;
        }
        *self
            .state
            .inventory
            .entry(ItemKind::SitrusBerry.raw_value().to_string())
            .or_insert(0) -= 1;
        let before_stage = self
            .state
            .active
            .as_ref()
            .map(|a| a.stage_index)
            .unwrap_or(0);
        self.candy_feedback_amount = SitrusBerry::XP;
        self.candy_feedback_seq += 1;
        self.berry_feedback_kind = Some("sitrusBerry".to_string());
        self.berry_feedback_seq += 1;
        self.golden_aura_until = Some((self.clock)() + chrono::Duration::seconds(3600));
        let mon_id = self.state.active.as_ref().map(|a| a.current_id());
        let is_shiny = self
            .state
            .active
            .as_ref()
            .map(|a| a.is_shiny)
            .unwrap_or(false);
        if let Some(active) = self.state.active.as_mut() {
            active.add_ribbon("gourmet");
        }
        self.add_journal_entry(
            "berry",
            "Fed Sitrus Berry 🍊",
            "Boosted XP (+50M) and ignited a 1-hour Golden Sparkle Aura!",
            "🍊",
            mon_id,
            is_shiny,
        );
        self.apply_usage(SitrusBerry::XP);
        if self.state.active.is_none() {
            return CandyUseResult::Graduated;
        }
        if self.state.active.as_ref().unwrap().stage_index > before_stage {
            return CandyUseResult::Evolved;
        }
        CandyUseResult::Progressed
    }

    pub fn has_golden_aura(&self) -> bool {
        if let Some(until) = self.golden_aura_until {
            (self.clock)() < until
        } else {
            false
        }
    }

    pub fn consume_berry_feedback(&mut self) {
        self.berry_feedback_kind = None;
    }

    pub fn add_journal_entry(
        &mut self,
        kind: &str,
        title: &str,
        description: &str,
        icon: &str,
        species_id: Option<i64>,
        is_shiny: bool,
    ) {
        let entry = JournalEntry {
            id: Uuid::new_v4().to_string(),
            timestamp: (self.clock)().to_rfc3339(),
            kind: kind.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            icon: icon.to_string(),
            species_id,
            is_shiny,
        };
        self.state.journal.insert(0, entry);
        if self.state.journal.len() > 100 {
            self.state.journal.truncate(100);
        }
    }

    pub fn set_trainer_name(&mut self, name: &str) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            self.state.trainer_name = trimmed.to_string();
            self.save();
        }
    }

    pub fn set_trainer_avatar(&mut self, species_id: Option<i64>) {
        self.state.avatar_species_id = species_id;
        self.save();
    }

    pub fn trainer_title(&self) -> &'static str {
        match self.state.used_since_install {
            t if t >= 500_000_000 => "AI Grandmaster Champion",
            t if t >= 100_000_000 => "Elite Four AI Champion",
            t if t >= 50_000_000 => "Gym Leader Developer",
            t if t >= 10_000_000 => "Ace Prompt Engineer",
            t if t >= 1_000_000 => "Journeyman Coder",
            _ => "Novice Pokémon Trainer",
        }
    }

    pub fn pet_buddy(&mut self) {
        let earned = if let Some(active) = self.state.active.as_mut() {
            let newly = active.add_ribbon("affection");
            Some((active.current_id(), active.is_shiny, newly))
        } else {
            None
        };
        if let Some((id, is_shiny, true)) = earned {
            self.add_journal_entry(
                "ribbon",
                "Earned Best Buddy Ribbon! 💖",
                "Bonded and showed affection to your companion.",
                "💖",
                Some(id),
                is_shiny,
            );
        }
        self.save();
    }

    pub fn active_ribbons(&self) -> Vec<String> {
        let mut r = self
            .state
            .active
            .as_ref()
            .map(|a| a.ribbons.clone())
            .unwrap_or_default();
        r.sort();
        r
    }

    pub fn check_and_award_ribbons(&mut self) {
        let is_overdrive = self.is_mega_overdrive;
        let hour = (self.clock)().time().hour();
        let mon_info = if let Some(active) = self.state.active.as_mut() {
            let mut newly_earned = Vec::new();
            if active.add_ribbon("starter") {
                newly_earned.push("starter");
            }
            if active.is_shiny && active.add_ribbon("shiny") {
                newly_earned.push("shiny");
            }
            if is_overdrive && active.add_ribbon("overdrive") {
                newly_earned.push("overdrive");
            }
            if hour < 5 && active.add_ribbon("nightOwl") {
                newly_earned.push("nightOwl");
            }
            let cumulative = active.used_at_stage;
            if cumulative >= 10_000_000 && active.add_ribbon("bronzeBurner") {
                newly_earned.push("bronzeBurner");
            }
            if cumulative >= 50_000_000 && active.add_ribbon("silverBurner") {
                newly_earned.push("silverBurner");
            }
            if cumulative >= 100_000_000 && active.add_ribbon("goldBurner") {
                newly_earned.push("goldBurner");
            }
            if cumulative >= 500_000_000 && active.add_ribbon("platinumBurner") {
                newly_earned.push("platinumBurner");
            }
            Some((active.current_id(), active.is_shiny, newly_earned))
        } else {
            None
        };

        if let Some((id, is_shiny, newly_earned)) = mon_info {
            for rib in newly_earned {
                let (name, desc) = match rib {
                    "starter" => ("Starter Ribbon 🐣", "Began coding journey together."),
                    "shiny" => ("Star Sparkle Ribbon ✨", "Gleaming Shiny companion honor."),
                    "overdrive" => (
                        "Overdrive Surge Ribbon ⚡",
                        "Sprinted at blazing burn pace.",
                    ),
                    "nightOwl" => (
                        "Midnight Coder Ribbon 🌙",
                        "Burned tokens during late-night coding.",
                    ),
                    "bronzeBurner" => {
                        ("Bronze 10M Ribbon 🥉", "Passed 10 Million lifetime tokens.")
                    }
                    "silverBurner" => {
                        ("Silver 50M Ribbon 🥈", "Passed 50 Million lifetime tokens.")
                    }
                    "goldBurner" => ("Gold 100M Ribbon 🥇", "Passed 100 Million lifetime tokens."),
                    "platinumBurner" => (
                        "Titan 500M Ribbon 👑",
                        "Passed 500 Million lifetime tokens.",
                    ),
                    _ => ("Honor Ribbon 🏅", "Earned a companion achievement."),
                };
                self.add_journal_entry(
                    "ribbon",
                    &format!("Earned {}!", name),
                    desc,
                    "🏅",
                    Some(id),
                    is_shiny,
                );
            }
        }
    }

    pub fn set_mega_overdrive_enabled(&mut self, enabled: bool) {
        self.mega_overdrive_enabled = enabled;
    }

    pub fn is_mega_overdrive_active(&self) -> bool {
        self.is_mega_overdrive
    }

    // MARK: mint (nature reroll)

    /// Mint usable with an active mon + stock. Nature lives on `MonState`
    /// alone, so no line is required (usable right after restart / offline).
    pub fn can_use_mint(&self) -> bool {
        self.has_active() && self.item_count(ItemKind::Mint) > 0
    }

    /// Uses one mint — rerolls the current mon's nature to a *different* random
    /// one (always changes). Purely cosmetic. `None` = unusable (no spend).
    pub fn use_mint(&mut self) -> Option<PokemonNature> {
        if !self.can_use_mint() {
            return None;
        }
        let cur = self.state.active.as_ref().unwrap().nature;
        let pool: Vec<PokemonNature> = PokemonNature::ALL
            .iter()
            .copied()
            .filter(|n| Some(*n) != cur) // nil (legacy) → full 25 pool
            .collect();
        let new = pool[(self.rng.next_u64() % pool.len() as u64) as usize];
        self.state.active.as_mut().unwrap().nature = Some(new);
        self.state.inventory.insert(
            ItemKind::Mint.raw_value().to_string(),
            self.item_count(ItemKind::Mint) - 1,
        );
        self.mint_feedback_nature = Some(new);
        self.mint_feedback_seq += 1;
        self.save();
        Some(new)
    }

    pub fn consume_mint_feedback(&mut self) {
        self.mint_feedback_nature = None;
    }

    // MARK: shop (currency = used tokens)

    /// Spendable tokens = real-usage cumulative − shop-spending cumulative.
    /// Purchases only raise `spent_tokens` (growth meter untouched).
    pub fn available_tokens(&self) -> i64 {
        (self.state.used_since_install - self.state.spent_tokens).max(0)
    }

    /// Sold items, price-ascending, with purchased passive items sunk to the
    /// bottom (no repurchase → no reason to sit on top).
    pub fn purchasable_items(&self) -> Vec<ItemKind> {
        let mut items: Vec<ItemKind> = ItemKind::ALL
            .iter()
            .copied()
            .filter(|k| k.shop_price().is_some())
            .collect();
        items.sort_by(|a, b| {
            let a_done = a.is_passive() && self.item_count(*a) > 0;
            let b_done = b.is_passive() && self.item_count(*b) > 0;
            if a_done != b_done {
                return a_done.cmp(&b_done); // not-done first
            }
            a.shop_price()
                .unwrap_or(0)
                .cmp(&b.shop_price().unwrap_or(0))
        });
        items
    }

    /// Shop display order — sold items + (with an active mon) the 3 egg tiers
    /// merged into a single price-ascending list. Eggs are immediate actions, so
    /// they join purely on price.
    pub fn shop_entries(&self) -> Vec<ShopEntry> {
        let mut entries: Vec<ShopEntry> = self
            .purchasable_items()
            .into_iter()
            .map(ShopEntry::Item)
            .collect();
        if self.has_active() {
            entries.extend(FreshEgg::SHOP_TIERS.iter().copied().map(ShopEntry::Egg));
        }
        entries.sort_by(|a, b| {
            let a_done = is_purchased_passive(*a, self);
            let b_done = is_purchased_passive(*b, self);
            if a_done != b_done {
                return a_done.cmp(&b_done);
            }
            a.price().cmp(&b.price())
        });
        entries
    }

    /// Purchasable — wallet at or above the price (unsold → false). Passive
    /// items are one-time (no repurchase).
    pub fn can_buy(&self, kind: ItemKind) -> bool {
        let Some(price) = kind.shop_price() else {
            return false;
        };
        if kind.is_passive() && self.item_count(kind) > 0 {
            return false;
        }
        self.available_tokens() >= price
    }

    /// Buys one item — debits the wallet, inventory +1. No effect on growth /
    /// evolution progress (only the spend ledger rises). `false` = no-op.
    pub fn buy(&mut self, kind: ItemKind) -> bool {
        let Some(price) = kind.shop_price() else {
            return false;
        };
        if self.available_tokens() < price {
            return false;
        }
        if kind.is_passive() && self.item_count(kind) > 0 {
            return false; // defensive — canBuy already gates it
        }
        self.state.spent_tokens += price;
        *self
            .state
            .inventory
            .entry(kind.raw_value().to_string())
            .or_insert(0) += 1;
        self.save();
        true
    }

    /// The current egg's guaranteed rarity floor (display only). An active mon
    /// means there is no egg → nil.
    pub fn egg_guarantee(&self) -> Option<Rarity> {
        if self.state.active.is_none() {
            self.state.egg_tier
        } else {
            None
        }
    }

    /// Egg purchaseable — an active mon to discard + wallet at/above the tier
    /// price. Only sold tiers are enforceable (a legendary-only floor cannot be
    /// expressed via capture_rate and would brick the egg forever).
    pub fn can_buy_egg(&self, tier: Option<Rarity>) -> bool {
        if !FreshEgg::SHOP_TIERS.contains(&tier) {
            return false;
        }
        self.has_active() && self.available_tokens() >= FreshEgg::price(tier)
    }

    /// Buys an egg — discards the current mon (not a graduation: dex /
    /// collectedFinals untouched) and starts incubating a new egg from zero.
    /// The species is NOT rolled here (it needs the network); only the guarantee
    /// floor is recorded in state and consumed by the roll/hatch paths.
    pub fn buy_egg(&mut self, tier: Option<Rarity>) -> bool {
        if !self.can_buy_egg(tier) {
            return false;
        }
        self.state.spent_tokens += FreshEgg::price(tier);
        self.state.active = None; // discard — not graduation
        self.active_generation += 1;
        self.current_line = None;
        self.state.egg_usage = 0; // re-incubate from scratch
        self.state.egg_tier = tier;
        self.state.pending_hatch_id = None; // roll again under the new guarantee
        self.prefetched_line_id = None;
        self.just_graduated = None;
        self.just_evolved_to = None;
        self.event_until = None;
        self.ensure_egg_prefetch(); // warm up the next hatch
        self.save();
        true
    }

    pub fn can_buy_fresh_egg(&self) -> bool {
        self.can_buy_egg(None)
    }

    pub fn buy_fresh_egg(&mut self) -> bool {
        self.buy_egg(None)
    }

    /// Pure grant decision (edge trigger) — a window is only granted the moment
    /// it freshly crosses 100%.
    /// - Below 100% → removed from the map (re-arm).
    /// - Already-granted window (tier ≥ 1) → no re-grant.
    /// - session = 1 candy · weekly = `weeklyGrant`.
    pub fn evaluate_candy_grants(
        windows: &[CandyWindow],
        grant_tier: &mut HashMap<String, i64>,
    ) -> Vec<CandyGrant> {
        let mut grants = Vec::new();
        for w in windows {
            if w.utilization < 100.0 {
                grant_tier.remove(&w.key);
                continue;
            }
            let previous = grant_tier.get(&w.key).copied().unwrap_or(0);
            if previous >= 1 {
                continue;
            }
            grant_tier.insert(w.key.clone(), 1);
            let count = if w.kind == WindowClass::Weekly {
                RareCandy::WEEKLY_GRANT
            } else {
                1
            };
            grants.push(CandyGrant {
                window_key: w.key.clone(),
                window_name: w.name.clone(),
                count,
            });
        }
        grants
    }

    /// Candy grants driven by limit-window state (edge + persistent).
    /// - First run: seed tiers for already-100% windows without granting
    ///   (blocks retroactive grants) → later crossings grant.
    /// - `limits_ready` = false → wait (retried next refresh).
    pub fn grant_candies(&mut self, windows: &[CandyWindow], limits_ready: bool) {
        if !limits_ready {
            return;
        }
        if !self.state.candy_feature_seeded {
            for w in windows {
                if w.utilization >= 100.0 {
                    self.state.candy_grant_tier.insert(w.key.clone(), 1);
                }
            }
            self.state.candy_feature_seeded = true;
            self.save();
            return;
        }
        let before = self.state.candy_grant_tier.clone();
        let grants = Self::evaluate_candy_grants(windows, &mut self.state.candy_grant_tier);
        for g in &grants {
            *self
                .state
                .inventory
                .entry(ItemKind::RareCandy.raw_value().to_string())
                .or_insert(0) += g.count;
            self.notify_companion_event("", "");
        }
        // Re-arming (100% → below, tier removed) must persist even with no
        // grant — otherwise a stale tier=1 misjudges the next crossing as
        // already-granted after a restart.
        if !grants.is_empty() || self.state.candy_grant_tier != before {
            self.save();
        }
    }

    // MARK: updates (the ledger)

    /// Applies a usage snapshot tick. Port of the full Swift ledger bookkeeping:
    /// baseline seeding, date-change rebase with the zero-open `new_ledger`,
    /// same-day per-provider deltas with regression rebase, usage accumulation,
    /// event-window expiry, egg prefetch/hatch/line-load/Ditto-reveal triggers,
    /// then `compute_state` + save.
    pub fn update(
        &mut self,
        today_tokens_by_provider: &HashMap<String, i64>,
        today_date: &str,
        _month_total: i64,
        burn_tier: BurnTier,
        limit_warning: bool,
        has_usage_data: bool,
    ) {
        let is_overdrive = self.mega_overdrive_enabled
            && (burn_tier == BurnTier::Fast || burn_tier == BurnTier::Blazing);
        self.is_mega_overdrive = is_overdrive;
        let coin_multiplier = if is_overdrive { 2 } else { 1 };

        let today_tokens: i64 = today_tokens_by_provider.values().sum();
        // hasUsageData = a display snapshot exists; this map only carries data
        // whose today-date was confirmed. A stale snapshot or carrier-only
        // refresh is not an observation that may move the ledger baseline.
        let has_current_provider_data = has_usage_data && !today_tokens_by_provider.is_empty();
        if !self.state.install_baseline_set {
            // Install baseline — the first real snapshot's today becomes the
            // baseline (pre-install usage never counts).
            if !has_current_provider_data {
                // A save-load may have handed baseline judgment to this path
                // (SaveTransfer::rebased_for_this_device) — the mon may already
                // be present, so never show an egg there, and keep retrying the
                // line load.
                self.display_state = if self.state.active.is_some() {
                    CompanionStateKind::Idle
                } else {
                    CompanionStateKind::Egg
                };
                self.kick_line_load_if_needed();
                return;
            }
            self.state.install_baseline_set = true;
            self.state.claimed_today_tokens_by_provider = Some(today_tokens_by_provider.clone());
            self.state.last_date = today_date.to_string();
            self.save();
        } else if has_current_provider_data {
            let date_changed = today_date != self.state.last_date;
            if self.state.claimed_today_tokens_by_provider.is_none() {
                // Legacy save — only an aggregate high-water mark existed, not
                // per-provider values. The first valid observation only seeds the
                // new ledger's baseline; past usage is never credited retroactively.
                self.state.claimed_today_tokens_by_provider =
                    Some(today_tokens_by_provider.clone());
                self.state.last_date = today_date.to_string();
            } else if date_changed {
                // Daily snapshots are not comparable. A new date opens every
                // known provider at zero and credits that date's cumulative
                // values in full — but a provider missing on the first refresh
                // keeps its zero baseline so its recovery later credits the
                // current-day value rather than losing it.
                self.state.last_date = today_date.to_string();
                let mut new_ledger: HashMap<String, i64> = self
                    .state
                    .claimed_today_tokens_by_provider
                    .as_ref()
                    .map(|m| m.keys().map(|k| (k.clone(), 0)).collect())
                    .unwrap_or_default();
                for (provider_id, current) in today_tokens_by_provider {
                    new_ledger.insert(provider_id.clone(), *current);
                }
                self.state.claimed_today_tokens_by_provider = Some(new_ledger);
                let delta = today_tokens_by_provider.values().sum::<i64>();
                if delta > 0 {
                    self.state.used_since_install += delta * coin_multiplier;
                    *self
                        .state
                        .daily_history
                        .entry(today_date.to_string())
                        .or_insert(0) += delta;
                    if self.state.active.is_none() {
                        self.state.egg_usage += delta;
                    } else {
                        self.apply_usage(delta);
                    }
                }
            } else {
                // Same day — per provider: new provider → seed (no retro); a
                // drop below the previous baseline rebases only that provider's
                // line (no credit); otherwise the increase is credited.
                let mut ledger = self
                    .state
                    .claimed_today_tokens_by_provider
                    .clone()
                    .unwrap_or_default();
                let mut delta: i64 = 0;
                for (provider_id, current) in today_tokens_by_provider {
                    let previous = ledger.get(provider_id).copied();
                    let Some(previous) = previous else {
                        ledger.insert(provider_id.clone(), *current);
                        continue;
                    };
                    if *current < previous {
                        ledger.insert(provider_id.clone(), *current);
                        continue;
                    }
                    delta += *current - previous;
                    ledger.insert(provider_id.clone(), *current);
                }
                self.state.claimed_today_tokens_by_provider = Some(ledger);
                if delta > 0 {
                    self.state.used_since_install += delta * coin_multiplier;
                    *self
                        .state
                        .daily_history
                        .entry(today_date.to_string())
                        .or_insert(0) += delta;
                    if self.state.active.is_none() {
                        self.state.egg_usage += delta; // egg incubation accrual
                    } else {
                        self.apply_usage(delta);
                    }
                }
            }
        }
        // Event (evolve/graduate/hatch) window expiry — clears the copy flags
        // together at the end of the levelUp window. justEvolvedTo is only ever
        // cleared here (a mid-window tick must not rewind the "…evolved" copy).
        if let Some(until) = self.event_until {
            if (self.clock)() > until {
                self.just_graduated = None;
                self.just_evolved_to = None;
                self.event_until = None;
            }
        }
        // Egg prefetch — species pre-roll + line warm-up. Retried every tick
        // until it succeeds (then no-op).
        if self.state.active.is_none() && self.state.install_baseline_set && !self.is_hatching {
            self.ensure_egg_prefetch();
        }
        // Egg reaches the hatch threshold → hatch.
        if self.state.active.is_none()
            && self.state.egg_usage >= PokemonBalance::EGG_HATCH_THRESHOLD
            && !self.is_hatching
        {
            self.hatch_if_needed();
        }
        // Active with the line not yet loaded (restart) → load it.
        if self.state.active.is_some() && self.current_line.is_none() && !self.is_hatching {
            self.load_current_line();
        }
        // Disguised Ditto reached its first evolution threshold → reveal
        // (backup trigger when apply_usage never kicked).
        if let Some(active) = self.state.active.as_ref() {
            if active.ditto_disguise.is_some()
                && !active.ditto_revealed
                && self.current_line.is_some()
                && !self.is_hatching
                && !self.is_revealing_ditto
                && active.used_at_stage
                    >= PokemonBalance::phase_threshold(active.rarity, active.total_forms, 0)
            {
                self.reveal_ditto();
            }
        }
        self.display_state =
            self.compute_state(burn_tier, limit_warning, has_usage_data, today_tokens);
        self.save();
    }

    /// Applies a token delta to the current mon — evolves/graduates at stage
    /// thresholds. Usage always accrues even with the line unloaded (restart /
    /// offline) — only the evolution decision waits for the line.
    pub fn apply_usage(&mut self, delta: i64) {
        if self.state.active.is_none() {
            return;
        }
        self.state.active.as_mut().unwrap().used_at_stage += delta;
        self.check_and_award_ribbons();
        let Some(line) = self.current_line.clone() else {
            self.save();
            return;
        };
        let mut guard_count = 0;
        while self.state.active.is_some() && guard_count < 50 {
            guard_count += 1;
            let thr = {
                let active = self.state.active.as_ref().unwrap();
                PokemonBalance::phase_threshold(
                    active.rarity,
                    active.total_forms,
                    active.stage_index,
                )
            };
            let mut active = self.state.active.clone().unwrap();
            if active.used_at_stage < thr {
                break;
            }
            let Some(node) = line.tree.node_with_id(active.current_id()).cloned() else {
                break;
            };
            // Disguised Ditto reveals before any terminal graduation — the
            // disguise species must never graduate into the dex.
            if active.ditto_disguise.is_some() && !active.ditto_revealed {
                if !self.is_revealing_ditto {
                    self.reveal_ditto();
                }
                break;
            }
            if node.children.is_empty() {
                self.graduate();
                break;
            } else {
                let next_index = active.stage_index + 1;
                let next: EvoNode = {
                    let idx = next_index as usize;
                    if idx < active.planned_path_ids.len() {
                        let planned_id = active.planned_path_ids[idx];
                        if let Some(found) =
                            node.children.iter().find(|c| c.species_id == planned_id)
                        {
                            found.clone()
                        } else {
                            let n = self.pick_planned_child(&node, active.base_id);
                            let fallback_route = self.fallback_route(&node, &n, active.base_id);
                            let repaired = Self::repaired_plan(
                                &active.path_ids,
                                active.stage_index,
                                &fallback_route,
                            );
                            active.planned_path_ids = repaired;
                            active.total_forms = active.planned_path_ids.len() as i64;
                            n
                        }
                    } else {
                        let n = self.pick_planned_child(&node, active.base_id);
                        let fallback_route = self.fallback_route(&node, &n, active.base_id);
                        let repaired = Self::repaired_plan(
                            &active.path_ids,
                            active.stage_index,
                            &fallback_route,
                        );
                        active.planned_path_ids = repaired;
                        active.total_forms = active.planned_path_ids.len() as i64;
                        n
                    }
                };
                let mut new_path = active.path_ids[..=(active.stage_index as usize)].to_vec();
                new_path.push(next.species_id);
                active.path_ids = new_path;
                active.stage_index += 1;
                active.used_at_stage -= thr; // overflow carries
                let new_name = line.localized_name(next.species_id, self.state.language);
                self.just_evolved_to = Some(new_name.clone());
                self.add_journal_entry(
                    "evolution",
                    &format!("Evolved into {}! ✨", new_name),
                    &format!(
                        "Reached Stage {} after powering up with AI coding tokens.",
                        active.stage_index + 1
                    ),
                    "✨",
                    Some(next.species_id),
                    active.is_shiny,
                );
                self.fire_celebration(Celebration::Evolve);
                self.event_until = Some((self.clock)() + chrono::Duration::seconds(4));
                self.notify_companion_event("", "");
                self.state.active = Some(active);
            }
        }
        self.save();
    }

    /// Weighted toward uncollected finals (branch diversity); falls back to all
    /// children once every final is collected.
    fn pick_planned_child(&mut self, node: &EvoNode, base_id: i64) -> EvoNode {
        let fresh: Vec<&EvoNode> = node
            .children
            .iter()
            .filter(|ch| {
                ch.final_ids().iter().any(|fid| {
                    !self
                        .state
                        .collected_finals
                        .contains(&format!("{base_id}:{fid}"))
                })
            })
            .collect();
        let pool: Vec<&EvoNode> = if fresh.is_empty() {
            node.children.iter().collect()
        } else {
            fresh
        };
        let roll = (self.rng.next_u64() % pool.len() as u64) as usize;
        pool[roll].clone()
    }

    /// Builds a full root→leaf plan, rolling at each branch.
    fn make_evolution_plan(&mut self, root: &EvoNode, base_id: i64) -> Vec<i64> {
        let mut plan = vec![root.species_id];
        let mut node = root.clone();
        while !node.children.is_empty() {
            let next = self.pick_planned_child(&node, base_id);
            plan.push(next.species_id);
            node = next;
        }
        plan
    }

    /// `[node.speciesID] + makeEvolutionPlan(from: next)` — the repaired plan
    /// route used when the stored plan no longer matches the asset tree.
    fn fallback_route(&mut self, node: &EvoNode, next: &EvoNode, base_id: i64) -> Vec<i64> {
        let mut route = vec![node.species_id];
        route.extend(self.make_evolution_plan(next, base_id));
        route
    }

    /// Re-anchors a plan onto the realized path: keep the realized prefix and
    /// append the fallback route's tail (when it matches the last realized id).
    pub fn repaired_plan(
        realized_path: &[i64],
        stage_index: i64,
        fallback_route: &[i64],
    ) -> Vec<i64> {
        if realized_path.is_empty() {
            return fallback_route.to_vec();
        }
        let current_index = (stage_index as usize).min(realized_path.len() - 1);
        let prefix = realized_path[..=current_index].to_vec();
        if fallback_route.first() != prefix.last() {
            return prefix;
        }
        let mut plan = prefix;
        plan.extend_from_slice(&fallback_route[1..]);
        plan
    }

    /// The longest id path that actually follows from the root, plus its last
    /// node. A first id that differs from the root recovers to the root.
    fn longest_valid_path(&self, ids: &[i64], root: &EvoNode) -> (Vec<i64>, EvoNode) {
        let mut path = vec![root.species_id];
        let mut node = root.clone();
        if ids.first() != Some(&root.species_id) {
            return (path, node);
        }
        for id in ids.iter().skip(1) {
            let Some(child) = node.children.iter().find(|c| c.species_id == *id) else {
                break;
            };
            path.push(*id);
            node = child.clone();
        }
        (path, node)
    }

    /// Fits a saved mon's realized path + plan to the current asset tree. Only a
    /// complete plan is reused, so restart never re-rolls the RNG.
    fn normalized_evolution_state(&mut self, saved: &MonState, root: &EvoNode) -> MonState {
        let (realized_path, realized_last) = self.longest_valid_path(&saved.path_ids, root);
        let (candidate_path, candidate_last) =
            self.longest_valid_path(&saved.planned_path_ids, root);
        let can_reuse_plan = candidate_path == saved.planned_path_ids
            && candidate_path.starts_with(&realized_path)
            && candidate_last.children.is_empty();
        let plan: Vec<i64> = if can_reuse_plan {
            candidate_path
        } else {
            let suffix = self.make_evolution_plan(&realized_last, saved.base_id);
            let mut plan = realized_path.clone();
            plan.extend(suffix.into_iter().skip(1));
            plan
        };
        let mut normalized = saved.clone();
        normalized.stage_index = realized_path.len() as i64 - 1;
        normalized.path_ids = realized_path;
        normalized.planned_path_ids = plan.clone();
        normalized.total_forms = plan.len() as i64;
        normalized
    }

    /// Graduation — the final form reaches its final threshold: preserve the
    /// whole line in the dex, add the (base,final) pair to collectedFinals, and
    /// reset to a fresh egg.
    fn graduate(&mut self) {
        let Some(active) = self.state.active.clone() else {
            return;
        };
        let final_id = active.current_id();
        self.state
            .collected_finals
            .insert(format!("{}:{}", active.base_id, final_id));
        let names = self.current_line.as_ref().map(|line| {
            let mut m = HashMap::new();
            for id in &active.path_ids {
                if let Some(n) = line.names.get(id) {
                    m.insert(*id, n.clone());
                }
            }
            m
        });
        let mut final_ribbons = active.ribbons.clone();
        if !final_ribbons.iter().any(|r| r == "graduate") {
            final_ribbons.push("graduate".to_string());
        }
        self.state.dex.push(
            DexEntry::new(
                Uuid::new_v4(),
                active.base_id,
                final_id,
                active.path_ids.clone(),
                active.rarity,
                Some((self.clock)()),
                active.is_shiny,
                active.nature,
                names,
            )
            .with_ribbons(final_ribbons),
        );
        let name = self
            .current_line
            .as_ref()
            .map(|line| line.localized_name(final_id, self.state.language))
            .unwrap_or_default();
        self.just_graduated = Some(name.clone());
        self.add_journal_entry(
            "graduate",
            &format!("{} Graduated! 🎓", name),
            "Reached final evolutionary stage and entered the Pokédex Hall of Fame!",
            "🎓",
            Some(final_id),
            active.is_shiny,
        );
        self.notify_companion_event("", "");
        self.event_until = Some((self.clock)() + chrono::Duration::seconds(6));
        self.state.active = None;
        self.active_generation += 1;
        self.current_line = None;
        self.state.egg_usage = 0; // the new egg incubates from scratch
                                  // eggTier is untouched — reaching graduation means a mon existed, so the
                                  // guarantee was already consumed at hatch (hatchCore is the only place).
        self.ensure_egg_prefetch(); // warm the next hatch immediately
    }

    // MARK: hatching

    pub fn hatch_if_needed(&mut self) {
        if self.state.active.is_some()
            || self.is_hatching
            || self.state.egg_usage < PokemonBalance::EGG_HATCH_THRESHOLD
        {
            return;
        }
        // Wait only while the prefetch is mid-species-roll (pending unconfirmed)
        // — double RNG consumption. Post-pending warm-up may run concurrently.
        if self.state.pending_hatch_id.is_none() && self.prefetch_in_flight {
            return;
        }
        let generation = self.active_generation;
        self.is_hatching = true;
        // Use the prefetched species when present (line warmed → ~0 delay),
        // else roll now.
        let base: Option<i64> = if let Some(pending) = self.state.pending_hatch_id {
            Some(pending)
        } else {
            self.choose_base()
        };
        self.is_hatching = false;
        let Some(base) = base else {
            return; // network flaky → keep the egg, retry next tick
        };
        // Generation check here — the subject may have been wholly replaced
        // during the species roll (save import); discard the stale roll.
        if self.active_generation != generation || self.state.active.is_some() {
            self.kick_line_load_if_needed();
            return;
        }
        self.state.pending_hatch_id = None;
        self.hatch_core(base);
    }

    pub fn hatch(&mut self, base_id: i64) {
        if self.is_hatching {
            return;
        }
        self.is_hatching = true;
        self.hatch_core(base_id);
        self.is_hatching = false;
    }

    /// The actual hatch — the isHatching lock is owned by the caller
    /// (hatch / hatch_if_needed).
    fn hatch_core(&mut self, base_id: i64) {
        let generation = self.active_generation;
        let line = match self.provider.line(base_id) {
            Ok(line) => line,
            Err(_) => return, // egg kept, retry next tick
        };
        // The subject may have been replaced during the line fetch.
        if self.active_generation != generation {
            self.kick_line_load_if_needed();
            return;
        }
        // Last gate on the purchased guarantee — only here is the true rarity
        // known. A filter miss (stale index etc.) keeps the egg and drops the
        // pre-roll instead of handing out a lower grade.
        if let Some(tier) = self.state.egg_tier {
            if line.rarity.sort_rank() < tier.sort_rank() {
                self.state.pending_hatch_id = None;
                self.prefetched_line_id = None;
                self.save();
                return;
            }
        }
        self.current_line = Some(line.clone());
        // Hatch-threshold overflow carries into the hatched mon's growth.
        let overflow = (self.state.egg_usage - PokemonBalance::EGG_HATCH_THRESHOLD).max(0);
        self.state.egg_usage = 0;
        self.state.egg_tier = nil_opt(); // consumed by this hatch; next egg is ungated
                                         // Rolls — shiny(1/64·48) and nature(25) are fixed at hatch, kept across
                                         // evolution. The Ditto gate short-circuits so non-app builds consume no
                                         // RNG (test sequences stay stable).
        let is_shiny = Self::rolls_shiny(self.rng.next_u64(), self.owns_shiny_charm());
        let nature =
            PokemonNature::ALL[(self.rng.next_u64() % PokemonNature::ALL.len() as u64) as usize];
        let mut ditto_disguise: Option<i64> = None;
        if self.ditto_disguise_rolling_enabled
            && Self::ditto_disguise_hit(line.rarity, line.total_forms(), self.rng.next_u64())
        {
            ditto_disguise = Some(line.base_id);
        }
        let evolution_plan = self.make_evolution_plan(&line.tree, line.base_id);
        // The shiny stays hidden while disguised — the hatch look/reveal stays
        // regular (identity is revealed at reveal time).
        let show_shiny = is_shiny && ditto_disguise.is_none();
        self.active_generation += 1;
        self.state.active = Some(MonState::new(
            line.base_id,
            vec![line.base_id],
            Some(evolution_plan.clone()),
            0,
            0,
            line.rarity,
            evolution_plan.len() as i64,
            is_shiny,
            Some(nature),
            ditto_disguise,
            false,
        ));
        let name = line.localized_name(line.base_id, self.state.language);
        self.add_journal_entry(
            "hatch",
            &format!("Hatched {}! 🐣", name),
            &format!("A new {} companion joined your coding journey.", name),
            "🐣",
            Some(line.base_id),
            is_shiny,
        );
        self.notify_companion_event("", &name);
        self.just_evolved_to = None; // a fresh hatch is "growth", not "evolved"
        self.display_state = CompanionStateKind::LevelUp;
        self.event_until = Some((self.clock)() + chrono::Duration::seconds(4));
        if overflow > 0 {
            self.apply_usage(overflow); // immediate carryover (evolve/reveal as needed)
        }
        // The celebration fires after the overflow evolution so an evolve can't
        // mask the shiny-hatch burst; an instant-graduation skips it (already
        // in the dex).
        if self.state.active.is_some() {
            self.fire_celebration(Celebration::Hatch { shiny: show_shiny });
        }
        self.save();
    }

    // MARK: Ditto disguise / reveal

    /// Pure Ditto disguise roll — common · ≥2-form only, 1/128.
    pub fn ditto_disguise_hit(rarity: Rarity, total_forms: i64, roll: u64) -> bool {
        rarity == Rarity::Common
            && total_forms >= 2
            && roll.is_multiple_of(PokemonOdds::DITTO_DISGUISE_DENOMINATOR)
    }

    /// Pure shiny hatch roll — `roll % denominator == 0` (48 with the charm,
    /// else 64).
    pub fn rolls_shiny(roll: u64, charm_owned: bool) -> bool {
        let denominator = if charm_owned {
            ShinyCharm::SHINY_DENOMINATOR
        } else {
            PokemonOdds::SHINY_DENOMINATOR
        };
        roll.is_multiple_of(denominator)
    }

    /// Disguise → reveal: a Ditto that can't evolve shows its identity at the
    /// "first evolution" threshold instead of evolving. Loads the Ditto line,
    /// converts the state (rarity → Ditto's, single-form, overflow carries,
    /// isShiny/nature preserved) and fires the reveal celebration.
    fn reveal_ditto(&mut self) {
        let Some(active) = self.state.active.clone() else {
            return;
        };
        if active.ditto_disguise.is_none() || active.ditto_revealed || self.is_revealing_ditto {
            return;
        }
        let generation = self.active_generation;
        let first_evo_thr = PokemonBalance::phase_threshold(active.rarity, active.total_forms, 0);
        if active.used_at_stage < first_evo_thr {
            return; // below the threshold — defensive
        }
        self.is_revealing_ditto = true;
        let ditto_line = match self.provider.line(PokemonOdds::DITTO_SPECIES_ID) {
            Ok(line) => line,
            Err(_) => {
                self.is_revealing_ditto = false;
                return; // retry next tick
            }
        };
        let mut m = match self.state.active.clone() {
            Some(m) if m.ditto_disguise.is_some() && !m.ditto_revealed => m,
            _ => {
                self.is_revealing_ditto = false;
                return;
            }
        };
        if self.active_generation != generation {
            self.is_revealing_ditto = false;
            return;
        }
        let latest_first_evo_thr = PokemonBalance::phase_threshold(m.rarity, m.total_forms, 0);
        if m.used_at_stage < latest_first_evo_thr {
            self.is_revealing_ditto = false;
            return;
        }
        let disguise_name = self
            .current_line
            .as_ref()
            .map(|line| line.localized_name(m.base_id, self.state.language))
            .unwrap_or_else(|| format!("#{}", m.base_id));
        let carry_over = (m.used_at_stage - latest_first_evo_thr).max(0);
        // Switch to Ditto — rarity/forms from the loaded line, identity kept.
        m.base_id = ditto_line.base_id;
        let evolution_plan = self.make_evolution_plan(&ditto_line.tree, ditto_line.base_id);
        m.path_ids = vec![ditto_line.base_id];
        m.planned_path_ids = evolution_plan.clone();
        m.stage_index = 0;
        m.rarity = ditto_line.rarity;
        m.total_forms = evolution_plan.len() as i64;
        m.used_at_stage = carry_over;
        m.ditto_revealed = true;
        let shiny = m.is_shiny;
        self.state.active = Some(m);
        self.current_line = Some(ditto_line);
        self.fire_celebration(Celebration::DittoReveal { shiny });
        self.display_state = CompanionStateKind::LevelUp;
        self.event_until = Some((self.clock)() + chrono::Duration::seconds(5));
        self.notify_companion_event("", &disguise_name);
        self.save();
        self.apply_usage(0); // re-evaluate graduation with the carried overflow
        self.is_revealing_ditto = false;
    }

    /// Loads the current mon's evolution line (restart / offline recovery).
    fn load_current_line(&mut self) {
        if self.state.active.is_none() || self.current_line.is_some() || self.is_hatching {
            return;
        }
        let generation = self.active_generation;
        self.is_hatching = true;
        let base_id = self.state.active.as_ref().unwrap().base_id;
        if let Ok(line) = self.provider.line(base_id) {
            if self.active_generation != generation {
                self.is_hatching = false;
                return;
            }
            let same_subject = self
                .state
                .active
                .as_ref()
                .map(|a| a.base_id == base_id)
                .unwrap_or(false);
            if same_subject && self.current_line.is_none() {
                let normalized = {
                    let latest = self.state.active.clone().unwrap();
                    self.normalized_evolution_state(&latest, &line.tree)
                };
                self.state.active = Some(normalized);
                self.current_line = Some(line);
                self.save(); // persist the migration choice before re-evaluating
                self.apply_usage(0); // drain usage accrued while the line was unloaded
            }
        }
        self.is_hatching = false;
    }

    /// Reloads the line of a mon that survived a discarded hatch/save import —
    /// `load_current_line` demands `!is_hatching`, so a load queued mid-hatch
    /// would otherwise stay unloaded until the next tick.
    fn kick_line_load_if_needed(&mut self) {
        if self.state.active.is_some() && self.current_line.is_none() {
            self.load_current_line();
        }
    }

    // MARK: egg prefetch

    /// Prepares the hatch while egged — ① species pre-roll (`pending_hatch_id`,
    /// persistent) ② evolution-line fetch (provider cache warm-up). Sprite
    /// prefetch is deferred to a later UI phase. Fails resume on the next tick.
    fn ensure_egg_prefetch(&mut self) {
        if self.state.active.is_some() || self.is_hatching || self.prefetch_in_flight {
            return;
        }
        let generation = self.active_generation;
        self.prefetch_in_flight = true;
        if self.state.pending_hatch_id.is_none() {
            let Some(id) = self.choose_base() else {
                self.prefetch_in_flight = false;
                return; // offline → retry next tick
            };
            // A hatch completed or the whole state was replaced while rolling.
            if self.state.active.is_some() || self.active_generation != generation {
                self.prefetch_in_flight = false;
                return;
            }
            self.state.pending_hatch_id = Some(id);
            self.save();
        }
        let Some(id) = self.state.pending_hatch_id else {
            self.prefetch_in_flight = false;
            return;
        };
        if self.prefetched_line_id == Some(id) {
            self.prefetch_in_flight = false;
            return;
        }
        // Species pre-roll + line warm-up. TODO: sprite prefetch
        // (`SpriteStore.data(...)`) is deferred to the UI phase. A failed warm-up
        // leaves prefetchedLineID nil → retry next tick.
        if self.provider.line(id).is_ok() {
            self.prefetched_line_id = Some(id);
        }
        self.prefetch_in_flight = false;
    }

    // MARK: hatch candidate selection

    /// Hatched-species selection — weighted over the full gen-I–V base index:
    /// capture_rate as the weight (Caterpie 255 vs Mewtwo 3 = 85:1), already-
    /// collected bases at ½ weight, exactly one weighted roll. Falls back to
    /// REST when the index endpoint is down.
    fn choose_base(&mut self) -> Option<i64> {
        let tier = self.state.egg_tier;
        if let Ok(full) = self.provider.base_species_index() {
            if !full.is_empty() {
                // A guaranteed egg narrows candidates first — the capture_rate
                // ceiling IS the rarity floor, so legendaries pass the uncommon/
                // rare filters naturally. An empty narrowed set keeps the egg
                // (never falls back to the full pool — the guarantee must hold).
                let index: Vec<&BaseSpecies> = match tier {
                    Some(t) => full.iter().filter(|e| t.includes(e.capture_rate)).collect(),
                    None => full.iter().collect(),
                };
                if index.is_empty() {
                    return None;
                }
                let weights: Vec<i64> = index
                    .iter()
                    .map(|e| {
                        let collected = self
                            .state
                            .collected_finals
                            .iter()
                            .any(|s| s.starts_with(&format!("{}:", e.id)));
                        if collected {
                            (e.capture_rate / 2).max(1)
                        } else {
                            e.capture_rate.max(1)
                        }
                    })
                    .collect();
                let total: i64 = weights.iter().sum();
                let mut r = (self.rng.next_u64() % total as u64) as i64;
                for (i, w) in weights.iter().enumerate() {
                    r -= w;
                    if r < 0 {
                        return Some(index[i].id);
                    }
                }
                return index.last().map(|e| e.id); // unreachable (defensive)
            }
        }
        self.choose_base_via_rest()
    }

    /// REST fallback — rejection sampling over the animated-asset range. The
    /// tier guarantee is filtered with the same rule as the weighted path so an
    /// index outage never silently breaks the guarantee.
    fn choose_base_via_rest(&mut self) -> Option<i64> {
        let tier = self.state.egg_tier;
        let range = PokemonAssets::ANIMATED_SPECIES_IDS;
        let start = *range.start();
        let count = (*range.end() - start + 1) as u64;
        for _attempt in 0..16 {
            let id = (self.rng.next_u64() % count) as i64 + start;
            match self.provider.base_species(id) {
                Ok(Some(bs)) => {
                    if let Some(t) = tier {
                        if !t.includes(bs.capture_rate) {
                            continue;
                        }
                    }
                    return Some(id);
                }
                // nil = not a base (evolution intermediate) → next attempt.
                Ok(None) => continue,
                Err(_) => return None, // REST also down → keep the egg
            }
        }
        None
    }

    // MARK: display state

    fn compute_state(
        &self,
        burn_tier: BurnTier,
        limit_warning: bool,
        has_usage_data: bool,
        today: i64,
    ) -> CompanionStateKind {
        if self.state.active.is_none() {
            return CompanionStateKind::Egg;
        }
        if self.just_graduated.is_some()
            || (self.event_until.is_some() && (self.clock)() < self.event_until.unwrap())
        {
            return CompanionStateKind::LevelUp;
        }
        if limit_warning {
            return CompanionStateKind::Tired;
        }
        if !has_usage_data || today == 0 {
            return CompanionStateKind::Sleep;
        }
        match burn_tier {
            BurnTier::Idle => CompanionStateKind::Idle,
            BurnTier::Normal => CompanionStateKind::Working,
            BurnTier::Fast | BurnTier::Blazing => CompanionStateKind::Focus,
        }
    }

    // MARK: save transfer

    /// Summary used by the overwrite confirmation — what this device has now.
    pub fn transfer_summary(&self) -> SaveSummary {
        SaveSummary::new(&self.state)
    }

    /// Default file name for the export panel, drawn from the **same clock** as
    /// the envelope's exportedAt (a view calling its own now() could straddle
    /// midnight between file name and content).
    pub fn suggested_export_file_name(&self) -> String {
        SaveTransfer::suggested_file_name((self.clock)())
    }

    /// Export payload — file writing is left to the caller (the UI writes where
    /// the user chose).
    pub fn exported_save_data(
        &self,
        app_version: &str,
        device_name: &str,
    ) -> Result<Vec<u8>, SaveTransferError> {
        SaveTransfer::encode(self.state.clone(), app_version, device_name, (self.clock)())
            .map_err(|_| SaveTransferError::Serialization)
    }

    /// Applies a validated save to this device — backs up the current state,
    /// rebases to this device, saves, then reloads the line.
    pub fn apply_save(
        &mut self,
        envelope: SaveEnvelope,
        today_tokens_by_provider: &HashMap<String, i64>,
        today_date: &str,
        has_usage_data: bool,
    ) -> Result<(), SaveTransferError> {
        self.backup_state_before_import()?;
        let current = self.state.clone();
        self.state = SaveTransfer::rebased_for_this_device(
            envelope.state,
            current,
            today_tokens_by_provider.clone(),
            today_date.to_string(),
            has_usage_data,
        );
        // Invalidate every in-flight async/celebration keyed to the old subject.
        self.active_generation += 1;
        self.current_line = None;
        self.prefetched_line_id = None;
        self.just_evolved_to = None;
        self.just_graduated = None;
        self.event_until = None;
        self.celebration = None;
        // Old-subject one-shot feedback must not float over the imported mon.
        self.candy_feedback_amount = 0;
        self.mint_feedback_nature = None;
        self.display_state = if self.state.active.is_some() {
            CompanionStateKind::Idle
        } else {
            CompanionStateKind::Egg
        };
        self.save();
        if self.state.active.is_some() {
            self.load_current_line();
        }
        Ok(())
    }

    /// Saves the pre-overwrite state next to the file — the user's promised
    /// rollback. A fresh slot per import (the second import must not overwrite
    /// the original), oldest pruned beyond the keep count.
    fn backup_state_before_import(&self) -> Result<PathBuf, SaveTransferError> {
        let data = serde_json::to_vec(&self.state).map_err(|_| SaveTransferError::BackupFailed)?;
        let dir = self
            .file_url
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let backup = dir.join(SaveTransfer::backup_file_name((self.clock)()));
        if atomic_write(&backup, &data).is_err() {
            return Err(SaveTransferError::BackupFailed);
        }
        self.prune_import_backups(&dir);
        Ok(backup)
    }

    /// Keeps the most recent N backups and deletes the rest. Backup file names
    /// are second-stamped, so lexical order = chronological order.
    fn prune_import_backups(&self, dir: &Path) {
        let Ok(names) = std::fs::read_dir(dir) else {
            return;
        };
        let mut backups: Vec<String> = names
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(SaveTransfer::BACKUP_FILE_PREFIX)
                    .then_some(name)
            })
            .collect();
        backups.sort();
        if backups.len() <= SaveTransfer::BACKUPS_TO_KEEP {
            return;
        }
        let stale = backups.len() - SaveTransfer::BACKUPS_TO_KEEP;
        for name in backups.into_iter().take(stale) {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }

    // MARK: persistence

    fn load(&mut self) {
        let Ok(data) = std::fs::read(&self.file_url) else {
            return; // no file = fresh install
        };
        match serde_json::from_slice::<CompanionState>(&data) {
            Ok(s) => {
                // The same trust-boundary normalization as imports also runs on
                // disk reads — a stored extreme value that *decodes* successfully
                // would otherwise re-kill arithmetic on every startup.
                self.state = SaveTransfer::sanitized(s);
            }
            Err(_) => {
                // Total decode failure (front corruption / future schema) →
                // start fresh, but preserve the original as `.corrupt` before
                // the next save() overwrites it forever.
                let backup = self.file_url.with_extension("json.corrupt");
                let _ = std::fs::remove_file(&backup);
                let _ = std::fs::rename(&self.file_url, &backup);
            }
        }
    }

    fn save(&self) {
        let Ok(data) = serde_json::to_vec_pretty(&self.state) else {
            return;
        };
        let _ = atomic_write(&self.file_url, &data);
    }

    // MARK: notifications

    /// Companion-event system notification. TODO: notifications arrive in a
    /// later phase — this is a no-op that only advances the sequence counter.
    /// All call sites still call it; nothing is logged.
    fn notify_companion_event(&mut self, _title: &str, _body: &str) {
        self.notif_seq += 1;
    }
}

/// Deterministic pseudo-UUID for the synthesized "active" dex entry so
/// `is_active_dex_entry` can recognize it across recomputations (Swift used a
/// string id `active-{base}-{current}`).
fn active_entry_id(base_id: i64, current_id: i64) -> Uuid {
    let s = format!("active-{base_id}-{current_id}");
    let mut hash: u128 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= b as u128;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let mut bytes = hash.to_be_bytes();
    bytes[6] = (bytes[6] & 0x0F) | 0x50; // UUID v5-style version bits (cosmetic)
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // RFC 4122 variant bits
    Uuid::from_bytes(bytes)
}

/// `state.egg_tier = nil` — written as an explicit helper so the "guarantee is
/// consumed by this hatch" intent reads clearly.
fn nil_opt() -> Option<Rarity> {
    None
}

/// Whether a shop entry is an already-purchased passive item (sinks to the
/// bottom of `shopEntries`). Eggs are immediate actions — no "owned" concept.
fn is_purchased_passive(entry: ShopEntry, store: &CompanionStore) -> bool {
    match entry {
        ShopEntry::Item(kind) => kind.is_passive() && store.item_count(kind) > 0,
        ShopEntry::Egg(_) => false,
    }
}

/// Mirrors Swift's `.atomic` write: temp file + rename.
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    #[cfg(windows)]
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)
}

/// Default state file location. Base = `platform::data_dir()`; a non-empty
/// `PTB_STATE_DIR` env var overrides the *directory* (dev/QA isolation — blank
/// values are ignored so an empty override never resolves to CWD).
fn default_file_url() -> PathBuf {
    let override_dir = std::env::var("PTB_STATE_DIR")
        .unwrap_or_default()
        .trim()
        .to_string();
    let dir = if override_dir.is_empty() {
        platform::data_dir()
    } else {
        PathBuf::from(override_dir)
    };
    let _ = std::fs::create_dir_all(&dir);
    dir.join("companion-state.json")
}

#[cfg(test)]
mod tests {
    // The tests deliberately mirror the Swift style: construct the default
    // state then mutate the fields one at a time.
    #![allow(clippy::field_reassign_with_default)]

    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::companion::{AppLanguage, DexEntry, MonState, PokemonNature, Rarity};
    use crate::providers::pokeapi::PokeError;
    use uuid::Uuid;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn node(id: i64, children: Vec<EvoNode>) -> EvoNode {
        EvoNode::new(id, children)
    }

    fn leaf(id: i64) -> EvoNode {
        node(id, Vec::new())
    }

    fn all_ids(n: &EvoNode) -> Vec<i64> {
        let mut out = vec![n.species_id];
        for c in &n.children {
            out.extend(all_ids(c));
        }
        out
    }

    fn make_line(base: i64, tree: EvoNode, rarity: Rarity) -> EvoLine {
        let mut names = HashMap::new();
        for id in all_ids(&tree) {
            names.insert(
                id,
                HashMap::from([
                    ("en".to_string(), format!("P{id}")),
                    ("ko".to_string(), format!("포{id}")),
                    ("ja".to_string(), format!("ポ{id}")),
                ]),
            );
        }
        EvoLine::new(base, tree, rarity, names)
    }

    /// A 3-stage linear common line: 1 → 2 → 3 (125M / 250M / 375M).
    fn linear3() -> EvoLine {
        make_line(1, node(1, vec![node(2, vec![leaf(3)])]), Rarity::Common)
    }

    /// A single-form common line (750M single threshold).
    fn no_evo() -> EvoLine {
        make_line(20, leaf(20), Rarity::Common)
    }

    /// A branching line 265 → {266 → 267, 268 → 269}.
    fn wurmple_line() -> EvoLine {
        make_line(
            265,
            node(
                265,
                vec![node(266, vec![leaf(267)]), node(268, vec![leaf(269)])],
            ),
            Rarity::Common,
        )
    }

    /// A branching line 10 → {11, 12, 13} (single-form branches).
    fn branch3() -> EvoLine {
        make_line(
            10,
            node(10, vec![leaf(11), leaf(12), leaf(13)]),
            Rarity::Common,
        )
    }

    struct StubProvider {
        line: EvoLine,
        index: Vec<BaseSpecies>,
        line_calls: AtomicUsize,
    }

    impl StubProvider {
        fn new(line: EvoLine) -> Self {
            Self {
                index: vec![BaseSpecies {
                    id: line.base_id,
                    capture_rate: 255,
                }],
                line,
                line_calls: AtomicUsize::new(0),
            }
        }
    }

    impl PokeProvider for StubProvider {
        fn line(&self, _base_species_id: i64) -> Result<EvoLine, PokeError> {
            self.line_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.line.clone())
        }
        fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
            Ok(self.index.clone())
        }
        fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
            Ok(self.index.iter().find(|b| b.id == id).cloned())
        }
    }

    /// Ditto provider — id 132 returns the Ditto line, anything else the
    /// disguise line.
    struct DittoProvider {
        disguise: EvoLine,
        ditto: EvoLine,
    }

    impl PokeProvider for DittoProvider {
        fn line(&self, base_species_id: i64) -> Result<EvoLine, PokeError> {
            if base_species_id == PokemonOdds::DITTO_SPECIES_ID {
                Ok(self.ditto.clone())
            } else {
                Ok(self.disguise.clone())
            }
        }
        fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
            Ok(vec![BaseSpecies {
                id: self.disguise.base_id,
                capture_rate: 255,
            }])
        }
        fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
            Ok((id == self.disguise.base_id).then_some(BaseSpecies {
                id,
                capture_rate: 255,
            }))
        }
    }

    struct StoreBuilder {
        provider: Box<dyn PokeProvider>,
        clock: fn() -> DateTime<Utc>,
        rng: Vec<u64>,
        ditto: bool,
        seed_state: Option<CompanionState>,
    }

    impl StoreBuilder {
        fn new(provider: Box<dyn PokeProvider>) -> Self {
            Self {
                provider,
                clock: now,
                rng: Vec::new(),
                ditto: false,
                seed_state: None,
            }
        }

        fn rng(mut self, values: Vec<u64>) -> Self {
            self.rng = values;
            self
        }

        fn ditto_rolling(mut self, on: bool) -> Self {
            self.ditto = on;
            self
        }

        fn seed_state(mut self, state: CompanionState) -> Self {
            self.seed_state = Some(state);
            self
        }

        fn build(self) -> CompanionStore {
            let dir = std::env::temp_dir().join(format!("ptb-store-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let file_url = dir.join("companion-state.json");
            if let Some(state) = self.seed_state {
                let data = serde_json::to_vec(&state).unwrap();
                std::fs::write(&file_url, data).unwrap();
            }
            CompanionStore::new(
                self.provider,
                self.clock,
                Some(file_url),
                Box::new(SequenceRng::new(self.rng)),
                self.ditto,
            )
        }
    }

    fn store_for(line: EvoLine, rng: Vec<u64>) -> CompanionStore {
        StoreBuilder::new(Box::new(StubProvider::new(line)))
            .rng(rng)
            .build()
    }

    fn update_map(
        s: &mut CompanionStore,
        map: &HashMap<String, i64>,
        date: &str,
        has_usage_data: bool,
    ) {
        s.update(map, date, 0, BurnTier::Idle, false, has_usage_data);
    }

    fn map1(k: &str, v: i64) -> HashMap<String, i64> {
        HashMap::from([(k.to_string(), v)])
    }

    fn base(s: &mut CompanionStore) {
        update_map(s, &map1("test", 0), "d1", true);
    }

    fn use_today(s: &mut CompanionStore, today: i64) {
        update_map(s, &map1("test", today), "d1", true);
    }

    fn default_store() -> CompanionStore {
        store_for(linear3(), Vec::new())
    }

    // MARK: persistence / decode recovery

    #[test]
    fn corrupt_state_file_backed_up_before_reset() {
        let dir = std::env::temp_dir().join(format!("ptb-corrupt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = dir.join("companion-state.json");
        std::fs::write(&url, "this is not valid json {{{ 손상").unwrap();

        let _s = CompanionStore::new(
            Box::new(StubProvider::new(linear3())),
            now,
            Some(url.clone()),
            Box::new(SequenceRng::new(Vec::new())),
            false,
        );

        let backup = url.with_extension("json.corrupt");
        assert!(
            backup.exists(),
            "corrupt original must be backed up as .corrupt"
        );
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "this is not valid json {{{ 손상",
            "backup content = original verbatim"
        );
        assert!(!url.exists(), "original moved away");
    }

    #[test]
    fn corrupt_active_falls_back_to_egg_while_rest_survives() {
        let dir = std::env::temp_dir().join(format!("ptb-active-corrupt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = dir.join("companion-state.json");
        let json = r#"{"active":{"baseID":1},"dex":[{"baseID":1,"finalID":3,"chainOrder":[1,2,3],"rarity":"common"}],"inventory":{"rareCandy":3},"usedSinceInstall":5000}"#;
        std::fs::write(&url, json).unwrap();

        let s = CompanionStore::new(
            Box::new(StubProvider::new(linear3())),
            now,
            Some(url),
            Box::new(SequenceRng::new(Vec::new())),
            false,
        );
        assert!(s.state.active.is_none());
        assert_eq!(s.state.dex.len(), 1);
        assert_eq!(s.state.inventory.get("rareCandy"), Some(&3));
        assert_eq!(s.state.used_since_install, 5000);
    }

    #[test]
    fn save_load_round_trip_preserves_state() {
        let line = linear3();
        let mut store = StoreBuilder::new(Box::new(StubProvider::new(line.clone())))
            .rng(vec![1, 2, 3, 4, 5])
            .build();
        store.hatch(1);
        let saved_used = store.state.used_since_install;
        store.set_language(AppLanguage::Ja);
        let url = store.file_url.clone();

        let reloaded = CompanionStore::new(
            Box::new(StubProvider::new(line)),
            now,
            Some(url),
            Box::new(SequenceRng::new(Vec::new())),
            false,
        );
        assert_eq!(
            reloaded.state.active.as_ref().map(|a| a.current_id()),
            Some(1)
        );
        assert_eq!(reloaded.language(), AppLanguage::Ja);
        assert_eq!(reloaded.state.used_since_install, saved_used);
    }

    // MARK: ledger bookkeeping

    #[test]
    fn install_baseline_excludes_pre_install_usage() {
        let mut s = default_store();
        update_map(&mut s, &map1("test", 0), "d1", false);
        assert!(!s.state.install_baseline_set);

        update_map(&mut s, &map1("test", 48_000_000), "d1", true);
        assert!(s.state.install_baseline_set);
        assert_eq!(s.state.used_since_install, 0);

        update_map(&mut s, &map1("test", 148_000_000), "d1", true);
        assert_eq!(s.state.used_since_install, 100_000_000);
    }

    #[test]
    fn usage_increase_after_valid_drop_continues_egg_progress() {
        let mut s = default_store();
        base(&mut s);
        use_today(&mut s, 200);
        assert_eq!(s.state.egg_usage, 200);

        use_today(&mut s, 40);
        assert_eq!(s.state.egg_usage, 200);
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap()["test"],
            40
        );

        use_today(&mut s, 75);
        assert_eq!(s.state.egg_usage, 235);
        assert_eq!(s.state.used_since_install, 235);
    }

    #[test]
    fn empty_usage_snapshot_does_not_rebase_daily_ledger() {
        let mut s = default_store();
        base(&mut s);
        use_today(&mut s, 200);

        update_map(&mut s, &map1("test", 0), "d1", false);
        assert_eq!(s.state.egg_usage, 200);
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap()["test"],
            200
        );

        use_today(&mut s, 250);
        assert_eq!(s.state.egg_usage, 250);
        assert_eq!(s.state.used_since_install, 250);
    }

    #[test]
    fn provider_ledger_does_not_recredit_missing_provider_after_partial_snapshot_loss() {
        let mut s = default_store();
        update_map(
            &mut s,
            &HashMap::from([("claude_code".to_string(), 0), ("codex".to_string(), 0)]),
            "d1",
            true,
        );
        update_map(
            &mut s,
            &HashMap::from([
                ("claude_code".to_string(), 1_000),
                ("codex".to_string(), 500),
            ]),
            "d1",
            true,
        );
        assert_eq!(s.state.used_since_install, 1_500);

        update_map(&mut s, &map1("claude_code", 1_000), "d1", true);
        assert_eq!(s.state.used_since_install, 1_500);
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap(),
            &HashMap::from([
                ("claude_code".to_string(), 1_000),
                ("codex".to_string(), 500)
            ])
        );

        update_map(
            &mut s,
            &HashMap::from([
                ("claude_code".to_string(), 1_000),
                ("codex".to_string(), 500),
            ]),
            "d1",
            true,
        );
        assert_eq!(s.state.used_since_install, 1_500);
        update_map(
            &mut s,
            &HashMap::from([
                ("claude_code".to_string(), 1_000),
                ("codex".to_string(), 700),
            ]),
            "d1",
            true,
        );
        assert_eq!(s.state.used_since_install, 1_700);
    }

    #[test]
    fn date_rollover_credits_current_day_usage() {
        let mut s = default_store();
        update_map(&mut s, &map1("codex", 0), "d1", true);
        update_map(&mut s, &map1("codex", 200), "d1", true);
        assert_eq!(s.state.used_since_install, 200);

        update_map(&mut s, &map1("codex", 100), "d2", true);
        assert_eq!(s.state.used_since_install, 300);
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap()["codex"],
            100
        );

        update_map(&mut s, &map1("codex", 150), "d2", true);
        assert_eq!(s.state.used_since_install, 350);
    }

    #[test]
    fn late_provider_recovery_after_date_rollover_credits_current_day_usage() {
        let mut s = default_store();
        update_map(
            &mut s,
            &HashMap::from([("claude_code".to_string(), 0), ("codex".to_string(), 0)]),
            "d1",
            true,
        );
        update_map(
            &mut s,
            &HashMap::from([
                ("claude_code".to_string(), 1_000),
                ("codex".to_string(), 500),
            ]),
            "d1",
            true,
        );
        assert_eq!(s.state.used_since_install, 1_500);

        update_map(&mut s, &map1("claude_code", 100), "d2", true);
        assert_eq!(s.state.used_since_install, 1_600);
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap(),
            &HashMap::from([("claude_code".to_string(), 100), ("codex".to_string(), 0)])
        );

        update_map(
            &mut s,
            &HashMap::from([("claude_code".to_string(), 100), ("codex".to_string(), 700)]),
            "d2",
            true,
        );
        assert_eq!(s.state.used_since_install, 2_300);

        update_map(
            &mut s,
            &HashMap::from([("claude_code".to_string(), 100), ("codex".to_string(), 900)]),
            "d2",
            true,
        );
        assert_eq!(s.state.used_since_install, 2_500);
    }

    #[test]
    fn stale_snapshot_does_not_consume_date_boundary() {
        let mut s = default_store();
        update_map(&mut s, &map1("codex", 0), "d1", true);
        update_map(&mut s, &map1("codex", 200), "d1", true);

        update_map(&mut s, &HashMap::new(), "d2", true);
        assert_eq!(s.state.used_since_install, 200);
        assert_eq!(s.state.last_date, "d1");
        assert_eq!(
            s.state.claimed_today_tokens_by_provider.as_ref().unwrap()["codex"],
            200
        );

        update_map(&mut s, &map1("codex", 100), "d2", true);
        assert_eq!(s.state.used_since_install, 300);
    }

    // MARK: hatch

    #[test]
    fn egg_does_not_hatch_below_threshold() {
        let mut s = default_store();
        base(&mut s);
        use_today(&mut s, 500_000);
        assert_eq!(s.state.egg_usage, 500_000);
        assert!(s.is_egg());
        s.hatch_if_needed();
        assert!(s.state.active.is_none());
    }

    #[test]
    fn egg_hatches_at_threshold() {
        let mut s = default_store();
        base(&mut s);
        use_today(&mut s, PokemonBalance::EGG_HATCH_THRESHOLD);
        assert!(s.state.active.is_some());
        assert_eq!(s.state.egg_usage, 0);
    }

    #[test]
    fn egg_overflow_carries_to_hatched_mon() {
        let mut s = default_store();
        base(&mut s);
        use_today(&mut s, PokemonBalance::EGG_HATCH_THRESHOLD + 500_000);
        assert_eq!(s.state.active.as_ref().unwrap().used_at_stage, 500_000);
    }

    #[test]
    fn hatch_assigns_deterministic_shiny_and_nature() {
        let shiny_roll = PokemonOdds::SHINY_DENOMINATOR; // multiple of 64 → shiny
        let mut s = store_for(linear3(), vec![shiny_roll, 0, 0]);
        s.hatch(1);
        let active = s.state.active.as_ref().unwrap();
        assert!(active.is_shiny);
        assert_eq!(active.nature, Some(PokemonNature::ALL[0]));

        let mut s2 = store_for(linear3(), vec![1, 1, 1]);
        s2.hatch(1);
        let active2 = s2.state.active.as_ref().unwrap();
        assert!(!active2.is_shiny);
        assert_eq!(active2.nature, Some(PokemonNature::ALL[1]));
    }

    #[test]
    fn hatch_consumes_guarantee_and_resets_pending() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.egg_usage = PokemonBalance::EGG_HATCH_THRESHOLD;
        state.egg_tier = Some(Rarity::Rare);
        state.pending_hatch_id = Some(1);
        let rare_line = make_line(99, leaf(99), Rarity::Rare);
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(rare_line)))
            .seed_state(state)
            .rng(vec![0, 0])
            .build();
        s.hatch_if_needed();
        assert!(s.state.active.is_some());
        assert!(s.state.egg_tier.is_none());
        assert!(s.state.pending_hatch_id.is_none());
    }

    // MARK: evolve / graduate

    #[test]
    fn evolves_through_line_and_graduates_with_full_chain() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        assert_eq!(s.current_species_id(), Some(1));

        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 0));
        assert_eq!(s.current_species_id(), Some(2));
        assert_eq!(s.just_evolved_to.as_deref(), Some("P2"));

        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 1));
        assert_eq!(s.current_species_id(), Some(3));
        assert!(s.is_final_stage());

        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 2));
        assert!(s.state.active.is_none());
        assert_eq!(s.state.dex.len(), 1);
        assert_eq!(s.state.dex[0].chain_order, vec![1, 2, 3]);
        assert_eq!(s.just_graduated.as_deref(), Some("P3"));
    }

    #[test]
    fn evolve_carries_overflow() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        let thr = PokemonBalance::phase_threshold(Rarity::Common, 3, 0);
        s.apply_usage(thr + 42);
        assert_eq!(s.state.active.as_ref().unwrap().stage_index, 1);
        assert_eq!(s.state.active.as_ref().unwrap().used_at_stage, 42);
    }

    #[test]
    fn no_evolution_graduates_at_single_threshold() {
        let mut s = store_for(no_evo(), vec![1, 2, 3]);
        s.hatch(20);
        assert!(s.is_final_stage());
        s.apply_usage(PokemonBalance::graduation_total(Rarity::Common));
        assert_eq!(s.state.dex.len(), 1);
        assert_eq!(s.state.dex[0].chain_order, vec![20]);
        assert!(s.state.active.is_none());
        assert_eq!(s.state.egg_usage, 0, "new egg re-incubates");
    }

    #[test]
    fn graduation_appends_dex_entry_and_collects_final() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 0));
        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 1));
        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 2));
        assert!(s.state.collected_finals.contains("1:3"));
        assert_eq!(s.state.dex.len(), 1);
        assert_eq!(s.state.dex[0].final_id, 3);
    }

    #[test]
    fn celebration_fires_on_hatch_and_evolve() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(s.celebration_seq, 0);
        s.hatch(1);
        assert_eq!(s.celebration_seq, 1);
        assert_eq!(s.celebration, Some(Celebration::Hatch { shiny: false }));
        s.consume_celebration();
        assert!(s.celebration.is_none());
        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 0));
        assert_eq!(s.celebration_seq, 2);
        assert_eq!(s.celebration, Some(Celebration::Evolve));
    }

    // MARK: rare candy

    #[test]
    fn use_progresses_without_evolution() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        give_candies(&mut s, 1);
        assert_eq!(s.rare_candy_count(), 1);
        let before = s.state.used_since_install;
        let result = s.use_rare_candy();
        assert_eq!(result, CandyUseResult::Progressed);
        assert_eq!(
            s.state.active.as_ref().unwrap().used_at_stage,
            RareCandy::XP
        );
        assert_eq!(s.state.active.as_ref().unwrap().stage_index, 0);
        assert_eq!(s.rare_candy_count(), 0);
        assert_eq!(s.state.used_since_install, before);
    }

    #[test]
    fn use_evolves_when_crossing_threshold() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        s.apply_usage(50_000_000);
        give_candies(&mut s, 1);
        let result = s.use_rare_candy();
        assert_eq!(result, CandyUseResult::Evolved);
        assert_eq!(s.current_species_id(), Some(2));
        assert_eq!(s.state.active.as_ref().unwrap().stage_index, 1);
    }

    #[test]
    fn single_candy_advances_at_most_one_stage() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        s.apply_usage(124_000_000);
        give_candies(&mut s, 1);
        _ = s.use_rare_candy();
        assert_eq!(s.state.active.as_ref().unwrap().stage_index, 1);
    }

    #[test]
    fn use_graduates_final_stage() {
        let mut s = store_for(no_evo(), vec![1, 2, 3]);
        s.hatch(20);
        s.apply_usage(700_000_000);
        give_candies(&mut s, 1);
        let result = s.use_rare_candy();
        assert_eq!(result, CandyUseResult::Graduated);
        assert!(s.state.active.is_none());
        assert_eq!(s.dex_entries().len(), 1);
    }

    #[test]
    fn cannot_use_on_egg() {
        let mut s = default_store();
        give_candies(&mut s, 2);
        assert!(s.is_egg());
        assert!(!s.can_use_rare_candy());
        assert_eq!(s.use_rare_candy(), CandyUseResult::Unavailable);
        assert_eq!(s.rare_candy_count(), 2);
    }

    #[test]
    fn use_bumps_candy_feedback() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        give_candies(&mut s, 1);
        let before = s.candy_feedback_seq;
        _ = s.use_rare_candy();
        assert_eq!(s.candy_feedback_seq, before + 1);
        assert_eq!(s.candy_feedback_amount, RareCandy::XP);
        s.consume_candy_feedback();
        assert_eq!(s.candy_feedback_amount, 0);
    }

    // MARK: candy grants

    #[test]
    fn session_grant_evaluation_grants_one() {
        let mut tier: HashMap<String, i64> = HashMap::new();
        let grants = CompanionStore::evaluate_candy_grants(
            &[window("s", WindowClass::Session, 100.0)],
            &mut tier,
        );
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].count, 1);
        assert_eq!(tier["s"], 1);
    }

    #[test]
    fn weekly_grant_evaluation_grants_five() {
        let mut tier: HashMap<String, i64> = HashMap::new();
        let grants = CompanionStore::evaluate_candy_grants(
            &[window("wk", WindowClass::Weekly, 100.0)],
            &mut tier,
        );
        assert_eq!(grants[0].count, RareCandy::WEEKLY_GRANT);
    }

    #[test]
    fn below_100_evaluates_no_grant_and_removes() {
        let mut tier: HashMap<String, i64> = HashMap::from([("s".to_string(), 1)]);
        let grants = CompanionStore::evaluate_candy_grants(
            &[window("s", WindowClass::Session, 40.0)],
            &mut tier,
        );
        assert!(grants.is_empty());
        assert!(!tier.contains_key("s"), "below 100 → re-arm (remove)");
    }

    #[test]
    fn no_double_grant_while_at_100() {
        let mut tier: HashMap<String, i64> = HashMap::new();
        _ = CompanionStore::evaluate_candy_grants(
            &[window("s", WindowClass::Session, 100.0)],
            &mut tier,
        );
        let again = CompanionStore::evaluate_candy_grants(
            &[window("s", WindowClass::Session, 100.0)],
            &mut tier,
        );
        assert!(again.is_empty());
    }

    #[test]
    fn grant_candies_seed_vs_grant() {
        let mut s = default_store();
        s.grant_candies(&[], true); // seed, no grant
        assert_eq!(s.rare_candy_count(), 0);
        assert!(s.state.candy_feature_seeded);

        s.grant_candies(&[window("t.0", WindowClass::Session, 100.0)], true);
        assert_eq!(s.rare_candy_count(), 1);

        s.grant_candies(&[window("t.0", WindowClass::Session, 100.0)], true);
        assert_eq!(
            s.rare_candy_count(),
            1,
            "already-granted window no re-grant"
        );
    }

    #[test]
    fn grant_candies_weekly_grants_five() {
        let mut s = default_store();
        s.grant_candies(&[], true);
        s.grant_candies(
            &[window("claude.sevenDay", WindowClass::Weekly, 100.0)],
            true,
        );
        assert_eq!(s.rare_candy_count(), RareCandy::WEEKLY_GRANT);
    }

    #[test]
    fn grant_candies_waits_when_limits_not_ready() {
        let mut s = default_store();
        s.grant_candies(&[window("s", WindowClass::Session, 100.0)], false);
        assert!(!s.state.candy_feature_seeded);
        assert_eq!(s.rare_candy_count(), 0);
    }

    // MARK: mint

    #[test]
    fn use_mint_changes_nature_to_different() {
        let mut state = CompanionState::default();
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            Some(PokemonNature::Adamant),
            None,
            false,
        ));
        state.inventory.insert("mint".to_string(), 1);
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .rng(vec![0, 1, 2, 3]) // pool excludes Adamant → index 0 = Hardy
            .build();
        let new = s.use_mint().unwrap();
        assert_ne!(new, PokemonNature::Adamant);
        assert_eq!(s.state.active.as_ref().unwrap().nature, Some(new));
        assert_eq!(s.item_count(ItemKind::Mint), 0);
    }

    #[test]
    fn use_mint_from_nil_nature_sets_valid() {
        let mut state = CompanionState::default();
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));
        state.inventory.insert("mint".to_string(), 1);
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .rng(vec![0])
            .build();
        let new = s.use_mint();
        assert!(new.is_some());
        assert_eq!(s.state.active.as_ref().unwrap().nature, new);
    }

    #[test]
    fn cannot_use_mint_on_egg_or_without_stock() {
        let mut s = default_store();
        assert!(!s.can_use_mint());
        assert!(s.use_mint().is_none());

        let mut state = CompanionState::default();
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            Some(PokemonNature::Adamant),
            None,
            false,
        ));
        let mut s2 = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .rng(vec![0])
            .build();
        assert!(!s2.can_use_mint());
        assert!(s2.use_mint().is_none());
    }

    // MARK: shop

    fn wallet_store(used: i64, spent: i64, inventory: &[(&str, i64)]) -> CompanionStore {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.used_since_install = used;
        state.spent_tokens = spent;
        for (k, v) in inventory {
            state.inventory.insert(k.to_string(), *v);
        }
        StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build()
    }

    #[test]
    fn available_tokens_never_negative() {
        let s = wallet_store(100_000_000, 500_000_000, &[]);
        assert_eq!(s.available_tokens(), 0);
        assert_eq!(
            wallet_store(1_000_000_000, 300_000_000, &[]).available_tokens(),
            700_000_000
        );
    }

    #[test]
    fn buy_debits_wallet_and_credits_inventory() {
        let mut s = wallet_store(1_000_000_000, 0, &[]);
        assert!(s.buy(ItemKind::RareCandy));
        assert_eq!(s.rare_candy_count(), 1);
        assert_eq!(s.state.spent_tokens, RareCandy::PRICE);
        assert_eq!(s.available_tokens(), 1_000_000_000 - RareCandy::PRICE);
        assert_eq!(s.state.used_since_install, 1_000_000_000);
    }

    #[test]
    fn buy_insufficient_is_no_op() {
        let mut s = wallet_store(400_000_000, 0, &[]);
        assert!(!s.buy(ItemKind::RareCandy));
        assert_eq!(s.rare_candy_count(), 0);
        assert_eq!(s.state.spent_tokens, 0);
    }

    #[test]
    fn can_buy_at_exact_price_and_not_below() {
        assert!(wallet_store(RareCandy::PRICE, 0, &[]).can_buy(ItemKind::RareCandy));
        assert!(!wallet_store(RareCandy::PRICE - 1, 0, &[]).can_buy(ItemKind::RareCandy));
    }

    #[test]
    fn items_sorted_by_price_ascending() {
        let items = wallet_store(0, 0, &[]).purchasable_items();
        assert_eq!(
            items,
            vec![
                ItemKind::OranBerry,
                ItemKind::Mint,
                ItemKind::SitrusBerry,
                ItemKind::RareCandy,
                ItemKind::ShinyCharm
            ]
        );
    }

    #[test]
    fn owned_passive_sinks_to_bottom() {
        let s = wallet_store(0, 0, &[("shinyCharm", 1)]);
        assert_eq!(s.item_count(ItemKind::ShinyCharm), 1);
        assert_eq!(s.purchasable_items().last(), Some(&ItemKind::ShinyCharm));
    }

    #[test]
    fn passive_is_one_time_purchase() {
        let mut s = wallet_store(10_000_000_000, 0, &[("shinyCharm", 1)]);
        assert!(!s.can_buy(ItemKind::ShinyCharm));
        let before = s.state.spent_tokens;
        assert!(!s.buy(ItemKind::ShinyCharm));
        assert_eq!(s.state.spent_tokens, before);
        assert_eq!(s.item_count(ItemKind::ShinyCharm), 1);
    }

    #[test]
    fn shiny_charm_rolls_flip_denominator() {
        assert!(CompanionStore::rolls_shiny(48, true));
        assert!(!CompanionStore::rolls_shiny(48, false));
        assert!(CompanionStore::rolls_shiny(64, false));
        assert!(!CompanionStore::rolls_shiny(64, true));
        assert!(CompanionStore::rolls_shiny(0, true));
        assert!(!CompanionStore::rolls_shiny(1, false));
    }

    #[test]
    fn shop_entries_interleave_fresh_egg_by_price() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.used_since_install = 5_000_000_000;
        state.active = Some(MonState::new(
            10,
            vec![10],
            Some(vec![10, 11, 12]),
            0,
            200_000_000,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));
        let s = StoreBuilder::new(Box::new(StubProvider::new(branch3())))
            .seed_state(state)
            .build();
        assert!(s.has_active());
        assert_eq!(
            s.shop_entries(),
            vec![
                ShopEntry::Item(ItemKind::OranBerry),
                ShopEntry::Item(ItemKind::Mint),
                ShopEntry::Item(ItemKind::SitrusBerry),
                ShopEntry::Item(ItemKind::RareCandy),
                ShopEntry::Egg(None),
                ShopEntry::Egg(Some(Rarity::Uncommon)),
                ShopEntry::Item(ItemKind::ShinyCharm),
                ShopEntry::Egg(Some(Rarity::Rare)),
            ]
        );
    }

    #[test]
    fn shop_entries_omit_fresh_egg_when_no_active() {
        let s = wallet_store(5_000_000_000, 0, &[]);
        assert!(!s.has_active());
        assert_eq!(
            s.shop_entries(),
            vec![
                ShopEntry::Item(ItemKind::OranBerry),
                ShopEntry::Item(ItemKind::Mint),
                ShopEntry::Item(ItemKind::SitrusBerry),
                ShopEntry::Item(ItemKind::RareCandy),
                ShopEntry::Item(ItemKind::ShinyCharm),
            ]
        );
    }

    #[test]
    fn buy_egg_discards_active_and_sets_guarantee() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.used_since_install = 20_000_000_000;
        state.active = Some(MonState::new(
            10,
            vec![10],
            Some(vec![10]),
            0,
            200_000_000,
            Rarity::Common,
            1,
            false,
            None,
            None,
            false,
        ));
        state.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            1,
            3,
            vec![1, 2, 3],
            Rarity::Common,
            None,
            false,
            None,
            None,
        )];
        let dex_ids: Vec<Uuid> = state.dex.iter().map(|e| e.id).collect();
        let collected: HashSet<String> = state.collected_finals.clone();

        let mut s = StoreBuilder::new(Box::new(StubProvider::new(no_evo())))
            .seed_state(state)
            .rng(vec![0, 1])
            .build();
        assert!(s.buy_egg(Some(Rarity::Rare)));
        assert_eq!(s.state.egg_tier, Some(Rarity::Rare));
        assert_eq!(s.state.spent_tokens, FreshEgg::price(Some(Rarity::Rare)));
        assert!(s.state.active.is_none());
        assert_eq!(s.state.egg_usage, 0);
        assert_eq!(
            s.state.dex.iter().map(|e| e.id).collect::<Vec<_>>(),
            dex_ids,
            "dex unchanged"
        );
        assert_eq!(
            s.state.collected_finals, collected,
            "collectedFinals unchanged"
        );
    }

    #[test]
    fn cannot_buy_egg_while_incubating() {
        let mut s = wallet_store(5_000_000_000, 0, &[]);
        assert!(!s.has_active());
        for tier in FreshEgg::SHOP_TIERS {
            assert!(!s.can_buy_egg(tier));
            assert!(!s.buy_egg(tier));
        }
        assert_eq!(s.state.spent_tokens, 0);
    }

    // MARK: ditto

    fn ditto_store(used_at_stage: i64, shiny: bool, revealed: bool) -> CompanionStore {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.last_date = "d1".to_string();
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            used_at_stage,
            Rarity::Common,
            3,
            shiny,
            None,
            Some(1),
            revealed,
        ));
        let provider = DittoProvider {
            disguise: linear3(),
            ditto: make_line(132, leaf(132), Rarity::Rare),
        };
        StoreBuilder::new(Box::new(provider))
            .seed_state(state)
            .rng(vec![1, 2])
            .build()
    }

    #[test]
    fn ditto_disguise_hit_pure_function() {
        assert!(CompanionStore::ditto_disguise_hit(Rarity::Common, 2, 0));
        assert!(CompanionStore::ditto_disguise_hit(Rarity::Common, 3, 128));
        assert!(!CompanionStore::ditto_disguise_hit(Rarity::Common, 3, 1));
        assert!(
            !CompanionStore::ditto_disguise_hit(Rarity::Common, 1, 0),
            "single-form excluded"
        );
        assert!(
            !CompanionStore::ditto_disguise_hit(Rarity::Uncommon, 3, 0),
            "non-common excluded"
        );
    }

    #[test]
    fn shiny_hidden_during_disguise() {
        let s = ditto_store(0, true, false);
        assert!(s.state.active.as_ref().unwrap().is_shiny);
        assert!(!s.current_is_shiny(), "shiny hidden while disguised");
    }

    #[test]
    fn reveal_at_first_evolution() {
        let mut s = ditto_store(0, false, false);
        s.apply_usage(300_000_000); // accrue while the line is unloaded
        assert_eq!(s.state.active.as_ref().unwrap().used_at_stage, 300_000_000);
        s.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);

        let active = s.state.active.as_ref().unwrap();
        assert!(
            active.ditto_revealed,
            "revealed at first evolution threshold"
        );
        assert_eq!(active.base_id, PokemonOdds::DITTO_SPECIES_ID);
        assert_eq!(active.rarity, Rarity::Rare);
        assert_eq!(active.total_forms, 1);
        assert_eq!(active.path_ids, vec![PokemonOdds::DITTO_SPECIES_ID]);
        assert_eq!(
            active.used_at_stage,
            300_000_000 - PokemonBalance::phase_threshold(Rarity::Common, 3, 0),
            "first-evo overflow carries"
        );
        assert!(active.ditto_disguise.is_some());
        assert_eq!(
            s.celebration,
            Some(Celebration::DittoReveal { shiny: false })
        );
    }

    #[test]
    fn shiny_unmasked_after_reveal() {
        let mut s = ditto_store(0, true, false);
        s.apply_usage(300_000_000);
        s.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);
        assert!(s.state.active.as_ref().unwrap().ditto_revealed);
        assert!(s.current_is_shiny(), "shiny unmasked after reveal");
        assert_eq!(
            s.celebration,
            Some(Celebration::DittoReveal { shiny: true })
        );
    }

    #[test]
    fn no_reveal_below_threshold() {
        let mut s = ditto_store(0, false, false);
        s.apply_usage(100_000_000); // below the 125M first-evo threshold
        s.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);
        assert!(!s.state.active.as_ref().unwrap().ditto_revealed);
        assert_eq!(s.current_species_id(), Some(1));
    }

    #[test]
    fn ditto_hatch_roll_gate_consumes_no_rng_when_disabled() {
        // Disguise rolling disabled → the ditto roll must NOT consume rng: the
        // sequence [0,0,0] yields shiny=false, nature=Hardy, plan=1.
        let mut s = store_for(linear3(), vec![0, 0, 0]);
        s.hatch(1);
        let active = s.state.active.as_ref().unwrap();
        assert!(active.ditto_disguise.is_none());
        assert_eq!(active.nature, Some(PokemonNature::ALL[0]));
    }

    #[test]
    fn ditto_hatch_roll_hit_when_enabled() {
        // ditto_disguise_hit(common, 3, roll=128) → true.
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .rng(vec![0, 0, 128])
            .ditto_rolling(true)
            .build();
        s.hatch(1);
        let active = s.state.active.as_ref().unwrap();
        assert_eq!(active.ditto_disguise, Some(1));
        assert!(!active.ditto_revealed);
    }

    // MARK: line nodes / plan helpers

    #[test]
    fn realized_line_items_uses_stage_index_for_current_marker() {
        assert_eq!(
            CompanionStore::realized_line_items(&[1, 2], 0),
            vec![
                EvoLineItem::new(EvoLineItemContent::Species(1), EvoLineItemState::Current),
                EvoLineItem::new(EvoLineItemContent::Species(2), EvoLineItemState::Done),
            ]
        );
    }

    #[test]
    fn repaired_plan_appends_fallback_route_to_current_path() {
        assert_eq!(
            CompanionStore::repaired_plan(&[265], 0, &[265, 266, 267]),
            vec![265, 266, 267]
        );
        assert_eq!(
            CompanionStore::repaired_plan(&[265, 266], 1, &[266, 999]),
            vec![265, 266, 999]
        );
        assert_eq!(
            CompanionStore::repaired_plan(&[265, 266], 1, &[999]),
            vec![265, 266],
            "mismatched fallback root keeps only the realized prefix"
        );
    }

    #[test]
    fn line_nodes_previews_complete_linear_evolution() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        assert_eq!(
            s.line_nodes(),
            vec![
                EvoLineItem::new(EvoLineItemContent::Species(1), EvoLineItemState::Current),
                EvoLineItem::new(EvoLineItemContent::Species(2), EvoLineItemState::Future),
                EvoLineItem::new(EvoLineItemContent::Species(3), EvoLineItemState::Future),
            ]
        );
    }

    #[test]
    fn line_nodes_hides_unresolved_branch_as_single_mystery() {
        let mut s = store_for(wurmple_line(), vec![0, 0, 0]);
        s.hatch(265);
        assert_eq!(
            s.line_nodes(),
            vec![
                EvoLineItem::new(EvoLineItemContent::Species(265), EvoLineItemState::Current),
                EvoLineItem::new(EvoLineItemContent::Mystery, EvoLineItemState::Future),
            ]
        );
    }

    #[test]
    fn branching_prefers_uncollected_finals() {
        let mut s = store_for(branch3(), vec![0, 0, 0, 1, 1, 1]);
        let evo = PokemonBalance::phase_threshold(Rarity::Common, 2, 0);
        let grad = PokemonBalance::phase_threshold(Rarity::Common, 2, 1);
        let mut finals: Vec<i64> = Vec::new();
        for _ in 0..3 {
            s.hatch(10);
            s.apply_usage(evo);
            s.apply_usage(grad);
            finals.push(s.state.dex.last().unwrap().final_id);
        }
        assert_eq!(
            finals.into_iter().collect::<HashSet<_>>(),
            HashSet::from([11, 12, 13])
        );
    }

    #[test]
    fn hatch_preselects_route_and_evolution_does_not_consume_rng() {
        // A fixed fallback RNG (empty queue → 0) drives every plan roll to the
        // first branch. After hatch, the planned path is stored; evolution must
        // follow it without consuming any more rng.
        let mut s = store_for(wurmple_line(), vec![0, 0, 0]);
        s.hatch(265);
        let plan = s.state.active.as_ref().unwrap().planned_path_ids.clone();
        assert!(plan == vec![265, 266, 267] || plan == vec![265, 268, 269]);
        s.apply_usage(PokemonBalance::phase_threshold(
            Rarity::Common,
            plan.len() as i64,
            0,
        ));
        assert_eq!(s.current_species_id(), Some(plan[1]));
    }

    // MARK: dex aggregation

    #[test]
    fn dex_species_folds_duplicate_lines_and_resolves_names() {
        let mut state = CompanionState::default();
        state.language = AppLanguage::Ko;
        let names: HashMap<i64, HashMap<String, String>> = HashMap::from([
            (1, HashMap::from([("ko".to_string(), "포1".to_string())])),
            (2, HashMap::from([("ko".to_string(), "포2".to_string())])),
            (3, HashMap::from([("ko".to_string(), "포3".to_string())])),
        ]);
        state.dex = vec![
            DexEntry::new(
                Uuid::new_v4(),
                1,
                3,
                vec![1, 2, 3],
                Rarity::Common,
                Some(now()),
                false,
                None,
                Some(names.clone()),
            ),
            DexEntry::new(
                Uuid::new_v4(),
                1,
                3,
                vec![1, 2, 3],
                Rarity::Common,
                Some(now()),
                false,
                None,
                Some(names),
            ),
        ];
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        let species = s.dex_species();
        assert_eq!(
            species.iter().map(|sp| sp.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            species
                .iter()
                .map(|sp| sp.name.as_str())
                .collect::<Vec<_>>(),
            vec!["포1", "포2", "포3"]
        );
        assert!(!species.iter().any(|sp| sp.is_raising));
    }

    #[test]
    fn dex_species_counts_only_reached_stages_of_active() {
        let mut state = CompanionState::default();
        state.language = AppLanguage::Ko;
        state.active = Some(MonState::new(
            1,
            vec![1, 2],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            Some(PokemonNature::Brave),
            None,
            false,
        ));
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        let species = s.dex_species();
        assert_eq!(species.iter().map(|sp| sp.id).collect::<Vec<_>>(), vec![1]);
        assert!(species[0].is_raising);
    }

    #[test]
    fn dex_species_marks_shiny_across_the_chain() {
        let mut state = CompanionState::default();
        state.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            1,
            2,
            vec![1, 2],
            Rarity::Common,
            Some(now()),
            true,
            None,
            None,
        )];
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        let species = s.dex_species();
        assert_eq!(
            species.iter().map(|sp| sp.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(species.iter().all(|sp| sp.is_shiny));
    }

    #[test]
    fn dex_species_name_falls_back_to_hash_id() {
        let mut state = CompanionState::default();
        state.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            1,
            3,
            vec![1, 2, 3],
            Rarity::Common,
            Some(now()),
            false,
            None,
            None,
        )];
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        assert_eq!(
            s.dex_species()
                .iter()
                .map(|sp| sp.name.clone())
                .collect::<Vec<_>>(),
            vec!["#1".to_string(), "#2".to_string(), "#3".to_string()]
        );
    }

    #[test]
    fn dex_stored_chain_names_resolves_per_language() {
        let mut state = CompanionState::default();
        state.language = AppLanguage::Ko;
        let names: HashMap<i64, HashMap<String, String>> = HashMap::from([
            (
                1,
                HashMap::from([
                    ("en".to_string(), "P1".to_string()),
                    ("ko".to_string(), "포1".to_string()),
                ]),
            ),
            (2, HashMap::from([("en".to_string(), "P2".to_string())])),
        ]);
        let entry = DexEntry::new(
            Uuid::new_v4(),
            1,
            2,
            vec![1, 2],
            Rarity::Common,
            None,
            false,
            None,
            Some(names),
        );
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        let resolved = s.dex_stored_chain_names(&entry).unwrap();
        assert_eq!(resolved[&1], "포1");
        assert_eq!(resolved[&2], "P2", "en fallback when ko missing");
        assert!(s
            .dex_stored_chain_names(&DexEntry::new(
                Uuid::new_v4(),
                1,
                2,
                vec![1, 2],
                Rarity::Common,
                None,
                false,
                None,
                None
            ))
            .is_none());
    }

    #[test]
    fn backfill_fills_names_for_legacy_entries() {
        let mut state = CompanionState::default();
        state.language = AppLanguage::Ko;
        state.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            1,
            3,
            vec![1, 2, 3],
            Rarity::Common,
            Some(now()),
            false,
            None,
            None,
        )];
        let provider = StubProvider::new(linear3());
        let mut s = StoreBuilder::new(Box::new(provider))
            .seed_state(state)
            .build();
        s.backfill_missing_dex_names();
        assert!(s.state.dex[0].names.is_some(), "legacy entry backfilled");
        assert_eq!(
            s.dex_species()
                .iter()
                .map(|sp| sp.name.as_str())
                .collect::<Vec<_>>(),
            vec!["포1", "포2", "포3"]
        );
    }

    // MARK: active dex entry

    #[test]
    fn active_companion_appears_in_dex_before_graduation_without_duplicate() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        assert!(s.state.dex.is_empty());
        assert_eq!(s.dex_entries().len(), 1);
        assert_eq!(s.dex_entries()[0].chain_order, vec![1]);
        assert_eq!(s.dex_entries()[0].final_id, 1);

        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 0));
        assert_eq!(s.dex_entries()[0].chain_order, vec![1, 2]);
        assert_eq!(s.dex_entries()[0].final_id, 2);

        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 1));
        s.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 3, 2));
        assert!(s.state.active.is_none());
        assert_eq!(s.state.dex.len(), 1);
        assert_eq!(s.dex_entries().len(), 1, "no duplicate at graduation");
        assert_eq!(s.dex_entries()[0].chain_order, vec![1, 2, 3]);
    }

    #[test]
    fn active_dex_entry_is_deterministically_identifiable() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        let entries = s.dex_entries();
        assert!(s.is_active_dex_entry(&entries[0]));
        assert_eq!(entries[0].id, s.active_dex_entry().unwrap().id);
        assert!(!s.is_active_dex_entry(&DexEntry::new(
            Uuid::new_v4(),
            1,
            1,
            vec![1],
            Rarity::Common,
            None,
            false,
            None,
            None
        )));
    }

    #[test]
    fn dex_entries_sorted_pins_active_first_then_recency() {
        let mut state = CompanionState::default();
        state.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            150,
            150,
            vec![150],
            Rarity::Legendary,
            None,
            false,
            None,
            None,
        )];
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            5,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        let sorted = s.dex_entries_sorted();
        assert!(s.is_active_dex_entry(&sorted[0]));
        assert!(
            !s.is_active_dex_entry(&sorted[1]),
            "legacy no-caughtAt entry is not active"
        );
    }

    // MARK: line reload / normalization

    #[test]
    fn reload_normalizes_invalid_plan_without_rewinding_realized_path() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.last_date = "d1".to_string();
        state.active = Some(MonState::new(
            265,
            vec![265, 266],
            Some(vec![265, 266, 269]),
            1,
            42,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(wurmple_line())))
            .seed_state(state)
            .build();
        s.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);

        let active = s.state.active.as_ref().unwrap();
        assert_eq!(active.path_ids, vec![265, 266], "realized path intact");
        assert_eq!(
            active.planned_path_ids,
            vec![265, 266, 267],
            "invalid suffix repaired"
        );
        assert_eq!(active.stage_index, 1);
        assert_eq!(active.total_forms, 3);
        assert_eq!(active.used_at_stage, 42);
    }

    #[test]
    fn reload_wrong_root_normalizes_path_without_changing_identity_or_disguise() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.last_date = "d1".to_string();
        state.active = Some(MonState::new(
            265,
            vec![999],
            Some(vec![999]),
            0,
            42,
            Rarity::Common,
            1,
            true,
            Some(PokemonNature::Timid),
            Some(265),
            false,
        ));
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(wurmple_line())))
            .seed_state(state)
            .build();
        s.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);

        let active = s.state.active.as_ref().unwrap();
        assert_eq!(active.path_ids, vec![265]);
        assert_eq!(active.used_at_stage, 42);
        assert!(active.is_shiny);
        assert_eq!(active.nature, Some(PokemonNature::Timid));
        assert_eq!(active.ditto_disguise, Some(265));
        assert!(!active.ditto_revealed);
    }

    #[test]
    fn usage_accrues_while_line_unloaded_then_evolves_on_load() {
        let line = linear3();
        let mut s1 = StoreBuilder::new(Box::new(StubProvider::new(line.clone())))
            .rng(vec![0, 0, 0])
            .build();
        s1.hatch(1);
        let url = s1.file_url.clone();

        let mut s2 = CompanionStore::new(
            Box::new(StubProvider::new(line)),
            now,
            Some(url),
            Box::new(SequenceRng::new(Vec::new())),
            false,
        );
        assert!(s2.state.active.is_some());
        assert!(s2.current_line.is_none());

        s2.apply_usage(300_000_000);
        assert_eq!(
            s2.state.active.as_ref().unwrap().used_at_stage,
            300_000_000,
            "no loss while unloaded"
        );
        assert_eq!(s2.state.active.as_ref().unwrap().stage_index, 0);

        s2.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);
        assert!(s2.current_line.is_some());
        assert_eq!(
            s2.state.active.as_ref().unwrap().stage_index,
            1,
            "evolve after load"
        );
        assert_eq!(
            s2.state.active.as_ref().unwrap().used_at_stage,
            300_000_000 - PokemonBalance::phase_threshold(Rarity::Common, 3, 0)
        );
    }

    #[test]
    fn reload_preserves_complete_short_planned_route() {
        let line = make_line(
            1,
            node(1, vec![leaf(2), node(3, vec![leaf(4)])]),
            Rarity::Common,
        );
        let mut s1 = StoreBuilder::new(Box::new(StubProvider::new(line.clone())))
            .rng(vec![0, 0, 0])
            .build();
        s1.hatch(1);
        assert_eq!(
            s1.state.active.as_ref().unwrap().planned_path_ids,
            vec![1, 2],
            "seed picks short route"
        );
        let url = s1.file_url.clone();

        // A post-restart RNG that would pick the opposite branch (3).
        let mut s2 = CompanionStore::new(
            Box::new(StubProvider::new(line)),
            now,
            Some(url),
            Box::new(SequenceRng::new(vec![0, 0, 0])),
            false,
        );
        s2.update(&map1("test", 0), "d1", 0, BurnTier::Idle, false, true);
        assert_eq!(
            s2.state.active.as_ref().unwrap().planned_path_ids,
            vec![1, 2]
        );
        assert_eq!(s2.state.active.as_ref().unwrap().total_forms, 2);

        s2.apply_usage(PokemonBalance::phase_threshold(Rarity::Common, 2, 0));
        assert_eq!(
            s2.current_species_id(),
            Some(2),
            "persisted route beats post-restart RNG"
        );
    }

    // MARK: save transfer

    #[test]
    fn apply_save_rebases_and_reloads() {
        let mut current_state = CompanionState::default();
        current_state.install_baseline_set = true;
        current_state.used_since_install = 1_000;
        current_state.language = AppLanguage::En;

        let mut imported = CompanionState::default();
        imported.used_since_install = 8_000_000_000;
        imported.language = AppLanguage::Ja;
        imported.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));

        let data = SaveTransfer::encode(imported, "2.5.0", "Old Mac", now()).unwrap();
        let envelope = SaveTransfer::decode(&data).unwrap();

        let mut s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(current_state)
            .build();
        s.apply_save(envelope, &map1("test", 5), "2026-08-03", true)
            .unwrap();

        assert_eq!(
            s.state.used_since_install, 8_000_000_000,
            "progress comes in"
        );
        assert_eq!(s.language(), AppLanguage::En, "device preference kept");
        assert!(s.current_line.is_some(), "line reloaded after import");

        let dir = s.file_url.parent().unwrap();
        let backups: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(SaveTransfer::BACKUP_FILE_PREFIX))
            .collect();
        assert_eq!(backups.len(), 1, "pre-import backup created");
    }

    #[test]
    fn import_backups_are_pruned_to_keep_count() {
        let dir = std::env::temp_dir().join(format!("ptb-prune-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..7 {
            let name = format!(
                "{}-2026-08-0{}-00000{i}.json",
                SaveTransfer::BACKUP_FILE_PREFIX,
                i % 7
            );
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let file_url = dir.join("companion-state.json");
        let mut s = StoreBuilder::new(Box::new(StubProvider::new(linear3()))).build();
        s.file_url = file_url.clone();
        s.prune_import_backups(&dir);

        let remaining: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(SaveTransfer::BACKUP_FILE_PREFIX))
            .collect();
        assert_eq!(remaining.len(), SaveTransfer::BACKUPS_TO_KEEP);
    }

    // MARK: display state

    #[test]
    fn compute_state_level_up_during_event_window() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        assert_eq!(s.display_state, CompanionStateKind::LevelUp);
        s.update(&map1("test", 100), "d1", 0, BurnTier::Idle, false, true);
        assert_eq!(
            s.display_state,
            CompanionStateKind::LevelUp,
            "within the 4s window"
        );
    }

    #[test]
    fn compute_state_reflects_burn_tier_and_gates() {
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1, 2, 3]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        ));
        let s = StoreBuilder::new(Box::new(StubProvider::new(linear3())))
            .seed_state(state)
            .build();
        assert_eq!(
            s.compute_state(BurnTier::Idle, false, true, 10),
            CompanionStateKind::Idle
        );
        assert_eq!(
            s.compute_state(BurnTier::Normal, false, true, 10),
            CompanionStateKind::Working
        );
        assert_eq!(
            s.compute_state(BurnTier::Fast, false, true, 10),
            CompanionStateKind::Focus
        );
        assert_eq!(
            s.compute_state(BurnTier::Blazing, false, true, 10),
            CompanionStateKind::Focus
        );
        assert_eq!(
            s.compute_state(BurnTier::Idle, true, true, 10),
            CompanionStateKind::Tired
        );
        assert_eq!(
            s.compute_state(BurnTier::Idle, false, false, 10),
            CompanionStateKind::Sleep
        );
        assert_eq!(
            s.compute_state(BurnTier::Idle, false, true, 0),
            CompanionStateKind::Sleep
        );
    }

    #[test]
    fn compute_state_egg_when_no_active() {
        let s = default_store();
        assert_eq!(
            s.compute_state(BurnTier::Idle, false, true, 100),
            CompanionStateKind::Egg
        );
    }

    // MARK: rarity / guarantee

    #[test]
    fn guarantee_filters_choose_base_candidates() {
        // Index: common(255) / uncommon(100) / rare(30) / legendary(3).
        let entries = vec![
            BaseSpecies {
                id: 1,
                capture_rate: 255,
            },
            BaseSpecies {
                id: 2,
                capture_rate: 100,
            },
            BaseSpecies {
                id: 3,
                capture_rate: 30,
            },
            BaseSpecies {
                id: 4,
                capture_rate: 3,
            },
        ];
        let provider = IndexProvider {
            index: entries.clone(),
            line: linear3(),
        };
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.egg_tier = Some(Rarity::Rare);
        let mut s = StoreBuilder::new(Box::new(provider))
            .seed_state(state)
            .rng(vec![0])
            .build();
        // roll 0 → first candidate within the rare floor (30 and 3).
        let chosen = s.choose_base().unwrap();
        assert!(chosen == 3 || chosen == 4, "rare tier must not pick 1 or 2");
    }

    #[test]
    fn hatch_discards_species_below_guarantee_when_index_is_stale() {
        // The index claims capture_rate 30 (rare) but the actual line is common.
        let provider = LyingIndexProvider { line: no_evo() };
        let mut state = CompanionState::default();
        state.install_baseline_set = true;
        state.egg_usage = PokemonBalance::EGG_HATCH_THRESHOLD;
        state.egg_tier = Some(Rarity::Rare);
        let mut s = StoreBuilder::new(Box::new(provider))
            .seed_state(state)
            .rng(vec![1, 2])
            .build();
        s.hatch_if_needed();
        assert!(
            s.state.active.is_none(),
            "below-floor species never hatches"
        );
        assert!(s.state.pending_hatch_id.is_none(), "pre-roll dropped");
        assert_eq!(s.state.egg_tier, Some(Rarity::Rare), "guarantee kept");
        assert_eq!(
            s.state.egg_usage,
            PokemonBalance::EGG_HATCH_THRESHOLD,
            "incubation kept"
        );
    }

    struct IndexProvider {
        index: Vec<BaseSpecies>,
        line: EvoLine,
    }

    impl PokeProvider for IndexProvider {
        fn line(&self, _base_species_id: i64) -> Result<EvoLine, PokeError> {
            Ok(self.line.clone())
        }
        fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
            Ok(self.index.clone())
        }
        fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
            Ok(self.index.iter().find(|b| b.id == id).cloned())
        }
    }

    struct LyingIndexProvider {
        line: EvoLine,
    }

    impl PokeProvider for LyingIndexProvider {
        fn line(&self, _base_species_id: i64) -> Result<EvoLine, PokeError> {
            Ok(self.line.clone())
        }
        fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
            Ok(vec![BaseSpecies {
                id: 7,
                capture_rate: 30,
            }])
        }
        fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
            Ok((id == 7).then_some(BaseSpecies {
                id: 7,
                capture_rate: 30,
            }))
        }
    }

    fn give_candies(s: &mut CompanionStore, n: usize) {
        s.grant_candies(&[], true); // seed (0 granted)
        for i in 0..n {
            s.grant_candies(
                &[window(
                    &format!("t.session.{i}"),
                    WindowClass::Session,
                    100.0,
                )],
                true,
            );
        }
    }

    fn window(key: &str, kind: WindowClass, utilization: f64) -> CandyWindow {
        CandyWindow {
            key: key.to_string(),
            name: "T".to_string(),
            kind,
            utilization,
        }
    }

    #[test]
    fn test_use_oran_berry_and_sitrus_berry() {
        let mut s = store_for(linear3(), vec![1, 2, 3, 4, 5, 6]);
        s.hatch(1);
        s.state
            .inventory
            .insert(ItemKind::OranBerry.raw_value().to_string(), 2);
        s.state
            .inventory
            .insert(ItemKind::SitrusBerry.raw_value().to_string(), 1);

        assert!(s.can_use_oran_berry());
        assert!(s.can_use_sitrus_berry());
        assert!(!s.has_golden_aura());

        // Use Oran Berry (+15M XP)
        let res = s.use_oran_berry();
        assert_eq!(res, CandyUseResult::Progressed);
        assert_eq!(s.item_count(ItemKind::OranBerry), 1);
        assert_eq!(s.state.active.as_ref().unwrap().used_at_stage, 15_000_000);
        assert_eq!(s.berry_feedback_kind.as_deref(), Some("oranBerry"));

        // Use Sitrus Berry (+50M XP + Golden Aura)
        let res2 = s.use_sitrus_berry();
        assert_eq!(res2, CandyUseResult::Progressed);
        assert_eq!(s.item_count(ItemKind::SitrusBerry), 0);
        assert_eq!(s.state.active.as_ref().unwrap().used_at_stage, 65_000_000);
        assert_eq!(s.berry_feedback_kind.as_deref(), Some("sitrusBerry"));
        assert!(s.has_golden_aura());
    }

    #[test]
    fn test_mega_overdrive_and_coin_rush() {
        let mut s = store_for(linear3(), vec![1, 2, 3]);
        s.hatch(1);
        assert!(!s.mega_overdrive_enabled);
        assert!(!s.is_mega_overdrive);

        let mut provider_tokens = HashMap::new();
        provider_tokens.insert("claude".to_string(), 100);

        // 1. Initial baseline
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Normal,
            false,
            true,
        );
        assert_eq!(s.available_tokens(), 0);

        // 2. Normal tier with overdrive disabled
        provider_tokens.insert("claude".to_string(), 200);
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Normal,
            false,
            true,
        );
        assert_eq!(s.available_tokens(), 100);
        assert!(!s.is_mega_overdrive);

        // 3. Fast burn tier but overdrive still disabled -> 1x token delta
        provider_tokens.insert("claude".to_string(), 300);
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Fast,
            false,
            true,
        );
        assert_eq!(s.available_tokens(), 200);
        assert!(!s.is_mega_overdrive);

        // 4. Enable mega overdrive and blazing tier -> 2x Coin Rush!
        s.set_mega_overdrive_enabled(true);
        provider_tokens.insert("claude".to_string(), 400);
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Blazing,
            false,
            true,
        );
        assert!(s.is_mega_overdrive);
        // +100 delta * 2 multiplier = +200 coins -> total 400 available tokens
        assert_eq!(s.available_tokens(), 400);
    }

    #[test]
    fn test_pokemon_ribbons_and_achievements() {
        let mut s = store_for(linear3(), vec![1, 2, 3]);
        s.hatch(1);

        // 1. Initial hatch awards "starter" ribbon
        let active_ribbons = s.active_ribbons();
        assert!(active_ribbons.contains(&"starter".to_string()));

        // 2. Petting buddy awards "affection" ribbon
        s.pet_buddy();
        assert!(s.active_ribbons().contains(&"affection".to_string()));

        // 3. Feeding berry awards "gourmet" ribbon and milestone ribbons
        s.state
            .inventory
            .insert(ItemKind::SitrusBerry.raw_value().to_string(), 2);
        s.use_sitrus_berry(); // +50M XP
        let ribbons_after_berry = s.active_ribbons();
        assert!(ribbons_after_berry.contains(&"gourmet".to_string()));
        assert!(ribbons_after_berry.contains(&"bronzeBurner".to_string()));
        assert!(ribbons_after_berry.contains(&"silverBurner".to_string()));

        // 4. Overdrive sprint awards "overdrive" ribbon
        s.set_mega_overdrive_enabled(true);
        s.is_mega_overdrive = true;
        s.apply_usage(100);
        assert!(s.active_ribbons().contains(&"overdrive".to_string()));

        // 5. Dex species captures accumulated ribbons
        let dex_species = s.dex_species();
        let mon = dex_species.iter().find(|d| d.id == 1).unwrap();
        assert!(mon.ribbons.contains(&"starter".to_string()));
        assert!(mon.ribbons.contains(&"affection".to_string()));
        assert!(mon.ribbons.contains(&"gourmet".to_string()));
    }

    #[test]
    fn test_trainer_passport_and_journal() {
        let mut s = store_for(linear3(), vec![1, 2, 3]);
        s.hatch(1);

        // 1. Trainer Defaults
        assert_eq!(s.state.trainer_name, "Trainer");
        assert!(s.state.trainer_id.starts_with("TR-"));
        assert_eq!(s.trainer_title(), "Novice Pokémon Trainer");

        // 2. Customizing Trainer Name & Avatar
        s.set_trainer_name("Ash Ketchum");
        assert_eq!(s.state.trainer_name, "Ash Ketchum");
        s.set_trainer_avatar(Some(25));
        assert_eq!(s.state.avatar_species_id, Some(25));

        // 3. Journal captures Hatching
        assert!(!s.state.journal.is_empty());
        let first_entry = s.state.journal.iter().find(|j| j.kind == "hatch").unwrap();
        assert_eq!(first_entry.species_id, Some(1));

        // 4. Daily history updates with tokens
        let mut provider_tokens = HashMap::new();
        provider_tokens.insert("claude".to_string(), 500);
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Normal,
            false,
            true,
        );
        provider_tokens.insert("claude".to_string(), 1500);
        s.update(
            &provider_tokens,
            "2026-08-21",
            0,
            BurnTier::Normal,
            false,
            true,
        );
        assert_eq!(s.state.daily_history.get("2026-08-21"), Some(&1000));
    }

    impl CompanionStore {
        fn consume_celebration(&mut self) {
            self.celebration = None;
        }
    }
}
