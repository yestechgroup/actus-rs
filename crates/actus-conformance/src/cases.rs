//! Loader for the official ACTUS conformance testbeds
//! (`vendor/actus/tests/actus-tests-<type>.json`).
//!
//! A testbed file maps case identifiers to cases; each case carries the
//! contract `terms`, an optional analysis horizon `to`, observed risk factor
//! series (`dataObserved`, keyed by market object code) and the expected
//! `results` event stream produced by the upstream reference implementation.
//!
//! Wire quirks handled here (see `vendor/actus/README.md`): `eventDate`
//! values use minute precision without seconds, numeric values arrive as
//! JSON strings or numbers (both f64-printed by the upstream reference), and
//! `23:59:59` timestamps denote midnight of the following day.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::de::{Deserializer, Error as DeError, Unexpected, Visitor};
use serde::Deserialize;

use actus_engine::daycount::normalize_timestamp;
use actus_engine::{EngineError, EngineRegistry, RiskFactorProvider};
use actus_model::serde_helpers::parse_timestamp;
use actus_model::{ContractPerformance, ContractTerms, ContractType, EventType};

/// Errors raised while loading a testbed file.
#[derive(Debug)]
pub enum Error {
    /// The testbed file could not be read.
    Io(std::io::Error),
    /// The testbed file is not valid JSON or does not match the schema.
    Json(serde_json::Error),
    /// The requested contract type has no testbed file name.
    UnknownContractType(ContractType),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "testbed read error: {e}"),
            Error::Json(e) => write!(f, "testbed parse error: {e}"),
            Error::UnknownContractType(t) => {
                write!(f, "no testbed for contract type {t}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::UnknownContractType(_) => None,
        }
    }
}

/// One conformance case of a testbed file.
#[derive(Debug, Deserialize)]
pub struct ActusTestCase {
    /// Case identifier, e.g. `pam01`.
    pub identifier: String,
    /// Contract terms of the case (deserialized directly from the wire bag).
    pub terms: ContractTerms,
    /// Analysis horizon (`to`); empty or null means the full contract
    /// lifetime. Events strictly after the horizon are not part of the
    /// expected results.
    #[serde(default)]
    pub to: Option<String>,
    /// Observed risk factor series keyed by market object code.
    #[serde(rename = "dataObserved", default)]
    pub data_observed: BTreeMap<String, RiskFactorObservation>,
    /// Externally observed events, e.g. the analysis dates a CSH contract
    /// was valued at; replayed by the CSH engine through the risk factor
    /// provider.
    #[serde(rename = "eventsObserved", default)]
    pub events_observed: Vec<ObservedEvent>,
    /// Expected event stream with payoffs and post-event states.
    #[serde(default)]
    pub results: Vec<ExpectedEvent>,
}

impl ActusTestCase {
    /// The effective analysis horizon end, when the `to` field carries a
    /// parseable timestamp.
    pub fn horizon(&self) -> Option<NaiveDateTime> {
        self.to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|s| parse_timestamp(s).ok())
            .map(normalize_timestamp)
    }
}

/// Evaluates one testbed case through `registry` and truncates the event
/// stream at the case analysis horizon.
///
/// The testbed `to` field bounds the reported event stream (ann08 reports
/// only the events up to its horizon); the engine itself evaluates the full
/// contract lifetime, so the horizon is applied here, mirroring the upstream
/// reference behaviour. The observed events (`eventsObserved`) are part of
/// the evaluation environment the engines may replay (CSH analysis dates,
/// the CLM exercise notice).
///
/// One quirk resolved against the CLM testbed: the settlement consequences
/// of an in-window exercise notice are reported even when they fall beyond
/// the horizon — clm07 carries `to = 2015-09-30` yet expects the final
/// interest payment and settlement (`IP`, `STD`) at the notice-plus-`XDN`
/// date 2015-10-21. Settlement events and their same-timestamp companions
/// therefore survive the truncation.
pub fn evaluate_case(
    registry: &EngineRegistry,
    case: &ActusTestCase,
) -> Result<Vec<actus_engine::ContractEvent>, EngineError> {
    use std::collections::BTreeSet;

    use actus_model::EventType;

    let risk = ObservedRiskFactors::new(case.data_observed.clone())
        .with_events(case.events_observed.clone());
    let mut events = registry.evaluate(&case.terms, &risk)?;
    if let Some(horizon) = case.horizon() {
        let settlement_times: BTreeSet<chrono::NaiveDateTime> = events
            .iter()
            .filter(|event| event.event_type == EventType::Settlement)
            .map(|event| event.time)
            .collect();
        events.retain(|event| {
            event.time <= horizon
                || event.event_type == EventType::Settlement
                || settlement_times.contains(&event.time)
        });
    }
    Ok(events)
}

