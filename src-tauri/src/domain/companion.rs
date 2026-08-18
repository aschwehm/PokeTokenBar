//! The companion model: companion display state, Pokémon identity, evolution
//! trees, economy constants, and the persistent save state.
//!
//! Mirrors the original `Core/CompanionModel.swift`. `CompanionState`,
//! `MonState`, and `DexEntry` round-trip through save files, so they implement
//! `Serialize` + `Deserialize` — with a deliberately lenient, field-by-field
//! `Deserialize` so one corrupted field never wipes the whole Pokédex/state.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::decoding::{
    get_bool, get_i64, get_language, get_opt_i64, get_opt_i64_vec, get_opt_nature,
    get_opt_species_names, get_opt_string, get_rarity, get_required_i64, get_required_i64_vec,
    get_required_rarity, get_string, get_string_i64_map, get_string_set, parse_iso8601,
};

/// Display state — derived from usage/burn (sprite motion intensity, status copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompanionStateKind {
    Egg,
    Idle,
    Working,
    Focus,
    Tired,
    Sleep,
    LevelUp,
}

/// App language. Pokémon names come from PokéAPI multilingual names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppLanguage {
    Ko,
    En,
    Ja,
    Es,
}

impl AppLanguage {
    pub const ALL: [AppLanguage; 4] = [
        AppLanguage::Ko,
        AppLanguage::En,
        AppLanguage::Ja,
        AppLanguage::Es,
    ];

    /// PokéAPI language.name candidates (first match wins).
    pub fn api_codes(&self) -> &'static [&'static str] {
        match self {
            AppLanguage::Ko => &["ko"],
            AppLanguage::En => &["en"],
            AppLanguage::Ja => &["ja-Hrkt", "ja"],
            AppLanguage::Es => &["es"],
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AppLanguage::Ko => "한국어",
            AppLanguage::En => "English",
            AppLanguage::Ja => "日本語",
            AppLanguage::Es => "Español",
        }
    }

    /// Picks this language's name from byLang(langCode → name) — first apiCodes
    /// match, then English fallback.
    pub fn resolve_name<'a>(&self, by_lang: &'a HashMap<String, String>) -> Option<&'a str> {
        for code in self.api_codes() {
            if let Some(n) = by_lang.get(*code) {
                return Some(n);
            }
        }
        by_lang.get("en").map(String::as_str)
    }

    /// Default language for new installs, inferred from the system preferred
    /// language (ko/ja/es only; everything else is English fallback). Existing
    /// users keep their stored language.
    pub fn system_default() -> AppLanguage {
        // The macOS original read `Locale.preferredLanguages.first`; on
        // Linux/Windows the closest equivalent is the LANG env var.
        let lang = std::env::var("LANG")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if lang.starts_with("ko") {
            AppLanguage::Ko
        } else if lang.starts_with("ja") {
            AppLanguage::Ja
        } else if lang.starts_with("es") {
            AppLanguage::Es
        } else {
            AppLanguage::En
        }
    }

    pub fn from_raw(s: &str) -> Option<AppLanguage> {
        match s {
            "ko" => Some(AppLanguage::Ko),
            "en" => Some(AppLanguage::En),
            "ja" => Some(AppLanguage::Ja),
            "es" => Some(AppLanguage::Es),
            _ => None,
        }
    }
}

/// Rarity — derived from PokéAPI capture_rate / is_legendary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Legendary,
}

impl Rarity {
    /// Grade size (higher = rarer). Not a list sort key — catch logs are
    /// time-ordered and the dex is dex-number-ordered; rarity only narrows via
    /// filters. Its only consumer is the premium-egg guarantee gate
    /// (`line.rarity.sort_rank < tier.sort_rank`).
    pub fn sort_rank(&self) -> i64 {
        match self {
            Rarity::Common => 0,
            Rarity::Uncommon => 1,
            Rarity::Rare => 2,
            Rarity::Legendary => 3,
        }
    }

    /// This grade's capture_rate ceiling — at or below this the species is at
    /// least this grade. Single source of truth for both the
    /// `Rarity::from(captureRate:…)` classification threshold and the premium
    /// egg's pre-filter of hatch candidates.
    ///
    /// nil = not expressible via capture_rate. Legendaries are only determined
    /// by `is_legendary`/`is_mythical`, so a legendary-only egg cannot exist
    /// (and is not sold). Legendaries all have capture_rate ≤ 45, so they pass
    /// the uncommon/rare filters naturally ("at least" rules hold).
    pub fn capture_rate_ceiling(&self) -> Option<i64> {
        match self {
            Rarity::Rare => Some(45),
            Rarity::Uncommon => Some(120),
            Rarity::Common => Some(255),
            Rarity::Legendary => None,
        }
    }

    /// Whether capture_rate means "at least this grade" — legendary is always
    /// false because capture_rate cannot express it.
    pub fn includes(&self, capture_rate: i64) -> bool {
        match self.capture_rate_ceiling() {
            Some(ceiling) => capture_rate <= ceiling,
            None => false,
        }
    }

    pub fn from(capture_rate: i64, is_legendary: bool, is_mythical: bool) -> Rarity {
        if is_legendary || is_mythical {
            return Rarity::Legendary;
        }
        if Rarity::Rare.includes(capture_rate) {
            return Rarity::Rare;
        }
        if Rarity::Uncommon.includes(capture_rate) {
            return Rarity::Uncommon;
        }
        Rarity::Common
    }

    pub fn from_raw(s: &str) -> Option<Rarity> {
        match s {
            "common" => Some(Rarity::Common),
            "uncommon" => Some(Rarity::Uncommon),
            "rare" => Some(Rarity::Rare),
            "legendary" => Some(Rarity::Legendary),
            _ => None,
        }
    }
}

/// Token economy — based on the measured average (~253M/day). Graduation total
/// T is the same for any number of evolution stages at the same rarity. The
/// i-th form's growth cost in a k-form line is T·i / (k(k+1)/2) → sums to T,
/// higher stages cost more.
pub struct PokemonBalance;

impl PokemonBalance {
    /// Egg hatch threshold — this much usage cracks the egg (anticipation
    /// instead of instant hatch). The overflow carries into the hatched mon's
    /// growth.
    pub const EGG_HATCH_THRESHOLD: i64 = 5_000_000;

    pub fn graduation_total(rarity: Rarity) -> i64 {
        match rarity {
            Rarity::Common => 750_000_000,
            Rarity::Uncommon => 1_875_000_000,
            Rarity::Rare => 3_000_000_000,
            Rarity::Legendary => 6_000_000_000,
        }
    }

    /// Tokens needed from stageIndex (0-based) to the next stage / graduation.
    pub fn phase_threshold(rarity: Rarity, total_forms: i64, stage_index: i64) -> i64 {
        let k = total_forms.max(1);
        let i = stage_index + 1; // 1-based
        let total = Self::graduation_total(rarity) as f64;
        let denom = (k * (k + 1)) as f64 / 2.0;
        ((total * i as f64 / denom).round()) as i64
    }
}

/// Inventory item kinds — enum for future expansion (currently one candy).
/// Stored by rawValue in CompanionState.inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    RareCandy,
    Mint,
    ShinyCharm,
}

impl ItemKind {
    /// All item kinds in declaration order — the canonical bag/shop iteration
    /// order (mirrors Swift's `ItemKind.allCases`).
    pub const ALL: [ItemKind; 3] = [ItemKind::RareCandy, ItemKind::Mint, ItemKind::ShinyCharm];

