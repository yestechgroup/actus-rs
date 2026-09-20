//! SWAPS: Swap (ACTUS techspec sections 4 "Contract Composition", 6 "Child
//! Contract Observer" and 7.13 "SWAPS: Swap").
//!
//! A SWAPS contract combines two child contracts — the first leg
//! (`CTST^FirstLeg_Contract`) and the second leg (`CTST^SecondLeg_Contract`)
//! referenced through the `contractStructure` attribute — and evaluates them
//! through the child contract observer `U_ev`: each leg's event stream is
//! produced by the leg's own contract engine, with the parent's contract
//! role propagated into the child evaluation (techspec Example 14: a parent
//! `CNTRL=PFL` evaluates the first leg with `CNTRL=RPL`). Per dictionary
//! Table 1 the `RFL` (receive first leg) parent carries sign `+1` and
//! evaluates the first leg as `RPA` / second leg as `RPL`; `PFL` (pay first
//! leg) carries `-1` and flips the assignment.
//!
//! The merged parent stream follows the settlement mode `DS` resolved
//! against the official testbed:
//!
//! - `D` (cash / pay-as-you-go settlement, swaps01 to swaps10): the leg
//!   events pass through unmerged — congruent events (same time and event
//!   type) stay separate rows, each carrying its leg's post-event state.
//! - `S` (single-settlement mode, swaps11): congruent events merge into
//!   aggregate events `z_m^tau` (techspec 7.13). The aggregate payoff sums
//!   the leg payoffs, the aggregate notional state sums the leg notionals,
//!   and the interest-bearing states (`IPNR`, `IPAC`) collapse to the
//!   parent's own (unset, hence zero) values: the parent swap carries no
//!   interest state of its own.
//!
//! Parent-level purchase and termination (swaps09, swaps10) act as an
//! observation window around the leg streams: leg events before `PRD`
//! respectively after `TD` are not observed, the parent `PRD`/`TD` event is
//! emitted once with the role-signed price as payoff and a zeroed state.

use rust_decimal::Decimal;

use actus_model::{
    ContractReference, ContractReferenceRole, ContractRole, ContractTerms, ContractType,
    DeliverySettlement, EventType,
};

use crate::ann::AnnEngine;
use crate::clm::ClmEngine;
use crate::daycount::normalize_timestamp;
use crate::engine::{ContractEngine, EngineRegistry};
use crate::event::{sort_events, ContractEvent};
use crate::lam::LamEngine;
use crate::nam::NamEngine;
use crate::pam::PamEngine;
use crate::risk::RiskFactorProvider;
use crate::state::ContractState;
use crate::EngineError;

/// Implementation of the SWAPS contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct SwapsEngine;

impl ContractEngine for SwapsEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Swaps
    }

    /// Evaluates both child legs, applies the parent observation window and
    /// settlement mode, and returns the merged event stream.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let (first, second) = child_legs(terms)?;
        let parent_sign = parent_role_sign(terms.contract_role)?;
        let mut stream = evaluate_leg(first, leg_role(parent_sign, true), risk)?;
        stream.extend(evaluate_leg(second, leg_role(parent_sign, false), risk)?);
        apply_observation_window(terms, &mut stream, parent_sign)?;
        sort_events(&mut stream);
        if terms.delivery_settlement == Some(DeliverySettlement::S) {
            stream = merge_congruent(terms, stream);
        }
        Ok(stream)
    }
}

/// The engines a child leg may route through.
///
/// The SWAPS testbed legs are PAM and ANN; the remaining fixed-income and
/// call-money engines are registered so any schedule-driven leg type
/// evaluates through its own implementation.
fn leg_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(ClmEngine));
    registry
}

/// Resolves the first and second leg of the contract structure.
///
/// References carrying the dictionary leg roles (`FIL`/`SEL`) are matched by
/// role; otherwise the array order decides (first entry = first leg). The
/// official testbed fixtures carry both the roles and the canonical order.
fn child_legs(
    terms: &ContractTerms,
) -> Result<(&ContractReference, &ContractReference), EngineError> {
    let structure = terms
        .contract_structure
        .as_deref()
        .ok_or(EngineError::MissingAttribute("contractStructure"))?;
    let first = structure
        .iter()
        .find(|r| r.reference_role == Some(ContractReferenceRole::FirstLeg))
        .or_else(|| structure.first());
    let second = structure
        .iter()
        .find(|r| r.reference_role == Some(ContractReferenceRole::SecondLeg))
        .or_else(|| structure.get(1));
    match (first, second) {
        (Some(first), Some(second)) => Ok((first, second)),
        _ => Err(EngineError::InvalidTransition(
            "contract structure needs a first and a second leg reference".to_string(),
        )),
    }
}

