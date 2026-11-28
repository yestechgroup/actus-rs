//! Conformance harness running the official ACTUS testbeds
//! (`vendor/actus/tests`) against the Rust engine.
//!
//! Layout:
//!
//! - [`cases`]: testbed file loading and the risk factor provider over
//!   observed `dataObserved` series.
//! - [`comparator`]: tolerance comparison of evaluated events against the
//!   expected results.
//! - [`corpus`]: the deterministic randomized contract corpus for the
//!   differential layer (D1).
//! - [`oracle`]: the independent in-Rust fixed-income oracle (PAM, LAM,
//!   NAM, ANN) with its own fixed point numerical regime (D3).
//! - `engine_registry`: the engine registry used by the conformance runs,
//!   with one registered implementation per supported contract type.
//!
//! The integration test `tests/conformance.rs` gates all registered
//! types against their official testbeds; the integration test
//! `tests/differential.rs` gates the randomized fixed-income corpus against
//! the oracle (512 cases per type, 2048 in total). The `report` binary
//! prints the per-case report for any registered type.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
pub mod cases;
pub mod comparator;
pub mod corpus;
pub mod oracle;

use actus_engine::EngineRegistry;

/// The engine registry: every engine of `actus-engine`
/// ([`EngineRegistry::with_all_engines`]).
#[must_use]
pub fn engine_registry() -> EngineRegistry {
    EngineRegistry::with_all_engines()
}
