//! CEG: Credit Enhancement Guarantee (paper §7.17, sections 5 "Risk Factor
//! Observer", 6 "Child Contract Observer"; mirrors the CEC engine's
//! credit-event and child-observer machinery).
//!
//! A guarantee covers one or more covered contracts (`CTST^CoveredContract`
//! references: `COVE`-tagged, else every embedded `CNT` and every `CID`
//! reference) and is triggered by credit events observed on them (techspec
//! "CEG", l. 2541 ff.). The events:
//!
//! - **Notional** `Nt` (states table): `CECV x sgn x NT` when the guarantee
//!   sets its own `NT` (the only option for a `CID` reference, which has no
//!   child to observe), else `CECV x sgn x sum(n_i)` with the per-contract
//!   exposure `n_i` resolved per `CEGE` from the child's own event stream:
//!   the notional for `NO`, notional plus accrued interest for `NI`, the
//!   child's market value observation for `MV`. Every event state reports
//!   it (`PRD`, `FP`, `XD`).
//! - **Purchase** `PRD` (schedule "Same as PAM", `POF_PRD_STK`): paying
//!   `-sgn x PPRD` when `purchaseDate` is after the status date.
//! - **Fees** `FP` on `FEANX` (else `PRD + FECL`) per `FECL`: `FEB = A`
//!   pays `sgn x FER`, `FEB = N` (default) pays `Nt x Y x FER`, with `Y`
//!   from the previous fee point (the anchor accrues zero).
//! - **Protection**: a qualifying observed credit event (covered contract
//!   identifier, performance equal to `CETC`, default `DF`, at or before
//!   maturity) emits an `XD` at the observation time (zero payoff, state
//!   `Nt`) and a `STD` at the credit event time plus `settlementPeriod`,
//!   shifted per the business day convention, paying `Xa + Feac`: the
//!   exercise amount `Nt` plus the fee accrued since the last fee point.
//!   Fees after the credit event are cut and no `MD` follows. Without a
//!   trigger a zero-payoff `MD` closes the stream.
//!
//! Contract role sign: `common::contract_role_sign` (`SEL` is `-1`).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::enums::FeeBasis;
use actus_model::{
    BusinessDayConvention, Calendar, ContractReference, ContractReferenceRole,
    ContractReferenceType, ContractTerms, ContractType, CreditEventType, DayCountConvention,
    EndOfMonthConvention, EventType,
};

