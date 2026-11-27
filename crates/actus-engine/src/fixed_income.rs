//! Schedule and state building blocks shared by the fixed-income family
//! (PAM, LAM, NAM, ANN, LAX).
//!
//! Each engine keeps its own slot and event-kind types; the helpers here
//! return plain [`Timing`]s that the engines map onto their slots. Where the
//! engines differ on purpose (for example the sign applied to a carried
//! `accruedInterest` term), the difference is an explicit parameter, so
//! every divergence is visible at the call site.

use std::collections::BTreeSet;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, Cycle, DayCountConvention,
    EndOfMonthConvention, EventType, InterestCalculationBase, ScalingEffect,
};

use crate::common::{anchored_series, calculation_time, stub_series, stub_series_unshifted};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::event::ContractEvent;
use crate::state::ContractState;
use crate::EngineError;

/// The resolved dates and conventions that schedule generation depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Conventions {
    /// Status date `t0`.
    pub t0: NaiveDateTime,
    /// Maturity date (given or inferred).
    pub maturity: NaiveDateTime,
    /// Day count convention.
    pub dcc: DayCountConvention,
    /// End-of-month convention.
    pub eomc: EndOfMonthConvention,
    /// Business day convention.
    pub bdc: BusinessDayConvention,
    /// Business day calendar.
    pub cal: Calendar,
}

/// When a schedule element is calculated and when it is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timing {
    /// Calculation time (input of the accrual).
    pub calc: NaiveDateTime,
    /// Emission time (the reported event time).
    pub emit: NaiveDateTime,
}

/// The interest schedule: `IPCI` capitalization elements up to `IPCED`,
/// then the `IP` payment elements that are not capitalization dates.
#[derive(Debug, Default)]
pub(crate) struct InterestSchedule {
    /// `IPCI` elements, in schedule order.
    pub capitalization: Vec<Timing>,
    /// `IP` elements, in schedule order.
    pub payment: Vec<Timing>,
}

/// The kind of a rate reset schedule element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateReset {
    /// `RRF`: applies the fixed `RRNXT` rate.
    Fixed,
    /// `RR`: observes the market object.
    Variable,
}

/// The `IP`/`IPCI` series from `IPANX` with cycle `IPCL` up to maturity.
///
/// Elements up to `IPCED` become capitalizations; the payment series skips
/// those dates. Empty when the anchor or the cycle is absent.
pub(crate) fn interest_schedule(c: &Conventions, terms: &ContractTerms) -> InterestSchedule {
    let (Some(anchor), Some(cycle)) = (
        terms
            .cycle_anchor_date_of_interest_payment
            .map(normalize_timestamp),
        terms.cycle_of_interest_payment.as_ref(),
    ) else {
        return InterestSchedule::default();
    };
    let mut schedule = InterestSchedule::default();
    let capitalized: BTreeSet<NaiveDateTime> =
        match terms.capitalization_end_date.map(normalize_timestamp) {
            Some(ipced) if ipced < c.maturity => {
                let series = anchored_series(anchor, cycle, ipced, c.eomc, c.bdc, c.cal);
                let anchors = series.iter().map(|(unshifted, _)| *unshifted).collect();
                schedule.capitalization = series
                    .into_iter()
                    .map(|(unshifted, emit)| Timing {
                        calc: calculation_time(c.bdc, unshifted, emit),
                        emit,
                    })
                    .collect();
                anchors
            }
            _ => BTreeSet::new(),
        };
    schedule.payment = stub_series_unshifted(anchor, cycle, c.maturity, c.eomc, c.bdc, c.cal)
        .into_iter()
        .filter(|(unshifted, _)| !capitalized.contains(unshifted))
        .map(|(unshifted, emit)| Timing {
            calc: calculation_time(c.bdc, unshifted, emit),
            emit,
        })
        .collect();
    schedule
}

/// The `RR`/`RRF` series from `RRANX` with cycle `RRCL`, without the
/// maturity element.
///
/// With `RRNXT` set, the first element after the status date is a fixed
/// reset. Empty when the anchor or the cycle is absent.
pub(crate) fn rate_reset_schedule(
    c: &Conventions,
    terms: &ContractTerms,
) -> Vec<(RateReset, Timing)> {
    let (Some(anchor), Some(cycle)) = (
        terms
            .cycle_anchor_date_of_rate_reset
            .map(normalize_timestamp),
        terms.cycle_of_rate_reset.as_ref(),
    ) else {
        return Vec::new();
    };
    let mut series = stub_series(anchor, cycle, c.maturity, c.eomc, c.bdc, c.cal);
    if series.last().map(|(_, emit)| *emit) == Some(c.maturity) {
        series.pop();
    }
    let mut fixed_pending = terms.next_reset_rate.is_some();
    series
        .into_iter()
        .map(|(calc, emit)| {
            let kind = if fixed_pending && calc > c.t0 {
                fixed_pending = false;
                RateReset::Fixed
            } else {
                RateReset::Variable
            };
            (kind, Timing { calc, emit })
        })
        .collect()
}