    /// The raw string key under which this item is stored in
    /// `CompanionState.inventory` (matches the serialized `camelCase` raw value).
    pub fn raw_value(&self) -> &'static str {
        match self {
            ItemKind::RareCandy => "rareCandy",
            ItemKind::Mint => "mint",
            ItemKind::ShinyCharm => "shinyCharm",
        }
    }

    /// PokéAPI item sprite file name (.../sprites/items/{name}.png).
    /// nil = no sprite (emoji fallback only).
    pub fn sprite_name(&self) -> Option<&'static str> {
        match self {
            ItemKind::RareCandy => Some("rare-candy"),
            // PokéAPI has no mint sprite (gen-8 item) → emoji fallback.
            ItemKind::Mint => None,
            ItemKind::ShinyCharm => Some("shiny-charm"),
        }
    }

    /// Fallback emoji before/while the sprite loads or on failure.
    pub fn fallback_emoji(&self) -> &'static str {
        match self {
            ItemKind::RareCandy => "🍬",
            ItemKind::Mint => "🌿",
            ItemKind::ShinyCharm => "✨",
        }
    }

    /// Shop price (currency = used tokens). nil = not sold in the shop.
    pub fn shop_price(&self) -> Option<i64> {
        match self {
            ItemKind::RareCandy => Some(RareCandy::PRICE),
            ItemKind::Mint => Some(Mint::PRICE),
            ItemKind::ShinyCharm => Some(ShinyCharm::PRICE),
        }
    }

    /// Held (passive) item — not consumed, has a constant effect while owned.
    /// One-time purchase (no repurchase), shown as "applied" in the bag.
    pub fn is_passive(&self) -> bool {
        match self {
            ItemKind::RareCandy | ItemKind::Mint => false,
            ItemKind::ShinyCharm => true,
        }
    }
}

/// Rare Candy balance constants.
pub struct RareCandy;

impl RareCandy {
    /// XP injected into the current mon on use (token-denominated). Smaller
    /// than the minimum evolution threshold (common 1-form 125M), so one candy
    /// advances at most one stage (no chain/graduation burst). applyUsage
    /// injects it → carryover/evolution/graduation happen automatically.
    pub const XP: i64 = 100_000_000;
    /// Candies granted when the weekly limit hits 100% (session-level: 1).
    pub const WEEKLY_GRANT: i64 = 5;
    /// Shop price (currency = used tokens: usedSinceInstall − spentTokens).
    /// 5× the XP value (100M). Tokens are double-used ("growth meter + shop
    /// wallet"), so pricing the candy equal to its XP would make a purchase a
    /// free +150M (150M spent buys 250M growth). At 500M the 500M passive
    /// growth + 100M candy ≈ +20% real bonus. Always more expensive than the
    /// value so the free 100%-reward route stays better.
    pub const PRICE: i64 = 500_000_000;
}

/// Mint balance constants.
pub struct Mint;

impl Mint {
    /// Shop price. Nature change is purely cosmetic (no growth/stats effect),
    /// so there is no balance basis — a "feel" value at 1/5 of a candy (500M)
    /// to encourage trying natures. No double-counting issue (price = pure
    /// consumption, no growth granted).
    pub const PRICE: i64 = 100_000_000;
}

/// Shiny Charm balance constants — held item (one-time purchase, permanent, not consumed).
pub struct ShinyCharm;

impl ShinyCharm {
    /// Shop price. A permanent luck upgrade applying to all future hatches,
    /// hence premium (one rare graduation = 3B).
    pub const PRICE: i64 = 3_000_000_000;
    /// Shiny hatch probability denominator when owned — 1/64 → 1/48 (+33%).
    /// Homage to the main-series 'Shiny Charm'. ×2 (1/32) would be too much.
    /// No retroactivity for already-hatched individuals.
    pub const SHINY_DENOMINATOR: u64 = 48;
}

/// Fresh egg (reroll) balance constants — buying discards the current mon and
/// returns it to a new egg.
pub struct FreshEgg;

impl FreshEgg {
    /// Shop price. Premium reroll for an unwanted hatch (a sink for hoarded
    /// tokens). A discarded individual is not a graduation — it just
    /// disappears with no dex/probability (collectedFinals) impact, "as if
    /// never rolled". The new egg re-incubates from zero (5M) and loses
    /// growth (usedAtStage), which naturally suppresses spam/farming.
    pub const PRICE: i64 = 1_000_000_000;

    /// Eggs sold in the shop — no guarantee (base) → uncommon+ → rare+.
    /// `None` = the existing ungated egg. Legendary-only eggs are NOT sold
    /// (the floor can't be expressed via capture_rate, and the top grade is
    /// never a guaranteed product); legendaries appear naturally weighted in
    /// uncommon/rare eggs (~10% in rare eggs).
    pub const SHOP_TIERS: [Option<Rarity>; 3] = [None, Some(Rarity::Uncommon), Some(Rarity::Rare)];

    /// Guaranteed-tier egg price. The multiplier reuses the existing
    /// graduation-total table instead of a new constant (common 750M :
    /// uncommon 1.875B : rare 3B = 1 : 2.5 : 4 → 1B / 2.5B / 4B).
    ///
    /// Pricing by hatch odds (uncommon 7.16% : rare 6.98% ≈ 1 : 2.03) would
    /// make two uncommon eggs strictly dominate one rare egg (1.039 rare+
    /// / 0.104 legendary vs 1.000 / 0.100), so a graduation-volume ratio keeps
    /// the higher tier cheaper per rare+ individual (4.00B vs 4.81B).
    pub fn price(guaranteeing: Option<Rarity>) -> i64 {
        match guaranteeing {
            None => Self::PRICE,
            Some(tier) => {
                let multiplier = PokemonBalance::graduation_total(tier) as f64
                    / PokemonBalance::graduation_total(Rarity::Common) as f64;
                ((Self::PRICE as f64) * multiplier).round() as i64
            }
        }
    }
}

/// One shop row — a sold item (ItemKind) or an egg reroll (an immediate action,
/// so not an ItemKind). The egg's associated value is the **guarantee floor**
/// (nil = ungated existing egg). CompanionStore.shopEntries merges these by
/// ascending price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShopEntry {
    Item(ItemKind),
    Egg(Option<Rarity>),
}

impl ShopEntry {
    pub fn price(&self) -> i64 {
        match self {
            ShopEntry::Item(kind) => kind.shop_price().unwrap_or(0),
            ShopEntry::Egg(tier) => FreshEgg::price(*tier),
        }
    }
}

/// Classification of a candy-grant limit window — session = 1 candy, weekly = weeklyGrant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowClass {
    Session,
    Weekly,
}

/// Candy-grant decision input — one provider-agnostic limit window.
/// (UsageStore.candyEligibleWindows produces these.)
#[derive(Debug, Clone, PartialEq)]
pub struct CandyWindow {
    /// Stable identifier (tier tracking) — no volatile fields like resets_at.
    pub key: String,
    /// Display name (notification "why you got this").
    pub name: String,
    /// session = 1 candy · weekly = 5.
    pub kind: WindowClass,
    /// 0..100+
    pub utilization: f64,
}

/// One candy grant (pure decision result) — separated from side effects
/// (inventory, notifications) so it is testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandyGrant {
    pub window_key: String,
    /// Notification "why you got this".
    pub window_name: String,
    pub count: i64,
}

