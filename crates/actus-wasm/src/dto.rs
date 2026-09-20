//! Pure-Rust core of the WASM bindings: DTOs and functions over
//! [`actus_engine`] and [`actus_model`], natively testable. The
//! `#[wasm_bindgen]` layer in [`crate`] only marshals JSON strings to and
//! from these functions.
//!
//! All wire types serialize with camelCase keys; decimal values are
//! serialized as strings (exact `Decimal` rendering, e.g. `"12.34"`), event
//! timestamps as `YYYY-MM-DDTHH:MM:SS` strings.

use std::collections::BTreeMap;
use std::str::FromStr;

use actus_engine::{
    ContractEvent, ContractStatus, EngineError, EngineRegistry, ObservedCreditEvent, ObservedEvent,
    RiskFactorProvider,
};
use actus_model::generated::applicability;
use actus_model::generated::attribute;
use actus_model::metadata::{self, ApplicabilityInfo};
use actus_model::{ContractPerformance, ContractTerms, ContractType, EventType};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Wire timestamp format of event dates.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// Dictionary gap carried by the testbeds but absent from dictionary v1.4
/// (see the `actus-model` crate documentation); treated as a known attribute
/// by validation.
const KNOWN_IDENTIFIER_GAP: &str = "fixingDays";

/// One contract event with payoff and post-event state, mirroring the
/// vendored testbed result objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    /// Event time, e.g. `2026-01-01T00:00:00`.
    pub event_date: String,
    /// Event type acronym, e.g. `IP`.
    pub event_type: String,
    /// Payoff in the contract currency, role-signed.
    pub payoff: String,
    /// Settlement currency, `null` when unknown.
    pub currency: Option<String>,
    /// Post-event notional principal `NT`.
    pub notional_principal: String,
    /// Post-event nominal interest rate `IPNR`.
    pub nominal_interest_rate: String,
    /// Post-event accrued interest `IPAC`.
    pub accrued_interest: String,
}

/// Result of evaluating one contract: the full event sequence plus the
/// lifetime status after the final event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationResult {
    /// Events in deterministic evaluation order.
    pub events: Vec<EventDto>,
    /// `active` | `matured` | `terminated`, from the final event state.
    pub contract_status: String,
}

/// One applicability violation or unknown attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    /// `AttributeNotApplicable` | `MissingAttribute` | `UnknownAttribute`.
    pub code: String,
    /// Dictionary identifier of the attribute, e.g. `notionalPrincipal`.
    pub attribute: String,
}

/// Result of validating a terms object against the applicability tables of
/// its contract type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    /// `true` when [`ValidationReport::errors`] is empty.
    pub valid: bool,
    /// Violations in deterministic order.
    pub errors: Vec<ValidationError>,
    /// Per-attribute status for every applicable-or-required attribute:
    /// `required-set` | `required-missing` | `optional-set` |
    /// `optional-unset`.
    pub term_status: BTreeMap<String, String>,
}

/// One entry of the full applicability matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicabilityMatrixEntry {
    /// Contract type acronym, e.g. `PAM`.
    pub acronym: String,
    /// The applicability tables of the type.
    #[serde(flatten)]
    pub tables: ApplicabilityInfo,
}

/// A decimal carried by scenario JSON: the testbeds and web clients send
/// numbers, the DTO convention carries strings; both are accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct WireDecimal(pub Decimal);

impl Serialize for WireDecimal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for WireDecimal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let decimal = match value {
            serde_json::Value::Number(n) => Decimal::try_from(n.as_f64().unwrap_or(f64::NAN))
                .map_err(|_| {
                    serde::de::Error::custom(format!("number {n} is not representable as decimal"))
                })?,
            serde_json::Value::String(s) => s
                .parse::<Decimal>()
                .map_err(|e| serde::de::Error::custom(format!("invalid decimal {s:?}: {e}")))?,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "expected number or decimal string, got {other}"
                )))
            }
        };
        Ok(WireDecimal(decimal))
    }
}

