//! WASM bindings for the ACTUS engine: contract metadata, terms validation
//! and schedule evaluation for browser clients (issue #1 web visualizer).
//!
//! Architecture: a pure-Rust core in [`dto`] (natively testable) plus the
//! thin `#[wasm_bindgen]` functions below, which exchange **JSON strings**
//! (never `JsValue` structs). All wire objects use camelCase keys; decimal
//! values are strings (`"12.34"`); timestamps are `YYYY-MM-DDTHH:MM:SS`
//! strings.
//!
//! # JSON contract
//!
//! `contractTypes() -> string` — array of
//! [`ContractTypeInfo`](actus_model::ContractTypeInfo):
//!
//! ```json
//! [{"acronym": "PAM", "identifier": "principalAtMaturity",
//!   "name": "Principal at Maturity (Released)", "category": "Basic"}]
//! ```
//!
//! `applicability(contractType: string) -> string` — tables of one type;
//! unknown acronyms reject the call:
//!
//! ```json
//! {"required": ["contractRole", "statusDate", "..."],
//!  "baseRequired": ["contractRole", "..."],
//!  "applicable": ["accruedInterest", "..."]}
//! ```
//!
//! `applicabilityMatrix() -> string` — one entry per contract type:
//!
//! ```json
//! [{"acronym": "PAM", "required": ["..."], "baseRequired": ["..."],
//!   "applicable": ["..."]}]
//! ```
//!
//! `attributeMeta() -> string` — the labelled attribute dictionary:
//!
//! ```json
//! [{"acronym": "NT", "name": "Notional Principal",
//!   "description": "Current nominal value ...", "dataType": "Real"}]
//! ```
//!
//! `validate(termsJson: string) -> string` — applicability validation with
//! builder-aligned semantics. Errors: `AttributeNotApplicable`,
//! `MissingAttribute` (base-required and absent), `UnknownAttribute`.
//! Per-attribute status: `required-set`, `required-missing`, `optional-set`,
//! `optional-unset`. `contractType` itself is always allowed.
//!
//! ```json
//! {"valid": false,
//!  "errors": [{"code": "MissingAttribute", "attribute": "notionalPrincipal"}],
//!  "termStatus": {"notionalPrincipal": "required-missing",
//!                 "accruedInterest": "optional-unset"}}
//! ```
//!
//! `evaluate(termsJson: string) -> string` — full event schedule with an
//! empty risk factor environment (scheduled events only). Errors reject the
//! promise / throw with a plain message string.
//!
//! ```json
//! {"events": [{"eventDate": "2026-01-01T00:00:00", "eventType": "IED",
//!              "payoff": "-1000", "currency": "USD",
//!              "notionalPrincipal": "1000", "nominalInterestRate": "0.05",
//!              "accruedInterest": "0"}],
//!  "contractStatus": "active|matured|terminated"}
//! ```
//!
//! `evaluateWithRisk(termsJson: string, scenarioJson: string) -> string` —
//! the same evaluation under a risk factor scenario (camelCase keys):
//!
//! ```json
//! {"rates": {"<marketObjectCode>": {"<timestamp>": 0.02}},
//!  "fxRates": {"<CUR2/CUR>": {"<timestamp>": 1.1}},
//!  "unitPrices": {"<marketObjectCode>": {"<timestamp>": 105}},
//!  "observedEvents": [{"eventType": "PP", "time": "...", "payoff": "1000"}],
//!  "creditEvents": [{"time": "...", "performance": "DF"}]}
//! ```
//!
//! Series lookups are step functions: the latest observation at or before
//! the requested time applies.

pub mod dto;
pub mod specs;

use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// Metadata of every released/implemented contract type as a JSON array.
#[must_use]
#[wasm_bindgen]
pub fn contract_types() -> String {
    serde_json::to_string(&actus_model::metadata::contract_types())
        .unwrap_or_else(|_| "[]".to_string())
}

