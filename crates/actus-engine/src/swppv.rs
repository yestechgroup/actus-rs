//! SWPPV: Plain Vanilla Interest Rate Swap (paper §7.12, sections 4
//! "Contract Composition" and 6 "Child Contract Observer").
//!
//! A plain vanilla swap exchanges fixed against floating interest on a
//! common notional over a common schedule. Two construction modes are
//! supported:
//!
//! - **Composed mode** (`contractStructure` set, paper §4): the legs are the
//!   `CTST^FirstLeg_Contract` (fixed) and `CTST^SecondLeg_Contract`
//!   (floating) references, evaluated through the child contract observer
//!   `U_ev` — each leg's event stream is produced by the leg's own contract
//!   engine with the parent's contract role propagated into the child
//!   evaluation (techspec Example 14), mirroring the SWAPS composition
//!   machinery.
//! - **Degenerate mode** (no `contractStructure`): a built-in two-leg
//!   construction directly from the parent terms. Both legs are synthetic
//!   PAM contracts on the parent's `IED`/`MD`/`NT`/`IPANX`/`IPCL` schedule;
//!   the fixed leg carries `IPNR`, the floating leg resets per the parent
//!   rate-reset schedule (`RRANX`/`RRCL`) — or, when the parent sets
//!   `RRMO` without a reset schedule, once per interest period — observed
//!   through `obs(rf, RRMO, t)` scaled by `RRMLT`/`RRSP`.
//!
//! **Net settlement** (paper §7.12 "IP Payoff: Net settlement of fixed vs
//! floating accrued interest"): unlike SWAPS in cash-settlement mode, SWPPV
//! always merges congruent events — leg events sharing time and event type
//! collapse into one net event summing the payoffs and notionals. Because a
//! plain vanilla swap exchanges no principal, congruent `IED`/`MD` legs net
//! to zero for equal leg notionals (the no-principal-exchange invariant the
//! tests assert); the net `IP` payoff is
//! `R(CNTRL) x NT x (IPNR_fixed - IPNR_float) x Y` per period.
//!
//! Parent role (dictionary Table 1): `RFL` (receive first/fixed leg, and an
//! unset role) carries `+1` and evaluates the first leg as `RPA` / second
//! leg as `RPL`; `PFL` (pay first leg) carries `-1` and flips the
//! assignment. Merged post-event states report the parent's own
//! interest-bearing states (the parent swap carries no accrued state of its
//! own, mirroring the SWAPS single-settlement behaviour).

use rust_decimal::Decimal;

use actus_model::{
    ContractReference, ContractReferenceRole, ContractRole, ContractTerms, ContractType,
    DayCountConvention,
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
use crate::EngineError;

/// Implementation of the SWPPV contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct SwppvEngine;

impl ContractEngine for SwppvEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Swppv
    }

    /// Evaluates both legs (composed or degenerate), applies the parent role
    /// orientation and returns the net-settled event stream.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let (first, second) = match &terms.contract_structure {
            Some(structure) => (
                clone_reference_leg(first_leg(structure)?)?,
                clone_reference_leg(second_leg(structure)?)?,
            ),
            None => (
                degenerate_leg(terms, false, risk)?,
                degenerate_leg(terms, true, risk)?,
            ),
        };
        let parent_sign = parent_role_sign(terms.contract_role)?;
        let mut stream = evaluate_leg(&first, leg_role(parent_sign, true), risk)?;
        stream.extend(evaluate_leg(&second, leg_role(parent_sign, false), risk)?);
        sort_events(&mut stream);
        let merged = merge_congruent(terms, stream);
        Ok(merged)
    }
}

/// The engines a child leg may route through (the SWPPV testbeds are
/// schedule-driven fixed income legs; the same registry as SWAPS).
fn leg_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(ClmEngine));
    registry
}

/// The first (fixed) leg reference: matched by the `FIL` role, else array
/// order (first entry = first leg), mirroring the SWAPS resolution.
fn first_leg(structure: &[ContractReference]) -> Result<&ContractReference, EngineError> {
    structure
        .iter()
        .find(|r| r.reference_role == Some(ContractReferenceRole::FirstLeg))
        .or_else(|| structure.first())
        .ok_or(EngineError::InvalidTransition(
            "contract structure needs a first leg reference".to_string(),
        ))
}

