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
use crate::num::{self, ck};
use crate::pam::PamEngine;
use crate::risk::{ObservedCreditEvent, RiskFactorProvider};
use crate::schedule::{cycle_step, shift_business_day};
use crate::state::{ContractState, ContractStatus};
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
                .embedded_contract()
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
    // `contract_identifier` covers both reference shapes: an embedded
    // contract (`CNT`) and an identifier alone (`CID`, guarantee01-08).
    let covered_ids: Vec<&str> = covered
        .iter()
        .filter_map(|reference| reference.contract_identifier())
        .collect();
    risk.observed_credit_events()
        .into_iter()
        .filter(|event| event.time <= maturity)
        .filter(|event| {
            event
                .contract_id
                .as_deref()
                .is_some_and(|id| covered_ids.contains(&id))
        })
        .filter(|event| event.performance.map(|p| p.as_token()) == Some(covered_type.as_token()))
        .min_by_key(|event| event.time)
}

/// The covered exposure `sum(n_i)` at `at`, before coverage (techspec CEG
/// and CEC states tables, `Nt` initialisation).
///
/// Each covered contract is evaluated under its own contract role through
/// its own engine; the parent role sign is applied by the caller after
/// aggregation.
pub(crate) fn covered_exposure(
    covered: &[&ContractReference],
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    let values = covered
        .iter()
        .map(|reference| covered_contract_value(reference, at, basis, risk))
        .collect::<Result<Vec<Decimal>, EngineError>>()?;
    num::sum(values)
}

/// The guaranteed value `n_i` of one covered contract at `at`.
///
/// `NO` guarantees the notional state the child's last event at or before
/// `at` reports; `NI` adds the interest accrued from that event's status
/// date to `at`; `MV` reads the child's own market value observation.
fn covered_contract_value(
    reference: &ContractReference,
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    let child_terms =
        reference
            .embedded_contract()
            .cloned()
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
            (ck(state.notional_principal) + projected_accrued(&state, at, &child_terms)?).get()
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
    (ck(state.accrued_interest) + ck(year_fraction) * state.nominal_interest_rate * base).get()
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
/// cycle machinery with the end of month convention applied. A cycle-style
/// stub suffix (`L0`, `L1`, `R0`, ...: `P5DL0`) is accepted and ignored:
/// the FXOUT and CEG testbeds write periods that way, and a stub has no
/// meaning for a single period (known quirk 10, `vendor/actus/README.md`).
pub(crate) fn add_period(
    at: NaiveDateTime,
    raw: &str,
    terms: &ContractTerms,
) -> Result<NaiveDateTime, EngineError> {
    let upper = raw.trim().to_ascii_uppercase();
    let invalid = || EngineError::InvalidTransition(format!("invalid settlementPeriod: {raw}"));
    let body = upper.strip_prefix('P').ok_or_else(invalid)?;
    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &body[digits.len()..];
    let period = match rest.char_indices().nth(1) {
        // `<unit>` alone, or `<unit>` plus a one-letter stub and one digit.
        None => rest,
        Some((split, _)) => {
            let suffix = rest.as_bytes().get(split..).unwrap_or_default();
            match suffix {
                [b'L' | b'R', digit] if digit.is_ascii_digit() => &rest[..split],
                _ => return Err(invalid()),
            }
        }
    };
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
            Ok(cycle_step(at, 1, &cycle, eomc)?)
        }
        _ => Err(invalid()),
    }
}

/// A post-event state carrying the given notional at the event time.
pub(crate) fn event_state(notional: Decimal, time: NaiveDateTime) -> ContractState {
    ContractState {
        notional_principal: notional,
        status_date: time,
        ..ContractState::default()
    }
}

/// A terminal post-event state with an explicit lifetime status (`MD`
/// matures, a settled credit event terminates).
pub(crate) fn terminal_state(
    notional: Decimal,
    time: NaiveDateTime,
    status: ContractStatus,
) -> ContractState {
    ContractState {
        contract_status: status,
        ..event_state(notional, time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actus_model::ContractType;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The dictionary types `settlementPeriod` as a Period; the FXOUT and
    /// CEG testbeds also write it in cycle notation with a stub suffix
    /// (`P1DL0`, `P5DL0`, 7 cases), which adds the same period.
    #[test]
    fn periods_accept_the_cycle_notation_of_the_testbeds() {
        let terms = ContractTerms::new(ContractType::Ceg);
        let at = t("2015-01-10T00:00:00");
        for (raw, expected) in [
            ("P5D", "2015-01-15T00:00:00"),
            ("P5DL0", "2015-01-15T00:00:00"),
            ("P1DL0", "2015-01-11T00:00:00"),
            ("P1DL1", "2015-01-11T00:00:00"),
            ("P2W", "2015-01-24T00:00:00"),
            ("P1ML1", "2015-02-10T00:00:00"),
            ("P0D", "2015-01-10T00:00:00"),
        ] {
            assert_eq!(
                add_period(at, raw, &terms).expect(raw),
                t(expected),
                "{raw}"
            );
        }
        for bad in ["5D", "PD", "P5X", "P5DL", "P5DLX", "P5DL0X"] {
            assert!(add_period(at, bad, &terms).is_err(), "{bad}");
        }
    }
}
