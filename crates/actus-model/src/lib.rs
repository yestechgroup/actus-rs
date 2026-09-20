//! ACTUS Data Standard vocabulary and contract terms, generated from the
//! vendored ACTUS dictionary.
//!
//! Layout:
//! - [`generated`] — vocabulary emitted by `cargo run -p actus-model --bin
//!   generate` from `vendor/actus/dictionary` (contract types, event types,
//!   attribute vocabulary, applicability matrices). Committed; regenerate and
//!   re-diff when the pinned dictionary changes.
//! - [`terms`] — hand-written [`terms::ContractTerms`] attribute bag for the
//!   fixed-income family, deserializing directly from testbed `terms`
//!   objects (camelCase keys, decimal/date/cycle coercion).
//! - [`builders`] — applicability-enforcing typed builders
//!   (`PamBuilder`, `LamBuilder`, `NamBuilder`, `AnnBuilder`).
//! - [`metadata`] — serde-serializable metadata views (contract type
//!   taxonomy, applicability tables, labelled attribute dictionary) for
//!   UI clients.
//! - [`cycle`], [`enums`], [`error`], [`serde_helpers`], [`source`] —
//!   supporting hand-written model pieces.
//!
//! Dictionary quirks handled here: curly quotes in
//! `actus-dictionary-terms.json` (see [`source::normalize_quotes`]), the
//! `ScalingEffect` zero/letter-`O` spelling split between dictionary and
//! testbeds, the duplicated `SCMP` business day convention token, and
//! `fixingDays` (in the testbeds, absent from dictionary v1.4).

pub mod builders;
pub mod cycle;
pub mod enums;
pub mod error;
pub mod generated;
pub mod metadata;
pub mod serde_helpers;
pub mod source;
pub mod terms;

pub use builders::{AnnBuilder, LamBuilder, NamBuilder, PamBuilder};
pub use cycle::{Cycle, CyclePeriod, CycleStub};
pub use enums::{
    BusinessDayConvention, Calendar, ContractPerformance, ContractReferenceRole,
    ContractReferenceType, ContractRole, CreditEventType, DayCountConvention, DeliverySettlement,
    EndOfMonthConvention, GuaranteedExposure, InterestCalculationBase, ScalingEffect,
};
pub use error::ModelError;
pub use generated::applicability::{self, Applicability};
pub use generated::attribute::{self, Attribute, AttributeType};
pub use generated::contract_type::{self, ContractType};
pub use generated::event_type::{self, EventType};
pub use metadata::{ApplicabilityInfo, AttributeInfo, ContractTypeInfo};
pub use terms::{ContractReference, ContractTerms};