/// Applicability tables of one contract type (case-insensitive acronym) as a
/// JSON object; unknown acronyms throw.
///
/// # Errors
/// The acronym is not a known contract type.
#[wasm_bindgen]
pub fn applicability(contract_type: String) -> Result<String, JsValue> {
    dto::applicability_json(&contract_type).map_err(|e| JsValue::from_str(&e))
}

/// The full applicability matrix (one entry per contract type) as a JSON
/// array.
#[must_use]
#[wasm_bindgen]
pub fn applicability_matrix() -> String {
    serde_json::to_string(&dto::applicability_matrix()).unwrap_or_else(|_| "[]".to_string())
}

/// The labelled attribute dictionary as a JSON array.
#[must_use]
#[wasm_bindgen]
pub fn attribute_meta() -> String {
    serde_json::to_string(&actus_model::metadata::attribute_meta())
        .unwrap_or_else(|_| "[]".to_string())
}

/// Validates a terms object (JSON string) against the applicability tables
/// of its contract type; returns the report as a JSON string.
///
/// # Errors
/// `termsJson` is not a JSON object or does not carry a parseable
/// `contractType`.
#[wasm_bindgen]
pub fn validate(terms_json: String) -> Result<String, JsValue> {
    dto::validate_json(&terms_json).map_err(|e| JsValue::from_str(&e))
}

/// Evaluates the full event schedule of a terms object (JSON string) and
/// returns the events plus final contract status as a JSON string.
///
/// # Errors
/// `termsJson` does not parse into contract terms, or the engine rejects the
/// evaluation (unsupported type, missing attribute, ...).
#[wasm_bindgen]
pub fn evaluate(terms_json: String) -> Result<String, JsValue> {
    dto::evaluate_json(&terms_json).map_err(|e| JsValue::from_str(&e))
}

/// Evaluates the full event schedule of a terms object (JSON string) under a
/// risk factor scenario (JSON string; see the crate documentation for the
/// wire shape) and returns the events plus final contract status.
///
/// Series lookups are step functions: the latest observation at or before
/// the requested time applies.
///
/// # Errors
/// `termsJson` or `scenarioJson` does not parse, or the engine rejects the
/// evaluation (unsupported type, missing attribute, missing observation,
/// ...).
#[wasm_bindgen]
pub fn evaluate_with_risk(terms_json: String, scenario_json: String) -> Result<String, JsValue> {
    dto::evaluate_json_with_risk(&terms_json, &scenario_json).map_err(|e| JsValue::from_str(&e))
}

/// The parameter form specification of one contract type (case-insensitive
/// acronym) as a JSON array; unknown acronyms throw.
///
/// Each entry: `{key, label, kind, value, step?, options?}` where `kind` is
/// `number` | `date` | `select` | `text` and `value` is the default.
///
/// # Errors
/// The acronym is not a known contract type.
#[wasm_bindgen]
pub fn contract_param_specs(contract_type: String) -> Result<String, JsValue> {
    let t: actus_model::ContractType = contract_type
        .parse()
        .map_err(|e: actus_model::ModelError| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&specs::param_specs(t))
        .map_err(|e| JsValue::from_str(&format!("serialization failed: {e}")))
}

/// Builds the contract terms JSON (string) for one contract type from form
/// values (JSON object `{key: value}`, empty values unset). Virtual
/// composed-contract keys are expanded into `contractStructure` entries.
///
/// # Errors
/// The acronym is not a known contract type.
#[wasm_bindgen]
pub fn build_terms_json(contract_type: String, values_json: String) -> Result<String, JsValue> {
    let t: actus_model::ContractType = contract_type
        .parse()
        .map_err(|e: actus_model::ModelError| JsValue::from_str(&e.to_string()))?;
    let values: BTreeMap<String, String> = serde_json::from_str(&values_json)
        .map_err(|e| JsValue::from_str(&format!("invalid values JSON: {e}")))?;
    serde_json::to_string(&specs::terms_json(t, &values))
        .map_err(|e| JsValue::from_str(&format!("serialization failed: {e}")))
}
