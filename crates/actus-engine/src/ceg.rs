//! CEG: Credit Enhancement Guarantee (paper §7.17, sections 5 "Risk Factor
//! Observer", 6 "Child Contract Observer"; mirrors the CEC engine's
//! credit-event and child-observer machinery).
//!
//! A guarantee covers one or more covered contracts (`CTST^CoveredContract`
//! references — `COVE`-tagged, else every embedded `CNT` reference) and is
//! triggered by credit events observed on them. The engine emits two event
//! groups:
//!
//! - **Guarantee notional / protection** (dictionary Table 1 `BUY`/`SEL`,
//!   `R(CNTRL)`): the guaranteed exposure is
//!   `CECVR x sum(n_i)` over the covered contracts, with the per-contract
//!   exposure `n_i` resolved per `CEGE` from the child's own event stream
//!   (child contract observer): the notional state for `NO`, notional plus
//!   the interest accrued since the child's last event for `NI`, and the
//!   child's own market value observation for `MV`. A qualifying observed
//!   credit event (matching covered contract identifier, contract
//!   performance equal to `creditEventTypeCovered` `CETC`, default `DF`,
//!   at or before maturity) emits an `XD` exercise at the observation time
//!   (zero payoff, `Nt = sgn x exposure` state) and a `STD` settlement at
//!   the credit event time plus the `settlementPeriod` `STP` (default
//!   `P0D`), shifted per the business day convention, paying
//!   `sgn x exposure`. After a settlement the guarantee is terminated (no
//!   further events).
//! - **Guarantee fee** (`FP` events on the fee schedule `FEANX`/`FECL`):
//!   per `FEB`, the nominal basis `N` (default) pays
//!   `R(CNTRL) x FER x NT x Y` with `Y` the day count fraction between the
//!   previous and current fee schedule point (the anchor point itself
//!   accrues zero), and the absolute basis `A` pays `R(CNTRL) x FER`. The
//!   notional `NT` is re-aggregated from the covered children at every fee
//!   date, so amortising exposures pay declining fees. Without a trigger,
//!   the fee schedule runs to maturity and a zero-payoff `MD` closes the
//!   stream; with a trigger, fees at or before the credit event time remain.
//!
//! Contract role sign: the protection seller (`SEL`) carries `-1`, every
//! other role defaults to the buying orientation `+1` (mirroring CEC).

use chrono::{Duration, NaiveDateTime};
use rust_decimal::Decimal;

use actus_model::enums::FeeBasis;
use actus_model::{
    BusinessDayConvention, Calendar, ContractReference, ContractReferenceRole,
    ContractReferenceType, ContractRole, ContractTerms, ContractType, CreditEventType, Cycle,
    CyclePeriod, CycleStub, DayCountConvention, EndOfMonthConvention, EventType,
    GuaranteedExposure,
};

use crate::ann::AnnEngine;
use crate::clm::ClmEngine;
use crate::common::stub_series;
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::{ContractEngine, EngineRegistry};
use crate::event::{sort_events, ContractEvent};
use crate::lam::LamEngine;
use crate::nam::NamEngine;
use crate::pam::PamEngine;
use crate::risk::{ObservedCreditEvent, RiskFactorProvider};
use crate::schedule::{cycle_step, shift_business_day};
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the CEG contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CegEngine;

