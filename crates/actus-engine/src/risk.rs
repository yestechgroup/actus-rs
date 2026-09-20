//! Risk factor observation primitives (ACTUS techspec section "Risk Factor Observer").

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractPerformance, EventType};

/// An externally observed event carried by the evaluation environment.
///
/// The testbed files (and, in production, the surrounding system) carry
/// events observed from outside the contract, e.g. the analysis dates a cash
/// transfer (`CSH`) was valued at. The event time is already normalised via
/// the ACTUS timestamp convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedEvent {
    /// Observation time of the event.
    pub time: NaiveDateTime,
    /// Observed event type, e.g. [`EventType::Monitoring`] (`AD`).
    pub event_type: EventType,
}

/// An externally observed credit event (`CE`) on a referenced contract.
///
/// Credit enhancement contracts (techspec sections "CEG: Credit Enhancement
/// Guarantee" and "CEC: Credit Enhancement Collateral") are triggered by
/// credit events observed on their covered contracts: the observation names
/// the affected contract and the contract performance state it entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCreditEvent {
    /// Observation time of the credit event; the `XD` exercise time.
    pub time: NaiveDateTime,
    /// Contract identifier of the affected contract, matched against the
    /// covered contract references of the observing contract.
    pub contract_id: Option<String>,
    /// Contract performance state the contract entered (`PRF`), matched
    /// against the `creditEventTypeCovered` attribute.
    pub performance: Option<ContractPerformance>,
}

/// Provider of market risk factor observations used during contract evaluation.
///
/// Mirrors the ACTUS Risk Factor Observer `'rf'` interface
/// `obs(rf, i, t) -> Real` (techspec section "Risk Factor Observer"): the
/// engine requests the state of a risk factor identified by a market object
/// code at a given time. The default implementation observes nothing, i.e.
/// every lookup yields `None`.
///
/// Observation semantics are step functions: an observation is valid from its
/// observation time until the next observation of the same market object.
pub trait RiskFactorProvider {
    /// Interest rate observed for `market_object_code` at time `at`.
    fn rate(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        let _ = (market_object_code, at);
        None
    }

    /// Index value observed for `market_object_code` at time `at`.
    fn index(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        let _ = (market_object_code, at);
        None
    }

    /// The externally observed event stream (`eventsObserved`), replayed by
    /// contract types whose schedule consists of contingent analysis events
    /// (`CSH`). The default environment observes no events.
    fn observed_events(&self) -> Vec<ObservedEvent> {
        Vec::new()
    }

    /// The externally observed credit events (`CE`), triggering the credit
    /// enhancement contract types (CEC). The default environment observes
    /// none.
    fn observed_credit_events(&self) -> Vec<ObservedCreditEvent> {
        Vec::new()
    }
}

/// A [`RiskFactorProvider`] built from a fixed map of observations.
///
/// Test and offline-evaluation double: observations are `(market object code,
/// observation time, value)` triples. Rates and indexes are kept in separate
/// namespaces so the same code can be used for both. Lookups and iteration are
/// fully deterministic (`BTreeMap` ordering); no hashing is involved.
#[derive(Debug, Clone, Default)]
pub struct StateProvider {
    rates: BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
    indexes: BTreeMap<String, BTreeMap<NaiveDateTime, Decimal>>,
    events: Vec<ObservedEvent>,
    credit_events: Vec<ObservedCreditEvent>,
}

impl StateProvider {
    /// Creates an empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a rate observation for `market_object_code` at `at`.
    pub fn with_rate(
        mut self,
        market_object_code: impl Into<String>,
        at: NaiveDateTime,
        value: Decimal,
    ) -> Self {
        self.rates
            .entry(market_object_code.into())
            .or_default()
            .insert(at, value);
        self
    }

    /// Records an index observation for `market_object_code` at `at`.
    pub fn with_index(
        mut self,
        market_object_code: impl Into<String>,
        at: NaiveDateTime,
        value: Decimal,
    ) -> Self {
        self.indexes
            .entry(market_object_code.into())
            .or_default()
            .insert(at, value);
        self
    }

    /// Records an externally observed event.
    pub fn with_observed_event(mut self, event: ObservedEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Records an externally observed credit event.
    pub fn with_observed_credit_event(mut self, event: ObservedCreditEvent) -> Self {
        self.credit_events.push(event);
        self
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

impl RiskFactorProvider for StateProvider {
    fn rate(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        Self::observe(&self.rates, market_object_code, at)
    }

    fn index(&self, market_object_code: &str, at: NaiveDateTime) -> Option<Decimal> {
        Self::observe(&self.indexes, market_object_code, at)
    }

    fn observed_events(&self) -> Vec<ObservedEvent> {
        self.events.clone()
    }

    fn observed_credit_events(&self) -> Vec<ObservedCreditEvent> {
        self.credit_events.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(
            &format!("{y:04}-{m:02}-{d:02}T00:00:00"),
            "%Y-%m-%dT%H:%M:%S",
        )
        .unwrap()
    }

    #[test]
    fn default_provider_observes_nothing() {
        let provider = StateProvider::new();
        assert_eq!(provider.rate("ADR-MKT", t(2015, 1, 1)), None);
        assert_eq!(provider.index("IDX", t(2015, 1, 1)), None);
    }

    #[test]
    fn step_function_valid_from_observation_until_next() {
        let provider = StateProvider::new()
            .with_rate("USD-LIBOR-3M", t(2015, 1, 1), Decimal::new(2, 2))
            .with_rate("USD-LIBOR-3M", t(2015, 7, 1), Decimal::new(3, 2));

        assert_eq!(provider.rate("USD-LIBOR-3M", t(2014, 12, 31)), None);
        assert_eq!(
            provider.rate("USD-LIBOR-3M", t(2015, 1, 1)),
            Some(Decimal::new(2, 2))
        );
        assert_eq!(
            provider.rate("USD-LIBOR-3M", t(2015, 6, 30)),
            Some(Decimal::new(2, 2))
        );
        assert_eq!(
            provider.rate("USD-LIBOR-3M", t(2015, 7, 1)),
            Some(Decimal::new(3, 2))
        );
        assert_eq!(
            provider.rate("USD-LIBOR-3M", t(2020, 1, 1)),
            Some(Decimal::new(3, 2))
        );
    }

    #[test]
    fn rate_and_index_namespaces_are_independent() {
        let provider = StateProvider::new()
            .with_rate("COD-USD-3M", t(2015, 1, 1), Decimal::new(25, 4))
            .with_index("IDX-A", t(2015, 1, 1), Decimal::new(1100, 0));

        assert_eq!(
            provider.rate("COD-USD-3M", t(2015, 3, 1)),
            Some(Decimal::new(25, 4))
        );
        assert_eq!(provider.rate("IDX-A", t(2015, 3, 1)), None);
        assert_eq!(
            provider.index("IDX-A", t(2015, 3, 1)),
            Some(Decimal::new(1100, 0))
        );
        assert_eq!(provider.index("COD-USD-3M", t(2015, 3, 1)), None);
    }

    #[test]
    fn codes_are_independent() {
        let provider = StateProvider::new()
            .with_rate("CODE-A", t(2015, 1, 1), Decimal::ONE)
            .with_rate("CODE-B", t(2015, 4, 1), Decimal::TWO);

        assert_eq!(provider.rate("CODE-A", t(2015, 5, 1)), Some(Decimal::ONE));
        assert_eq!(provider.rate("CODE-B", t(2015, 5, 1)), Some(Decimal::TWO));
        assert_eq!(provider.rate("CODE-C", t(2015, 5, 1)), None);
    }
}