/// One observed risk factor series (`dataObserved` entry).
#[derive(Debug, Clone, Deserialize)]
pub struct RiskFactorObservation {
    /// Market object code of the series, e.g. `USD_SWP`.
    pub identifier: String,
    /// Time/value observation points.
    pub data: Vec<RiskFactorObservationPoint>,
}

/// A single observation of a risk factor series.
#[derive(Debug, Clone, Deserialize)]
pub struct RiskFactorObservationPoint {
    /// Observation time.
    #[serde(deserialize_with = "wire_timestamp")]
    pub timestamp: NaiveDateTime,
    /// Observed value.
    #[serde(deserialize_with = "wire_decimal")]
    pub value: Decimal,
}

/// One externally observed event (`eventsObserved` entry).
///
/// Credit event observations (`CE`) additionally name the affected contract
/// (`contractId`) and the contract performance state it entered
/// (`states.contractPerformance`); both are replayed to the credit
/// enhancement engines through the risk factor provider.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservedEvent {
    /// Observation time.
    #[serde(deserialize_with = "wire_timestamp")]
    pub time: NaiveDateTime,
    /// Observed event type acronym, e.g. `AD`.
    #[serde(rename = "type")]
    pub event_type: EventType,
    /// Contract identifier of the affected contract (credit events).
    #[serde(rename = "contractId", default)]
    pub contract_id: Option<String>,
    /// Contract states at the observation (credit events).
    #[serde(default)]
    pub states: Option<ObservedEventStates>,
}

/// The `states` bag of an externally observed credit event.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservedEventStates {
    /// The contract performance state (`PRF`) the contract entered.
    #[serde(rename = "contractPerformance", default)]
    pub contract_performance: Option<ContractPerformance>,
}

impl From<ObservedEvent> for actus_engine::ObservedEvent {
    fn from(event: ObservedEvent) -> actus_engine::ObservedEvent {
        actus_engine::ObservedEvent {
            time: event.time,
            event_type: event.event_type,
        }
    }
}

/// One expected event of the reference implementation.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedEvent {
    /// Event time (minute precision, second precision or bare date).
    #[serde(rename = "eventDate", deserialize_with = "wire_timestamp")]
    pub event_date: NaiveDateTime,
    /// Event type acronym, e.g. `IED`.
    #[serde(rename = "eventType")]
    pub event_type: EventType,
    /// Payoff in the contract currency, from the contract role perspective.
    #[serde(deserialize_with = "wire_decimal")]
    pub payoff: Decimal,
    /// Settlement currency.
    pub currency: String,
    /// Post-event notional principal `NT`.
    #[serde(rename = "notionalPrincipal", deserialize_with = "wire_decimal")]
    pub notional_principal: Decimal,
    /// Post-event nominal interest rate `IPNR`; absent when the testbed
    /// does not report the state (the credit enhancement family).
    #[serde(
        default,
        rename = "nominalInterestRate",
        deserialize_with = "wire_decimal_option"
    )]
    pub nominal_interest_rate: Option<Decimal>,
    /// Post-event accrued interest `IPAC`; absent when the testbed does not
    /// report the state (the credit enhancement family).
    #[serde(
        default,
        rename = "accruedInterest",
        deserialize_with = "wire_decimal_option"
    )]
    pub accrued_interest: Option<Decimal>,
}