/// One externally observed risk factor value at one point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireObservation {
    /// Observation time, `YYYY-MM-DDTHH:MM:SS` (a bare date is accepted).
    pub time: String,
    /// Observed value.
    pub value: WireDecimal,
}

/// A risk factor scenario carried alongside the contract terms
/// (`evaluateWithRisk`): market observations replayed through the engine's
/// risk factor provider while evaluating the schedule.
///
/// Series map market object codes (or `"CUR2/CUR"` pairs for FX) to
/// time/value observations; the engine reads the latest observation at or
/// before each requested time (step function).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    /// Interest rate series keyed by market object code
    /// (attribute `RRMO`/`IPMO` codes).
    #[serde(default)]
    pub rates: BTreeMap<String, BTreeMap<String, WireDecimal>>,
    /// FX rate series keyed by `"CUR2/CUR"` pairs (attribute `CUR2`/`CUR`).
    #[serde(default)]
    pub fx_rates: BTreeMap<String, BTreeMap<String, WireDecimal>>,
    /// Unit price series keyed by market object code
    /// (attribute `PMO`/`MOC` codes; stock and commodity valuations).
    #[serde(default)]
    pub unit_prices: BTreeMap<String, BTreeMap<String, WireDecimal>>,
    /// Externally observed events replayed into the evaluation
    /// environment (e.g. `PP` prepayments on `PAM`, analysis dates on
    /// `CSH`).
    #[serde(default)]
    pub observed_events: Vec<WireObservedEvent>,
    /// Externally observed credit events (`CE`) triggering the credit
    /// enhancement contract types (`CEC`).
    #[serde(default)]
    pub credit_events: Vec<WireCreditEvent>,
}

/// One externally observed event of a [`Scenario`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireObservedEvent {
    /// Observed event type acronym, e.g. `PP` or `AD`.
    #[serde(rename = "eventType")]
    pub event_type: String,
    /// Observation time, `YYYY-MM-DDTHH:MM:SS` (a bare date is accepted).
    pub time: String,
    /// Observed payoff (informational for the current engines; the
    /// provider interface carries the observation, engines consume the
    /// parts they need).
    #[serde(default)]
    pub payoff: Option<WireDecimal>,
}

/// One externally observed credit event of a [`Scenario`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireCreditEvent {
    /// Observation time, `YYYY-MM-DDTHH:MM:SS` (a bare date is accepted).
    pub time: String,
    /// Contract performance state the covered contract entered
    /// (`PRF`), e.g. `DF`.
    #[serde(default)]
    pub performance: Option<String>,
}

/// A [`RiskFactorProvider`] built from a [`Scenario`].
///
/// Mapping onto the engine's provider interface: `rates` drive
/// [`RiskFactorProvider::rate`], `unitPrices` drive
/// [`RiskFactorProvider::index`], and `fxRates` are readable through both
/// lookups (the provider interface has no dedicated FX accessor, and rate
/// and unit-price series take precedence on code collision).
/// Series lookups are step functions: the latest observation at or before
/// the requested time.
#[derive(Debug, Clone, Default)]
pub struct ScenarioProvider {
    rates: BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
    fx_rates: BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
    unit_prices: BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
    observed_events: Vec<ObservedEvent>,
    credit_events: Vec<ObservedCreditEvent>,
}