/// A series `S(anchor, cycle, MD)` without the maturity element (shared
/// shape of the `PR`, `SC` and `IPCB` schedule rows).
fn fixing_schedule(
    c: &Conventions,
    anchor: Option<NaiveDateTime>,
    cycle: Option<&Cycle>,
) -> Vec<Timing> {
    let (Some(anchor), Some(cycle)) = (anchor.map(normalize_timestamp), cycle) else {
        return Vec::new();
    };
    stub_series(anchor, cycle, c.maturity, c.eomc, c.bdc, c.cal)
        .into_iter()
        .filter(|(calc, _)| *calc != c.maturity)
        .map(|(calc, emit)| Timing { calc, emit })
        .collect()
}

/// The `PR` redemption series `S(PRANX, PRCL, MD)` without the maturity
/// element (the final redemption belongs to `MD`).
pub(crate) fn principal_redemption_schedule(c: &Conventions, terms: &ContractTerms) -> Vec<Timing> {
    fixing_schedule(
        c,
        terms.cycle_anchor_date_of_principal_redemption,
        terms.cycle_of_principal_redemption.as_ref(),
    )
}

/// The `IPCB` fixing series: defined only for `IPCB = NTL`, from `IPCBANX`
/// with cycle `IPCBCL`, without the maturity element.
pub(crate) fn interest_calculation_base_schedule(
    c: &Conventions,
    terms: &ContractTerms,
) -> Vec<Timing> {
    if terms.interest_calculation_base != Some(InterestCalculationBase::Ntl) {
        return Vec::new();
    }
    fixing_schedule(
        c,
        terms.cycle_anchor_date_of_interest_calculation_base,
        terms.cycle_of_interest_calculation_base.as_ref(),
    )
}

/// The `SC` scaling index series: defined only when the scaling effect
/// scales the notional or the interest, from `SCANX` with cycle `SCCL`,
/// without the maturity element.
pub(crate) fn scaling_index_schedule(c: &Conventions, terms: &ContractTerms) -> Vec<Timing> {
    if !scales_notional(terms.scaling_effect) && !scales_interest(terms.scaling_effect) {
        return Vec::new();
    }
    fixing_schedule(
        c,
        terms.cycle_anchor_date_of_scaling_index,
        terms.cycle_of_scaling_index.as_ref(),
    )
}

/// Whether the scaling effect drives the notional scaling multiplier
/// (techspec `SCEF = [x]N[x]`).
pub(crate) fn scales_notional(effect: Option<ScalingEffect>) -> bool {
    matches!(
        effect,
        Some(ScalingEffect::Notional) | Some(ScalingEffect::InterestAndNotional)
    )
}

/// Whether the scaling effect drives the interest scaling multiplier
/// (techspec `SCEF = I[x][x]`).
pub(crate) fn scales_interest(effect: Option<ScalingEffect>) -> bool {
    matches!(
        effect,
        Some(ScalingEffect::Interest) | Some(ScalingEffect::InterestAndNotional)
    )
}

/// Whether an event at `t` is reported, given the status date and the
/// purchase/termination window.
///
/// `sequence` is the dictionary sequence number of the event. Events at the
/// purchase date that precede `PRD` in the sequence (an `IP` or `PR` due
/// that day) still change the state but are not reported: they settle with
/// the seller (the official lam02 fixture pins this).
pub(crate) fn observed(
    t0: NaiveDateTime,
    t: NaiveDateTime,
    sequence: u8,
    terms: &ContractTerms,
) -> bool {
    if t < t0 {
        return false;
    }
    if let Some(prd) = terms.purchase_date {
        let precedes_purchase = t == prd && sequence < EventType::Purchase.priority();
        if t < prd || precedes_purchase {
            return false;
        }
    }
    if let Some(td) = terms.termination_date {
        if t > td {
            return false;
        }
    }
    true
}

/// One schedule element of a fixed-income engine, as the shared event loop
/// needs to see it. Each engine keeps its own slot type and implements this.
pub(crate) trait ScheduledEvent: Copy {
    /// The dictionary event type the element reports as.
    fn event_type(&self) -> EventType;
    /// The same-timestamp sequence number (see [`observed`]).
    fn sequence(&self) -> u8;
    /// The emission (reported) time.
    fn emit(&self) -> NaiveDateTime;
}