/// Evaluates one child leg under an effective contract role.
///
/// The parent's role sign propagates by overriding the leg role: the leg
/// engines sign their payoffs and notional states from `CNTRL`
/// (`role_sign`), so `RPA` reproduces the parent's `+1` orientation and
/// `RPL` its `-1` orientation (techspec Example 14). The leg's risk factor
/// observations resolve against the same observer the parent was given —
/// the testbed `dataObserved` series are shared by both legs.
fn evaluate_leg(
    reference: &ContractReference,
    role: ContractRole,
    risk: &dyn RiskFactorProvider,
) -> Result<Vec<ContractEvent>, EngineError> {
    let mut leg_terms = reference
        .object
        .clone()
        .ok_or(EngineError::InvalidTransition(
            "contract reference without an embedded contract object".to_string(),
        ))?;
    leg_terms.contract_role = Some(role);
    leg_registry().evaluate(&leg_terms, risk)
}

/// The role sign of the parent swap (dictionary Table 1): `RFL` receives the
/// first leg (`+1`), `PFL` pays it (`-1`); an unset role defaults to the
/// receiving orientation.
fn parent_role_sign(role: Option<ContractRole>) -> Result<Decimal, EngineError> {
    match role {
        Some(ContractRole::Rfl) | None => Ok(Decimal::ONE),
        Some(ContractRole::Pfl) => Ok(-Decimal::ONE),
        Some(other) => Err(EngineError::InvalidTransition(format!(
            "unsupported SWAPS contract role {other}"
        ))),
    }
}

/// The effective leg role for the parent role sign: `+1` evaluates the first
/// leg as `RPA` and the second as `RPL`, `-1` flips the assignment.
fn leg_role(parent_sign: Decimal, first: bool) -> ContractRole {
    let receive = parent_sign.is_sign_positive() == first;
    if receive {
        ContractRole::Rpa
    } else {
        ContractRole::Rpl
    }
}

/// Applies the parent-level purchase/termination observation window and
/// emits the single parent `PRD`/`TD` events (swaps09, swaps10).
///
/// Leg events before the purchase date respectively after the termination
/// date are not observed; the parent event carries the role-signed price as
/// payoff and a zeroed state (the swap itself holds no notional).
fn apply_observation_window(
    terms: &ContractTerms,
    stream: &mut Vec<ContractEvent>,
    parent_sign: Decimal,
) -> Result<(), EngineError> {
    if let Some(prd) = terms.purchase_date.map(normalize_timestamp) {
        stream.retain(|event| event.time >= prd);
        let price = terms.price_at_purchase_date.unwrap_or(Decimal::ZERO);
        stream.push(parent_event(
            EventType::Purchase,
            prd,
            parent_sign * price,
            terms,
        ));
    }
    if let Some(td) = terms.termination_date.map(normalize_timestamp) {
        stream.retain(|event| event.time <= td);
        let price = terms.price_at_termination_date.unwrap_or(Decimal::ZERO);
        stream.push(parent_event(
            EventType::Termination,
            td,
            parent_sign * price,
            terms,
        ));
    }
    Ok(())
}

/// Builds a parent-level event with a zeroed contract state.
fn parent_event(
    event_type: EventType,
    time: chrono::NaiveDateTime,
    payoff: Decimal,
    terms: &ContractTerms,
) -> ContractEvent {
    ContractEvent {
        event_type,
        time,
        payoff,
        currency: terms.currency.clone(),
        state: ContractState::default(),
    }
}

