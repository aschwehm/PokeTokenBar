//! Save transfer for device migration — wraps the state in an envelope for
//! export and re-import on another device.
//!
//! Mirrors the original `Core/SaveTransfer.swift`. The state is not written
//! raw because its decoding is intentionally lenient (`lenient*` — one broken
//! field must not wipe the dex), so **any** JSON would "succeed" as an
//! all-default state. The envelope's `format`/`schema` are not lenient (no
//! defaults), which rejects foreign JSON first.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::companion::CompanionState;

/// SaveEnvelope: format/schema gate, everything else travels with the state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEnvelope {
    pub format: String,
    pub schema: i32,
    pub app_version: String,
    pub exported_at: DateTime<Utc>,
    pub source_device: String,
    pub state: CompanionState,
}

impl SaveEnvelope {
    pub const FORMAT_ID: &'static str = "poketokenbar.save";
    pub const SCHEMA_VERSION: i32 = 1;
}

/// Minimal header read first so a newer-schema file (whose body may not even
/// parse) is reported as "update your app", not "not a save file".
#[derive(Deserialize)]
struct SaveHeader {
    format: String,
    schema: i32,
}

/// Summary for the overwrite confirmation — shows *what is being replaced*
/// numerically rather than a generic "are you sure?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSummary {
    pub dex_count: usize,
    pub lifetime_tokens: i64,
}