/// The range of animated Pokémon sprites the service provides. PokéAPI's
/// Gen-V animated assets only exist for national dex #1…649.
pub struct PokemonAssets;

impl PokemonAssets {
    pub const ANIMATED_SPECIES_IDS: std::ops::RangeInclusive<i64> = 1..=649;

    pub fn has_animated_sprite(species_id: i64) -> bool {
        Self::ANIMATED_SPECIES_IDS.contains(&species_id)
    }
}

/// A parsed PokéAPI evolution chain tree. Branches (multiple evolves_to) are children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvoNode {
    pub species_id: i64,
    pub children: Vec<EvoNode>,
}

impl EvoNode {
    pub fn new(species_id: i64, children: Vec<EvoNode>) -> Self {
        Self {
            species_id,
            children,
        }
    }

    /// Longest path length (form count). Branches are usually the same depth,
    /// so this is the representative value.
    pub fn depth(&self) -> i64 {
        1 + self.children.iter().map(EvoNode::depth).max().unwrap_or(0)
    }

    pub fn node_with_id(&self, id: i64) -> Option<&EvoNode> {
        if self.species_id == id {
            return Some(self);
        }
        for c in &self.children {
            if let Some(found) = c.node_with_id(id) {
                return Some(found);
            }
        }
        None
    }

    /// All reachable final-evolution ids from this node.
    pub fn final_ids(&self) -> Vec<i64> {
        if self.children.is_empty() {
            vec![self.species_id]
        } else {
            self.children.iter().flat_map(EvoNode::final_ids).collect()
        }
    }