impl ScenarioProvider {
    /// The empty scenario: nothing observed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a wire [`Scenario`]; timestamp or acronym errors are
    /// reported with the offending key.
    pub fn from_wire(scenario: &Scenario) -> Result<Self, String> {
        let parse_series = |series: &BTreeMap<String, BTreeMap<String, WireDecimal>>,
                            what: &str| {
            series
                .iter()
                .map(|(code, points)| {
                    let parsed = points
                        .iter()
                        .map(|(time, value)| {
                            let time = parse_scenario_timestamp(time)
                                .map_err(|e| format!("{what} {code:?}: {e}"))?;
                            Ok((time, value.0))
                        })
                        .collect::<Result<BTreeMap<NaiveDateTime, Decimal>, String>>()?;
                    Ok((code.clone(), parsed))
                })
                .collect::<Result<BTreeMap<_, _>, String>>()
        };
        let rates = parse_series(&scenario.rates, "rates")?;
        let fx_rates = parse_series(&scenario.fx_rates, "fxRates")?;
        let unit_prices = parse_series(&scenario.unit_prices, "unitPrices")?;
        let mut observed_events = Vec::new();
        for event in &scenario.observed_events {
            let event_type = EventType::from_str(&event.event_type).map_err(|e| {
                format!(
                    "observedEvents: unknown event type {:?}: {e}",
                    event.event_type
                )
            })?;
            let time = parse_scenario_timestamp(&event.time)
                .map_err(|e| format!("observedEvents {}: {e}", event.event_type))?;
            observed_events.push(ObservedEvent { time, event_type });
        }
        let mut credit_events = Vec::new();
        for event in &scenario.credit_events {
            let time =
                parse_scenario_timestamp(&event.time).map_err(|e| format!("creditEvents: {e}"))?;
            let performance = event
                .performance
                .as_deref()
                .map(|token| {
                    ContractPerformance::from_str(token)
                        .map_err(|e| format!("creditEvents: unknown performance {token:?}: {e}"))
                })
                .transpose()?;
            credit_events.push(ObservedCreditEvent {
                time,
                contract_id: None,
                performance,
            });
        }
        Ok(Self {
            rates,
            fx_rates,
            unit_prices,
            observed_events,
            credit_events,
        })
    }

    /// Step-function lookup: the latest observation at or before `at`.
    fn observe(
        series: &BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
        market_object_code: &str,
        at: NaiveDateTime,
    ) -> Option<Decimal> {
        let series = series.get(market_object_code)?;
        series.range(..=at).next_back().map(|(_, value)| *value)
    }
}

impl RiskFactorProvider for ScenarioProvider {
    fn rate(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        Self::observe(&self.rates, market_object_code, at)
            .or_else(|| Self::observe(&self.fx_rates, market_object_code, at))
    }

    fn index(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        Self::observe(&self.unit_prices, market_object_code, at)
            .or_else(|| Self::observe(&self.fx_rates, market_object_code, at))
    }

    fn observed_events(&self) -> Vec<ObservedEvent> {
        self.observed_events.clone()
    }

    fn observed_credit_events(&self) -> Vec<ObservedCreditEvent> {
        self.credit_events.clone()
    }
}

/// Parses a scenario timestamp: `YYYY-MM-DDTHH:MM:SS`, or a bare
/// `YYYY-MM-DD` date (midnight).
fn parse_scenario_timestamp(raw: &str) -> Result<NaiveDateTime, String> {
    let raw = raw.trim();
    if let Ok(time) = NaiveDateTime::parse_from_str(raw, TIMESTAMP_FORMAT) {
        return Ok(time);
    }
    let date = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|e| format!("invalid timestamp {raw:?}: {e}"))?;
    Ok(date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
}

/// The engine registry of the WASM bindings: one registered implementation
/// per supported contract type (mirrors the conformance harness registry).
#[must_use]
pub fn engine_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(actus_engine::pam::PamEngine));
    registry.register(Box::new(actus_engine::lam::LamEngine));
    registry.register(Box::new(actus_engine::lax::LaxEngine));
    registry.register(Box::new(actus_engine::nam::NamEngine));
    registry.register(Box::new(actus_engine::ann::AnnEngine));
    registry.register(Box::new(actus_engine::csh::CshEngine));
    registry.register(Box::new(actus_engine::clm::ClmEngine));
    registry.register(Box::new(actus_engine::ump::UmpEngine));
    registry.register(Box::new(actus_engine::stk::StkEngine));
    registry.register(Box::new(actus_engine::com::ComEngine));
    registry.register(Box::new(actus_engine::fxout::FxoutEngine));
    registry.register(Box::new(actus_engine::swppv::SwppvEngine));
    registry.register(Box::new(actus_engine::swaps::SwapsEngine));
    registry.register(Box::new(actus_engine::capfl::CapflEngine));
    registry.register(Box::new(actus_engine::optns::OptnsEngine));
    registry.register(Box::new(actus_engine::futur::FuturEngine));
    registry.register(Box::new(actus_engine::ceg::CegEngine));
    registry.register(Box::new(actus_engine::cec::CecEngine));
    registry
}

