#![deny(unsafe_code)]

//! Cleanup rule schema and built-in rule packs.
//!
//! This crate is intentionally a placeholder in phase 0. The scanner and CLI do
//! not depend on it yet. Phase 2 will add TOML rule packs, risk levels, and
//! recommendation generation here.

/// Version of the rule schema understood by this build.
pub const RULES_SCHEMA_VERSION: u32 = 1;