    /// The evolution tree keeping only species with GIF assets in the service.
    /// Unsupported species prune their whole downstream chain.
    pub fn keeping_animated_sprites(&self) -> Option<EvoNode> {
        if !PokemonAssets::has_animated_sprite(self.species_id) {
            return None;
        }
        Some(EvoNode {
            species_id: self.species_id,
            children: self
                .children
                .iter()
                .filter_map(EvoNode::keeping_animated_sprites)
                .collect(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvoLineItemContent {
    Species(i64),
    Mystery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvoLineItemState {
    Done,
    Current,
    Future,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvoLineItem {
    pub content: EvoLineItemContent,
    pub state: EvoLineItemState,
}

impl EvoLineItem {
    pub fn new(content: EvoLineItemContent, state: EvoLineItemState) -> Self {
        Self { content, state }
    }
}

/// Line info finalized at hatch (tree + rarity + multilingual names).
#[derive(Debug, Clone, PartialEq)]
pub struct EvoLine {
    pub base_id: i64,
    pub tree: EvoNode,
    pub rarity: Rarity,
    /// speciesID → (langCode → name)
    pub names: HashMap<i64, HashMap<String, String>>,
}

impl EvoLine {
    pub fn new(
        base_id: i64,
        tree: EvoNode,
        rarity: Rarity,
        names: HashMap<i64, HashMap<String, String>>,
    ) -> Self {
        let tree = tree
            .keeping_animated_sprites()
            .unwrap_or_else(|| EvoNode::new(base_id, Vec::new()));
        Self {
            base_id,
            tree,
            rarity,
            names,
        }
    }

    pub fn total_forms(&self) -> i64 {
        self.tree.depth()
    }

    pub fn localized_name(&self, id: i64, lang: AppLanguage) -> String {
        let names = self.names.get(&id).cloned().unwrap_or_default();
        // Fallback order is owned by AppLanguage.resolve_name (single source).
        lang.resolve_name(&names)
            .map(str::to_string)
            .unwrap_or_else(|| format!("#{id}"))
    }
}

/// Nature — all 25 main-series ones. Decided at hatch; no stat effect (identity display only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PokemonNature {
    Hardy,
    Lonely,
    Brave,
    Adamant,
    Naughty,
    Bold,
    Docile,
    Relaxed,
    Impish,
    Lax,
    Timid,
    Hasty,
    Serious,
    Jolly,
    Naive,
    Modest,
    Mild,
    Quiet,
    Bashful,
    Rash,
    Calm,
    Gentle,
    Sassy,
    Careful,
    Quirky,
}

impl PokemonNature {
    pub const ALL: [PokemonNature; 25] = [
        PokemonNature::Hardy,
        PokemonNature::Lonely,
        PokemonNature::Brave,
        PokemonNature::Adamant,
        PokemonNature::Naughty,
        PokemonNature::Bold,
        PokemonNature::Docile,
        PokemonNature::Relaxed,
        PokemonNature::Impish,
        PokemonNature::Lax,
        PokemonNature::Timid,
        PokemonNature::Hasty,
        PokemonNature::Serious,
        PokemonNature::Jolly,
        PokemonNature::Naive,
        PokemonNature::Modest,
        PokemonNature::Mild,
        PokemonNature::Quiet,
        PokemonNature::Bashful,
        PokemonNature::Rash,
        PokemonNature::Calm,
        PokemonNature::Gentle,
        PokemonNature::Sassy,
        PokemonNature::Careful,
        PokemonNature::Quirky,
    ];

    /// Official localized names (ko/en/ja/es).
    pub fn name(&self, lang: AppLanguage) -> &'static str {
        let (ko, en, ja, es) = match self {
            PokemonNature::Hardy => ("노력", "Hardy", "がんばりや", "Fuerte"),
            PokemonNature::Lonely => ("외로움", "Lonely", "さみしがり", "Huraña"),
            PokemonNature::Brave => ("용감", "Brave", "ゆうかん", "Audaz"),
            PokemonNature::Adamant => ("고집", "Adamant", "いじっぱり", "Firme"),
            PokemonNature::Naughty => ("개구쟁이", "Naughty", "やんちゃ", "Pícara"),
            PokemonNature::Bold => ("대담", "Bold", "ずぶとい", "Osada"),
            PokemonNature::Docile => ("온순", "Docile", "すなお", "Dócil"),
            PokemonNature::Relaxed => ("무사태평", "Relaxed", "のんき", "Plácida"),
            PokemonNature::Impish => ("장난꾸러기", "Impish", "わんぱく", "Agitada"),
            PokemonNature::Lax => ("촐랑", "Lax", "のうてんき", "Floja"),
            PokemonNature::Timid => ("겁쟁이", "Timid", "おくびょう", "Miedosa"),
            PokemonNature::Hasty => ("성급", "Hasty", "せっかち", "Activa"),
            PokemonNature::Serious => ("성실", "Serious", "まじめ", "Seria"),
            PokemonNature::Jolly => ("명랑", "Jolly", "ようき", "Alegre"),
            PokemonNature::Naive => ("천진난만", "Naive", "むじゃき", "Ingenua"),
            PokemonNature::Modest => ("조심", "Modest", "ひかえめ", "Modesta"),
            PokemonNature::Mild => ("의젓", "Mild", "おっとり", "Afable"),
            PokemonNature::Quiet => ("냉정", "Quiet", "れいせい", "Mansa"),
            PokemonNature::Bashful => ("수줍음", "Bashful", "てれや", "Tímida"),
            PokemonNature::Rash => ("덜렁", "Rash", "うっかりや", "Alocada"),
            PokemonNature::Calm => ("차분", "Calm", "おだやか", "Serena"),
            PokemonNature::Gentle => ("얌전", "Gentle", "おとなしい", "Amable"),
            PokemonNature::Sassy => ("건방", "Sassy", "なまいき", "Grosera"),
            PokemonNature::Careful => ("신중", "Careful", "しんちょう", "Cauta"),
            PokemonNature::Quirky => ("변덕", "Quirky", "きまぐれ", "Rara"),
        };
        match lang {
            AppLanguage::Ko => ko,
            AppLanguage::En => en,
            AppLanguage::Ja => ja,
            AppLanguage::Es => es,
        }
    }

    pub fn from_raw(s: &str) -> Option<PokemonNature> {
        match s {
            "hardy" => Some(PokemonNature::Hardy),
            "lonely" => Some(PokemonNature::Lonely),
            "brave" => Some(PokemonNature::Brave),
            "adamant" => Some(PokemonNature::Adamant),
            "naughty" => Some(PokemonNature::Naughty),
            "bold" => Some(PokemonNature::Bold),
            "docile" => Some(PokemonNature::Docile),
            "relaxed" => Some(PokemonNature::Relaxed),
            "impish" => Some(PokemonNature::Impish),
            "lax" => Some(PokemonNature::Lax),
            "timid" => Some(PokemonNature::Timid),
            "hasty" => Some(PokemonNature::Hasty),
            "serious" => Some(PokemonNature::Serious),
            "jolly" => Some(PokemonNature::Jolly),
            "naive" => Some(PokemonNature::Naive),
            "modest" => Some(PokemonNature::Modest),
            "mild" => Some(PokemonNature::Mild),
            "quiet" => Some(PokemonNature::Quiet),
            "bashful" => Some(PokemonNature::Bashful),
            "rash" => Some(PokemonNature::Rash),
            "calm" => Some(PokemonNature::Calm),
            "gentle" => Some(PokemonNature::Gentle),
            "sassy" => Some(PokemonNature::Sassy),
            "careful" => Some(PokemonNature::Careful),
            "quirky" => Some(PokemonNature::Quirky),
            _ => None,
        }
    }
}

/// Game balance — individual roll odds.
pub struct PokemonOdds;

impl PokemonOdds {
    /// Shiny hatch probability denominator — 1/64 (main series 1/4096 would
    /// never be seen at desktop-app scale).
    pub const SHINY_DENOMINATOR: u64 = 64;
    /// Ditto disguise probability denominator — only on common·≥2-form hatches,
    /// 1/128 (rarer than GO's estimated 1/50–70 disguised Ditto).
    pub const DITTO_DISGUISE_DENOMINATOR: u64 = 128;
    /// Ditto species id — reveal-only (excluded from the normal hatch pool).
    pub const DITTO_SPECIES_ID: i64 = 132;
}

/// The Pokémon currently being raised.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonState {
    #[serde(rename = "baseID")]
    pub base_id: i64,
    /// Realized evolution path (branch choice reflected).
    #[serde(rename = "pathIDs")]
    pub path_ids: Vec<i64>,
    /// The full evolution path selected in advance.
    #[serde(rename = "plannedPathIDs")]
    pub planned_path_ids: Vec<i64>,
    /// Current position within path_ids.
    #[serde(rename = "stageIndex")]
    pub stage_index: i64,
    /// Cumulative usage at the current form.
    #[serde(rename = "usedAtStage")]
    pub used_at_stage: i64,
    pub rarity: Rarity,
    #[serde(rename = "totalForms")]
    pub total_forms: i64,
    /// Decided at hatch, kept across evolution.
    #[serde(rename = "isShiny")]
    pub is_shiny: bool,
    /// Decided at hatch (older saves have none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nature: Option<PokemonNature>,
    /// Ditto disguise — nil = normal; a value means "identity Ditto disguised as
    /// this species" (same as base_id while disguised; the original disguise is
    /// kept even after reveal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ditto_disguise: Option<i64>,
    /// Disguise → reveal transition.
    #[serde(rename = "dittoRevealed")]
    pub ditto_revealed: bool,
}

impl MonState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_id: i64,
        path_ids: Vec<i64>,
        planned_path_ids: Option<Vec<i64>>,
        stage_index: i64,
        used_at_stage: i64,
        rarity: Rarity,
        total_forms: i64,
        is_shiny: bool,
        nature: Option<PokemonNature>,
        ditto_disguise: Option<i64>,
        ditto_revealed: bool,
    ) -> Self {
        let planned_path_ids = match planned_path_ids {
            Some(plan) if !plan.is_empty() => plan,
            _ => path_ids.clone(),
        };
        Self {
            base_id,
            path_ids,
            planned_path_ids,
            stage_index,
            used_at_stage,
            rarity,
            total_forms,
            is_shiny,
            nature,
            ditto_disguise,
            ditto_revealed,
        }
    }

    /// With empty pathIDs (corrupted state file) falls back to baseID — read on
    /// every render, so this prevents out-of-bounds crashes.
    pub fn current_id(&self) -> i64 {
        if self.path_ids.is_empty() {
            self.base_id
        } else {
            let idx = self.stage_index.min((self.path_ids.len() - 1) as i64);
            self.path_ids[idx as usize]
        }
    }
}

impl<'de> Deserialize<'de> for MonState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        // REQUIRED fields — an error drops this active / dex item.
        let base_id = get_required_i64(m, "baseID")?;
        let path_ids = get_required_i64_vec(m, "pathIDs")?;
        // Empty pathIDs is a corrupt state → fail the decode so the whole
        // CompanionState falls back to the default (egg).
        if path_ids.is_empty() {
            return Err(de::Error::custom("empty pathIDs"));
        }
        let planned_path_ids = match get_opt_i64_vec(m, "plannedPathIDs") {
            Some(plan) if !plan.is_empty() => plan,
            _ => path_ids.clone(),
        };
        let decoded_stage_index = get_required_i64(m, "stageIndex")?;
        let stage_index = decoded_stage_index.clamp(0, (path_ids.len() - 1) as i64);
        let used_at_stage = get_required_i64(m, "usedAtStage")?;
        let rarity = get_required_rarity(m, "rarity")?;
        let total_forms = get_required_i64(m, "totalForms")?;
        Ok(Self {
            base_id,
            path_ids,
            planned_path_ids,
            stage_index,
            used_at_stage,
            rarity,
            total_forms,
            is_shiny: get_bool(m, "isShiny"),
            nature: get_opt_nature(m, "nature"),
            ditto_disguise: get_opt_i64(m, "dittoDisguise"),
            ditto_revealed: get_bool(m, "dittoRevealed"),
        })
    }
}

/// One dex entry — the whole line (initial→final), order preserved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DexEntry {
    pub id: Uuid,
    #[serde(rename = "baseID")]
    pub base_id: i64,
    #[serde(rename = "finalID")]
    pub final_id: i64,
    /// Initial→final species ids.
    #[serde(rename = "chainOrder")]
    pub chain_order: Vec<i64>,
    pub rarity: Rarity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caught_at: Option<DateTime<Utc>>,
    #[serde(rename = "isShiny")]
    pub is_shiny: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nature: Option<PokemonNature>,
    /// Each evolution-chain species' multilingual names (speciesID → langCode →
    /// name). Stored at graduation from the loaded line so the dex shows names
    /// under stage sprites instantly and survives language switches, offline.
    /// Older saves lack it (nil) and the view backfills via a line fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<i64, HashMap<String, String>>>,
}

