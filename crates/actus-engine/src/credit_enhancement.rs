//! Helpers shared by the credit enhancement contract types CEC
//! (collateral) and CEG (guarantee): the maturity of the enhancement, the
//! credit event trigger on the covered contracts, the covered exposure, and
//! the settlement date arithmetic.

use chrono::{Duration, NaiveDateTime};
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractReference, ContractTerms, CreditEventType, Cycle,
    CyclePeriod, CycleStub, DayCountConvention, EndOfMonthConvention, GuaranteedExposure,
};

use crate::ann::AnnEngine;
use crate::clm::ClmEngine;
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::EngineRegistry;
use crate::lam::LamEngine;
use crate::nam::NamEngine;
use crate::pam::PamEngine;
use crate::risk::{ObservedCreditEvent, RiskFactorProvider};
use crate::schedule::{cycle_step, shift_business_day};
use crate::state::ContractState;
use crate::EngineError;

/// The engines a covered child may route through.
///
/// The CEC testbed covers PAM and LAM contracts; the remaining
/// schedule-driven engines are registered so any fixed-income child
/// evaluates through its own implementation.
pub(crate) fn child_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(ClmEngine));
    registry
}

/// The CEC maturity: the parent `MD` term when set, else the latest covered
/// contract maturity (techspec "CEC" states table, `Md` initialisation).
pub(crate) fn maturity_date(
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

/// The first observed credit event that qualifies as the trigger.
///
/// A credit event qualifies when it names a covered contract, carries the
/// covered contract performance state (`CETC`) and occurs at or before the
/// CEC maturity. The earliest qualifying observation triggers.
pub(crate) fn credit_event_trigger(
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

/// The guaranteed value `n_i` of one covered contract at `at`.
///
/// `NO` guarantees the notional state the child's last event at or before
/// `at` reports; `NI` adds the interest accrued from that event's status
/// date to `at`; `MV` reads the child's own market value observation.
pub(crate) fn covered_contract_value(
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

/// The interest accrued from the state's status date to `at`.
///
/// Mirrors the accrual transition `IPAC(t+) = IPAC(t-) + YF(SD(t-), t) x
/// IPNR(t-) x IP base(t-)`: the child's last event freezes the states at its
/// status date, and the accrual to the credit event time is projected with
/// the child's day count convention (dictionary default `A365`) over the
/// interest calculation base (the notional when the base state is unset).
pub(crate) fn projected_accrued(
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

/// The settlement date: the credit event time plus `settlementPeriod`
/// (`STP`, default `P0D`), shifted per the business day convention.
///
/// The `XD` event itself emits at the unshifted credit event time; only the
/// settlement obligation is shifted (collateral13 settles the Sunday notice
/// on the following Monday under `MF` / `CSF`).
pub(crate) fn settlement_date(
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

/// Adds an ISO 8601 period (`P<n>D|W|M|Y`) to a timestamp.
///
/// Days and weeks add calendar durations; months and years step through the
/// cycle machinery with the end of month convention applied.
pub(crate) fn add_period(
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