/// Loads the testbed cases for `contract_type`, ordered by identifier.
///
/// The testbed files live under `vendor/actus/tests` relative to the crate;
/// the path is resolved via `CARGO_MANIFEST_DIR` so both `cargo test` and the
/// `report` binary find them without runtime configuration.
pub fn load_testbed(contract_type: ContractType) -> Result<Vec<ActusTestCase>, Error> {
    let file_name = match contract_type {
        ContractType::Pam => "actus-tests-pam.json",
        ContractType::Lam => "actus-tests-lam.json",
        ContractType::Nam => "actus-tests-nam.json",
        ContractType::Ann => "actus-tests-ann.json",
        ContractType::Csh => "actus-tests-csh.json",
        ContractType::Clm => "actus-tests-clm.json",
        ContractType::Swaps => "actus-tests-swaps.json",
        ContractType::Cec => "actus-tests-cec.json",
        other => return Err(Error::UnknownContractType(other)),
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/actus/tests")
        .join(file_name);
    let raw = fs::read_to_string(&path).map_err(Error::Io)?;
    let cases: BTreeMap<String, ActusTestCase> = serde_json::from_str(&raw).map_err(Error::Json)?;
    let mut ordered: Vec<ActusTestCase> = cases.into_values().collect();
    ordered.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(ordered)
}

/// A [`RiskFactorProvider`] over a testbed `dataObserved` map.
///
/// Observations act as step functions: an observation is valid from its
/// timestamp until the next observation of the same market object code
/// (techspec section "Risk Factor Observer"). The testbed files do not
/// distinguish rate and index namespaces, so both lookups read the same map.
/// The externally observed events (`eventsObserved`) are part of the same
/// evaluation environment.
#[derive(Debug, Clone, Default)]
pub struct ObservedRiskFactors {
    observations: BTreeMap<String, RiskFactorObservation>,
    events: Vec<ObservedEvent>,
}

impl ObservedRiskFactors {
    /// Builds a provider from a case's `dataObserved` map.
    pub fn new(observations: BTreeMap<String, RiskFactorObservation>) -> ObservedRiskFactors {
        ObservedRiskFactors {
            observations,
            events: Vec::new(),
        }
    }

    /// Adds the case's externally observed events to the environment.
    pub fn with_events(mut self, events: Vec<ObservedEvent>) -> ObservedRiskFactors {
        self.events = events;
        self
    }

    /// Step-function lookup: the latest observation at or before `at`.
    fn observe(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        let series = self.observations.get(market_object_code)?;
        series
            .data
            .iter()
            .filter(|point| point.timestamp <= at)
            .max_by_key(|point| point.timestamp)
            .map(|point| point.value)
    }
}

impl RiskFactorProvider for ObservedRiskFactors {
    fn rate(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        self.observe(market_object_code, at)
    }

    fn index(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        self.observe(market_object_code, at)
    }

    fn observed_events(&self) -> Vec<actus_engine::ObservedEvent> {
        self.events.iter().cloned().map(Into::into).collect()
    }

    fn observed_credit_events(&self) -> Vec<actus_engine::ObservedCreditEvent> {
        self.events
            .iter()
            .filter(|event| event.event_type == EventType::CreditEvent)
            .map(|event| actus_engine::ObservedCreditEvent {
                time: event.time,
                contract_id: event.contract_id.clone(),
                performance: event
                    .states
                    .as_ref()
                    .and_then(|states| states.contract_performance),
            })
            .collect()
    }
}

/// Deserializes a non-optional ACTUS timestamp wire value.
fn wire_timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NaiveDateTime, D::Error> {
    struct WireTimestampVisitor;

    impl<'de> Visitor<'de> for WireTimestampVisitor {
        type Value = NaiveDateTime;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "an ACTUS timestamp (ISO 8601 date or datetime)")
        }

        fn visit_str<E: DeError>(self, v: &str) -> Result<NaiveDateTime, E> {
            parse_timestamp(v)
                .map(normalize_timestamp)
                .map_err(|_| DeError::invalid_value(Unexpected::Str(v), &"an ISO 8601 timestamp"))
        }
    }

    deserializer.deserialize_str(WireTimestampVisitor)
}