impl ContractEngine for CegEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Ceg
    }

    /// Evaluates the fee schedule and the credit-event protection.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let structure = terms
            .contract_structure
            .as_deref()
            .ok_or(EngineError::MissingAttribute("contractStructure"))?;
        let covered = covered_contracts(structure);
        if covered.is_empty() {
            return Err(EngineError::InvalidTransition(
                "contract structure needs a covered contract reference".to_string(),
            ));
        }
        let sign = ceg_role_sign(terms.contract_role);
        let maturity = maturity_date(terms, &covered)?;
        let trigger = credit_event_trigger(
            risk,
            &covered,
            terms
                .credit_event_type_covered
                .unwrap_or(CreditEventType::Default),
            maturity,
        );

        let mut events = Vec::new();
        let fee_cutoff = trigger.as_ref().map(|t| t.time).unwrap_or(maturity);
        for (time, notional, fee) in fee_schedule(terms, &covered, maturity, risk)? {
            if time > fee_cutoff {
                continue;
            }
            events.push(ContractEvent {
                event_type: EventType::FeePayment,
                time,
                payoff: sign * fee,
                currency: terms.currency.clone(),
                state: event_state(sign * notional, time),
            });
        }

        match trigger {
            Some(credit_event) => {
                let coverage = terms.coverage_of_credit_enhancement.unwrap_or(Decimal::ONE);
                let exposure = coverage
                    * covered_exposure(
                        &covered,
                        credit_event.time,
                        terms.guaranteed_exposure,
                        risk,
                    )?;
                let settlement_time = settlement_date(terms, credit_event.time)?;
                events.push(ContractEvent {
                    event_type: EventType::Exercise,
                    time: credit_event.time,
                    payoff: Decimal::ZERO,
                    currency: terms.currency.clone(),
                    state: event_state(sign * exposure, credit_event.time),
                });
                events.push(ContractEvent {
                    event_type: EventType::Settlement,
                    time: settlement_time,
                    payoff: sign * exposure,
                    currency: terms.currency.clone(),
                    state: terminal_state(
                        Decimal::ZERO,
                        settlement_time,
                        ContractStatus::Terminated,
                    ),
                });
            }
            None => {
                events.push(ContractEvent {
                    event_type: EventType::Maturity,
                    time: maturity,
                    payoff: Decimal::ZERO,
                    currency: terms.currency.clone(),
                    state: terminal_state(Decimal::ZERO, maturity, ContractStatus::Matured),
                });
            }
        }
        sort_events(&mut events);
        Ok(events)
    }
}

/// The engines a covered child may route through (the CEC registry).
fn child_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(ClmEngine));
    registry
}

/// The covered contract references: `COVE`-tagged entries, else every
/// embedded `CNT` reference.
fn covered_contracts(structure: &[ContractReference]) -> Vec<&ContractReference> {
    let tagged: Vec<&ContractReference> = structure
        .iter()
        .filter(|r| r.reference_role == Some(ContractReferenceRole::CoveredContract))
        .collect();
    if !tagged.is_empty() {
        return tagged;
    }
    structure
        .iter()
        .filter(|r| r.reference_type == Some(ContractReferenceType::Contract) && r.object.is_some())
        .collect()
}

/// The contract role sign of the CEG parent (dictionary Table 1): the
/// protection seller (`SEL`) carries `-1`, other roles default to `+1`.
fn ceg_role_sign(role: Option<ContractRole>) -> Decimal {
    match role {
        Some(ContractRole::Sel) => -Decimal::ONE,
        _ => Decimal::ONE,
    }
}

/// The CEG maturity: the parent `MD` term when set, else the latest covered
/// contract maturity (mirroring CEC).
fn maturity_date(
    terms: &ContractTerms,
    covered: &[&ContractReference],
) -> Result<NaiveDateTime, EngineError> {
    if let Some(maturity) = terms.maturity_date {
        return Ok(normalize_timestamp(maturity));
    }
    covered
        .iter()
        .filter_map(|reference| {
            reference
                .object
                .as_ref()
                .and_then(|object| object.maturity_date)
        })
        .map(normalize_timestamp)
        .max()
        .ok_or(EngineError::MissingAttribute("maturityDate"))
}