/// `true` when the registry has an engine for the contract type. Probed by
/// dispatching an empty terms bag: a registered engine reports any concrete
/// error, a missing engine reports
/// [`EngineError::UnsupportedContractType`].
#[must_use]
pub fn engine_supports(contract_type: ContractType) -> bool {
    struct EmptyProvider;
    impl RiskFactorProvider for EmptyProvider {}

    let terms = ContractTerms::new(contract_type);
    !matches!(
        engine_registry().evaluate(&terms, &EmptyProvider),
        Err(EngineError::UnsupportedContractType(_))
    )
}

/// Evaluates the full event sequence of the terms with an empty risk factor
/// environment (no observations; scheduled events only).
///
/// # Errors
/// [`EngineError`] of the engine: unsupported contract type, missing
/// attribute, risk factor or state transition failures.
pub fn evaluate_terms(terms: &ContractTerms) -> Result<EvaluationResult, EngineError> {
    evaluate_terms_with_risk(terms, &Scenario::default())
}

/// Evaluates the full event sequence of the terms under a risk factor
/// [`Scenario`] (market observations, observed events and credit events).
///
/// # Errors
/// [`EngineError`] of the engine: unsupported contract type, missing
/// attribute, risk factor or state transition failures.
pub fn evaluate_terms_with_risk(
    terms: &ContractTerms,
    scenario: &Scenario,
) -> Result<EvaluationResult, EngineError> {
    let provider = ScenarioProvider::from_wire(scenario)
        .map_err(|e| EngineError::InvalidTransition(format!("invalid scenario: {e}")))?;
    let events = engine_registry().evaluate(terms, &provider)?;
    let contract_status = events
        .last()
        .map_or(ContractStatus::Active, |e| e.state.contract_status);
    Ok(EvaluationResult {
        contract_status: contract_status_name(contract_status).to_string(),
        events: events.iter().map(event_dto).collect(),
    })
}

/// Wire projection of one [`ContractEvent`].
#[must_use]
pub fn event_dto(e: &ContractEvent) -> EventDto {
    EventDto {
        event_date: e.time.format(TIMESTAMP_FORMAT).to_string(),
        event_type: e.event_type.as_acronym().to_string(),
        payoff: e.payoff.to_string(),
        currency: e.currency.clone(),
        notional_principal: e.state.notional_principal.to_string(),
        nominal_interest_rate: e.state.nominal_interest_rate.to_string(),
        accrued_interest: e.state.accrued_interest.to_string(),
    }
}

/// Validates a terms object against the applicability tables of its contract
/// type (semantics aligned with the `actus-model` builders): every
/// non-null JSON key must be a known dictionary attribute applicable to the
/// contract type, and every base-required attribute must be present.
/// `contractType` itself is always allowed.
#[must_use]
pub fn validate_terms(
    terms: &ContractTerms,
    raw: &serde_json::Map<String, serde_json::Value>,
) -> ValidationReport {
    let tables = applicability::tables(terms.contract_type);
    let is_present = |id: &str| raw.get(id).is_some_and(|v| !v.is_null());
    let is_allowed = |id: &str| tables.applicable.contains(&id) || tables.required.contains(&id);

    let mut errors = Vec::new();
    for (key, value) in raw {
        if key == "contractType" || value.is_null() {
            continue;
        }
        if !is_known_attribute(key) {
            errors.push(ValidationError {
                code: "UnknownAttribute".to_string(),
                attribute: key.clone(),
            });
        } else if !is_allowed(key) {
            errors.push(ValidationError {
                code: "AttributeNotApplicable".to_string(),
                attribute: key.clone(),
            });
        }
    }
    for id in tables.base_required {
        if !is_present(id) {
            errors.push(ValidationError {
                code: "MissingAttribute".to_string(),
                attribute: (*id).to_string(),
            });
        }
    }

    let mut term_status = BTreeMap::new();
    for id in tables.applicable {
        let status = match (tables.base_required.contains(id), is_present(id)) {
            (true, true) => "required-set",
            (true, false) => "required-missing",
            (false, true) => "optional-set",
            (false, false) => "optional-unset",
        };
        term_status.insert(id.to_string(), status.to_string());
    }

    ValidationReport {
        valid: errors.is_empty(),
        errors,
        term_status,
    }
}