/// Merges congruent events into aggregate events `z_m^tau` (techspec 7.13).
///
/// `stream` arrives sorted, so congruent events (same time and event type)
/// are adjacent and one fold pass suffices. The aggregate payoff and
/// notional state sum the leg values; the interest-bearing states collapse
/// to the parent's own values (unset for the parent swap, hence zero, as the
/// swaps11 fixture reports). Non-congruent events pass through untouched.
fn merge_congruent(terms: &ContractTerms, stream: Vec<ContractEvent>) -> Vec<ContractEvent> {
    let parent_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
    let parent_accrued = terms.accrued_interest.unwrap_or(Decimal::ZERO);
    let mut merged: Vec<ContractEvent> = Vec::with_capacity(stream.len());
    for event in stream {
        if let Some(last) = merged.last_mut() {
            if last.time == event.time && last.event_type == event.event_type {
                last.payoff += event.payoff;
                last.state.notional_principal += event.state.notional_principal;
                last.state.nominal_interest_rate = parent_rate;
                last.state.accrued_interest = parent_accrued;
                continue;
            }
        }
        merged.push(event);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use actus_model::EventType;
    use chrono::{NaiveDateTime, TimeZone, Utc};
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn risk() -> StateProvider {
        StateProvider::new()
    }

    /// The testbed fixture values are f64-printed by the upstream reference;
    /// assertions on day-count quantities compare within a narrow tolerance.
    fn assert_close(actual: Decimal, expected: Decimal) {
        let diff = (actual - expected).abs();
        assert!(
            diff < Decimal::new(1, 9),
            "expected {expected}, actual {actual}"
        );
    }

    /// swaps01: RFL parent, fixed PAM legs 1000 / 1200, cash settlement.
    fn swaps01() -> ContractTerms {
        terms(json!({
            "contractType": "SWAPS",
            "contractID": "swaps01",
            "contractRole": "RFL",
            "currency": "USD",
            "contractDealDate": "2012-12-28T00:00:00",
            "statusDate": "2012-12-30T00:00:00",
            "deliverySettlement": "D",
            "contractStructure": [
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "swaps01-leg1",
                        "contractDealDate": "2012-12-28T00:00:00",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "currency": "USD",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1000",
                        "dayCountConvention": "A365",
                        "nominalInterestRate": "0.1",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P1ML1",
                        "premiumDiscountAtIED": "0"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "FIL"
                },
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "swaps01-leg2",
                        "contractDealDate": "2012-12-28T00:00:00",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "currency": "USD",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1200",
                        "dayCountConvention": "A365",
                        "nominalInterestRate": "0.1",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P3ML1",
                        "premiumDiscountAtIED": "0"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "SEL"
                }
            ]
        }))
    }

    #[test]
    fn receive_first_leg_orients_first_leg_positive_and_second_negative() {
        let events = SwapsEngine.evaluate(&swaps01(), &risk()).expect("events");
        let ieds: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::InitialExchange)
            .collect();
        assert_eq!(ieds.len(), 2);
        assert_eq!(ieds[0].payoff, dec!(-1000));
        assert_eq!(ieds[0].state.notional_principal, dec!(1000));
        assert_eq!(ieds[1].payoff, dec!(1200));
        assert_eq!(ieds[1].state.notional_principal, dec!(-1200));
        let mds: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::Maturity)
            .collect();
        assert_eq!(mds[0].payoff, dec!(1000));
        assert_eq!(mds[1].payoff, dec!(-1200));
    }

    #[test]
    fn pay_first_leg_flips_the_orientation() {
        let mut terms = swaps01();
        terms.contract_role = Some(ContractRole::Pfl);
        let events = SwapsEngine.evaluate(&terms, &risk()).expect("events");
        let ieds: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::InitialExchange)
            .collect();
        assert_eq!(ieds[0].payoff, dec!(1000));
        assert_eq!(ieds[0].state.notional_principal, dec!(-1000));
        assert_eq!(ieds[1].payoff, dec!(-1200));
        assert_eq!(ieds[1].state.notional_principal, dec!(1200));
    }

    #[test]
    fn cash_settlement_keeps_congruent_leg_events_separate() {
        let events = SwapsEngine.evaluate(&swaps01(), &risk()).expect("events");
        assert_eq!(events.len(), 22);
        let april: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.time == t("2013-04-01T00:00:00"))
            .collect();
        assert_eq!(april.len(), 2);
        assert_eq!(april[0].event_type, EventType::InterestPayment);
        assert_close(april[0].payoff, dec!(8.49315068493151));
        assert_eq!(april[0].state.notional_principal, dec!(1000));
        assert_close(april[1].payoff, dec!(-29.5890410958904));
        assert_eq!(april[1].state.notional_principal, dec!(-1200));
    }

    #[test]
    fn leg_rate_resets_resolve_against_the_shared_risk_factors() {
        let raw = json!({
            "contractType": "SWAPS",
            "contractRole": "PFL",
            "currency": "USD",
            "contractDealDate": "2012-12-28T00:00:00",
            "statusDate": "2012-12-30T00:00:00",
            "deliverySettlement": "D",
            "contractStructure": [
                {
                    "object": {
                        "contractType": "PAM",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1000",
                        "dayCountConvention": "A365",
                        "nominalInterestRate": "0.1",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P1ML1"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "FIL"
                },
                {
                    "object": {
                        "contractType": "PAM",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1200",
                        "dayCountConvention": "A365",
                        "nominalInterestRate": "0.1",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P1ML1",
                        "cycleAnchorDateOfRateReset": "2013-02-01T00:00:00",
                        "cycleOfRateReset": "P3ML1",
                        "marketObjectCodeOfRateReset": "US_TREASURY",
                        "rateSpread": "0.01"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "SEL"
                }
            ]
        });
        let terms = terms(raw);
        let observed = StateProvider::new().with_rate(
            "US_TREASURY",
            Utc.timestamp_opt(1359676800, 0).unwrap().naive_utc(),
            dec!(0.05),
        );
        let events = SwapsEngine.evaluate(&terms, &observed).expect("events");
        let resets: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::RateResetVariable)
            .collect();
        assert_eq!(resets.len(), 4);
        assert_eq!(
            resets[0].state.nominal_interest_rate,
            dec!(0.05) + dec!(0.01)
        );
    }

    #[test]
    fn single_settlement_merges_congruent_events_into_aggregates() {
        let mut terms = swaps01();
        terms.delivery_settlement = Some(DeliverySettlement::S);
        let events = SwapsEngine.evaluate(&terms, &risk()).expect("events");
        assert_eq!(events.len(), 15);
        let ied = events
            .iter()
            .find(|e| e.event_type == EventType::InitialExchange)
            .expect("merged IED");
        assert_eq!(ied.time, t("2013-01-01T00:00:00"));
        assert_eq!(ied.payoff, dec!(200));
        assert_eq!(ied.state.notional_principal, dec!(-200));
        assert_eq!(ied.state.nominal_interest_rate, Decimal::ZERO);
        let april = events
            .iter()
            .find(|e| e.time == t("2013-04-01T00:00:00"))
            .expect("merged April IP");
        assert_close(april.payoff, dec!(-21.0958904109589));
        assert_eq!(april.state.notional_principal, dec!(-200));
        assert_eq!(april.state.nominal_interest_rate, Decimal::ZERO);
        let february = events
            .iter()
            .find(|e| e.time == t("2013-02-01T00:00:00"))
            .expect("unmerged February IP");
        assert_close(february.payoff, dec!(8.49315068493151));
        assert_eq!(february.state.notional_principal, dec!(1000));
        assert_eq!(february.state.nominal_interest_rate, dec!(0.1));
        let maturity = events.last().expect("merged MD");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.payoff, dec!(-200));
        assert_eq!(maturity.state.notional_principal, Decimal::ZERO);
    }

    #[test]
    fn parent_purchase_opens_the_observation_window_with_one_parent_event() {
        let mut terms = swaps01();
        terms.purchase_date = Some(t("2013-03-15T23:59:59"));
        terms.price_at_purchase_date = Some(dec!(1000));
        let events = SwapsEngine.evaluate(&terms, &risk()).expect("events");
        assert_eq!(events.first().expect("PRD").event_type, EventType::Purchase);
        let prd = events.first().expect("PRD");
        assert_eq!(prd.time, t("2013-03-16T00:00:00"));
        assert_eq!(prd.payoff, dec!(1000));
        assert_eq!(prd.state.notional_principal, Decimal::ZERO);
        assert_eq!(prd.state.nominal_interest_rate, Decimal::ZERO);
        assert_eq!(events.len(), 17);
        assert!(!events
            .iter()
            .any(|e| e.time < t("2013-03-16T00:00:00") && e.event_type != EventType::Purchase));
        let april: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.time == t("2013-04-01T00:00:00"))
            .collect();
        assert_eq!(april.len(), 2);
    }

    #[test]
    fn parent_termination_closes_the_observation_window() {
        let mut terms = swaps01();
        terms.termination_date = Some(t("2013-07-01T00:00:00"));
        terms.price_at_termination_date = Some(dec!(2000));
        let events = SwapsEngine.evaluate(&terms, &risk()).expect("events");
        assert_eq!(events.len(), 13);
        let last = events.last().expect("TD");
        assert_eq!(last.event_type, EventType::Termination);
        assert_eq!(last.time, t("2013-07-01T00:00:00"));
        assert_eq!(last.payoff, dec!(2000));
        assert_eq!(last.state.notional_principal, Decimal::ZERO);
        let july: Vec<EventType> = events
            .iter()
            .filter(|e| e.time == t("2013-07-01T00:00:00"))
            .map(|e| e.event_type)
            .collect();
        assert_eq!(
            july,
            vec![
                EventType::InterestPayment,
                EventType::InterestPayment,
                EventType::Termination
            ]
        );
        assert!(!events.iter().any(|e| e.event_type == EventType::Maturity));
    }

    #[test]
    fn missing_structure_reports_the_missing_attribute() {
        let terms = terms(json!({
            "contractType": "SWAPS",
            "contractRole": "RFL"
        }));
        let error = SwapsEngine.evaluate(&terms, &risk()).unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("contractStructure")
        ));
    }
}