/// The second (floating) leg reference: matched by the `SEL` role, else the
/// second array entry, mirroring the SWAPS resolution.
fn second_leg(structure: &[ContractReference]) -> Result<&ContractReference, EngineError> {
    structure
        .iter()
        .find(|r| r.reference_role == Some(ContractReferenceRole::SecondLeg))
        .or_else(|| structure.get(1))
        .ok_or(EngineError::InvalidTransition(
            "contract structure needs a second leg reference".to_string(),
        ))
}

fn clone_reference_leg(reference: &ContractReference) -> Result<ContractTerms, EngineError> {
    reference
        .object
        .clone()
        .ok_or(EngineError::InvalidTransition(
            "contract reference without an embedded contract object".to_string(),
        ))
}

/// Evaluates one child leg under an effective contract role (techspec
/// Example 14): the leg engine signs its payoffs and notional states from
/// `CNTRL`, so `RPA` reproduces the parent's `+1` orientation and `RPL` its
/// `-1` orientation. Both legs observe the same risk factor observer.
fn evaluate_leg(
    leg_terms: &ContractTerms,
    role: ContractRole,
    risk: &dyn RiskFactorProvider,
) -> Result<Vec<ContractEvent>, EngineError> {
    let mut leg = leg_terms.clone();
    leg.contract_role = Some(role);
    leg_registry().evaluate(&leg, risk)
}