/// The full applicability matrix, one entry per [`ContractType::ALL`].
#[must_use]
pub fn applicability_matrix() -> Vec<ApplicabilityMatrixEntry> {
    ContractType::ALL
        .iter()
        .map(|t| ApplicabilityMatrixEntry {
            acronym: t.as_acronym().to_string(),
            tables: metadata::applicability_info(*t),
        })
        .collect()
}

/// JSON-in/JSON-out core of [`crate::evaluate`]; the WASM wrapper only maps
/// the error into a `JsValue`.
pub fn evaluate_json(terms_json: &str) -> Result<String, String> {
    evaluate_json_with_risk(terms_json, "{}")
}

/// JSON-in/JSON-out core of [`crate::evaluate_with_risk`]; the WASM wrapper
/// only maps the error into a `JsValue`.
pub fn evaluate_json_with_risk(terms_json: &str, scenario_json: &str) -> Result<String, String> {
    let terms: ContractTerms =
        serde_json::from_str(terms_json).map_err(|e| format!("invalid contract terms: {e}"))?;
    let scenario: Scenario =
        serde_json::from_str(scenario_json).map_err(|e| format!("invalid scenario JSON: {e}"))?;
    let result = evaluate_terms_with_risk(&terms, &scenario)
        .map_err(|e| format!("evaluation failed: {e}"))?;
    serde_json::to_string(&result).map_err(|e| format!("serialization failed: {e}"))
}