impl DexEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        base_id: i64,
        final_id: i64,
        chain_order: Vec<i64>,
        rarity: Rarity,
        caught_at: Option<DateTime<Utc>>,
        is_shiny: bool,
        nature: Option<PokemonNature>,
        names: Option<HashMap<i64, HashMap<String, String>>>,
    ) -> Self {
        Self {
            id,
            base_id,
            final_id,
            chain_order,
            rarity,
            caught_at,
            is_shiny,
            nature,
            names,
        }
    }
}

impl<'de> Deserialize<'de> for DexEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let base_id = get_required_i64(m, "baseID")?;
        let final_id = get_required_i64(m, "finalID")?;
        let chain_order = get_required_i64_vec(m, "chainOrder")?;
        let rarity = get_required_rarity(m, "rarity")?;
        // id / caughtAt / isShiny / nature are lenient — one bad field must not
        // drop the whole entry.
        let id = m
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);
        let caught_at = get_opt_string(m, "caughtAt").and_then(|s| parse_iso8601(&s));
        // try? — even a legacy single-map format degrades names to nil instead
        // of dropping the entry (the view backfills via a line fetch).
        let names = get_opt_species_names(m, "names");
        Ok(Self {
            id,
            base_id,
            final_id,
            chain_order,
            rarity,
            caught_at,
            is_shiny: get_bool(m, "isShiny"),
            nature: get_opt_nature(m, "nature"),
            names,
        })
    }
}

/// Persistent state (Application Support JSON). Switching Pokémon — the old
/// custom character state is discarded (fresh start).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionState {
    // Tokens: measured since install only.
    #[serde(skip_serializing_if = "is_false")]
    pub install_baseline_set: bool,
    pub used_since_install: i64,
    // Cumulative shop spending (currency ledger). Spendable = usedSinceInstall
    // − spentTokens. The growth meter (usedSinceInstall) is immutable — buying
    // only raises this value (no growth rewind).
    pub spent_tokens: i64,
    // Tokens spent since the current egg appeared (hatch incubation). Separate
    // from the cumulative counter — resets to 0 for each new egg after graduation.
    pub egg_usage: i64,
    // Guarantee floor of the current egg (premium egg). nil = no guarantee
    // (free/base egg). ★Persistent — the species can't be chosen at purchase
    // (the roll needs the network), so the guarantee is written into state and
    // the roll reads it; it survives offline/restart. Consumed to nil at hatch/graduation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub egg_tier: Option<Rarity>,
    // Pre-rolled hatch species while egged (prefetch) — removes the network
    // delay at hatch. Survives restart.
    #[serde(rename = "pendingHatchID", skip_serializing_if = "Option::is_none")]
    pub pending_hatch_id: Option<i64>,
    /// Today's usage accrual baseline, kept per provider.
    ///
    /// nil = a legacy save that only had the aggregate `claimedTodayTokens`
    /// hasn't been seeded against a first valid snapshot yet. The first update
    /// only stores the current provider value as the baseline; past usage is
    /// not credited retroactively. An empty map is the normal post-seed state
    /// when no provider reported today, and is distinct from nil. Keys use
    /// UsageProvider.id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_today_tokens_by_provider: Option<HashMap<String, i64>>,
    pub last_date: String,
    // Current Pokémon (egg when absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<MonState>,
    // Dex.
    pub dex: Vec<DexEntry>,
    // Owned (base,final) pairs — branch diversity.
    pub collected_finals: HashSet<String>,
    // New installs = system locale.
    pub language: AppLanguage,
    // Inventory (ItemKind.rawValue → count).
    pub inventory: HashMap<String, i64>,
    // Candy-grant edge state (window key → tier granted). ★Persistent — unlike
    // the in-memory notifiedTier, prevents infinite re-grants across restarts.
    pub candy_grant_tier: HashMap<String, i64>,
    // First-run candy seeding done — blocks retroactive grants to windows that
    // were already at 100% right after an update.
    pub candy_feature_seeded: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Default for CompanionState {
    fn default() -> Self {
        Self {
            install_baseline_set: false,
            used_since_install: 0,
            spent_tokens: 0,
            egg_usage: 0,
            egg_tier: None,
            pending_hatch_id: None,
            claimed_today_tokens_by_provider: None,
            last_date: String::new(),
            active: None,
            dex: Vec::new(),
            collected_finals: HashSet::new(),
            language: AppLanguage::system_default(),
            inventory: HashMap::new(),
            candy_grant_tier: HashMap::new(),
            candy_feature_seeded: false,
        }
    }
}

impl<'de> Deserialize<'de> for CompanionState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Backward-compatible + corruption-recovering decode: missing keys,
        // type mismatches, and partially corrupt fields all collapse to
        // defaults — one broken field never wipes the whole state (dex,
        // inventory) (partial recovery > full reset). Only a top-level
        // non-object throws → load() backs up the original as .corrupt and
        // starts fresh.
        let v = Value::deserialize(deserializer)?;
        let m = v
            .as_object()
            .ok_or_else(|| de::Error::custom("not an object"))?;
        let claimed_today_tokens_by_provider = if m.contains_key("claimedTodayTokensByProvider") {
            // Present-but-null/corrupt → Some(empty); a present key means the
            // ledger has been seeded.
            Some(get_string_i64_map(m, "claimedTodayTokensByProvider"))
        } else {
            // Legacy aggregate claimedTodayTokens is intentionally NOT read —
            // provider split is impossible, so the next CompanionStore.update
            // seeds the current snapshot as the baseline.
            None
        };
        let active = match m.get("active") {
            // Corrupt active (empty pathIDs etc.) → fall back to egg, but keep
            // dex/inventory.
            None | Some(Value::Null) => None,
            Some(item) => MonState::deserialize(item).ok(),
        };
        let dex = match m.get("dex") {
            // Dex items decode in isolation — one corrupt item is dropped, not
            // fatal to the rest.
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|item| DexEntry::deserialize(item).ok())
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            install_baseline_set: get_bool(m, "installBaselineSet"),
            used_since_install: get_i64(m, "usedSinceInstall"),
            spent_tokens: get_i64(m, "spentTokens"),
            egg_usage: get_i64(m, "eggUsage"),
            egg_tier: get_rarity(m, "eggTier"),
            pending_hatch_id: get_opt_i64(m, "pendingHatchID"),
            claimed_today_tokens_by_provider,
            last_date: get_string(m, "lastDate"),
            active,
            dex,
            collected_finals: get_string_set(m, "collectedFinals"),
            language: get_language(m, "language"),
            inventory: get_string_i64_map(m, "inventory"),
            candy_grant_tier: get_string_i64_map(m, "candyGrantTier"),
            candy_feature_seeded: get_bool(m, "candyFeatureSeeded"),
        })
    }
}

#[cfg(test)]
mod tests {
    // The tests deliberately mirror the Swift style: construct the default
    // state then mutate the fields one at a time.
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use std::collections::{HashMap, HashSet};
    use uuid::Uuid;