/// The first observed credit event that qualifies as the trigger (mirroring
/// CEC): names a covered contract, carries the covered performance state
/// (`CETC`) and occurs at or before maturity.
fn credit_event_trigger(
    risk: &dyn RiskFactorProvider,
    covered: &[&ContractReference],
    covered_type: CreditEventType,
    maturity: NaiveDateTime,
) -> Option<ObservedCreditEvent> {
    let covered_ids: Vec<Option<&String>> = covered
        .iter()
        .map(|reference| {
            reference
                .object
                .as_ref()
                .and_then(|object| object.contract_id.as_ref())
        })
        .collect();
    risk.observed_credit_events()
        .into_iter()
        .filter(|event| event.time <= maturity)
        .filter(|event| covered_ids.contains(&event.contract_id.as_ref()))
        .filter(|event| event.performance.map(|p| p.as_token()) == Some(covered_type.as_token()))
        .min_by_key(|event| event.time)
}

/// The aggregated guaranteed exposure `sum(n_i)` at `at`, before coverage.
fn covered_exposure(
    covered: &[&ContractReference],
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    covered
        .iter()
        .map(|reference| covered_contract_value(reference, at, basis, risk))
        .sum()
}

/// The guaranteed value `n_i` of one covered contract at `at` (mirroring
/// CEC): the child's notional state for `NO`, notional plus the projected
/// accrual for `NI`, the child's own market value observation for `MV`.
fn covered_contract_value(
    reference: &ContractReference,
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    let child_terms = reference
        .object
        .clone()
        .ok_or(EngineError::InvalidTransition(
            "covered contract reference without an embedded contract object".to_string(),
        ))?;
    if basis == Some(GuaranteedExposure::MarketValue) {
        let code = child_terms
            .market_object_code
            .clone()
            .ok_or(EngineError::MissingAttribute("marketObjectCode"))?;
        return risk.index(&code, at).ok_or(EngineError::RiskFactorMissing {
            code,
            at: at.to_string(),
        });
    }
    let events = child_registry().evaluate(&child_terms, risk)?;
    let state = events
        .iter()
        .rev()
        .find(|event| event.time <= at)
        .map(|event| event.state.clone())
        .unwrap_or_else(|| ContractState::initial(&child_terms));
    match basis {
        Some(GuaranteedExposure::NominalValuePlusInterest) => {
            Ok(state.notional_principal + projected_accrued(&state, at, &child_terms)?)
        }
        _ => Ok(state.notional_principal),
    }
}

/// The interest accrued from the state's status date to `at` (mirroring the
/// accrual transition `IPAC(t+) = IPAC(t-) + YF(SD(t-), t) x IPNR(t-) x
/// base(t-)`; dictionary default convention `A365`).
fn projected_accrued(
    state: &ContractState,
    at: NaiveDateTime,
    child_terms: &ContractTerms,
) -> Result<Decimal, EngineError> {
    let convention = child_terms
        .day_count_convention
        .unwrap_or(DayCountConvention::A365);
    let year_fraction = day_count_fraction(state.status_date, at, convention)?;
    let base = if state.interest_calculation_base_amount.is_zero() {
        state.notional_principal
    } else {
        state.interest_calculation_base_amount
    };
    Ok(state.accrued_interest + year_fraction * state.nominal_interest_rate * base)
}

/// The fee schedule `(emission time, guaranteed notional, fee amount)`
/// triplets (techspec `FP`: `R(CNTRL) x FER x NT x Y` per `FEB`).
///
/// The schedule unrolls from `FEANX` per `FECL` to maturity; `Y` is the day
/// count fraction from the previous schedule point (the anchor accrues
/// zero), and the notional is re-aggregated from the covered children at
/// every fee date. `FEB = A` pays the absolute `FER`; `FEB = N` (default)
/// pays `FER x NT x Y`.
fn fee_schedule(
    terms: &ContractTerms,
    covered: &[&ContractReference],
    maturity: NaiveDateTime,
    risk: &dyn RiskFactorProvider,
) -> Result<Vec<(NaiveDateTime, Decimal, Decimal)>, EngineError> {
    let fee_rate = match terms.fee_rate {
        Some(rate) => rate,
        None => return Ok(Vec::new()),
    };
    let (anchor, cycle) = match (
        terms.cycle_anchor_date_of_fee.map(normalize_timestamp),
        terms.cycle_of_fee.as_ref(),
    ) {
        (Some(anchor), Some(cycle)) => (anchor, cycle),
        _ => return Ok(Vec::new()),
    };
    let eomc = terms
        .end_of_month_convention
        .unwrap_or(EndOfMonthConvention::Sd);
    let bdc = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let cal = terms.calendar.unwrap_or(Calendar::Nc);
    let dcc = terms
        .day_count_convention
        .unwrap_or(DayCountConvention::A365);
    let basis = terms.fee_basis;

    let mut schedule = Vec::new();
    let mut previous = anchor;
    for (calc, emit) in stub_series(anchor, cycle, maturity, eomc, bdc, cal) {
        let notional = covered_exposure(covered, emit, terms.guaranteed_exposure, risk)?;
        let year_fraction = day_count_fraction(previous, calc, dcc)?;
        let fee = match basis {
            Some(FeeBasis::AbsoluteValue) => fee_rate,
            _ => fee_rate * notional * year_fraction,
        };
        schedule.push((emit, notional, fee));
        previous = calc;
    }
    Ok(schedule)
}

