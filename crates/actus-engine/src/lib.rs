//! ACTUS algorithmic engine: schedule generation, state variables, event
//! sequencing and the dispatch seam for contract type implementations.
//!
//! This crate is the pure-finance core of the ACTUS execution engine: all
//! functions derive their results from their inputs alone. There is no I/O,
//! no clock access and no randomness. Numeric quantities use
//! `rust_decimal::Decimal` and timestamps use `chrono::NaiveDateTime`.
//!
//! Module map:
//!
//! - [`daycount`]: day count fractions (techspec "Year Fraction Convention").
//! - [`schedule`]: cycle unrolling with EOM and business day conventions,
//!   array schedules over segmented anchors/cycles (techspec "Schedule",
//!   paper §3.2 "Array Schedule").
//! - [`state`]: state variables and the pre-IED initial state (techspec
//!   "State Variables", states dictionary).
//! - [`event`]: contract events and the same-timestamp ordering rule
//!   (techspec "Event Sequence").
//! - [`risk`]: risk factor observation (techspec "Risk Factor Observer").
//! - [`engine`]: contract type dispatch ([`ContractEngine`],
//!   [`EngineRegistry`], [`evaluate`]).
//! - `num` (crate-private): checked decimal arithmetic; overflow and
//!   division by zero become [`EngineError::Arithmetic`].
//!
//! Contract type modules: [`pam`], [`lam`], [`lax`], [`nam`], [`ann`],
//! [`clm`], [`ump`], [`csh`], [`stk`], [`com`], [`fxout`], [`swppv`],
//! [`swaps`], [`capfl`], [`optns`], [`futur`], [`ceg`], [`cec`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Every Decimal and integer operation in library code must be checked:
// untrusted terms must produce an EngineError, never a panic (see `num`).
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]
pub mod ann;
pub mod capfl;
pub mod cec;
pub mod ceg;
pub mod clm;
pub mod com;
pub mod common;
pub(crate) mod credit_enhancement;
pub mod csh;
pub mod daycount;
pub mod engine;
pub mod event;
pub(crate) mod fixed_income;
pub mod futur;
pub mod fxout;
pub mod lam;
pub mod lax;
pub mod nam;
pub(crate) mod num;
pub mod optns;
pub mod pam;
pub mod risk;
pub mod schedule;
pub mod state;
pub mod stk;
pub mod swaps;
pub mod swppv;
pub mod ump;

pub use engine::{evaluate, ContractEngine, EngineRegistry};
pub use event::{sort_events, ContractEvent};
pub use num::ArithmeticFailure;
pub use risk::{ObservedCreditEvent, ObservedEvent, RiskFactorProvider, StateProvider};
pub use state::{ContractState, ContractStatus};

/// Errors raised by the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The day count fraction was requested for an inverted date range.
    #[error("day count fraction undefined: end {end} precedes start {start}")]
    InvalidDayCountRange {
        /// Normalised range start.
        start: String,
        /// Normalised range end.
        end: String,
    },
    /// No contract engine is registered for the terms' contract type.
    #[error("no engine registered for contract type {0:?}")]
    UnsupportedContractType(actus_model::ContractType),
    /// A required contract attribute is missing.
    #[error("contract terms missing required attribute: {0}")]
    MissingAttribute(&'static str),
    /// A risk factor market object was not observed at the requested time.
    #[error("risk factor market object {code} not observed at {at}")]
    RiskFactorMissing {
        /// Market object code of the missing risk factor.
        code: String,
        /// Observation time requested.
        at: String,
    },
    /// The terms describe a schedule that cannot be generated: a cycle
    /// increment outside the representable calendar, more schedule elements
    /// than the engine's safety limit, or an undefined cycle stub indicator.
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),
    /// An event sequence produced an invalid state transition.
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    /// A payoff or state computation left the representable `Decimal` range
    /// (about ±7.9e28) or divided by zero. Every operation is checked
    /// (see the crate's `num` module), so extreme terms produce this error
    /// instead of a panic.
    #[error("arithmetic failure in contract evaluation: {0}")]
    Arithmetic(ArithmeticFailure),
}
