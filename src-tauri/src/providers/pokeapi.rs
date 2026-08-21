//! PokéAPI client — species / evolution-chain runtime fetch + parse. Pokémon
//! data is not bundled in the repo; everything is fetched on demand.
//!
//! Port of the original `Core/PokeAPIClient.swift`. The Swift `actor` becomes a
//! plain synchronous struct whose caches sit behind [`std::sync::Mutex`], and
//! the `PokeProviding` protocol becomes the [`PokeProvider`] trait. All methods
//! are blocking (no async) — the integration layer drives
//! [`PokeAPIClient::build_base_index_via_rest`] on a background thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::domain::companion::{EvoLine, EvoNode, PokemonAssets, PokemonOdds, Rarity};
use crate::platform;

/// Supported PokéAPI `language.name` codes (first match wins in the UI).
const LANG_CODES: &[&str] = &["ko", "en", "ja-Hrkt", "ja", "es"];

/// Official GraphQL endpoint (one-query base index).
const GRAPHQL_ENDPOINT: &str = "https://graphql.pokeapi.co/v1beta2";

/// REST build concurrency batch size (kept from the Swift for PokéAPI courtesy).
const REST_BATCH_SIZE: i64 = 6;

/// A REST build with fewer results than this is considered a failed build and
/// is not persisted (next session retries).
const MIN_BASES_TO_PERSIST: usize = 150;

/// Disk-cache freshness window for the base index.
const DISK_TTL_DAYS: i64 = 30;

/// Hatch candidate — a base (evolution-line start) species + official capture
/// rate (3 = Mewtwo-grade … 255 = Caterpie-grade).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BaseSpecies {
    pub id: i64,
    pub capture_rate: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum PokeError {
    #[error("network error: {0}")]
    Network(String),
    #[error("bad URL")]
    BadUrl,
    #[error("http status {0}")]
    HttpStatus(u16),
    #[error("could not parse response")]
    Parse,
    #[error("no candidates")]
    NoCandidates,
}

impl From<ureq::Error> for PokeError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Status(code, _response) => PokeError::HttpStatus(code),
            ureq::Error::Transport(transport) => PokeError::Network(transport.to_string()),
        }
    }
}

/// Pokémon line data provider (injectable — tests use stubs).
///
/// Port of the Swift `PokeProviding` protocol.
pub trait PokeProvider: Send + Sync {
    fn line(&self, base_species_id: i64) -> Result<EvoLine, PokeError>;
    /// Full base index for gens I–V (one GraphQL query, disk-cached).
    fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError>;
    /// Whether a single species is a base (evolution-line start). `Ok(None)`
    /// when it is not — the REST fallback for the GraphQL index endpoint.
    fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError>;
}