    fn decode<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).expect("decode failed")
    }

    fn evo_node(id: i64, children: Vec<EvoNode>) -> EvoNode {
        EvoNode::new(id, children)
    }

    fn names_map(id: i64, pairs: &[(&str, &str)]) -> HashMap<i64, HashMap<String, String>> {
        let mut inner = HashMap::new();
        for (lang, name) in pairs {
            inner.insert(lang.to_string(), name.to_string());
        }
        let mut out = HashMap::new();
        out.insert(id, inner);
        out
    }

    // MARK: Rarity

    #[test]
    fn capture_rate_boundaries() {
        assert_eq!(Rarity::from(45, false, false), Rarity::Rare); // <=45
        assert_eq!(Rarity::from(46, false, false), Rarity::Uncommon);
        assert_eq!(Rarity::from(120, false, false), Rarity::Uncommon); // <=120
        assert_eq!(Rarity::from(121, false, false), Rarity::Common);
    }

    #[test]
    fn legendary_and_mythical_override_capture_rate() {
        assert_eq!(Rarity::from(255, true, false), Rarity::Legendary);
        assert_eq!(Rarity::from(255, false, true), Rarity::Legendary);
    }

    #[test]
    fn rarity_derivation() {
        assert_eq!(Rarity::from(255, false, false), Rarity::Common);
        assert_eq!(Rarity::from(90, false, false), Rarity::Uncommon);
        assert_eq!(Rarity::from(45, false, false), Rarity::Rare);
        assert_eq!(Rarity::from(3, true, false), Rarity::Legendary);
    }

    #[test]
    fn sort_rank_orders_rarity_ascending_by_value() {
        assert!(Rarity::Common.sort_rank() < Rarity::Uncommon.sort_rank());
        assert!(Rarity::Uncommon.sort_rank() < Rarity::Rare.sort_rank());
        assert!(Rarity::Rare.sort_rank() < Rarity::Legendary.sort_rank());
    }

    #[test]
    fn legendary_has_no_capture_rate_ceiling_but_passes_lower_tier_filters() {
        assert!(Rarity::Legendary.capture_rate_ceiling().is_none());
        assert!(!Rarity::Legendary.includes(3));
        // Real legendary capture rates (3·30·45) pass both the uncommon and rare filters.
        for cr in [3, 30, 45] {
            assert!(Rarity::Rare.includes(cr));
            assert!(Rarity::Uncommon.includes(cr));
        }
        assert!(
            !FreshEgg::SHOP_TIERS.contains(&Some(Rarity::Legendary)),
            "legendary-only eggs are not sold"
        );
    }

    #[test]
    fn capture_rate_ceiling_is_the_same_threshold_as_classification() {
        for cr in 0..=255i64 {
            let classified = Rarity::from(cr, false, false);
            for tier in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
                assert_eq!(
                    tier.includes(cr),
                    classified.sort_rank() >= tier.sort_rank(),
                    "capture_rate {cr} classified={classified:?}"
                );
            }
        }
    }

    // MARK: PokemonBalance

    #[test]
    fn graduation_total_is_constant_per_rarity_regardless_of_stages() {
        for rarity in [
            Rarity::Common,
            Rarity::Uncommon,
            Rarity::Rare,
            Rarity::Legendary,
        ] {
            let t = PokemonBalance::graduation_total(rarity);
            for k in 1..=3 {
                let sum: i64 = (0..k)
                    .map(|i| PokemonBalance::phase_threshold(rarity, k, i))
                    .sum();
                // rounding tolerance
                assert!(
                    (sum - t).abs() <= 2,
                    "rarity={rarity:?} k={k} sum={sum} T={t}"
                );
            }
        }
    }

    #[test]
    fn higher_stage_costs_more() {
        for k in 2..=3 {
            for i in 0..(k - 1) {
                assert!(
                    PokemonBalance::phase_threshold(Rarity::Common, k, i)
                        < PokemonBalance::phase_threshold(Rarity::Common, k, i + 1)
                );
            }
        }
    }

    #[test]
    fn rarer_costs_more() {
        assert!(
            PokemonBalance::graduation_total(Rarity::Common)
                < PokemonBalance::graduation_total(Rarity::Uncommon)
        );
        assert!(
            PokemonBalance::graduation_total(Rarity::Uncommon)
                < PokemonBalance::graduation_total(Rarity::Rare)
        );
        assert!(
            PokemonBalance::graduation_total(Rarity::Rare)
                < PokemonBalance::graduation_total(Rarity::Legendary)
        );
    }

    // MARK: EvoNode / EvoLine

    #[test]
    fn depth_is_longest_path() {
        // 1 → {2 → 3, 4}  (branches: 3-step path + 2-step path)
        let tree = evo_node(
            1,
            vec![evo_node(2, vec![evo_node(3, vec![])]), evo_node(4, vec![])],
        );
        assert_eq!(tree.depth(), 3); // 1-2-3
        assert_eq!(evo_node(20, vec![]).depth(), 1); // no evolution
    }

    #[test]
    fn node_lookup_by_id() {
        let tree = evo_node(
            1,
            vec![evo_node(2, vec![evo_node(3, vec![])]), evo_node(4, vec![])],
        );
        assert_eq!(tree.node_with_id(3).map(|n| n.species_id), Some(3));
        assert_eq!(tree.node_with_id(4).map(|n| n.species_id), Some(4));
        assert!(tree.node_with_id(99).is_none());
    }

    #[test]
    fn final_ids_are_leaves() {
        let tree = evo_node(
            1,
            vec![evo_node(2, vec![evo_node(3, vec![])]), evo_node(4, vec![])],
        );
        let mut ids = tree.final_ids();
        ids.sort();
        assert_eq!(ids, vec![3, 4]);
        assert_eq!(evo_node(20, vec![]).final_ids(), vec![20]); // a leaf is its own final
    }

    #[test]
    fn keeps_only_forms_with_animated_assets() {
        // Gen-V+ evolution after the PokéAPI animated range (#979) must be pruned.
        let line = EvoLine::new(
            56,
            evo_node(56, vec![evo_node(57, vec![evo_node(979, vec![])])]),
            Rarity::Common,
            HashMap::new(),
        );
        assert_eq!(line.total_forms(), 2);
        assert_eq!(line.tree.final_ids(), vec![57]);
        assert!(line.tree.node_with_id(979).is_none());
    }

    #[test]
    fn picks_language_specific_then_falls_back_to_english_then_id() {
        let mut names = HashMap::new();
        names.insert(
            1i64,
            HashMap::from([
                ("ja-Hrkt".to_string(), "ピカ".to_string()),
                ("ja".to_string(), "ピカチュウ".to_string()),
                ("en".to_string(), "Pika".to_string()),
                ("ko".to_string(), "피카".to_string()),
            ]),
        );
        names.insert(
            2i64,
            HashMap::from([("en".to_string(), "Eevee".to_string())]),
        );
        names.insert(3i64, HashMap::new());
        let line = EvoLine::new(1, evo_node(1, vec![]), Rarity::Common, names);
        // ja tries ja-Hrkt before ja.
        assert_eq!(line.localized_name(1, AppLanguage::Ja), "ピカ");
        assert_eq!(line.localized_name(1, AppLanguage::Ko), "피카");
        assert_eq!(line.localized_name(1, AppLanguage::En), "Pika");
        // no language match → English fallback.
        assert_eq!(line.localized_name(2, AppLanguage::Ja), "Eevee");
        assert_eq!(line.localized_name(2, AppLanguage::Ko), "Eevee");
        // no English either → #id.
        assert_eq!(line.localized_name(3, AppLanguage::Ko), "#3");
        // entirely missing id.
        assert_eq!(line.localized_name(99, AppLanguage::En), "#99");
    }

    #[test]
    fn ja_falls_back_from_hrkt_to_plain_ja() {
        let line = EvoLine::new(
            1,
            evo_node(1, vec![]),
            Rarity::Common,
            names_map(1, &[("ja", "ピカチュウ"), ("en", "Pika")]),
        );
        assert_eq!(line.localized_name(1, AppLanguage::Ja), "ピカチュウ"); // ja-Hrkt absent → ja
    }

    // MARK: MonState

    #[test]
    fn current_id_clamps_to_path() {
        let m = MonState::new(
            1,
            vec![1, 2, 3],
            None,
            1,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        );
        assert_eq!(m.current_id(), 2);
        // stageIndex beyond the path clamps to the last (defensive).
        let over = MonState::new(
            1,
            vec![1],
            None,
            5,
            0,
            Rarity::Common,
            1,
            false,
            None,
            None,
            false,
        );
        assert_eq!(over.current_id(), 1);
    }

    #[test]
    fn current_id_falls_back_to_base_when_path_empty() {
        let m = MonState::new(
            42,
            vec![],
            None,
            0,
            0,
            Rarity::Common,
            1,
            false,
            None,
            None,
            false,
        );
        assert_eq!(m.current_id(), 42);
    }

    #[test]
    fn mon_state_decode_clamps_stage_index_to_realized_path_bounds() {
        let upper: MonState = decode(
            r#"{"baseID":1,"pathIDs":[1,2],"stageIndex":5,"usedAtStage":0,"rarity":"common","totalForms":2}"#,
        );
        let lower: MonState = decode(
            r#"{"baseID":1,"pathIDs":[1,2],"stageIndex":-1,"usedAtStage":0,"rarity":"common","totalForms":2}"#,
        );
        assert_eq!(upper.stage_index, 1);
        assert_eq!(lower.stage_index, 0);
    }

    #[test]
    fn mon_state_round_trip_preserves_distinct_planned_path() {
        let state = MonState::new(
            265,
            vec![265],
            Some(vec![265, 266, 267]),
            0,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        );
        let decoded: MonState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(decoded.path_ids, vec![265]);
        assert_eq!(decoded.planned_path_ids, vec![265, 266, 267]);
    }

    #[test]
    fn mon_state_legacy_decode_uses_realized_path_as_plan() {
        let decoded: MonState = decode(
            r#"{"baseID":265,"pathIDs":[265,266],"stageIndex":1,"usedAtStage":0,"rarity":"common","totalForms":3}"#,
        );
        assert_eq!(decoded.path_ids, vec![265, 266]);
        assert_eq!(decoded.planned_path_ids, vec![265, 266]);
    }

    #[test]
    fn mon_state_empty_saved_plan_uses_realized_path() {
        let decoded: MonState = decode(
            r#"{"baseID":265,"pathIDs":[265,266],"plannedPathIDs":[],"stageIndex":1,"usedAtStage":0,"rarity":"common","totalForms":3}"#,
        );
        assert_eq!(decoded.planned_path_ids, vec![265, 266]);
    }

    #[test]
    fn mon_state_empty_initial_plan_uses_realized_path() {
        let state = MonState::new(
            265,
            vec![265, 266],
            Some(vec![]),
            1,
            0,
            Rarity::Common,
            3,
            false,
            None,
            None,
            false,
        );
        assert_eq!(state.planned_path_ids, vec![265, 266]);
    }

    #[test]
    fn empty_path_ids_active_falls_back_to_nil_preserving_rest() {
        let corrupt = r#"{"installBaselineSet":true,"eggUsage":0,"lastDate":"d1",
         "active":{"baseID":1,"pathIDs":[],"stageIndex":0,"usedAtStage":0,"rarity":"common","totalForms":3}}"#;
        let state: CompanionState = serde_json::from_str(corrupt).unwrap();
        // empty pathIDs invalidates only active — the whole decode succeeds (partial recovery).
        assert!(state.active.is_none());
        assert!(state.install_baseline_set, "remaining fields preserved");
    }

    #[test]
    fn backward_compatible_decode() {
        let old = r#"{"installBaselineSet":true,"usedSinceInstall":100,"eggUsage":0,
         "claimedTodayTokens":100,"lastDate":"d1",
         "active":{"baseID":1,"pathIDs":[1],"stageIndex":0,"usedAtStage":5,"rarity":"common","totalForms":3},
         "dex":[{"id":"x","baseID":4,"finalID":6,"chainOrder":[4,5,6],"rarity":"rare"}],
         "collectedFinals":["4:6"],"language":"ko"}"#;
        let s: CompanionState = decode(old);
        assert_eq!(s.active.as_ref().unwrap().planned_path_ids, vec![1]);
        assert!(!s.active.as_ref().unwrap().is_shiny);
        assert!(s.active.as_ref().unwrap().nature.is_none());
        // legacy aggregate ledger is never guessed into provider values.
        assert!(s.claimed_today_tokens_by_provider.is_none());
        assert!(!s.dex[0].is_shiny);
        assert!(s.dex[0].nature.is_none());
        // re-encode → re-decode stays stable (round trip).
        let round: CompanionState =
            serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert!(!round.active.as_ref().unwrap().is_shiny);
    }

    // MARK: CompanionState

    #[test]
    fn companion_state_encode_decode_round_trip() {
        let mut st = CompanionState::default();
        st.install_baseline_set = true;
        st.used_since_install = 42;
        st.egg_usage = 1234;
        st.claimed_today_tokens_by_provider = Some(HashMap::from([("test".to_string(), 7)]));
        st.last_date = "2026-06-27".to_string();
        st.collected_finals = HashSet::from(["1:3".to_string(), "10:12".to_string()]);
        st.language = AppLanguage::Ja;
        st.dex = vec![DexEntry::new(
            Uuid::new_v4(),
            1,
            3,
            vec![1, 2, 3],
            Rarity::Rare,
            None,
            false,
            None,
            None,
        )];

        let data = serde_json::to_vec(&st).unwrap();
        let back: CompanionState = serde_json::from_slice(&data).unwrap();

        assert!(back.install_baseline_set);
        assert_eq!(back.used_since_install, 42);
        assert_eq!(back.egg_usage, 1234);
        assert_eq!(
            back.claimed_today_tokens_by_provider,
            Some(HashMap::from([("test".to_string(), 7)]))
        );
        assert_eq!(back.last_date, "2026-06-27");
        assert_eq!(
            back.collected_finals,
            HashSet::from(["1:3".to_string(), "10:12".to_string()])
        );
        assert_eq!(back.language, AppLanguage::Ja);
        assert_eq!(back.dex.len(), 1);
        assert_eq!(back.dex[0].chain_order, vec![1, 2, 3]);
    }

    #[test]
    fn decodes_without_inventory_fields() {
        let json = r#"{"installBaselineSet":true,"usedSinceInstall":5,"lastDate":"d","dex":[],"language":"ko"}"#;
        let s: CompanionState = decode(json);
        assert!(s.inventory.is_empty());
        assert!(s.candy_grant_tier.is_empty());
        assert!(!s.candy_feature_seeded);
    }

    #[test]
    fn inventory_round_trip() {
        let mut st = CompanionState::default();
        st.inventory = HashMap::from([("rareCandy".to_string(), 3)]);
        st.candy_grant_tier = HashMap::from([("claude.fiveHour".to_string(), 1)]);
        st.candy_feature_seeded = true;
        let round: CompanionState =
            serde_json::from_slice(&serde_json::to_vec(&st).unwrap()).unwrap();
        assert_eq!(
            round.inventory,
            HashMap::from([("rareCandy".to_string(), 3)])
        );
        assert_eq!(
            round.candy_grant_tier,
            HashMap::from([("claude.fiveHour".to_string(), 1)])
        );
        assert!(round.candy_feature_seeded);
    }

    #[test]
    fn decodes_without_spent_tokens() {
        let json = r#"{"installBaselineSet":true,"usedSinceInstall":900,"lastDate":"d","dex":[]}"#;
        let s: CompanionState = decode(json);
        assert_eq!(s.spent_tokens, 0);
        assert_eq!(s.used_since_install, 900);
    }

    #[test]
    fn spent_tokens_round_trip() {
        let mut st = CompanionState::default();
        st.used_since_install = 1000;
        st.spent_tokens = 400;
        let round: CompanionState =
            serde_json::from_slice(&serde_json::to_vec(&st).unwrap()).unwrap();
        assert_eq!(round.spent_tokens, 400);
    }

    #[test]
    fn state_decodes_without_egg_usage() {
        let json = r#"{"installBaselineSet":true,"usedSinceInstall":5,"claimedTodayTokens":5,"lastDate":"d","active":null,"dex":[],"collectedFinals":[],"language":"ko"}"#;
        let state: CompanionState = decode(json);
        assert_eq!(state.egg_usage, 0);
        assert_eq!(state.used_since_install, 5);
        assert!(state.claimed_today_tokens_by_provider.is_none());
    }

    #[test]
    fn corrupt_dex_entry_dropped_while_rest_survives() {
        // 2 valid + 1 corrupt (finalID/chainOrder missing).
        let json = r#"{"dex":[{"baseID":1,"finalID":3,"chainOrder":[1,2,3],"rarity":"common"},
        {"baseID":99,"rarity":"rare"},
        {"baseID":7,"finalID":9,"chainOrder":[7,8,9],"rarity":"uncommon"}],"inventory":{"rareCandy":2}}"#;
        let s: CompanionState = decode(json);
        assert_eq!(s.dex.len(), 2, "corrupt item dropped, 2 valid kept");
        let bases: HashSet<i64> = s.dex.iter().map(|e| e.base_id).collect();
        assert_eq!(bases, HashSet::from([1, 7]));
        assert_eq!(
            s.inventory.get("rareCandy"),
            Some(&2),
            "dex damage must not wipe other state"
        );
    }

    #[test]
    fn corrupt_active_falls_back_to_egg_while_rest_survives() {
        // active missing pathIDs → MonState decode fails.
        let json = r#"{"active":{"baseID":1},
        "dex":[{"baseID":1,"finalID":3,"chainOrder":[1,2,3],"rarity":"common"}],
        "inventory":{"rareCandy":3},"usedSinceInstall":5000}"#;
        let s: CompanionState = decode(json);
        assert!(s.active.is_none(), "corrupt active → nil (egg) fallback");
        assert_eq!(s.dex.len(), 1, "dex preserved");
        assert_eq!(
            s.inventory.get("rareCandy"),
            Some(&3),
            "inventory preserved"
        );
        assert_eq!(s.used_since_install, 5000, "accumulated tokens preserved");
    }

    #[test]
    fn unknown_guarantee_decodes_as_no_guarantee() {
        let json = r#"{"installBaselineSet":true,"usedSinceInstall":0,"spentTokens":0,
        "lastDate":"d","dex":[],"collectedFinals":[],"eggTier":"mythic"}"#;
        let decoded: CompanionState = decode(json);
        assert!(decoded.egg_tier.is_none());
    }

    #[test]
    fn language_unknown_raw_value_falls_back_to_system_default() {
        let json = r#"{"language":"xx"}"#;
        let s: CompanionState = decode(json);
        assert_eq!(s.language, AppLanguage::system_default());
        assert!(AppLanguage::ALL.contains(&s.language));
    }

    #[test]
    fn system_default_language_resolves() {
        assert!(AppLanguage::ALL.contains(&AppLanguage::system_default()));
        assert_eq!(
            CompanionState::default().language,
            AppLanguage::system_default()
        );
    }

    // MARK: Nature names

    #[test]
    fn nature_names_complete() {
        assert_eq!(PokemonNature::ALL.len(), 25);
        for lang in AppLanguage::ALL {
            let names: Vec<&str> = PokemonNature::ALL.iter().map(|n| n.name(lang)).collect();
            let unique: HashSet<&str> = names.iter().copied().collect();
            assert_eq!(unique.len(), 25, "{lang:?} duplicate/missing");
            assert!(!names.iter().any(|s| s.is_empty()));
        }
    }

    // MARK: Shop constants

    #[test]
    fn fresh_egg_price_is_one_billion() {
        assert_eq!(FreshEgg::PRICE, 1_000_000_000);
    }

    #[test]
    fn prices_follow_graduation_total_ratio() {
        assert_eq!(FreshEgg::price(None), 1_000_000_000);
        assert_eq!(FreshEgg::price(Some(Rarity::Uncommon)), 2_500_000_000);
        assert_eq!(FreshEgg::price(Some(Rarity::Rare)), 4_000_000_000);
        assert_eq!(
            FreshEgg::SHOP_TIERS,
            [None, Some(Rarity::Uncommon), Some(Rarity::Rare)]
        );
    }

    #[test]
    fn shop_entry_prices() {
        assert_eq!(ShopEntry::Item(ItemKind::Mint).price(), 100_000_000);
        assert_eq!(ShopEntry::Item(ItemKind::RareCandy).price(), 500_000_000);
        assert_eq!(ShopEntry::Egg(None).price(), 1_000_000_000);
        assert_eq!(
            ShopEntry::Egg(Some(Rarity::Uncommon)).price(),
            2_500_000_000
        );
    }

    #[test]
    fn mint_shop_price_and_purchasable() {
        assert_eq!(ItemKind::Mint.shop_price(), Some(Mint::PRICE));
        assert_eq!(ItemKind::Mint.shop_price(), Some(100_000_000));
    }

    #[test]
    fn shiny_charm_constants_and_passive_flag() {
        assert_eq!(ShinyCharm::PRICE, 3_000_000_000);
        assert_eq!(ShinyCharm::SHINY_DENOMINATOR, 48);
        assert!(ItemKind::ShinyCharm.is_passive());
        assert!(!ItemKind::RareCandy.is_passive());
        assert!(!ItemKind::Mint.is_passive());
        assert_eq!(ItemKind::ShinyCharm.sprite_name(), Some("shiny-charm"));
        assert_eq!(ItemKind::RareCandy.sprite_name(), Some("rare-candy"));
        assert!(ItemKind::Mint.sprite_name().is_none());
        assert_eq!(ItemKind::RareCandy.fallback_emoji(), "🍬");
        assert_eq!(ItemKind::Mint.fallback_emoji(), "🌿");
        assert_eq!(ItemKind::ShinyCharm.fallback_emoji(), "✨");
    }

    #[test]
    fn rare_candy_constants() {
        assert_eq!(RareCandy::XP, 100_000_000);
        assert_eq!(RareCandy::WEEKLY_GRANT, 5);
        assert_eq!(RareCandy::PRICE, 500_000_000);
        assert_eq!(ItemKind::RareCandy.shop_price(), Some(500_000_000));
    }

    #[test]
    fn pokemon_odds_constants() {
        assert_eq!(PokemonOdds::SHINY_DENOMINATOR, 64);
        assert_eq!(PokemonOdds::DITTO_DISGUISE_DENOMINATOR, 128);
        assert_eq!(PokemonOdds::DITTO_SPECIES_ID, 132);
    }
}