/// The event loop shared by PAM, LAM, NAM, ANN and LAX.
///
/// The starting state is `progressed()` for a contract already running at
/// the status date (`IED` before `t0`), otherwise `pre_ied()`. Every slot is
/// then applied in schedule order through `apply` (the engine's state
/// transition and payoff functions), and the events that are observable are
/// reported: emitted at or after `IED`, after the status date, and inside
/// the purchase/termination window. For a progressed contract the `IED`
/// slot is skipped entirely, since its state already holds the `t0` values.
pub(crate) fn evaluate_schedule<S: ScheduledEvent>(
    ied: NaiveDateTime,
    t0: NaiveDateTime,
    slots: Vec<S>,
    terms: &ContractTerms,
    progressed: impl FnOnce() -> Result<ContractState, EngineError>,
    pre_ied: impl FnOnce() -> ContractState,
    mut apply: impl FnMut(&mut ContractState, S) -> Result<Decimal, EngineError>,
) -> Result<Vec<ContractEvent>, EngineError> {
    let is_progressed = ied < t0;
    let mut state = if is_progressed {
        progressed()?
    } else {
        pre_ied()
    };
    let mut events = Vec::new();
    for slot in slots {
        if is_progressed && slot.event_type() == EventType::InitialExchange {
            continue;
        }
        let payoff = apply(&mut state, slot)?;
        let emit = slot.emit();
        if emit < ied || !observed(t0, emit, slot.sequence(), terms) {
            continue;
        }
        events.push(ContractEvent {
            event_type: slot.event_type(),
            time: normalize_timestamp(emit),
            payoff,
            currency: terms.currency.clone(),
            state: state.clone(),
        });
    }
    Ok(events)
}

/// The accrued interest state at `IED`: the carried `accruedInterest` term
/// with the role sign `sgn` applied (it is an unsigned attribute; the state
/// is role-signed, as the official nam04 fixture pins), otherwise the
/// accrual since `IPANX` when the anchor precedes the initial exchange,
/// otherwise zero. The accrual is already signed through the notional.
pub(crate) fn initial_accrued(
    terms: &ContractTerms,
    state: &ContractState,
    ied: NaiveDateTime,
    dcc: DayCountConvention,
    sgn: Decimal,
) -> Result<Decimal, EngineError> {
    if let Some(accrued) = terms.accrued_interest {
        return Ok(sgn * accrued);
    }
    if let Some(anchor) = terms.cycle_anchor_date_of_interest_payment {
        if anchor < ied {
            let yf = day_count_fraction(anchor, ied, dcc)?;
            return Ok(yf * state.notional_principal * state.nominal_interest_rate);
        }
    }
    Ok(Decimal::ZERO)
}