// MARK: - DTO (PokéAPI response partial decode)

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FlavorTextEntryDTO {
    pub flavor_text: String,
    pub language: NamedRef,
    pub version: Option<NamedRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GenusDTO {
    pub genus: String,
    pub language: NamedRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PokemonDetailDTO {
    pub height: Option<f64>,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SpeciesDTO {
    #[serde(default)]
    pub id: i64,
    pub capture_rate: i64,
    pub is_legendary: bool,
    pub is_mythical: bool,
    #[serde(default)]
    pub names: Vec<NameDTO>,
    pub evolution_chain: URLRef,
    /// `None` = evolution-line start (base).
    pub evolves_from_species: Option<NamedRef>,
    #[serde(default)]
    pub flavor_text_entries: Vec<FlavorTextEntryDTO>,
    #[serde(default)]
    pub genera: Vec<GenusDTO>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NameDTO {
    pub name: String,
    pub language: NamedRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NamedRef {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct URLRef {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChainDTO {
    pub chain: ChainLink,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChainLink {
    pub species: NamedRef,
    #[serde(default)]
    pub evolves_to: Vec<ChainLink>,
}

// MARK: - GraphQL base-index response / disk snapshot

#[derive(Debug, Deserialize)]
struct GraphQLBaseResponse {
    data: GraphQLDataBox,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GraphQLDataBox {
    pokemonspecies: Vec<GraphQLRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GraphQLRow {
    id: i64,
    capture_rate: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaseIndexSnapshot {
    fetched_at: DateTime<Utc>,
    entries: Vec<BaseSpecies>,
}

/// PokéAPI client — species / evolution-chain runtime fetch + parse.
pub struct PokeAPIClient {
    base_url: String,
    lang_codes: &'static [&'static str],
    species_cache: Mutex<HashMap<i64, SpeciesDTO>>,
    line_cache: Mutex<HashMap<i64, EvoLine>>,
    base_index_cache: Mutex<Option<Vec<BaseSpecies>>>,
    base_index_file: PathBuf,
}

impl PokeAPIClient {
    pub fn new() -> Self {
        Self {
            base_url: "https://pokeapi.co/api/v2".to_string(),
            lang_codes: LANG_CODES,
            species_cache: Mutex::new(HashMap::new()),
            line_cache: Mutex::new(HashMap::new()),
            base_index_cache: Mutex::new(None),
            base_index_file: platform::data_dir().join("base-index.json"),
        }
    }

    /// Process-wide shared client (mirrors Swift's `PokeAPIClient.shared`).
    pub fn shared() -> &'static PokeAPIClient {
        static SHARED: OnceLock<PokeAPIClient> = OnceLock::new();
        SHARED.get_or_init(PokeAPIClient::new)
    }

    /// A line (evolution tree + rarity + multilingual names) for a base species.
    /// Prefetched lines hit the cache, so hatching costs zero network.
    pub fn line(&self, base_species_id: i64) -> Result<EvoLine, PokeError> {
        if let Some(cached) = self.line_cache.lock().unwrap().get(&base_species_id) {
            return Ok(cached.clone());
        }
        let base_species = self.species(base_species_id)?;
        // PokéAPI's URL — an abnormal/empty value throws instead of
        // force-unwrapping (the app keeps the egg state).
        let chain_url = Self::validated_chain_url(&base_species.evolution_chain.url)
            .ok_or(PokeError::BadUrl)?;
        let chain_dto: ChainDTO = self.get(&chain_url)?;
        let tree = Self::node_from_chain_link(&chain_dto.chain);
        let rarity = Rarity::from(
            base_species.capture_rate,
            base_species.is_legendary,
            base_species.is_mythical,
        );
        // Names of every species in the line, in the supported languages only.
        let mut names: HashMap<i64, HashMap<String, String>> = HashMap::new();
        for id in Self::all_ids(&tree) {
            let sp = self.species(id)?;
            let mut by_lang = HashMap::new();
            for n in &sp.names {
                if self.lang_codes.iter().any(|code| *code == n.language.name) {
                    by_lang.insert(n.language.name.clone(), n.name.clone());
                }
            }
            names.insert(id, by_lang);
        }
        let line = EvoLine::new(base_species_id, tree, rarity, names);
        self.line_cache
            .lock()
            .unwrap()
            .insert(base_species_id, line.clone());
        Ok(line)
    }

    // MARK: base index (hatch candidates)

    /// All gen I–V bases (evolution-line starts) — one PokéAPI GraphQL query.
    /// Priority: memory cache → disk cache (30-day TTL) → GraphQL fetch (success
    /// refreshes the disk) → stale-but-present disk (offline fallback). Throws
    /// only when every source fails (the app keeps the egg, retries next tick).
    pub fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
        // Memory cache.
        if let Some(cached) = self.base_index_cache.lock().unwrap().as_ref() {
            return Ok(cached.clone());
        }
        // Disk cache (30-day TTL).
        let disk = load_base_index_snapshot(&self.base_index_file);
        if let Some(snapshot) = disk.as_ref() {
            let fresh = Utc::now() - snapshot.fetched_at < chrono::Duration::days(DISK_TTL_DAYS);
            if fresh && !snapshot.entries.is_empty() {
                *self.base_index_cache.lock().unwrap() = Some(snapshot.entries.clone());
                return Ok(snapshot.entries.clone());
            }
        }
        // GraphQL fetch — success refreshes the disk cache.
        match self.fetch_base_index() {
            Ok(entries) => {
                *self.base_index_cache.lock().unwrap() = Some(entries.clone());
                self.persist_base_index_snapshot(&entries);
                Ok(entries)
            }
            Err(err) => {
                // Offline — use even a stale index if present.
                if let Some(entries) = disk
                    .filter(|snapshot| !snapshot.entries.is_empty())
                    .map(|snapshot| snapshot.entries)
                {
                    *self.base_index_cache.lock().unwrap() = Some(entries.clone());
                    return Ok(entries);
                }
                Err(err)
            }
        }
    }

    /// Builds the base index via REST `pokemon-species/{id}` fetches (batch
    /// size 6) when the GraphQL base index endpoint is down. A successful build
    /// is persisted to the disk cache (30-day TTL), so hatch selection later
    /// runs offline-capable — a self-healing cache that doesn't tie hatching to
    /// one endpoint's survival.
    ///
    /// Mirror of the Swift `buildBaseIndexViaREST` background task: **the
    /// integration layer calls this on a background thread** (it is never
    /// invoked synchronously from the index path).
    pub fn build_base_index_via_rest(&self) {
        if self.base_index_cache.lock().unwrap().is_some() {
            return;
        }
        crate::platform::log::write("base index: building via REST (GraphQL unavailable)…");
        let max_id = *PokemonAssets::ANIMATED_SPECIES_IDS.end();
        let mut bases: Vec<BaseSpecies> = Vec::new();
        let mut start = 1;
        while start <= max_id {
            let end = (start + REST_BATCH_SIZE - 1).min(max_id);
            for id in start..=end {
                if let Ok(Some(base)) = self.base_species(id) {
                    bases.push(base);
                }
            }
            start += REST_BATCH_SIZE;
        }
        // A mostly-failed build (unstable network) must not persist a sparse
        // index — next session retries.
        if bases.len() < MIN_BASES_TO_PERSIST {
            crate::platform::log::write(&format!(
                "base index: REST build incomplete ({}) — not cached, will retry next session",
                bases.len()
            ));
            return;
        }
        bases.sort_by_key(|base| base.id);
        *self.base_index_cache.lock().unwrap() = Some(bases.clone());
        self.persist_base_index_snapshot(&bases);
        crate::platform::log::write(&format!(
            "base index: REST build done — {} bases persisted (offline-capable now)",
            bases.len()
        ));
    }

    /// Official GraphQL — `evolves_from IS NULL` (= base) and id ≤ 649 (Gen-V
    /// animated sprite upper bound). Ditto (#132) is reveal-only, so it is
    /// excluded from the normal hatch pool.
    fn fetch_base_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
        let max_id = *PokemonAssets::ANIMATED_SPECIES_IDS.end();
        let ditto_id = PokemonOdds::DITTO_SPECIES_ID;
        let query = format!(
            "{{ pokemonspecies(where: {{evolves_from_species_id: {{_is_null: true}}, id: {{_lte: {max_id}, _neq: {ditto_id}}}}}, order_by: {{id: asc}}) {{ id capture_rate }} }}"
        );
        let response = ureq::post(GRAPHQL_ENDPOINT)
            .timeout(Duration::from_secs(15))
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({ "query": query }))
            .map_err(PokeError::from)?;
        let decoded: GraphQLBaseResponse = response.into_json().map_err(|_| PokeError::Parse)?;
        let entries = decoded
            .data
            .pokemonspecies
            .into_iter()
            .map(|row| BaseSpecies {
                id: row.id,
                capture_rate: row.capture_rate,
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Err(PokeError::NoCandidates);
        }
        Ok(entries)
    }

    /// REST fallback — single species detail decides base-ness + capture rate.
    /// Works even when the GraphQL base index is down (separate endpoint).
    pub fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
        // Ditto is reveal-only — excluded from the normal hatch pool.
        if id == PokemonOdds::DITTO_SPECIES_ID {
            return Ok(None);
        }
        let dto = self.species(id)?;
        // An evolution intermediate is not a hatch candidate.
        if dto.evolves_from_species.is_some() {
            return Ok(None);
        }
        Ok(Some(BaseSpecies {
            id,
            capture_rate: dto.capture_rate,
        }))
    }

    fn species(&self, id: i64) -> Result<SpeciesDTO, PokeError> {
        if let Some(cached) = self.species_cache.lock().unwrap().get(&id) {
            return Ok(cached.clone());
        }
        let url = format!("{}/pokemon-species/{id}", self.base_url);
        let dto: SpeciesDTO = self.get(&url)?;
        self.species_cache.lock().unwrap().insert(id, dto.clone());
        Ok(dto)
    }

    /// GET + JSON decode with a 15-second timeout.
    fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, PokeError> {
        let response = ureq::get(url)
            .timeout(Duration::from_secs(15))
            .call()
            .map_err(PokeError::from)?;
        response.into_json().map_err(|_| PokeError::Parse)
    }

    // MARK: evolution chain tree

    fn node_from_chain_link(link: &ChainLink) -> EvoNode {
        let species_id = Self::id_from_url(link.species.url.as_deref().unwrap_or(""));
        EvoNode::new(
            species_id,
            link.evolves_to
                .iter()
                .map(Self::node_from_chain_link)
                .collect(),
        )
    }

    fn all_ids(node: &EvoNode) -> Vec<i64> {
        let mut ids = vec![node.species_id];
        for child in &node.children {
            ids.extend(Self::all_ids(child));
        }
        ids
    }

    /// ".../pokemon-species/{id}/" → the numeric id (0 when absent/non-numeric).
    fn id_from_url(species_url: &str) -> i64 {
        species_url
            .split('/')
            .rfind(|part| !part.is_empty())
            .and_then(|part| part.parse::<i64>().ok())
            .unwrap_or(0)
    }

    /// PokéAPI evolution_chain URL validation (SSRF guard) — the value is
    /// server-controlled, so it is pinned to https + the exact host
    /// "pokeapi.co" to prevent a tampered response from fetching an arbitrary
    /// host. `None` when unsuitable (the caller throws → the app keeps the egg
    /// state).
    fn validated_chain_url(raw: &str) -> Option<String> {
        let (scheme, rest) = raw.split_once("://")?;
        if !scheme.eq_ignore_ascii_case("https") || rest.is_empty() {
            return None;
        }
        // Host ends at the first path/query/fragment delimiter; strip userinfo
        // ("user@host") and port (":443") like Foundation URL.host does.
        let before_path = rest.split(['/', '?', '#']).next()?;
        let host = before_path.rsplit('@').next()?.split(':').next()?;
        (host == "pokeapi.co").then_some(raw.to_string())
    }

    // MARK: disk cache

    fn persist_base_index_snapshot(&self, entries: &[BaseSpecies]) {
        let Ok(data) = serde_json::to_vec(&BaseIndexSnapshot {
            fetched_at: Utc::now(),
            entries: entries.to_vec(),
        }) else {
            return;
        };
        // Mirror Swift's `.atomic` write: temp file + rename.
        let tmp = self.base_index_file.with_extension("json.tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &self.base_index_file);
        }
    }
}

impl Default for PokeAPIClient {
    fn default() -> Self {
        Self::new()
    }
}

/// A fresh client with its own empty caches (caches are an optimization only —
/// correctness never depends on them), so `PokeAPIClient::shared().clone()`
/// yields an independently usable client.
impl Clone for PokeAPIClient {
    fn clone(&self) -> Self {
        Self {
            base_url: self.base_url.clone(),
            lang_codes: self.lang_codes,
            species_cache: Mutex::new(HashMap::new()),
            line_cache: Mutex::new(HashMap::new()),
            base_index_cache: Mutex::new(None),
            base_index_file: self.base_index_file.clone(),
        }
    }
}

impl PokeProvider for PokeAPIClient {
    fn line(&self, base_species_id: i64) -> Result<EvoLine, PokeError> {
        PokeAPIClient::line(self, base_species_id)
    }

    fn base_species_index(&self) -> Result<Vec<BaseSpecies>, PokeError> {
        PokeAPIClient::base_species_index(self)
    }

    fn base_species(&self, id: i64) -> Result<Option<BaseSpecies>, PokeError> {
        PokeAPIClient::base_species(self, id)
    }
}

fn load_base_index_snapshot(path: &Path) -> Option<BaseIndexSnapshot> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn sprite_path(id: i64, shiny: bool) -> PathBuf {
    let dir = platform::data_dir().join("sprites");
    let _ = std::fs::create_dir_all(&dir);
    if shiny {
        dir.join(format!("{id}_shiny.png"))
    } else {
        dir.join(format!("{id}.png"))
    }
}

pub fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn get_or_fetch_sprite(id: i64, shiny: bool) -> Option<String> {
    if id <= 0 {
        return None;
    }
    let p = sprite_path(id, shiny);
    if let Ok(bytes) = std::fs::read(&p) {
        if !bytes.is_empty() {
            let b64 = base64_encode(&bytes);
            return Some(format!("data:image/png;base64,{b64}"));
        }
    }

    let sub = if shiny { "shiny/" } else { "" };
    let url = format!(
        "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/{sub}{id}.png"
    );
    if let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(8)).call() {
        let mut bytes = Vec::new();
        if std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes).is_ok()
            && !bytes.is_empty()
        {
            let _ = std::fs::write(&p, &bytes);
            let b64 = base64_encode(&bytes);
            return Some(format!("data:image/png;base64,{b64}"));
        }
    }

    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokedexDetails {
    pub id: i64,
    pub name: String,
    pub genus: String,
    pub flavor_text: String,
    pub height_m: f64,
    pub weight_kg: f64,
    pub capture_rate: i64,
    pub is_legendary: bool,
    pub is_mythical: bool,
}

pub fn pokedex_cache_path(id: i64) -> PathBuf {
    let dir = platform::data_dir().join("pokedex");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{id}.json"))
}

pub fn get_or_fetch_pokedex_details(id: i64, lang: &str) -> Option<PokedexDetails> {
    if id <= 0 {
        return None;
    }
    let p = pokedex_cache_path(id);
    if let Ok(content) = std::fs::read_to_string(&p) {
        if let Ok(details) = serde_json::from_str::<PokedexDetails>(&content) {
            return Some(details);
        }
    }

    // Fetch species detail from PokeAPI
    let species_url = format!("https://pokeapi.co/api/v2/pokemon-species/{id}");
    let resp = ureq::get(&species_url)
        .timeout(Duration::from_secs(8))
        .call()
        .ok()?;
    let dto: SpeciesDTO = resp.into_json().ok()?;

    // Find name in requested lang, fallback to English or default
    let name = dto
        .names
        .iter()
        .find(|n| n.language.name == lang)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| {
            dto.names
                .iter()
                .find(|n| n.language.name == "en")
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("Pokemon #{id}"))
        });

    // Find genus in requested lang, fallback to English or default
    let genus = dto
        .genera
        .iter()
        .find(|g| g.language.name == lang)
        .map(|g| g.genus.clone())
        .unwrap_or_else(|| {
            dto.genera
                .iter()
                .find(|g| g.language.name == "en")
                .map(|g| g.genus.clone())
                .unwrap_or_else(|| "Pokémon".to_string())
        });

    // Find flavor text in requested lang, fallback to English
    let raw_flavor = dto
        .flavor_text_entries
        .iter()
        .filter(|f| f.language.name == lang)
        .last()
        .map(|f| f.flavor_text.clone())
        .or_else(|| {
            dto.flavor_text_entries
                .iter()
                .filter(|f| f.language.name == "en")
                .last()
                .map(|f| f.flavor_text.clone())
        })
        .unwrap_or_else(|| "A loyal Pokémon companion raised with AI coding tokens.".to_string());

    let clean_flavor = raw_flavor
        .replace('\n', " ")
        .replace('\x0C', " ")
        .replace('\r', " ")
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");

    // Fetch height and weight from pokemon/{id}
    let pokemon_url = format!("https://pokeapi.co/api/v2/pokemon/{id}");
    let (height_m, weight_kg) = if let Ok(r) = ureq::get(&pokemon_url)
        .timeout(Duration::from_secs(8))
        .call()
    {
        if let Ok(p_dto) = r.into_json::<PokemonDetailDTO>() {
            let h = p_dto.height.unwrap_or(0.0) / 10.0;
            let w = p_dto.weight.unwrap_or(0.0) / 10.0;
            (h, w)
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };

    let details = PokedexDetails {
        id,
        name,
        genus,
        flavor_text: clean_flavor,
        height_m,
        weight_kg,
        capture_rate: dto.capture_rate,
        is_legendary: dto.is_legendary,
        is_mythical: dto.is_mythical,
    };

    if let Ok(json) = serde_json::to_string_pretty(&details) {
        let _ = std::fs::write(&p, json);
    }

    Some(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    // MARK: id_from_url

    #[test]
    fn id_from_url_parses_trailing_slash_id() {
        assert_eq!(
            PokeAPIClient::id_from_url("https://pokeapi.co/api/v2/pokemon-species/25/"),
            25
        );
        assert_eq!(
            PokeAPIClient::id_from_url("https://pokeapi.co/api/v2/pokemon-species/649"),
            649
        );
    }

    #[test]
    fn id_from_url_defaults_zero_for_empty_and_non_numeric() {
        assert_eq!(PokeAPIClient::id_from_url(""), 0);
        assert_eq!(PokeAPIClient::id_from_url("/"), 0);
        assert_eq!(
            PokeAPIClient::id_from_url("https://pokeapi.co/api/v2/pokemon-species/"),
            0
        );
        assert_eq!(PokeAPIClient::id_from_url("not-a-number"), 0);
    }

    // MARK: validated_chain_url (SSRF guard)

    #[test]
    fn validated_chain_url_accepts_https_pokeapi_co() {
        let url = "https://pokeapi.co/api/v2/evolution-chain/1/";
        assert_eq!(
            PokeAPIClient::validated_chain_url(url).as_deref(),
            Some(url)
        );
    }

    #[test]
    fn validated_chain_url_rejects_bad_scheme_host_and_garbage() {
        // wrong scheme
        assert!(
            PokeAPIClient::validated_chain_url("http://pokeapi.co/api/v2/evolution-chain/1/")
                .is_none()
        );
        // wrong host
        assert!(PokeAPIClient::validated_chain_url(
            "https://evil.example.com/api/v2/evolution-chain/1/"
        )
        .is_none());
        // suffix-host spoof
        assert!(PokeAPIClient::validated_chain_url(
            "https://pokeapi.co.evil.example.com/api/v2/evolution-chain/1/"
        )
        .is_none());
        // userinfo trick (host is evil.example.com)
        assert!(PokeAPIClient::validated_chain_url(
            "https://pokeapi.co@evil.example.com/api/v2/evolution-chain/1/"
        )
        .is_none());
        // no scheme / garbage / empty
        assert!(
            PokeAPIClient::validated_chain_url("pokeapi.co/api/v2/evolution-chain/1/").is_none()
        );
        assert!(PokeAPIClient::validated_chain_url("garbage").is_none());
        assert!(PokeAPIClient::validated_chain_url("").is_none());
    }

    // MARK: node / all_ids (built from hand-written chain JSON)

    #[test]
    fn node_builds_two_stage_chain() {
        // Bulbasaur → Ivysaur → Venusaur.
        let dto: ChainDTO = serde_json::from_str(
            r#"{
                "chain": {
                    "species": {"name": "bulbasaur", "url": "https://pokeapi.co/api/v2/pokemon-species/1/"},
                    "evolves_to": [
                        {
                            "species": {"name": "ivysaur", "url": "https://pokeapi.co/api/v2/pokemon-species/2/"},
                            "evolves_to": [
                                {
                                    "species": {"name": "venusaur", "url": "https://pokeapi.co/api/v2/pokemon-species/3/"},
                                    "evolves_to": []
                                }
                            ]
                        }
                    ]
                }
            }"#,
        )
        .unwrap();
        let tree = PokeAPIClient::node_from_chain_link(&dto.chain);
        assert_eq!(tree.species_id, 1);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].species_id, 2);
        assert_eq!(tree.children[0].children[0].species_id, 3);
        assert_eq!(PokeAPIClient::all_ids(&tree), vec![1, 2, 3]);
    }

    #[test]
    fn node_builds_branching_chain() {
        // Eevee (133) → {Vaporeon 134, Jolteon 135, Flareon 136}.
        let dto: ChainDTO = serde_json::from_str(
            r#"{
                "chain": {
                    "species": {"name": "eevee", "url": "https://pokeapi.co/api/v2/pokemon-species/133/"},
                    "evolves_to": [
                        {"species": {"name": "vaporeon", "url": "https://pokeapi.co/api/v2/pokemon-species/134/"}, "evolves_to": []},
                        {"species": {"name": "jolteon", "url": "https://pokeapi.co/api/v2/pokemon-species/135/"}, "evolves_to": []},
                        {"species": {"name": "flareon", "url": "https://pokeapi.co/api/v2/pokemon-species/136/"}, "evolves_to": []}
                    ]
                }
            }"#,
        )
        .unwrap();
        let tree = PokeAPIClient::node_from_chain_link(&dto.chain);
        assert_eq!(tree.species_id, 133);
        let child_ids: Vec<i64> = tree.children.iter().map(|c| c.species_id).collect();
        assert_eq!(child_ids, vec![134, 135, 136]);
        assert_eq!(PokeAPIClient::all_ids(&tree), vec![133, 134, 135, 136]);
    }

    #[test]
    fn all_ids_walks_tree_preorder() {
        // 1 → {2 → {4}, 3}
        let dto: ChainDTO = serde_json::from_value(serde_json::json!({
            "chain": {
                "species": {"name": "a", "url": "https://pokeapi.co/api/v2/pokemon-species/1/"},
                "evolves_to": [
                    {"species": {"name": "b", "url": "https://pokeapi.co/api/v2/pokemon-species/2/"},
                     "evolves_to": [
                        {"species": {"name": "d", "url": "https://pokeapi.co/api/v2/pokemon-species/4/"}, "evolves_to": []}
                     ]},
                    {"species": {"name": "c", "url": "https://pokeapi.co/api/v2/pokemon-species/3/"}, "evolves_to": []}
                ]
            }
        }))
        .unwrap();
        let node = PokeAPIClient::node_from_chain_link(&dto.chain);
        assert_eq!(PokeAPIClient::all_ids(&node), vec![1, 2, 4, 3]);
    }

    // MARK: DTO decoding (PokéAPI response shapes)

    #[test]
    fn species_dto_decodes_pokeapi_shape() {
        let json = r#"{
            "capture_rate": 45,
            "is_legendary": false,
            "is_mythical": false,
            "names": [
                {"name": "イシツブテ", "language": {"name": "ja-Hrkt", "url": "https://pokeapi.co/api/v2/language/1/"}},
                {"name": "Rock", "language": {"name": "en", "url": "https://pokeapi.co/api/v2/language/9/"}}
            ],
            "evolution_chain": {"url": "https://pokeapi.co/api/v2/evolution-chain/13/"},
            "evolves_from_species": null
        }"#;
        let dto: SpeciesDTO = serde_json::from_str(json).unwrap();
        assert_eq!(dto.capture_rate, 45);
        assert!(!dto.is_legendary);
        assert!(!dto.is_mythical);
        assert_eq!(dto.names.len(), 2);
        assert_eq!(dto.names[0].language.name, "ja-Hrkt");
        assert_eq!(dto.names[1].name, "Rock");
        assert_eq!(
            dto.evolution_chain.url,
            "https://pokeapi.co/api/v2/evolution-chain/13/"
        );
        assert!(dto.evolves_from_species.is_none());
    }

    #[test]
    fn species_dto_decodes_evolves_from_species() {
        let json = r#"{
            "capture_rate": 45,
            "is_legendary": false,
            "is_mythical": false,
            "names": [],
            "evolution_chain": {"url": "https://pokeapi.co/api/v2/evolution-chain/2/"},
            "evolves_from_species": {"name": "bulbasaur", "url": "https://pokeapi.co/api/v2/pokemon-species/1/"}
        }"#;
        let dto: SpeciesDTO = serde_json::from_str(json).unwrap();
        let evo = dto.evolves_from_species.unwrap();
        assert_eq!(evo.name, "bulbasaur");
        assert_eq!(
            evo.url.as_deref(),
            Some("https://pokeapi.co/api/v2/pokemon-species/1/")
        );
    }

    #[test]
    fn species_dto_defaults_missing_optionals() {
        let json = r#"{
            "capture_rate": 45,
            "is_legendary": false,
            "is_mythical": false,
            "evolution_chain": {"url": "https://pokeapi.co/api/v2/evolution-chain/13/"}
        }"#;
        let dto: SpeciesDTO = serde_json::from_str(json).unwrap();
        assert!(dto.names.is_empty());
        assert!(dto.evolves_from_species.is_none());
    }

    #[test]
    fn test_base64_encode_roundtrip() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b""), "");
    }

    // MARK: BaseSpecies serde

    #[test]
    fn base_species_round_trips_with_snake_case_keys() {
        let species = BaseSpecies {
            id: 25,
            capture_rate: 190,
        };
        let json = serde_json::to_string(&species).unwrap();
        assert!(json.contains("\"capture_rate\""));
        assert!(json.contains("\"id\""));
        let decoded: BaseSpecies = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, species);
        // The GraphQL/REST field shape decodes directly.
        let from_api: BaseSpecies =
            serde_json::from_str(r#"{"id": 4, "capture_rate": 45}"#).unwrap();
        assert_eq!(
            from_api,
            BaseSpecies {
                id: 4,
                capture_rate: 45
            }
        );
    }

    // MARK: GraphQL response + disk snapshot

    #[test]
    fn graphql_base_response_decodes() {
        let json = r#"{"data": {"pokemonspecies": [{"id": 1, "capture_rate": 45}, {"id": 4, "capture_rate": 45}]}}"#;
        let resp: GraphQLBaseResponse = serde_json::from_str(json).unwrap();
        let ids: Vec<i64> = resp.data.pokemonspecies.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 4]);
        assert_eq!(resp.data.pokemonspecies[1].capture_rate, 45);
    }

    #[test]
    fn base_index_snapshot_round_trips() {
        let snapshot = BaseIndexSnapshot {
            fetched_at: Utc::now(),
            entries: vec![BaseSpecies {
                id: 1,
                capture_rate: 45,
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: BaseIndexSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.fetched_at, snapshot.fetched_at);
        assert_eq!(decoded.entries, snapshot.entries);
    }
}
