//! The companion state machine and aggregation.
//!
//! Mirrors the original `Core/CompanionStore.swift` and the generic parts of
//! `Core/UsageStore.swift` (today/week/month totals, burn tier, forecast,
//! alert evaluation). Includes the egg → hatch → evolve → graduate lifecycle,
//! shiny/nature rolls, Pokédex + catch log, and the Shop/Bag economy.
//!
//! Aggregation is provider-agnostic by design; provider-specific behavior lives
//! in the `providers/` module.

pub mod store;

pub use store::*;