/// The settlement date: the credit event time plus `settlementPeriod`
/// (`STP`, default `P0D`), shifted per the business day convention
/// (mirroring CEC).
fn settlement_date(
    terms: &ContractTerms,
    event_time: NaiveDateTime,
) -> Result<NaiveDateTime, EngineError> {
    let unshifted = match terms.settlement_period.as_deref() {
        None => event_time,
        Some(raw) => add_period(event_time, raw, terms)?,
    };
    let convention = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let calendar = terms.calendar.unwrap_or(Calendar::Nc);
    Ok(shift_business_day(unshifted, convention, calendar))
}

/// Adds an ISO 8601 period (`P<n>D|W|M|Y`) to a timestamp (mirroring CEC).
fn add_period(
    at: NaiveDateTime,
    raw: &str,
    terms: &ContractTerms,
) -> Result<NaiveDateTime, EngineError> {
    let upper = raw.trim().to_ascii_uppercase();
    let invalid = || EngineError::InvalidTransition(format!("invalid settlementPeriod: {raw}"));
    let body = upper.strip_prefix('P').ok_or_else(invalid)?;
    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
    let period = &body[digits.len()..];
    let n: i64 = digits.parse().map_err(|_| invalid())?;
    match period {
        "D" => at
            .checked_add_signed(Duration::days(n))
            .ok_or_else(|| EngineError::InvalidTransition("settlementPeriod overflow".into())),
        "W" => at
            .checked_add_signed(Duration::weeks(n))
            .ok_or_else(|| EngineError::InvalidTransition("settlementPeriod overflow".into())),
        "M" | "Y" => {
            let cycle_period = if period == "M" {
                CyclePeriod::Month
            } else {
                CyclePeriod::Year
            };
            let cycle = Cycle::new(n.max(1) as u32, cycle_period, CycleStub::Short, 0)
                .map_err(|e| EngineError::InvalidTransition(e.to_string()))?;
            let eomc = terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd);
            Ok(cycle_step(at, 1, &cycle, eomc))
        }
        _ => Err(invalid()),
    }
}

/// Builds a post-event state carrying the given notional at the event time.
fn event_state(notional: Decimal, time: NaiveDateTime) -> ContractState {
    ContractState {
        notional_principal: notional,
        status_date: time,
        ..ContractState::default()
    }
}

