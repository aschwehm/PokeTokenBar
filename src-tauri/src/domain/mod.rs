//! Pure data types and calculations — no UI, no OS integration, no I/O.
//!
//! Mirrors the original `Core/Models.swift`, `Core/ModelPricing.swift`,
//! `Core/TokenFormatter.swift`, `Core/CompanionModel.swift`, and
//! `Core/SaveTransfer.swift`.
//!
//! Everything here must compile and be testable on Linux, Windows, and macOS.

pub mod companion;
pub mod decoding;
pub mod format;
pub mod models;
pub mod pricing;
pub mod save;

pub use companion::*;
pub use decoding::*;
pub use format::*;
pub use models::*;
pub use pricing::*;
pub use save::*;