/// The role sign of the parent swap (dictionary Table 1): `RFL` receives the
/// first (fixed) leg (`+1`), `PFL` pays it (`-1`); an unset role defaults to
/// the receiving orientation.
fn parent_role_sign(role: Option<ContractRole>) -> Result<Decimal, EngineError> {
    match role {
        Some(ContractRole::Rfl) | None => Ok(Decimal::ONE),
        Some(ContractRole::Pfl) => Ok(-Decimal::ONE),
        Some(other) => Err(EngineError::InvalidTransition(format!(
            "unsupported SWPPV contract role {other}"
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

/// Builds one synthetic PAM leg of the degenerate two-leg construction.
///
/// Both legs share the parent's schedule attributes (`IED`, `MD`, `NT`,
/// `IPANX`, `IPCL`, day count and shift conventions). The floating leg
/// (`floating = true`) resets its rate: from the parent reset schedule
/// (`RRANX`/`RRCL`) when present, otherwise once per interest period when a
/// `RRMO` market object code is set; the initial floating rate is `RRNXT`,
/// else the rate observed at `IED`, else the parent `IPNR`. The fixed leg is
/// the plain `IPNR` leg. Because both legs carry the same notional, the
/// merged principal exchanges net to zero (no-principal-exchange
/// invariant).
fn degenerate_leg(
    terms: &ContractTerms,
    floating: bool,
    risk: &dyn RiskFactorProvider,
) -> Result<ContractTerms, EngineError> {
    let ied = terms
        .initial_exchange_date
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
    let maturity = terms
        .maturity_date
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute("maturityDate"))?;
    let notional = terms
        .notional_principal
        .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
    let fixed_rate = terms
        .nominal_interest_rate
        .ok_or(EngineError::MissingAttribute("nominalInterestRate"))?;
    let ipanx = terms
        .cycle_anchor_date_of_interest_payment
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute(
            "cycleAnchorDateOfInterestPayment",
        ))?;
    let ipcl = terms
        .cycle_of_interest_payment
        .as_ref()
        .ok_or(EngineError::MissingAttribute("cycleOfInterestPayment"))?;

    let mut leg = ContractTerms::new(actus_model::ContractType::Pam);
    leg.contract_id = terms.contract_id.clone();
    leg.status_date = terms.status_date;
    leg.contract_deal_date = terms.contract_deal_date;
    leg.initial_exchange_date = Some(ied);
    leg.maturity_date = Some(maturity);
    leg.notional_principal = Some(notional);
    leg.currency = terms.currency.clone();
    leg.cycle_anchor_date_of_interest_payment = Some(ipanx);
    leg.cycle_of_interest_payment = Some(*ipcl);
    leg.day_count_convention = Some(
        terms
            .day_count_convention
            .unwrap_or(DayCountConvention::A365),
    );
    leg.end_of_month_convention = terms.end_of_month_convention;
    leg.business_day_convention = terms.business_day_convention;
    leg.calendar = terms.calendar;

    if !floating {
        leg.nominal_interest_rate = Some(fixed_rate);
        return Ok(leg);
    }

    let code = terms.market_object_code_of_rate_reset.clone();
    let initial = match terms.next_reset_rate {
        Some(nrr) => nrr,
        None => match code.as_deref().and_then(|c| risk.rate(c, ied)) {
            Some(observed) => observed,
            None => fixed_rate,
        },
    };
    leg.nominal_interest_rate = Some(initial);
    match (
        terms
            .cycle_anchor_date_of_rate_reset
            .map(normalize_timestamp),
        terms.cycle_of_rate_reset.as_ref(),
    ) {
        (Some(anchor), Some(cycle)) => {
            leg.cycle_anchor_date_of_rate_reset = Some(anchor);
            leg.cycle_of_rate_reset = Some(*cycle);
        }
        _ if code.is_some() => {
            // No explicit reset schedule: reset once per interest period.
            leg.cycle_anchor_date_of_rate_reset = Some(ied);
            leg.cycle_of_rate_reset = Some(*ipcl);
        }
        _ => {}
    }
    leg.market_object_code_of_rate_reset = code;
    leg.rate_multiplier = terms.rate_multiplier;
    leg.rate_spread = terms.rate_spread;
    Ok(leg)
}

/// Merges congruent events into net-settled events (paper §7.12).
///
/// `stream` arrives sorted, so congruent events (same time and event type)
/// are adjacent and one fold pass suffices. The net payoff and notional sum
/// the leg values; the interest-bearing states collapse to the parent's own
/// values (`IPNR` of the parent is the fixed rate, `IPAC` its terms value,
/// typically unset). Non-congruent events pass through untouched.
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
    use crate::state::ContractStatus;
    use actus_model::EventType;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use std::str::FromStr;

    use DayCountConvention as Dcc;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The degenerate golden: 1Y, four quarterly IP periods, NT 10M, fixed
    /// 3% vs flat float observations 2.5%, 30E360 (each quarter is exactly
    /// 0.25) — net +12,500 per period for the RFL parent.
    fn golden() -> ContractTerms {
        terms(json!({
            "contractType": "SWPPV",
            "contractID": "swppv01",
            "contractRole": "RFL",
            "currency": "USD",
            "statusDate": "2025-01-01T00:00:00",
            "contractDealDate": "2025-01-01T00:00:00",
            "initialExchangeDate": "2025-01-01T00:00:00",
            "maturityDate": "2026-01-01T00:00:00",
            "notionalPrincipal": "10000000",
            "nominalInterestRate": "0.03",
            "cycleAnchorDateOfInterestPayment": "2025-01-01T00:00:00",
            "cycleOfInterestPayment": "P3ML0",
            "dayCountConvention": "30E360",
            "marketObjectCodeOfRateReset": "USD-SOFR"
        }))
    }

    fn flat_float_risk() -> StateProvider {
        StateProvider::new().with_rate("USD-SOFR", t("2025-01-01T00:00:00"), dec!(0.025))
    }

    #[test]
    fn golden_nets_twelve_thousand_five_hundred_per_quarter() {
        let events = SwppvEngine
            .evaluate(&golden(), &flat_float_risk())
            .expect("events");
        let ips: Vec<(NaiveDateTime, Decimal)> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| (e.time, e.payoff))
            .collect();
        assert_eq!(
            ips,
            vec![
                (t("2025-01-01T00:00:00"), Decimal::ZERO),
                (t("2025-04-01T00:00:00"), dec!(12500)),
                (t("2025-07-01T00:00:00"), dec!(12500)),
                (t("2025-10-01T00:00:00"), dec!(12500)),
                (t("2026-01-01T00:00:00"), dec!(12500)),
            ]
        );
        // IED marker + the five net IPs (the IED IP nets to zero) + the four
        // zero-payoff floating-leg rate resets + MD marker.
        assert_eq!(events.len(), 11);
        assert!(events
            .iter()
            .filter(|e| e.event_type == EventType::RateResetVariable)
            .all(|e| e.payoff.is_zero()));
    }

    #[test]
    fn no_principal_exchange_invariant_holds() {
        let events = SwppvEngine
            .evaluate(&golden(), &flat_float_risk())
            .expect("events");
        for event in &events {
            if event.event_type != EventType::InterestPayment {
                assert_eq!(
                    event.payoff,
                    Decimal::ZERO,
                    "non-IP event {} at {}",
                    event.event_type,
                    event.time
                );
            }
        }
        let ied = events
            .iter()
            .find(|e| e.event_type == EventType::InitialExchange)
            .expect("IED");
        assert_eq!(ied.time, t("2025-01-01T00:00:00"));
        let md = events.last().expect("MD");
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.state.contract_status, ContractStatus::Matured);
    }

    #[test]
    fn pay_fixed_orientation_flips_the_net_payoff() {
        let mut swap = golden();
        swap.contract_role = Some(ContractRole::Pfl);
        let events = SwppvEngine
            .evaluate(&swap, &flat_float_risk())
            .expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        assert_eq!(
            ips,
            vec![
                Decimal::ZERO,
                dec!(-12500),
                dec!(-12500),
                dec!(-12500),
                dec!(-12500)
            ]
        );
    }

    #[test]
    fn unobserved_float_rate_reports_the_risk_factor() {
        let error = SwppvEngine
            .evaluate(&golden(), &StateProvider::new())
            .unwrap_err();
        assert!(matches!(error, EngineError::RiskFactorMissing { .. }));
    }

    #[test]
    fn composed_structure_nets_congruent_leg_events() {
        // Two PAM legs, equal notionals 1000, quarterly 30E360, fixed rates
        // 10% vs 4% -> net +15 per quarter for the RFL parent; the principal
        // exchanges cancel.
        let raw = json!({
            "contractType": "SWPPV",
            "contractRole": "RFL",
            "currency": "USD",
            "statusDate": "2012-12-30T00:00:00",
            "contractStructure": [
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "leg1",
                        "contractRole": "RPA",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1000",
                        "nominalInterestRate": "0.1",
                        "dayCountConvention": "30E360",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P3ML0"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "FIL"
                },
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "leg2",
                        "contractRole": "RPA",
                        "initialExchangeDate": "2013-01-01T00:00:00",
                        "statusDate": "2012-12-30T00:00:00",
                        "notionalPrincipal": "1000",
                        "nominalInterestRate": "0.04",
                        "dayCountConvention": "30E360",
                        "maturityDate": "2014-01-01T00:00:00",
                        "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
                        "cycleOfInterestPayment": "P3ML0"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "SEL"
                }
            ]
        });
        let events = SwppvEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        assert_eq!(
            ips,
            vec![Decimal::ZERO, dec!(15), dec!(15), dec!(15), dec!(15)]
        );
        for event in &events {
            if event.event_type != EventType::InterestPayment {
                assert_eq!(event.payoff, Decimal::ZERO);
            }
        }
        // IED (net 0), 5 net IPs, MD (net 0).
        assert_eq!(events.len(), 7);
    }

    #[test]
    fn missing_structure_and_attributes_are_reported() {
        let error = SwppvEngine
            .evaluate(
                &terms(json!({"contractType": "SWPPV", "contractRole": "RFL"})),
                &StateProvider::new(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("initialExchangeDate")
        ));

        let error = SwppvEngine
            .evaluate(
                &terms(json!({
                    "contractType": "SWPPV",
                    "contractRole": "RPA",
                    "initialExchangeDate": "2025-01-01T00:00:00"
                })),
                &StateProvider::new(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("maturityDate")
        ));
    }

    #[test]
    fn unsupported_parent_role_is_reported() {
        let mut swap = golden();
        swap.contract_role = Some(ContractRole::Rpa);
        let error = SwppvEngine.evaluate(&swap, &flat_float_risk()).unwrap_err();
        assert!(matches!(error, EngineError::InvalidTransition(_)));
    }

    #[test]
    fn degenerate_float_leg_defaults_to_the_fixed_rate_without_market_object() {
        let mut swap = golden();
        swap.market_object_code_of_rate_reset = None;
        swap.next_reset_rate = Some(dec!(0.02));
        let events = SwppvEngine
            .evaluate(&swap, &StateProvider::new())
            .expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        // fixed 3% vs float 2%: 10M x 0.01 x 0.25 = 25,000 per quarter.
        assert_eq!(
            ips,
            vec![
                Decimal::ZERO,
                dec!(25000),
                dec!(25000),
                dec!(25000),
                dec!(25000)
            ]
        );
    }

    /// Documents the day count choice of the golden test: 30E360 makes each
    /// quarterly period exactly 0.25.
    #[test]
    fn golden_quarters_are_exact_quarters() {
        let y = crate::daycount::day_count_fraction(
            t("2025-01-01T00:00:00"),
            t("2025-04-01T00:00:00"),
            Dcc::from_str("30E360").unwrap(),
        )
        .unwrap();
        assert_eq!(y, dec!(0.25));
    }
}