/// Builds a terminal post-event state with an explicit lifetime status.
fn terminal_state(notional: Decimal, time: NaiveDateTime, status: ContractStatus) -> ContractState {
    ContractState {
        contract_status: status,
        ..event_state(notional, time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use actus_model::ContractPerformance;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn close(actual: Decimal, expected: Decimal) {
        let diff = (actual - expected).abs();
        assert!(
            diff < Decimal::new(1, 6),
            "expected {expected}, actual {actual}"
        );
    }

    /// A covered bullet PAM of 1M, quarterly 30E360 interest.
    fn covered_bond(id: &str) -> serde_json::Value {
        json!({
            "object": {
                "contractType": "PAM",
                "contractID": id,
                "statusDate": "2025-01-01T00:00:00",
                "contractDealDate": "2025-01-01T00:00:00",
                "currency": "USD",
                "notionalPrincipal": "1000000",
                "initialExchangeDate": "2025-01-01T00:00:00",
                "maturityDate": "2026-01-01T00:00:00",
                "nominalInterestRate": "0.03",
                "cycleAnchorDateOfInterestPayment": "2025-01-01T00:00:00",
                "cycleOfInterestPayment": "P3ML0",
                "dayCountConvention": "30E360",
                "contractRole": "RPA"
            },
            "referenceType": "CNT",
            "referenceRole": "COVE"
        })
    }

    /// The golden guarantee over two 1M bullets: coverage 1.0, `NO` basis,
    /// 0.5% quarterly nominal fee.
    fn golden() -> ContractTerms {
        terms(json!({
            "contractType": "CEG",
            "contractID": "ceg01",
            "contractRole": "BUY",
            "currency": "USD",
            "statusDate": "2025-01-01T00:00:00",
            "maturityDate": "2026-01-01T00:00:00",
            "coverageOfCreditEnhancement": "1.0",
            "guaranteedExposure": "NO",
            "creditEventTypeCovered": "DF",
            "feeRate": "0.005",
            "feeBasis": "N",
            "cycleAnchorDateOfFee": "2025-01-01T00:00:00",
            "cycleOfFee": "P3ML0",
            "settlementPeriod": "P0D",
            "dayCountConvention": "30E360",
            "contractStructure": [covered_bond("loan01"), covered_bond("loan02")]
        }))
    }

    fn risk_with(events: &[ObservedCreditEvent]) -> StateProvider {
        let mut risk = StateProvider::new();
        for event in events {
            risk = risk.with_observed_credit_event(event.clone());
        }
        risk
    }

    fn credit_event(id: &str, performance: ContractPerformance) -> ObservedCreditEvent {
        ObservedCreditEvent {
            time: t("2025-09-20T00:00:00"),
            contract_id: Some(id.to_string()),
            performance: Some(performance),
        }
    }

    #[test]
    fn fee_schedule_pays_quarterly_nominal_fees_without_trigger() {
        let events = CegEngine
            .evaluate(&golden(), &risk_with(&[]))
            .expect("events");
        let fees: Vec<(NaiveDateTime, Decimal)> = events
            .iter()
            .filter(|e| e.event_type == EventType::FeePayment)
            .map(|e| (e.time, e.payoff))
            .collect();
        // 0.5% x 2M x 0.25 = 2,500 per quarter; the anchor accrues zero and
        // the maturity-date fee sees the matured (zero) children.
        assert_eq!(
            fees,
            vec![
                (t("2025-01-01T00:00:00"), Decimal::ZERO),
                (t("2025-04-01T00:00:00"), dec!(2500)),
                (t("2025-07-01T00:00:00"), dec!(2500)),
                (t("2025-10-01T00:00:00"), dec!(2500)),
                (t("2026-01-01T00:00:00"), Decimal::ZERO),
            ]
        );
        let md = events.last().expect("MD");
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.payoff, Decimal::ZERO);
        assert_eq!(md.state.contract_status, ContractStatus::Matured);
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn credit_event_triggers_exercise_and_protection_settlement() {
        let events = CegEngine
            .evaluate(
                &golden(),
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        assert_eq!(events.len(), 5);
        let xd = events
            .iter()
            .find(|e| e.event_type == EventType::Exercise)
            .expect("XD");
        assert_eq!(xd.time, t("2025-09-20T00:00:00"));
        assert_eq!(xd.payoff, Decimal::ZERO);
        assert_eq!(xd.state.notional_principal, dec!(2000000));
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        assert_eq!(std.time, t("2025-09-20T00:00:00"));
        assert_eq!(std.payoff, dec!(2000000));
        assert_eq!(std.state.contract_status, ContractStatus::Terminated);
        // Fees at or before the trigger only; no maturity event.
        let fee_times: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::FeePayment)
            .map(|e| e.time)
            .collect();
        assert_eq!(
            fee_times,
            vec![
                t("2025-01-01T00:00:00"),
                t("2025-04-01T00:00:00"),
                t("2025-07-01T00:00:00"),
            ]
        );
        assert!(!events.iter().any(|e| e.event_type == EventType::Maturity));
    }

    #[test]
    fn nominal_plus_interest_basis_projects_the_child_accruals() {
        let mut guarantee = golden();
        guarantee.guaranteed_exposure = Some(GuaranteedExposure::NominalValuePlusInterest);
        let events = CegEngine
            .evaluate(
                &guarantee,
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        // 2M plus 2 x 0.03 x (79/360) x 1M accrued since the July IP.
        close(
            std.payoff,
            dec!(2000000) + dec!(2) * dec!(0.03) * dec!(79) / dec!(360) * dec!(1000000),
        );
    }

    #[test]
    fn seller_role_flips_fees_and_protection() {
        let mut guarantee = golden();
        guarantee.contract_role = Some(ContractRole::Sel);
        let events = CegEngine
            .evaluate(
                &guarantee,
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        let fee = events
            .iter()
            .find(|e| e.event_type == EventType::FeePayment && e.payoff != Decimal::ZERO)
            .expect("fee");
        assert_eq!(fee.payoff, dec!(-2500));
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        assert_eq!(std.payoff, dec!(-2000000));
    }

    #[test]
    fn coverage_ratio_scales_the_protection() {
        let mut guarantee = golden();
        guarantee.coverage_of_credit_enhancement = Some(dec!(0.5));
        let events = CegEngine
            .evaluate(
                &guarantee,
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        assert_eq!(std.payoff, dec!(1000000));
    }

    #[test]
    fn settlement_period_delays_the_settlement() {
        let mut guarantee = golden();
        guarantee.settlement_period = Some("P5D".to_string());
        let events = CegEngine
            .evaluate(
                &guarantee,
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        assert_eq!(std.time, t("2025-09-25T00:00:00"));
    }

    #[test]
    fn non_qualifying_credit_events_do_not_trigger() {
        // Wrong performance state.
        let events = CegEngine
            .evaluate(
                &golden(),
                &risk_with(&[credit_event("loan01", ContractPerformance::Delinquent)]),
            )
            .expect("events");
        assert!(events.iter().any(|e| e.event_type == EventType::Maturity));

        // Uncovered contract identifier.
        let events = CegEngine
            .evaluate(
                &golden(),
                &risk_with(&[credit_event("OTHER", ContractPerformance::Default)]),
            )
            .expect("events");
        assert!(events.iter().any(|e| e.event_type == EventType::Maturity));
    }

    #[test]
    fn cnt_references_without_roles_are_covered() {
        let mut guarantee = golden();
        if let Some(structure) = guarantee.contract_structure.as_mut() {
            for reference in structure.iter_mut() {
                reference.reference_role = None;
            }
        }
        let events = CegEngine
            .evaluate(
                &guarantee,
                &risk_with(&[credit_event("loan01", ContractPerformance::Default)]),
            )
            .expect("events");
        let std = events
            .iter()
            .find(|e| e.event_type == EventType::Settlement)
            .expect("STD");
        assert_eq!(std.payoff, dec!(2000000));
    }

    #[test]
    fn missing_structure_and_coverage_are_reported() {
        let error = CegEngine
            .evaluate(
                &terms(json!({"contractType": "CEG", "contractRole": "BUY"})),
                &StateProvider::new(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("contractStructure")
        ));

        let mut guarantee = golden();
        guarantee.guaranteed_exposure = Some(GuaranteedExposure::MarketValue);
        let error = CegEngine.evaluate(&guarantee, &risk_with(&[])).unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("marketObjectCode")
        ));
    }
}