/// JSON-in/JSON-out core of [`crate::validate`]; the WASM wrapper only maps
/// the error into a `JsValue`.
pub fn validate_json(terms_json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(terms_json).map_err(|e| format!("invalid terms JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "terms must be a JSON object".to_string())?;
    let terms: ContractTerms = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid contract terms: {e}"))?;
    let report = validate_terms(&terms, object);
    serde_json::to_string(&report).map_err(|e| format!("serialization failed: {e}"))
}

/// JSON-out core of [`crate::applicability`]; the WASM wrapper only maps the
/// error into a `JsValue`.
pub fn applicability_json(contract_type: &str) -> Result<String, String> {
    let t: ContractType = contract_type
        .parse()
        .map_err(|e: actus_model::ModelError| e.to_string())?;
    serde_json::to_string(&metadata::applicability_info(t))
        .map_err(|e| format!("serialization failed: {e}"))
}

fn is_known_attribute(identifier: &str) -> bool {
    attribute::lookup(identifier).is_some() || identifier == KNOWN_IDENTIFIER_GAP
}

fn contract_status_name(status: ContractStatus) -> &'static str {
    match status {
        ContractStatus::Active => "active",
        ContractStatus::Matured => "matured",
        ContractStatus::Terminated => "terminated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(json: &str) -> ContractTerms {
        serde_json::from_str(json).expect("terms parse")
    }

    #[test]
    fn evaluation_result_serializes_camel_case_with_string_decimals() {
        let terms = terms(
            r#"{
                "contractType": "PAM",
                "contractRole": "RPA",
                "statusDate": "2026-01-01T00:00:00",
                "contractDealDate": "2025-12-30T00:00:00",
                "currency": "USD",
                "notionalPrincipal": "1000",
                "initialExchangeDate": "2026-01-01T00:00:00",
                "maturityDate": "2027-01-01T00:00:00",
                "nominalInterestRate": "0.05",
                "dayCountConvention": "A365"
            }"#,
        );
        let result = evaluate_terms(&terms).expect("evaluation succeeds");
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["contractStatus"], "matured");
        let first = &value["events"][0];
        assert_eq!(first["eventDate"], "2026-01-01T00:00:00");
        assert_eq!(first["eventType"], "IED");
        assert_eq!(first["payoff"], "-1000");
        assert_eq!(first["currency"], "USD");
        assert_eq!(first["notionalPrincipal"], "1000");
        assert_eq!(first["nominalInterestRate"], "0.05");
        assert_eq!(first["accruedInterest"], "0");
        assert!(first.get("event_date").is_none());
    }

    #[test]
    fn validation_reports_status_tiers() {
        let terms = terms(
            r#"{
                "contractType": "PAM",
                "notionalPrincipal": "1000"
            }"#,
        );
        let raw: serde_json::Value = serde_json::from_str(
            r#"{
                "contractType": "PAM",
                "notionalPrincipal": "1000"
            }"#,
        )
        .unwrap();
        let report = validate_terms(&terms, raw.as_object().unwrap());
        assert!(!report.valid);
        assert_eq!(report.term_status["notionalPrincipal"], "required-set");
        assert_eq!(report.term_status["accruedInterest"], "optional-unset");
        assert_eq!(report.term_status["maturityDate"], "required-missing");
    }

    #[test]
    fn applicability_matrix_covers_all_types() {
        let matrix = applicability_matrix();
        assert_eq!(matrix.len(), ContractType::ALL.len());
        let value = serde_json::to_value(&matrix).unwrap();
        assert_eq!(value[0]["acronym"], "ANN");
        assert!(value[0]["baseRequired"].is_array());
        assert!(value
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["acronym"] == "PAM"));
    }

    fn pam_terms() -> ContractTerms {
        terms(
            r#"{
                "contractType": "PAM",
                "contractRole": "RPA",
                "statusDate": "2015-01-01T00:00:00",
                "contractDealDate": "2015-01-01T00:00:00",
                "initialExchangeDate": "2015-01-02T00:00:00",
                "maturityDate": "2015-07-02T00:00:00",
                "notionalPrincipal": "1000",
                "nominalInterestRate": "0.02",
                "dayCountConvention": "30E360",
                "cycleOfInterestPayment": "P1ML0",
                "cycleAnchorDateOfInterestPayment": "2015-02-02T00:00:00",
                "cycleOfRateReset": "P1ML0",
                "cycleAnchorDateOfRateReset": "2015-02-02T00:00:00",
                "marketObjectCodeOfRateReset": "ADR-MKT"
            }"#,
        )
    }

    #[test]
    fn scenario_rates_arrive_as_step_function() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "rates": {
                    "ADR-MKT": {
                        "2015-01-02T00:00:00": 0.02,
                        "2015-02-02T00:00:00": 0.04,
                        "2015-04-02T00:00:00": "0.06"
                    }
                }
            }"#,
        )
        .expect("scenario parses");

        // Before the first observation: no rate observed.
        let empty = evaluate_json_with_risk(
            r#"{
                "contractType": "PAM", "contractRole": "RPA",
                "statusDate": "2015-01-01T00:00:00",
                "initialExchangeDate": "2015-01-02T00:00:00",
                "maturityDate": "2015-02-02T00:00:00",
                "notionalPrincipal": "1000", "nominalInterestRate": "0.02",
                "dayCountConvention": "30E360",
                "cycleOfRateReset": "P1ML0",
                "cycleAnchorDateOfRateReset": "2015-01-02T00:00:00",
                "marketObjectCodeOfRateReset": "ADR-MKT"
            }"#,
            r#"{"rates": {"ADR-MKT": {"2015-03-02T00:00:00": 0.04}}}"#,
        );
        assert!(empty.is_err(), "missing observation must reject");

        // From the observation time on, resets observe the scenario rates.
        let result =
            evaluate_terms_with_risk(&pam_terms(), &scenario).expect("evaluation succeeds");
        let resets: Vec<_> = result
            .events
            .iter()
            .filter(|e| e.event_type == "RR")
            .collect();
        assert_eq!(resets.len(), 5);
        assert_eq!(resets[0].event_date, "2015-02-02T00:00:00");
        assert_eq!(resets[0].nominal_interest_rate, "0.04");
        assert_eq!(resets[1].nominal_interest_rate, "0.04");
        assert_eq!(resets[2].nominal_interest_rate, "0.06");
        assert_eq!(resets[3].nominal_interest_rate, "0.06");
        assert_eq!(resets[4].nominal_interest_rate, "0.06");
    }

    #[test]
    fn scenario_provider_namespaces_and_fall_back() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "rates": {"R": {"2015-01-01T00:00:00": 0.01}},
                "unitPrices": {"U": {"2015-01-01T00:00:00": 105}},
                "fxRates": {"USD/EUR": {"2015-01-01T00:00:00": 1.1}},
                "observedEvents": [
                    {"eventType": "PP", "time": "2015-03-01", "payoff": "100"}
                ],
                "creditEvents": [{"time": "2015-06-01T00:00:00", "performance": "DF"}]
            }"#,
        )
        .expect("scenario parses");
        let provider = ScenarioProvider::from_wire(&scenario).expect("provider builds");

        let t = |s: &str| parse_scenario_timestamp(s).unwrap();
        assert_eq!(
            provider.rate("R", t("2015-05-01T00:00:00")),
            Some(Decimal::new(1, 2))
        );
        assert_eq!(provider.rate("U", t("2015-05-01T00:00:00")), None);
        assert_eq!(
            provider.index("U", t("2015-05-01T00:00:00")),
            Some(Decimal::new(105, 0))
        );
        // FX rates are readable through both lookups.
        assert_eq!(
            provider.rate("USD/EUR", t("2015-05-01T00:00:00")),
            Some(Decimal::new(11, 1))
        );
        assert_eq!(
            provider.index("USD/EUR", t("2015-05-01T00:00:00")),
            Some(Decimal::new(11, 1))
        );
        // Nothing before the first observation.
        assert_eq!(provider.rate("R", t("2014-01-01T00:00:00")), None);
        // Observed events and credit events are carried into the provider.
        assert_eq!(provider.observed_events().len(), 1);
        assert_eq!(provider.observed_events()[0].time, t("2015-03-01T00:00:00"));
        assert_eq!(provider.observed_credit_events().len(), 1);
        assert_eq!(
            provider.observed_credit_events()[0].performance,
            Some(ContractPerformance::Default)
        );
    }

    #[test]
    fn empty_scenario_matches_plain_evaluation() {
        let values = crate::specs::default_values(ContractType::Pam);
        let terms = crate::specs::terms_json(ContractType::Pam, &values);
        let raw = serde_json::to_string(&terms).unwrap();
        let plain = evaluate_json(&raw).expect("plain evaluation succeeds");
        let with_empty_scenario = evaluate_json_with_risk(&raw, "{}").expect("scenario evaluation");
        assert_eq!(plain, with_empty_scenario);
        let value: serde_json::Value = serde_json::from_str(&plain).unwrap();
        assert!(value["events"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn scenario_json_errors_are_reported() {
        let terms = r#"{"contractType": "PAM"}"#;
        let err = evaluate_json_with_risk(terms, r#"{"rates": {"R": {"nonsense": 1}}}"#)
            .expect_err("invalid timestamp rejected");
        assert!(err.contains("invalid scenario"), "{err}");
        let err = evaluate_json_with_risk(
            terms,
            r#"{"creditEvents": [{"time": "2015-01-01", "performance": "XX"}]}"#,
        )
        .expect_err("unknown performance rejected");
        assert!(err.contains("unknown performance"), "{err}");
    }
}