impl SaveSummary {
    pub fn new(state: &CompanionState) -> Self {
        Self {
            dex_count: state.dex.len(),
            lifetime_tokens: state.used_since_install,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveTransferError {
    /// Not an envelope, or another app's JSON.
    #[error("not a save file")]
    NotASaveFile,
    /// Newer schema than this build — a save made by a newer version.
    #[error("newer schema: found {found}, supported {supported}")]
    NewerSchema { found: i32, supported: i32 },
    /// Too large to be a save — parsing would stall the main thread.
    #[error("file too large: {bytes} bytes, limit {limit}")]
    FileTooLarge { bytes: usize, limit: usize },
    /// The pre-overwrite backup could not be written — no recovery means the
    /// import must stop.
    #[error("backup failed")]
    BackupFailed,
    /// The state could not be serialized for export (never expected in practice).
    #[error("could not serialize save data")]
    Serialization,
}

/// Import confirmation button layout. Kept out of AppKit because this rule is
/// directly tied to data loss and `NSAlert` setup is unreachable from XCTest —
/// if the two rows were swapped, a single Return key would replace this
/// machine's progress with no test able to catch it.
pub struct ImportConfirmPolicy;

impl ImportConfirmPolicy {
    pub const REPLACE_BUTTON_INDEX: usize = 0;
    pub const CANCEL_BUTTON_INDEX: usize = 1;

    /// The default button (Return) must be **cancel** — never make a
    /// destructive action the default.
    pub fn key_equivalent(for_button_at: usize) -> &'static str {
        if for_button_at == Self::CANCEL_BUTTON_INDEX {
            "\r"
        } else {
            ""
        }
    }
}

pub struct SaveTransfer;

impl SaveTransfer {
    /// Save file size ceiling. A normal save is a few KB and even a full dex
    /// stays under a few hundred KB. Without a ceiling a giant JSON stalls the
    /// main thread for seconds (measured: 39MB ≈ 1.8s).
    pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

    /// Ceiling for numeric values in a save — 100,000× real usage (billions),
    /// so normal progress is never cut while sums/differences stay inside i64.
    pub const MAX_TOKEN_VALUE: i64 = 1_000_000_000_000_000;

    /// Export file name — dated so repeated exports never overwrite.
    pub fn suggested_file_name(date: DateTime<Utc>) -> String {
        format!("PokeTokenBar-Save-{}.json", day_stamp(date))
    }

    /// Backup file name — a new slot per import. Keeping one would let the
    /// second import overwrite the **original**, removing the very thing you
    /// want to undo with "I imported the wrong save".
    pub fn backup_file_name(date: DateTime<Utc>) -> String {
        format!("companion-state.pre-import-{}.json", second_stamp(date))
    }
    pub const BACKUP_FILE_PREFIX: &'static str = "companion-state.pre-import-";
    /// Backups to keep — oldest deleted first.
    pub const BACKUPS_TO_KEEP: usize = 5;

    pub fn encode(
        state: CompanionState,
        app_version: &str,
        device_name: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<u8>, serde_json::Error> {
        let envelope = SaveEnvelope {
            format: SaveEnvelope::FORMAT_ID.to_string(),
            schema: SaveEnvelope::SCHEMA_VERSION,
            app_version: app_version.to_string(),
            exported_at: now,
            source_device: device_name.to_string(),
            state,
        };
        // Pretty-printed so a human can read what is being moved (a few KB —
        // size is irrelevant).
        serde_json::to_vec_pretty(&envelope)
    }

    pub fn decode(bytes: &[u8]) -> Result<SaveEnvelope, SaveTransferError> {
        if bytes.len() > Self::MAX_FILE_BYTES {
            return Err(SaveTransferError::FileTooLarge {
                bytes: bytes.len(),
                limit: Self::MAX_FILE_BYTES,
            });
        }
        // Read the header first — even if the body is a newer schema and
        // unreadable, this reports "newer save" precisely.
        let header: SaveHeader =
            serde_json::from_slice(bytes).map_err(|_| SaveTransferError::NotASaveFile)?;
        if header.format != SaveEnvelope::FORMAT_ID {
            return Err(SaveTransferError::NotASaveFile);
        }
        if header.schema > SaveEnvelope::SCHEMA_VERSION {
            return Err(SaveTransferError::NewerSchema {
                found: header.schema,
                supported: SaveEnvelope::SCHEMA_VERSION,
            });
        }
        // Same schema but unreadable = corrupt.
        let mut envelope: SaveEnvelope =
            serde_json::from_slice(bytes).map_err(|_| SaveTransferError::NotASaveFile)?;
        envelope.state = Self::sanitized(envelope.state);
        Ok(envelope)
    }

    /// Trust-boundary value normalization — a save comes from **outside** the
    /// app (hand-edited, corrupted in transit, another build).
    ///
    /// `CompanionState`'s decoding is deliberately lenient (a broken field
    /// must not wipe the dex), so absurd values pass through. Stored as-is they
    /// would kill the process on later arithmetic (Swift overflow trap), and
    /// restarting reads the same file and dies again — the user can't use the
    /// app until they delete the file by hand (`.corrupt` auto-recovery never
    /// fires because the decode *succeeds*).
    ///
    /// Guarding every downstream arithmetic point would re-break each time a
    /// new one appears, so normalize at the single point values enter. Only
    /// fields actually used in arithmetic are touched — dex/inventory items
    /// are not clamped (that would be data loss).
    pub fn sanitized(state: CompanionState) -> CompanionState {
        fn clamp_token(v: i64) -> i64 {
            v.clamp(0, SaveTransfer::MAX_TOKEN_VALUE)
        }
        let mut s = state;
        s.used_since_install = clamp_token(s.used_since_install);
        s.spent_tokens = clamp_token(s.spent_tokens);
        s.egg_usage = clamp_token(s.egg_usage);
        if let Some(map) = s.claimed_today_tokens_by_provider.take() {
            s.claimed_today_tokens_by_provider =
                Some(map.into_iter().map(|(k, v)| (k, clamp_token(v))).collect());
        }
        // An egg guarantee only belongs to "the egg you are holding", so it
        // can't coexist with an active mon. Hand-edited/legacy combos with both
        // would leak the guarantee into the next egg → permanent premium, so
        // drop it here. The pre-rolled species bought with that guarantee
        // (pendingHatchID) is dropped too — clearing only the guarantee would
        // let the **free** post-graduation egg hatch from that pre-roll, an
        // un-purchased premium result.
        if s.active.is_some() {
            s.egg_tier = None;
            s.pending_hatch_id = None;
        }
        // An unsatisfiable guarantee bricks the egg permanently — legendary
        // can't be expressed via capture_rate (ceiling nil), so both roll
        // paths produce zero candidates, no hatch means the guarantee is never
        // consumed, and the `hasActive` gate blocks the new-egg escape. Lenient
        // decoding only filters unknown raw values; a *known-but-unsatisfiable*
        // value still passes through unless normalized here.
        if let Some(tier) = s.egg_tier {
            if tier.capture_rate_ceiling().is_none() {
                s.egg_tier = None;
            }
        }
        if let Some(active) = s.active.as_mut() {
            active.used_at_stage = clamp_token(active.used_at_stage);
            // totalForms feeds `kk * (kk + 1)` (PokemonBalance.phaseThreshold),
            // where a huge value is itself a trap.
            active.total_forms = active.total_forms.clamp(1, 12);
            active.stage_index = active
                .stage_index
                .clamp(0, (active.path_ids.len() as i64).saturating_sub(1));
        }
        s
    }

    /// Rearranges a state from another device to **this device's** baseline.
    ///
    /// `CompanionState` fields fall into three classes from the import view:
    ///  - **Progress**: true on any device (`usedSinceInstall`, `dex`,
    ///    `inventory`, `active`, `eggUsage`, `eggTier`…) → kept as-is. An egg
    ///    guarantee is a purchased good, not this device's ledger, so it
    ///    travels across devices.
    ///  - **Local ledger**: how far *that device* accrued
    ///    (`claimedTodayTokensByProvider`, `lastDate`, `installBaselineSet`) →
    ///    re-anchored to the new device. Imported as-is, the old device's
    ///    today-total would keep `CompanionStore.update`'s per-provider
    ///    incremental gate false all day, silently dropping the new device's
    ///    usage (it heals at midnight, so it looks fine).
    ///  - **Device preference**: how this device views things (`language`) →
    ///    keep the current device's value. A Japanese-Mac save must not change
    ///    the English Mac's UI language.
    ///
    /// The account-global candy ledger (`candyGrantTier`) is not a replacement
    /// but a **per-key max merge**: limit-window keys are account-wide, so both
    /// devices see the same window — replacing wholesale with an older save
    /// would lose already-granted windows and re-grant candy there.
    pub fn rebased_for_this_device(
        imported: CompanionState,
        current: CompanionState,
        today_tokens_by_provider: HashMap<String, i64>,
        today_date: String,
        has_usage_data: bool,
    ) -> CompanionState {
        let mut state = imported;
        state.language = current.language;
        state.candy_grant_tier =
            Self::merged_grant_tier(state.candy_grant_tier, current.candy_grant_tier);
        state.candy_feature_seeded = state.candy_feature_seeded || current.candy_feature_seeded;
        let has_current_provider_data = has_usage_data && !today_tokens_by_provider.is_empty();
        if has_current_provider_data {
            // Same rule as a fresh install: usage before the import is not
            // credited retroactively.
            state.install_baseline_set = true;
            state.claimed_today_tokens_by_provider = Some(today_tokens_by_provider);
            state.last_date = today_date;
        } else {
            // This device's today usage is still unknown (pre-parse, no
            // provider, or only stale snapshots). `hasUsageData` only says a
            // snapshot exists, not that today's date does — storing an empty
            // map as a seeded ledger would treat the first real snapshot as a
            // "new provider" and silently drop a whole day. Defer to the
            // fresh-install path instead.
            state.install_baseline_set = false;
            state.claimed_today_tokens_by_provider = None;
            state.last_date = String::new();
        }
        state
    }

    /// Per window key, keeps the higher tier — either side having granted it
    /// counts as granted.
    pub fn merged_grant_tier(
        a: HashMap<String, i64>,
        b: HashMap<String, i64>,
    ) -> HashMap<String, i64> {
        let mut merged = a;
        for (k, v) in b {
            let entry = merged.entry(k).or_insert(0);
            *entry = (*entry).max(v);
        }
        merged
    }
}

fn day_stamp(date: DateTime<Utc>) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn second_stamp(date: DateTime<Utc>) -> String {
    date.format("%Y-%m-%d-%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    // The tests deliberately mirror the Swift style: construct the default
    // state then mutate the fields one at a time.
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::domain::companion::{AppLanguage, DexEntry, MonState, Rarity};
    use std::collections::HashSet;
    use uuid::Uuid;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn old_mac_state(today: &str) -> CompanionState {
        let mut s = CompanionState::default();
        s.install_baseline_set = true;
        s.used_since_install = 8_000_000_000;
        s.spent_tokens = 3_500_000_000;
        s.claimed_today_tokens_by_provider =
            Some(HashMap::from([("test".to_string(), 56_800_000)]));
        s.last_date = today.to_string();
        s.inventory = HashMap::from([("rareCandy".to_string(), 2)]);
        s.collected_finals = HashSet::from(["1-3".to_string()]);
        s.dex = vec![DexEntry::new(
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
        s
    }

    // MARK: Envelope

    #[test]
    fn round_trip_preserves_progress() {
        let original = old_mac_state("2026-08-03");
        let data = SaveTransfer::encode(original.clone(), "2.5.0", "Old Mac", now()).unwrap();
        let envelope = SaveTransfer::decode(&data).unwrap();

        assert_eq!(envelope.format, SaveEnvelope::FORMAT_ID);
        assert_eq!(envelope.source_device, "Old Mac");
        assert_eq!(
            envelope.state.used_since_install,
            original.used_since_install
        );
        assert_eq!(envelope.state.spent_tokens, original.spent_tokens);
        assert_eq!(envelope.state.inventory, original.inventory);
        assert_eq!(envelope.state.dex.len(), 1);
        assert_eq!(envelope.state.collected_finals, original.collected_finals);
    }

    /// [core] Without the envelope, CompanionState's lenient decoding would
    /// absorb any JSON as an empty state — "import succeeded → dex gone".
    /// The format id gates that first.
    #[test]
    fn foreign_json_is_rejected_rather_than_imported_as_empty_state() {
        // A shape that lenient decoding would swallow wholesale.
        let foreign = br#"{"some":"other app","dex":123}"#;
        assert_eq!(
            SaveTransfer::decode(foreign),
            Err(SaveTransferError::NotASaveFile)
        );

        // A state-only (envelope-less) legacy file is not a save either.
        let bare = serde_json::to_vec(&old_mac_state("2026-08-03")).unwrap();
        assert_eq!(
            SaveTransfer::decode(&bare),
            Err(SaveTransferError::NotASaveFile)
        );
    }

    /// [regression] `decode` is an `A || B` gate (decode failure **or** format
    /// mismatch). Both legacy cases fail via A (missing required key), so the
    /// whole `format` comparison could be deleted and the suite still passed.
    /// This pins B alone: a fully valid envelope whose format differs.
    #[test]
    fn valid_envelope_with_wrong_format_id_is_rejected() {
        let data =
            SaveTransfer::encode(old_mac_state("2026-08-03"), "2.5.0", "Other App", now()).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(
            json["format"].as_str(),
            Some(SaveEnvelope::FORMAT_ID),
            "premise: original has the valid format"
        );
        json["format"] = serde_json::Value::String("someotherapp.save".to_string());
        let patched = serde_json::to_vec(&json).unwrap();

        // The structure itself must decode — only the B branch (format value
        // comparison) is exercised.
        let envelope: SaveEnvelope = serde_json::from_slice(&patched).unwrap();
        assert_ne!(
            envelope.format,
            SaveEnvelope::FORMAT_ID,
            "premise: still an envelope"
        );

        assert_eq!(
            SaveTransfer::decode(&patched),
            Err(SaveTransferError::NotASaveFile)
        );
    }

    #[test]
    fn newer_schema_is_rejected() {
        let data =
            SaveTransfer::encode(CompanionState::default(), "2.5.0", "Future Mac", now()).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
        json["schema"] = serde_json::Value::from(SaveEnvelope::SCHEMA_VERSION + 1);
        let patched = serde_json::to_vec(&json).unwrap();

        assert_eq!(
            SaveTransfer::decode(&patched),
            Err(SaveTransferError::NewerSchema {
                found: SaveEnvelope::SCHEMA_VERSION + 1,
                supported: SaveEnvelope::SCHEMA_VERSION,
            })
        );
    }

    /// A newer-schema save may have a totally different body that fails the
    /// full decode — still report "update your app" via the header.
    #[test]
    fn newer_schema_is_reported_even_when_the_body_is_unreadable() {
        let json = br#"{"format":"poketokenbar.save","schema":99,"whatever":{"unknown":true}}"#;
        assert_eq!(
            SaveTransfer::decode(json),
            Err(SaveTransferError::NewerSchema {
                found: 99,
                supported: SaveEnvelope::SCHEMA_VERSION
            })
        );
    }

    #[test]
    fn oversized_file_is_rejected_before_parsing() {
        let huge = vec![0u8; SaveTransfer::MAX_FILE_BYTES + 1];
        assert_eq!(
            SaveTransfer::decode(&huge),
            Err(SaveTransferError::FileTooLarge {
                bytes: SaveTransfer::MAX_FILE_BYTES + 1,
                limit: SaveTransfer::MAX_FILE_BYTES,
            })
        );
    }

    // MARK: Device rebase

    /// [regression] Tokens used on the new Mac earlier that day used to be
    /// silently lost: the imported `claimedTodayTokensByProvider` (56.8M) keeps
    /// the update's `todayTokens > claimedTodayTokens` gate false all day.
    #[test]
    fn transfer_day_tokens_still_count_after_rebase() {
        let today = "2026-08-03";
        let imported = old_mac_state(today);
        let new_mac_today_so_far = 5_000_000;

        let rebased = SaveTransfer::rebased_for_this_device(
            imported,
            CompanionState::default(),
            HashMap::from([("test".to_string(), new_mac_today_so_far)]),
            today.to_string(),
            true,
        );

        assert_eq!(
            rebased.claimed_today_tokens_by_provider.as_ref().unwrap()["test"],
            new_mac_today_so_far
        );
        assert_eq!(rebased.last_date, today);
        assert!(rebased.install_baseline_set);
    }

    #[test]
    fn import_without_usage_data_defers_baseline_instead_of_crediting_whole_day() {
        let today = "2026-08-03";
        let rebased = SaveTransfer::rebased_for_this_device(
            old_mac_state(today),
            CompanionState::default(),
            HashMap::from([("test".to_string(), 0)]),
            today.to_string(),
            false,
        );
        assert!(!rebased.install_baseline_set);
    }

    /// [regression] Only stale snapshots (hasUsageData=true but empty today
    /// map) — seeding an empty provider ledger as a valid baseline would treat
    /// the first real snapshot as a "new provider" and lose a whole day.
    #[test]
    fn import_with_stale_only_usage_defers_baseline_instead_of_seeding_empty_ledger() {
        let today = "2026-08-03";
        let rebased = SaveTransfer::rebased_for_this_device(
            old_mac_state(today),
            CompanionState::default(),
            HashMap::new(),
            today.to_string(),
            true,
        );
        assert!(!rebased.install_baseline_set);
        assert!(rebased.claimed_today_tokens_by_provider.is_none());
        assert_eq!(rebased.last_date, "");
    }

    #[test]
    fn import_keeps_progress_and_candy_grant_ledger() {
        let mut imported = old_mac_state("2026-08-03");
        // Limits are account-global, so the same window key is valid on the new
        // device — dropping the ledger would re-grant candy there.
        imported.candy_grant_tier =
            HashMap::from([("five_hour|2026-08-03T00:00:00Z".to_string(), 100)]);
        imported.candy_feature_seeded = true;

        let rebased = SaveTransfer::rebased_for_this_device(
            imported,
            CompanionState::default(),
            HashMap::from([("test".to_string(), 1)]),
            "2026-08-03".to_string(),
            true,
        );

        assert_eq!(rebased.used_since_install, 8_000_000_000);
        assert_eq!(rebased.spent_tokens, 3_500_000_000);
        assert_eq!(rebased.dex.len(), 1);
        assert_eq!(rebased.inventory.get("rareCandy"), Some(&2));
        assert_eq!(
            rebased
                .candy_grant_tier
                .get("five_hour|2026-08-03T00:00:00Z"),
            Some(&100)
        );
        assert!(rebased.candy_feature_seeded);
    }

    /// [regression] `language` is a device preference, not progress — a
    /// Japanese-Mac save must not silently change the English Mac's UI language.
    #[test]
    fn import_keeps_this_devices_language() {
        let mut imported = old_mac_state("2026-08-03");
        imported.language = AppLanguage::Ja;
        let mut mine = CompanionState::default();
        mine.language = AppLanguage::En;

        let rebased = SaveTransfer::rebased_for_this_device(
            imported,
            mine,
            HashMap::from([("test".to_string(), 1)]),
            "2026-08-03".to_string(),
            true,
        );

        assert_eq!(
            rebased.language,
            AppLanguage::En,
            "imported language must not override this device"
        );
        assert_eq!(rebased.dex.len(), 1, "progress comes in as-is");
    }

    /// The candy ledger is account-global — a wholesale replacement by an older
    /// save loses already-granted windows and re-grants there.
    #[test]
    fn candy_grant_ledger_merges_instead_of_being_replaced_by_an_older_save() {
        let mine = HashMap::from([
            ("five_hour|A".to_string(), 100),
            ("weekly|B".to_string(), 80),
        ]);
        let older = HashMap::from([
            ("weekly|B".to_string(), 50),
            ("five_hour|C".to_string(), 100),
        ]);
        let merged = SaveTransfer::merged_grant_tier(mine, older);
        assert_eq!(
            merged.get("five_hour|A"),
            Some(&100),
            "window already granted here must not vanish"
        );
        assert_eq!(
            merged.get("weekly|B"),
            Some(&80),
            "same window keeps the higher tier"
        );
        assert_eq!(
            merged.get("five_hour|C"),
            Some(&100),
            "the save's window comes in too"
        );
    }

    /// A guarantee is a purchased good, not a device ledger — it travels.
    #[test]
    fn guarantee_travels_with_save() {
        let mut imported = CompanionState::default();
        imported.egg_tier = Some(Rarity::Uncommon);
        imported.used_since_install = 1_000_000;
        let rebased = SaveTransfer::rebased_for_this_device(
            imported,
            CompanionState::default(),
            HashMap::from([("test".to_string(), 0)]),
            "d".to_string(),
            true,
        );
        assert_eq!(rebased.egg_tier, Some(Rarity::Uncommon));
    }

    // MARK: Trust-boundary normalization

    /// [regression] Stored as-is, extreme values kill the process on later
    /// arithmetic; restart reads the same file and dies again.
    #[test]
    fn extreme_values_are_clamped_at_the_trust_boundary() {
        let mut evil = CompanionState::default();
        evil.install_baseline_set = true;
        evil.used_since_install = i64::MAX;
        evil.spent_tokens = i64::MIN;
        evil.egg_usage = i64::MAX;
        evil.claimed_today_tokens_by_provider = Some(HashMap::from([("test".to_string(), -42)]));
        evil.active = Some(MonState::new(
            1,
            vec![1],
            Some(vec![1]),
            i64::MAX,
            i64::MAX,
            Rarity::Common,
            i64::MAX,
            false,
            None,
            None,
            false,
        ));

        let data = SaveTransfer::encode(evil, "2.5.0", "Corrupt", now()).unwrap();
        let envelope = SaveTransfer::decode(&data).unwrap();
        let s = envelope.state;

        assert_eq!(s.used_since_install, SaveTransfer::MAX_TOKEN_VALUE);
        assert_eq!(s.spent_tokens, 0, "negatives clamp to 0");
        assert_eq!(s.egg_usage, SaveTransfer::MAX_TOKEN_VALUE);
        assert_eq!(
            s.claimed_today_tokens_by_provider.as_ref().unwrap()["test"],
            0
        );
        let active = s.active.as_ref().unwrap();
        assert_eq!(active.used_at_stage, SaveTransfer::MAX_TOKEN_VALUE);
        assert_eq!(active.total_forms, 12);
        assert_eq!(active.stage_index, 0, "must not exceed pathIDs bounds");
    }

    /// Values already on disk are clamped on load, not just on import — a
    /// corrupt state file reads the same value every startup.
    #[test]
    fn corrupt_state_on_disk_is_clamped_on_load_not_just_on_import() {
        let json = r#"{"installBaselineSet":true,"usedSinceInstall":9223372036854775807,
        "spentTokens":-9223372036854775808,"eggUsage":9223372036854775807,
        "claimedTodayTokens":-1,"lastDate":"2026-08-03"}"#;
        let state: CompanionState = serde_json::from_str(json).unwrap();
        let s = SaveTransfer::sanitized(state);

        assert_eq!(s.used_since_install, SaveTransfer::MAX_TOKEN_VALUE);
        assert_eq!(s.spent_tokens, 0);
        assert_eq!(s.egg_usage, SaveTransfer::MAX_TOKEN_VALUE);
        // legacy aggregate field is not guessed into a provider ledger.
        assert!(s.claimed_today_tokens_by_provider.is_none());
    }

    /// An active mon and an egg guarantee can't coexist (there is no egg). Both
    /// must be dropped when hand-edited/legacy combos arrive — the pre-roll too,
    /// otherwise the free post-graduation egg hatches an un-purchased premium.
    #[test]
    fn sanitized_drops_guarantee_and_its_pre_roll_when_active_exists() {
        let mut s = CompanionState::default();
        s.egg_tier = Some(Rarity::Rare);
        s.pending_hatch_id = Some(3);
        s.active = Some(MonState::new(
            1,
            vec![1],
            None,
            0,
            0,
            Rarity::Common,
            1,
            false,
            None,
            None,
            false,
        ));
        let cleaned = SaveTransfer::sanitized(s);
        assert!(cleaned.egg_tier.is_none());
        assert!(
            cleaned.pending_hatch_id.is_none(),
            "the guarantee-bought pre-roll must not survive"
        );

        // Egg state (no active) keeps it.
        let mut egg = CompanionState::default();
        egg.egg_tier = Some(Rarity::Rare);
        egg.pending_hatch_id = Some(3);
        assert_eq!(
            SaveTransfer::sanitized(egg.clone()).egg_tier,
            Some(Rarity::Rare)
        );
        assert_eq!(SaveTransfer::sanitized(egg).pending_hatch_id, Some(3));
    }

    /// A legendary guarantee can't be satisfied (ceiling nil) — both roll paths
    /// produce zero candidates, so it must be normalized away or the egg is
    /// bricked forever.
    #[test]
    fn unsatisfiable_guarantee_is_normalized_away() {
        let mut s = CompanionState::default();
        s.egg_tier = Some(Rarity::Legendary);
        assert!(SaveTransfer::sanitized(s).egg_tier.is_none());
    }

    // MARK: Confirmation policy / names

    #[test]
    fn cancel_is_the_default_button_on_the_import_confirmation() {
        assert_eq!(
            ImportConfirmPolicy::key_equivalent(ImportConfirmPolicy::CANCEL_BUTTON_INDEX),
            "\r"
        );
        assert_eq!(
            ImportConfirmPolicy::key_equivalent(ImportConfirmPolicy::REPLACE_BUTTON_INDEX),
            "",
            "the replace (destructive) button must not be the default"
        );
    }

    #[test]
    fn suggested_file_name_carries_date() {
        // 2026-08-03 12:00 UTC — noon, so the date boundary doesn't shift
        // across timezones (file names use the local date).
        let date = DateTime::from_timestamp(1_785_758_400, 0).unwrap();
        let name = SaveTransfer::suggested_file_name(date);
        assert!(name.starts_with("PokeTokenBar-Save-"));
        assert!(name.ends_with(".json"));
        assert!(name.contains("2026-08-03"), "actual name: {name}");
    }

    #[test]
    fn backup_file_name_has_second_precision() {
        let name = SaveTransfer::backup_file_name(now());
        assert!(name.starts_with(SaveTransfer::BACKUP_FILE_PREFIX));
        assert!(name.ends_with(".json"));
        // Two imports one second apart must not collide (else the second
        // overwrites the original backup).
        let later = DateTime::from_timestamp(1_700_000_001, 0).unwrap();
        assert_ne!(
            SaveTransfer::backup_file_name(now()),
            SaveTransfer::backup_file_name(later)
        );
    }

    #[test]
    fn summary_reflects_state() {
        let state = old_mac_state("2026-08-03");
        let summary = SaveSummary::new(&state);
        assert_eq!(summary.dex_count, 1);
        assert_eq!(summary.lifetime_tokens, 8_000_000_000);
    }
}
