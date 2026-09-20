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

pub mod cases;
pub mod comparator;
pub mod corpus;
pub mod oracle;

use actus_engine::ann::AnnEngine;
use actus_engine::capfl::CapflEngine;
use actus_engine::cec::CecEngine;
use actus_engine::ceg::CegEngine;
use actus_engine::clm::ClmEngine;
use actus_engine::com::ComEngine;
use actus_engine::csh::CshEngine;
use actus_engine::futur::FuturEngine;
use actus_engine::fxout::FxoutEngine;
use actus_engine::lam::LamEngine;
use actus_engine::lax::LaxEngine;
use actus_engine::nam::NamEngine;
use actus_engine::optns::OptnsEngine;
use actus_engine::pam::PamEngine;
use actus_engine::stk::StkEngine;
use actus_engine::swaps::SwapsEngine;
use actus_engine::swppv::SwppvEngine;
use actus_engine::ump::UmpEngine;
use actus_engine::EngineRegistry;

/// Builds the conformance engine registry.
///
/// Each supported contract type registers its implementation here; future
/// contract waves add their registrations without touching the engine
/// crate's dispatch seam.
pub fn engine_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(LaxEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(CshEngine));
    registry.register(Box::new(ClmEngine));
    registry.register(Box::new(UmpEngine));
    registry.register(Box::new(StkEngine));
    registry.register(Box::new(ComEngine));
    registry.register(Box::new(FxoutEngine));
    registry.register(Box::new(SwppvEngine));
    registry.register(Box::new(SwapsEngine));
    registry.register(Box::new(CapflEngine));
    registry.register(Box::new(OptnsEngine));
    registry.register(Box::new(FuturEngine));
    registry.register(Box::new(CegEngine));
    registry.register(Box::new(CecEngine));
    registry
}