/// The accrued interest of a progressed contract on `base_amount`, from the
/// last interest schedule point before the status date; `None` when no
/// schedule point precedes the status date.
pub(crate) fn progressed_accrued(
    c: &Conventions,
    terms: &ContractTerms,
    base_amount: Decimal,
) -> Result<Option<Decimal>, EngineError> {
    let (Some(anchor), Some(cycle)) = (
        terms
            .cycle_anchor_date_of_interest_payment
            .map(normalize_timestamp),
        terms.cycle_of_interest_payment.as_ref(),
    ) else {
        return Ok(None);
    };
    let previous = stub_series(anchor, cycle, c.maturity, c.eomc, c.bdc, c.cal)
        .into_iter()
        .map(|(calc, _)| calc)
        .filter(|t| *t < c.t0)
        .max();
    match previous {
        Some(t_minus) => {
            let yf = day_count_fraction(t_minus, c.t0, c.dcc)?;
            Ok(Some(
                yf * base_amount * terms.nominal_interest_rate.unwrap_or(Decimal::ZERO),
            ))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn conventions(t0: &str, maturity: &str) -> Conventions {
        Conventions {
            t0: t(t0),
            maturity: t(maturity),
            dcc: DayCountConvention::A365,
            eomc: EndOfMonthConvention::Sd,
            bdc: BusinessDayConvention::Nos,
            cal: Calendar::Nc,
        }
    }

    fn quarterly_loan(extra: serde_json::Value) -> ContractTerms {
        let mut raw = json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2013-01-01T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.05",
            "dayCountConvention": "A365",
            "cycleAnchorDateOfInterestPayment": "2013-04-01T00:00:00",
            "cycleOfInterestPayment": "P3ML0"
        });
        for (key, value) in extra.as_object().expect("object") {
            raw[key] = value.clone();
        }
        terms(raw)
    }

    fn emits(timings: &[Timing]) -> Vec<NaiveDateTime> {
        timings.iter().map(|timing| timing.emit).collect()
    }

    #[test]
    fn interest_schedule_runs_from_the_anchor_to_maturity() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let schedule = interest_schedule(&c, &quarterly_loan(json!({})));
        assert!(schedule.capitalization.is_empty());
        assert_eq!(
            emits(&schedule.payment),
            vec![
                t("2013-04-01T00:00:00"),
                t("2013-07-01T00:00:00"),
                t("2013-10-01T00:00:00"),
                t("2014-01-01T00:00:00"),
            ]
        );
    }

    #[test]
    fn capitalization_dates_are_removed_from_the_payment_series() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let terms = quarterly_loan(json!({ "capitalizationEndDate": "2013-07-01T00:00:00" }));
        let schedule = interest_schedule(&c, &terms);
        assert_eq!(
            emits(&schedule.capitalization),
            vec![t("2013-04-01T00:00:00"), t("2013-07-01T00:00:00")]
        );
        assert_eq!(
            emits(&schedule.payment),
            vec![t("2013-10-01T00:00:00"), t("2014-01-01T00:00:00")]
        );
    }

    #[test]
    fn capitalization_end_at_or_after_maturity_capitalizes_nothing() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let terms = quarterly_loan(json!({ "capitalizationEndDate": "2014-01-01T00:00:00" }));
        let schedule = interest_schedule(&c, &terms);
        assert!(schedule.capitalization.is_empty());
        assert_eq!(schedule.payment.len(), 4);
    }

    #[test]
    fn interest_schedule_needs_both_anchor_and_cycle() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let mut terms = quarterly_loan(json!({}));
        terms.cycle_of_interest_payment = None;
        let schedule = interest_schedule(&c, &terms);
        assert!(schedule.payment.is_empty() && schedule.capitalization.is_empty());
    }

    #[test]
    fn rate_resets_drop_the_maturity_element_and_fix_the_first_reset_after_t0() {
        let c = conventions("2013-05-01T00:00:00", "2014-01-01T00:00:00");
        let terms = quarterly_loan(json!({
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML0",
            "nextResetRate": "0.07"
        }));
        let resets = rate_reset_schedule(&c, &terms);
        let kinds: Vec<(RateReset, NaiveDateTime)> = resets
            .iter()
            .map(|(kind, timing)| (*kind, timing.emit))
            .collect();
        assert_eq!(
            kinds,
            vec![
                // Before t0: stays a market observation.
                (RateReset::Variable, t("2013-04-01T00:00:00")),
                // First element after t0 applies RRNXT.
                (RateReset::Fixed, t("2013-07-01T00:00:00")),
                (RateReset::Variable, t("2013-10-01T00:00:00")),
            ]
        );
    }

    #[test]
    fn rate_resets_without_next_reset_rate_are_all_variable() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let terms = quarterly_loan(json!({
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML0"
        }));
        let resets = rate_reset_schedule(&c, &terms);
        assert_eq!(resets.len(), 3);
        assert!(resets.iter().all(|(kind, _)| *kind == RateReset::Variable));
        assert!(rate_reset_schedule(&c, &quarterly_loan(json!({}))).is_empty());
    }

    #[test]
    fn interest_calculation_base_fixings_exist_only_for_ntl() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let base = json!({
            "cycleAnchorDateOfInterestCalculationBase": "2013-07-01T00:00:00",
            "cycleOfInterestCalculationBase": "P6ML0"
        });
        let mut ntl = base.clone();
        ntl["interestCalculationBase"] = json!("NTL");
        let mut nt = base;
        nt["interestCalculationBase"] = json!("NT");
        assert_eq!(
            emits(&interest_calculation_base_schedule(
                &c,
                &quarterly_loan(ntl)
            )),
            // The maturity element belongs to MD.
            vec![t("2013-07-01T00:00:00")]
        );
        assert!(interest_calculation_base_schedule(&c, &quarterly_loan(nt)).is_empty());
    }

    #[test]
    fn scaling_fixings_follow_the_scaling_effect() {
        let c = conventions("2013-01-01T00:00:00", "2014-01-01T00:00:00");
        let scaled = |effect: &str| {
            quarterly_loan(json!({
                "scalingEffect": effect,
                "cycleAnchorDateOfScalingIndex": "2013-04-01T00:00:00",
                "cycleOfScalingIndex": "P3ML0"
            }))
        };
        for effect in ["INO", "ONO", "IOO"] {
            assert_eq!(
                scaling_index_schedule(&c, &scaled(effect)).len(),
                3,
                "{effect}"
            );
        }
        assert!(scaling_index_schedule(&c, &scaled("OOO")).is_empty());
        assert!(scales_notional(Some(ScalingEffect::Notional)));
        assert!(!scales_notional(Some(ScalingEffect::Interest)));
        assert!(scales_interest(Some(ScalingEffect::InterestAndNotional)));
        assert!(!scales_interest(None));
    }

    #[test]
    fn observed_respects_status_purchase_and_termination_dates() {
        let terms = quarterly_loan(json!({
            "purchaseDate": "2013-04-01T00:00:00",
            "terminationDate": "2013-10-01T00:00:00"
        }));
        let t0 = t("2013-01-01T00:00:00");
        let ip = EventType::InterestPayment.priority();
        assert!(!observed(t0, t("2012-12-31T00:00:00"), ip, &terms));
        assert!(!observed(t0, t("2013-03-31T00:00:00"), ip, &terms));
        assert!(observed(t0, t("2013-07-01T00:00:00"), ip, &terms));
        assert!(observed(t0, t("2013-10-01T00:00:00"), ip, &terms));
        assert!(!observed(t0, t("2013-10-02T00:00:00"), ip, &terms));
    }

    #[test]
    fn events_preceding_the_purchase_at_its_date_are_not_reported() {
        let terms = quarterly_loan(json!({ "purchaseDate": "2013-04-01T00:00:00" }));
        let t0 = t("2013-01-01T00:00:00");
        let prd = t("2013-04-01T00:00:00");
        let ip = EventType::InterestPayment.priority();
        let pr = EventType::PrincipalRedemption.priority();
        let purchase = EventType::Purchase.priority();
        let maturity = EventType::Maturity.priority();
        assert!(ip < purchase && pr < purchase && maturity > purchase);
        // An IP or PR due on the purchase date settles with the seller.
        assert!(!observed(t0, prd, ip, &terms));
        assert!(!observed(t0, prd, pr, &terms));
        // The purchase itself, and anything after it in the sequence, is reported.
        assert!(observed(t0, prd, purchase, &terms));
        assert!(observed(t0, prd, maturity, &terms));
        // Later dates are unaffected by the sequence.
        assert!(observed(t0, t("2013-07-01T00:00:00"), ip, &terms));
    }

    #[test]
    fn initial_accrued_role_signs_only_the_carried_term() {
        let mut state = ContractState::initial(&quarterly_loan(json!({})));
        state.notional_principal = dec!(1000);
        state.nominal_interest_rate = dec!(0.05);
        let ied = t("2013-01-01T00:00:00");
        let carried = quarterly_loan(json!({ "accruedInterest": "12" }));
        let dcc = DayCountConvention::A365;
        assert_eq!(
            initial_accrued(&carried, &state, ied, dcc, Decimal::ONE).unwrap(),
            dec!(12)
        );
        assert_eq!(
            initial_accrued(&carried, &state, ied, dcc, Decimal::NEGATIVE_ONE).unwrap(),
            dec!(-12)
        );
        // Anchor before IED: accrue from the anchor, sign not applied.
        let early_anchor =
            quarterly_loan(json!({ "cycleAnchorDateOfInterestPayment": "2012-10-01T00:00:00" }));
        let accrued =
            initial_accrued(&early_anchor, &state, ied, dcc, Decimal::NEGATIVE_ONE).unwrap();
        assert_eq!(accrued, dec!(92) / dec!(365) * dec!(1000) * dec!(0.05));
        // Anchor after IED: nothing accrued yet.
        assert_eq!(
            initial_accrued(&quarterly_loan(json!({})), &state, ied, dcc, Decimal::ONE).unwrap(),
            Decimal::ZERO
        );
    }

    #[test]
    fn progressed_accrual_starts_at_the_last_interest_date_before_t0() {
        let c = conventions("2013-05-01T00:00:00", "2014-01-01T00:00:00");
        let terms = quarterly_loan(json!({}));
        let accrued = progressed_accrued(&c, &terms, dec!(1000)).unwrap();
        // 2013-04-01 .. 2013-05-01 is 30 days.
        assert_eq!(
            accrued,
            Some(dec!(30) / dec!(365) * dec!(1000) * dec!(0.05))
        );
        let before_first = conventions("2013-02-01T00:00:00", "2014-01-01T00:00:00");
        assert_eq!(
            progressed_accrued(&before_first, &terms, dec!(1000)).unwrap(),
            None
        );
        let mut no_cycle = terms;
        no_cycle.cycle_of_interest_payment = None;
        assert_eq!(progressed_accrued(&c, &no_cycle, dec!(1000)).unwrap(), None);
    }
}