use crate::common::contract_role_sign;
use crate::common::stub_series;
use crate::credit_enhancement::{
    covered_exposure, credit_event_trigger, event_state, maturity_date, settlement_date,
    terminal_state,
};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::{sort_events, ContractEvent};
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::ContractStatus;
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
        let sign = contract_role_sign(terms.contract_role);
        let coverage = terms.coverage_of_credit_enhancement.unwrap_or(Decimal::ONE);
        let maturity = maturity_date(terms, &covered)?;
        let trigger = credit_event_trigger(
            risk,
            &covered,
            terms
                .credit_event_type_covered
                .unwrap_or(CreditEventType::Default),
            maturity,
        );
        let notional_at =
            |at: NaiveDateTime| guaranteed_notional(terms, &covered, at, sign, coverage, risk);

        let status_date = terms.status_date.map(normalize_timestamp);
        let mut events = Vec::new();
        if let Some(purchase) = terms.purchase_date {
            let at = normalize_timestamp(purchase);
            if status_date.is_none_or(|sd| at > sd) && at <= maturity {
                let price = terms
                    .price_at_purchase_date
                    .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                events.push(ContractEvent {
                    event_type: EventType::Purchase,
                    time: purchase,
                    payoff: (ck(sign) * Decimal::NEGATIVE_ONE * price).get()?,
                    currency: terms.currency.clone(),
                    state: event_state(notional_at(at)?, at),
                });
            }
        }

        let fee_cutoff = trigger.as_ref().map(|t| t.time).unwrap_or(maturity);
        let fees = fee_schedule(terms, maturity, sign, &notional_at)?;
        for fee in fees.iter().filter(|fee| fee.emit <= fee_cutoff) {
            events.push(ContractEvent {
                event_type: EventType::FeePayment,
                time: fee.emit,
                payoff: fee.payoff,
                currency: terms.currency.clone(),
                state: event_state(fee.notional, fee.emit),
            });
        }

        match trigger {
            Some(credit_event) => {
                let exercise_amount = notional_at(credit_event.time)?;
                let accrued_fee = fee_accrued(terms, &fees, credit_event.time, sign)?;
                let settlement_time = settlement_date(terms, credit_event.time)?;
                events.push(ContractEvent {
                    event_type: EventType::Exercise,
                    time: credit_event.time,
                    payoff: Decimal::ZERO,
                    currency: terms.currency.clone(),
                    state: event_state(exercise_amount, credit_event.time),
                });
                events.push(ContractEvent {
                    event_type: EventType::Settlement,
                    time: settlement_time,
                    payoff: (ck(exercise_amount) + accrued_fee).get()?,
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

/// The guaranteed notional `Nt` at `at` (techspec CEG states table):
/// `CECV x sgn x NT` when the guarantee sets its own notional, else
/// `CECV x sgn x sum(n_i)` over the covered contracts.
///
/// A guarantee that references its covered contract only by identifier
/// (`CID`) has no child to observe, so it needs `NT` (guarantee01-08).
fn guaranteed_notional(
    terms: &ContractTerms,
    covered: &[&ContractReference],
    at: NaiveDateTime,
    sign: Decimal,
    coverage: Decimal,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    let exposure = match terms.notional_principal {
        Some(notional) => notional,
        None => covered_exposure(covered, at, terms.guaranteed_exposure, risk)?,
    };
    (ck(coverage) * sign * exposure).get()
}

/// The covered contract references: `COVE`-tagged entries, else every
/// embedded `CNT` reference and every `CID` reference.
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
        .filter(|r| {
            (r.reference_type == Some(ContractReferenceType::Contract)
                && r.embedded_contract().is_some())
                || r.reference_type == Some(ContractReferenceType::ContractIdentifier)
        })
        .collect()
}

/// One point of the fee schedule.
struct Fee {
    /// The calculation time of the schedule point.
    calc: NaiveDateTime,
    /// The emission time (business day shifted).
    emit: NaiveDateTime,
    /// The guaranteed notional `Nt` at the point.
    notional: Decimal,
    /// The `FP` payoff.
    payoff: Decimal,
}

/// The fee schedule (techspec CEG `FP`).
///
/// The schedule unrolls from `FEANX` (else `PRD + FECL`) per `FECL` to
/// maturity. `FEB = A` pays `sgn x FER`; `FEB = N` (default) pays
/// `Nt x Y x FER`, with `Y` the day count fraction from the previous
/// schedule point (the anchor accrues zero) and `Nt` the signed, covered
/// notional at the fee date.
fn fee_schedule(
    terms: &ContractTerms,
    maturity: NaiveDateTime,
    sign: Decimal,
    notional_at: &dyn Fn(NaiveDateTime) -> Result<Decimal, EngineError>,
) -> Result<Vec<Fee>, EngineError> {
    let fee_rate = match terms.fee_rate {
        Some(rate) if !rate.is_zero() => rate,
        _ => return Ok(Vec::new()),
    };
    let Some(cycle) = terms.cycle_of_fee.as_ref() else {
        return Ok(Vec::new());
    };
    let eomc = terms
        .end_of_month_convention
        .unwrap_or(EndOfMonthConvention::Sd);
    let anchor = match (terms.cycle_anchor_date_of_fee, terms.purchase_date) {
        (Some(anchor), _) => normalize_timestamp(anchor),
        (None, Some(purchase)) => cycle_step(normalize_timestamp(purchase), 1, cycle, eomc)?,
        (None, None) => return Ok(Vec::new()),
    };
    let bdc = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let cal = terms.calendar.unwrap_or(Calendar::Nc);
    let dcc = fee_day_count(terms);

    let mut schedule = Vec::new();
    let mut previous = anchor;
    for (calc, emit) in stub_series(anchor, cycle, maturity, eomc, bdc, cal)? {
        let notional = notional_at(emit)?;
        let payoff = match terms.fee_basis {
            Some(FeeBasis::AbsoluteValue) => (ck(sign) * fee_rate).get()?,
            _ => (ck(notional) * fee_rate * day_count_fraction(previous, calc, dcc)?).get()?,
        };
        schedule.push(Fee {
            calc,
            emit,
            notional,
            payoff,
        });
        previous = calc;
    }
    Ok(schedule)
}

/// The fee day count convention (dictionary default `A365`).
fn fee_day_count(terms: &ContractTerms) -> DayCountConvention {
    terms
        .day_count_convention
        .unwrap_or(DayCountConvention::A365)
}

/// The fee accrued `Feac` at the exercise time `at` (techspec CEG `XD`
/// state transition), settled with the protection in `STD`.
///
/// With `tFP-` the last fee point before `at` and `tFP+` one regular fee
/// cycle after it,
/// `FEB = A` accrues `Y(tFP-, at) / Y(tFP-, tFP+) x sgn x FER` and
/// `FEB = N` accrues `Y(tFP-, at) x Nt(tFP-) x FER`. Before the first fee
/// point nothing has accrued.
fn fee_accrued(
    terms: &ContractTerms,
    fees: &[Fee],
    at: NaiveDateTime,
    sign: Decimal,
) -> Result<Decimal, EngineError> {
    let Some(fee_rate) = terms.fee_rate else {
        return Ok(Decimal::ZERO);
    };
    let Some(previous) = fees.iter().rev().find(|fee| fee.calc < at) else {
        return Ok(Decimal::ZERO);
    };
    let dcc = fee_day_count(terms);
    let elapsed = day_count_fraction(previous.calc, at, dcc)?;
    match terms.fee_basis {
        Some(FeeBasis::AbsoluteValue) => {
            // The fee period the exercise falls in is the regular cycle
            // period after `tFP-`: a long last stub merges the final roll
            // into the stub, but guarantee14 accrues over the regular
            // quarter (80/92 of `FER`, not 80/183).
            let Some(cycle) = terms.cycle_of_fee.as_ref() else {
                return Ok(Decimal::ZERO);
            };
            let eomc = terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd);
            let next = cycle_step(previous.calc, 1, cycle, eomc)?;
            let period = day_count_fraction(previous.calc, next, dcc)?;
            if period.is_zero() {
                return Ok(Decimal::ZERO);
            }
            (ck(elapsed) / period * sign * fee_rate).get()
        }
        _ => (ck(elapsed) * previous.notional * fee_rate).get(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::ObservedCreditEvent;
    use crate::risk::StateProvider;
    use actus_model::ContractPerformance;
    use actus_model::ContractRole;
    use actus_model::GuaranteedExposure;
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

    /// The fee accrued from the July fee date to the 2025-09-20 credit
    /// event on the guaranteed notional `nt`: `0.005 x nt x 79/360`
    /// (30E360), settled with the protection.
    fn accrued_fee(nt: Decimal) -> Decimal {
        dec!(0.005) * nt * dec!(79) / dec!(360)
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
        close(std.payoff, dec!(2000000) + accrued_fee(dec!(2000000)));
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
        // 2M plus 2 x 0.03 x (79/360) x 1M accrued since the July IP, plus
        // the fee accrued on the 2M notional of the July fee date.
        close(
            std.payoff,
            dec!(2000000)
                + dec!(2) * dec!(0.03) * dec!(79) / dec!(360) * dec!(1000000)
                + accrued_fee(dec!(2000000)),
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
        close(std.payoff, dec!(-2000000) - accrued_fee(dec!(2000000)));
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
        close(std.payoff, dec!(1000000) + accrued_fee(dec!(1000000)));
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
        close(std.payoff, dec!(2000000) + accrued_fee(dec!(2000000)));
    }

    /// A guarantee that names its covered contract by identifier (`CID`)
    /// guarantees its own `NT`, scaled by the coverage and role signed
    /// (guarantee06, guarantee08); the purchase settles at `PRD`.
    #[test]
    fn identifier_references_guarantee_the_own_notional() {
        let guarantee = terms(json!({
            "contractType": "CEG",
            "contractID": "ceg-cid",
            "contractRole": "SEL",
            "currency": "USD",
            "statusDate": "2020-01-01T00:00:00",
            "purchaseDate": "2020-01-02T00:00:00",
            "priceAtPurchaseDate": "10",
            "maturityDate": "2020-12-31T00:00:00",
            "notionalPrincipal": "1000000",
            "coverageOfCreditEnhancement": "0.7",
            "creditEventTypeCovered": "DF",
            "contractStructure": [{
                "object": {"contractIdentifier": "US91282XYZ01"},
                "referenceType": "CID",
                "referenceRole": "COVE"
            }]
        }));
        let risk = risk_with(&[ObservedCreditEvent {
            time: t("2020-09-20T00:00:00"),
            contract_id: Some("US91282XYZ01".to_string()),
            performance: Some(ContractPerformance::Default),
        }]);
        let events = CegEngine.evaluate(&guarantee, &risk).expect("events");
        let summary: Vec<(EventType, Decimal, Decimal)> = events
            .iter()
            .map(|e| (e.event_type, e.payoff, e.state.notional_principal))
            .collect();
        assert_eq!(
            summary,
            vec![
                (EventType::Purchase, dec!(10), dec!(-700000)),
                (EventType::Exercise, Decimal::ZERO, dec!(-700000)),
                (EventType::Settlement, dec!(-700000), Decimal::ZERO),
            ]
        );

        // Without a matching credit event the guarantee matures.
        let events = CegEngine
            .evaluate(&guarantee, &risk_with(&[]))
            .expect("events");
        assert_eq!(
            events.iter().map(|e| e.event_type).collect::<Vec<_>>(),
            vec![EventType::Purchase, EventType::Maturity]
        );
    }

    /// The fee states carry the covered notional (guarantee14: coverage
    /// 0.5 over 11M reports 5.5M).
    #[test]
    fn fee_states_carry_the_coverage() {
        let mut guarantee = golden();
        guarantee.coverage_of_credit_enhancement = Some(dec!(0.5));
        let events = CegEngine
            .evaluate(&guarantee, &risk_with(&[]))
            .expect("events");
        let fee = events
            .iter()
            .find(|e| e.event_type == EventType::FeePayment && e.time == t("2025-04-01T00:00:00"))
            .expect("fee");
        assert_eq!(fee.state.notional_principal, dec!(1000000));
        assert_eq!(fee.payoff, dec!(1250));
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