/// Deserializes a non-optional ACTUS decimal wire value (string or number).
fn wire_decimal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    struct WireDecimalVisitor;

    impl<'de> Visitor<'de> for WireDecimalVisitor {
        type Value = Decimal;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "an ACTUS decimal (string or number)")
        }

        fn visit_str<E: DeError>(self, v: &str) -> Result<Decimal, E> {
            actus_model::serde_helpers::parse_decimal(v)
                .map_err(|_| DeError::invalid_value(Unexpected::Str(v), &"a decimal value"))
        }

        fn visit_f64<E: DeError>(self, v: f64) -> Result<Decimal, E> {
            Decimal::try_from(v).map_err(DeError::custom)
        }

        fn visit_i64<E: DeError>(self, v: i64) -> Result<Decimal, E> {
            Ok(Decimal::from(v))
        }

        fn visit_u64<E: DeError>(self, v: u64) -> Result<Decimal, E> {
            Ok(Decimal::from(v))
        }
    }

    deserializer.deserialize_any(WireDecimalVisitor)
}

/// Deserializes an optional ACTUS decimal wire value (string or number).
///
/// Present values delegate to [`wire_decimal`]; an explicit `null` (or a
/// missing key under `#[serde(default)]`) yields `None`.
fn wire_decimal_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error> {
    struct WireDecimalOptionVisitor;

    impl<'de> Visitor<'de> for WireDecimalOptionVisitor {
        type Value = Option<Decimal>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "an ACTUS decimal (string or number)")
        }

        fn visit_none<E: DeError>(self) -> Result<Option<Decimal>, E> {
            Ok(None)
        }

        fn visit_unit<E: DeError>(self) -> Result<Option<Decimal>, E> {
            Ok(None)
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Option<Decimal>, D2::Error> {
            wire_decimal(d).map(Some)
        }
    }

    deserializer.deserialize_option(WireDecimalOptionVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(raw: &str) -> Decimal {
        Decimal::from_str(raw).expect("test decimal")
    }

    #[test]
    fn loads_pam_testbed_ordered_by_identifier() {
        let cases = load_testbed(ContractType::Pam).expect("pam testbed");
        assert_eq!(cases.len(), 25);
        assert_eq!(cases[0].identifier, "pam01");
        assert_eq!(cases[24].identifier, "pam25");
        assert_eq!(cases[0].terms.contract_type, ContractType::Pam);
    }

    #[test]
    fn expected_events_parse_minute_precision_and_numbers() {
        let cases = load_testbed(ContractType::Pam).expect("pam testbed");
        let first = &cases[0].results[0];
        assert_eq!(first.event_type, EventType::InitialExchange);
        assert_eq!(
            first.event_date,
            NaiveDateTime::parse_from_str("2013-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
        );
        assert_eq!(first.payoff, d("-3000"));
        assert_eq!(first.currency, "USD");
        assert_eq!(first.notional_principal, d("3000"));
        assert_eq!(first.nominal_interest_rate, Some(d("0.1")));
        assert_eq!(first.accrued_interest, Some(Decimal::ZERO));
    }

    #[test]
    fn end_of_day_timestamps_normalise_to_next_midnight() {
        let cases = load_testbed(ContractType::Pam).expect("pam testbed");
        let pam25 = cases
            .iter()
            .find(|c| c.identifier == "pam25")
            .expect("pam25");
        let maturity = pam25.results.last().expect("pam25 maturity");
        assert_eq!(
            maturity.event_date,
            NaiveDateTime::parse_from_str("2014-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
        );
    }

    #[test]
    fn observed_risk_factors_act_as_step_functions() {
        let cases = load_testbed(ContractType::Pam).expect("pam testbed");
        let pam21 = cases
            .iter()
            .find(|c| c.identifier == "pam21")
            .expect("pam21");
        let risk = ObservedRiskFactors::new(pam21.data_observed.clone());
        let before =
            NaiveDateTime::parse_from_str("2013-01-31T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let at = NaiveDateTime::parse_from_str("2013-02-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        let after =
            NaiveDateTime::parse_from_str("2013-04-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap();
        assert_eq!(risk.rate("USD_SWP", before), None);
        assert_eq!(risk.rate("USD_SWP", at), Some(d("0.0098271604945178")));
        assert_eq!(risk.rate("USD_SWP", after), Some(d("0.0098271604945178")));
        assert_eq!(risk.index("USD_SWP", at), Some(d("0.0098271604945178")));
    }

    #[test]
    fn empty_to_field_yields_no_horizon() {
        let cases = load_testbed(ContractType::Pam).expect("pam testbed");
        assert!(cases.iter().all(|c| c.horizon().is_none()));
    }
}
