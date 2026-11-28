//! STK: Stock (paper section 7.9 "STK: Stock"; dictionary applicability
//! `STK_*`).
//!
//! An equity position. The economic driver is the share count `quantity`
//! (`QT`, dictionary default 1): every payoff multiplies the quantity by a
//! per-share price. There is no notional state (techspec STK states: `Prf`
//! and `Sd` only; the official testbed reports `notionalPrincipal` 0). The
//! dictionary lists `notionalPrincipal` as required for STK, but it is
//! descriptive there and enters no payoff (stk01: `NT = 1000`,
//! `PPRD = 1100`, `PRD` pays -1100).
//!
//! Conventions implemented here, resolved against the paper and the sibling
//! engines:
//!
//! - Entry: the purchase cash flow `-sgn x QT x PPRD` settles at
//!   `purchaseDate` (`PRD`; optional, as in the techspec PRD schedule
//!   "Same as PAM", and then `PPRD` is required) or, when
//!   `initialExchangeDate` is set, at the exchange date if both coincide.
//!   When `IED` and `PRD` differ, `IED` emits as a zero-payoff agreement
//!   event and the price settles at `PRD`.
//! - Dividends (`DV`, techspec STK schedule and functions; SI-006): with
//!   `DVNP` set, a fixed `DV^(fix)` at `DVANX` pays `sgn x QT x DVNP`; the
//!   series `S(s, DVCL, tmax)` (`s = DVANX`, or `DVANX + DVCL` after a fixed
//!   one) pays the observed dividend `sgn x QT x O(code, t)`, where `code`
//!   is `marketObjectCodeOfDividends` (the STK testbed) or else the
//!   techspec's `concat(CID, "-DV")`. `tmax` is the analysis horizon of the
//!   environment (inclusive); a termination ends the series before `TD`.
//!   A cycle with neither is open-ended and an error. Dates follow
//!   `BDC`/`CLDR`.
//! - Sale: the terminal `TD` at `terminationDate` sells the position at
//!   `sgn x QT x PTD` and reports
//!   [`ContractStatus::Terminated`]. The per-unit sale price resolves as
//!   the market observation of `marketObjectCode` at `TD` (paper: "Payoffs
//!   driven by market observations"; convention: the market object carries
//!   the per-share price, observed through the provider's `rate` view and
//!   falling back to its `index` view), then `priceAtTerminationDate`, then
//!   `marketValueObserved`; with none present the evaluation reports
//!   [`EngineError::MissingAttribute`].
//! - Without a termination date the stream ends with the last dividend
//!   up to the horizon, with status [`ContractStatus::Active`] (an open
//!   position has no terminal event). The ex-dividend date `DVEX` (no-accrual window) is
//!   out of scope: dividends are attributed to their schedule dates.
//! - Events at or before the status date `t0` are not observed (the CLM
//!   analysis-window idiom).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, EndOfMonthConvention, EventType,
};
use chrono::Duration;

use crate::common::rolls_before;
use crate::common::{calculation_time, role_sign};
use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::schedule::shift_business_day;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the STK contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct StkEngine;

impl ContractEngine for StkEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Stk
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        evaluate_position(terms, risk, Dividends::Paid)
    }
}

/// Whether the position pays dividends (STK) or not (COM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dividends {
    /// Pay the `DVANX`/`DVCL` dividend series (STK).
    Paid,
    /// No dividend events (COM).
    None,
}

/// The shared equity/commodity position evaluation (paper sections 7.9/7.10:
/// COM is STK without the dividend schedule).
///
/// Emits the entry exchange (and `PRD` when it differs from `IED`), the
/// dividend series for [`Dividends::Paid`], and the terminal sale `TD` when
/// `terminationDate` is set.
pub(crate) fn evaluate_position(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    dividends: Dividends,
) -> Result<Vec<ContractEvent>, EngineError> {
    // `QT` defaults to 1 (dictionary `quantity` default); every STK case of
    // the official testbed omits it.
    let quantity = terms.quantity.unwrap_or(Decimal::ONE);
    let sgn = role_sign(terms);
    let t0 = ContractState::initial(terms).status_date;
    let ied = terms.initial_exchange_date.map(normalize_timestamp);
    let purchase = terms.purchase_date.map(normalize_timestamp);
    let termination = terms.termination_date.map(normalize_timestamp);
    // The entry is optional: the techspec PRD schedule is "Same as PAM"
    // (an event only when `PRD` is set), and a holding acquired before the
    // analysis window has none (com02, stk08-stk10). Its price is required
    // only when there is an entry to pay it.
    let entry_time = ied.or(purchase);
    let entry_price = match entry_time {
        Some(_) => terms
            .price_at_purchase_date
            .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?,
        None => Decimal::ZERO,
    };

    let mut slots: Vec<Slot> = Vec::new();
    let price_moves_with_purchase = purchase.is_some() && purchase != ied;
    if let Some(ied) = ied {
        if ied > t0 {
            slots.push(Slot {
                kind: Kind::InitialExchange,
                time: ied,
                payoff: if price_moves_with_purchase {
                    Decimal::ZERO
                } else {
                    (-ck(sgn) * quantity * entry_price).get()?
                },
            });
        }
    }
    if let Some(prd) = purchase {
        if prd > t0 && price_moves_with_purchase {
            slots.push(Slot {
                kind: Kind::Purchase,
                time: prd,
                payoff: (-ck(sgn) * quantity * entry_price).get()?,
            });
        }
    }
    if dividends == Dividends::Paid {
        dividend_slots(
            terms,
            risk,
            quantity,
            sgn,
            entry_time,
            t0,
            termination,
            &mut slots,
        )?;
    }
    if let Some(td) = termination {
        if td > t0 {
            let price = termination_unit_price(terms, risk, td)?;
            slots.push(Slot {
                kind: Kind::Termination,
                time: td,
                payoff: (ck(sgn) * quantity * price).get()?,
            });
        }
    }
    slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

    let mut state = ContractState::initial(terms);
    let mut events = Vec::new();
    for slot in slots {
        // STK and COM carry no notional state: the techspec state tables
        // hold only `Prf` and `Sd`, and the official testbeds report
        // `notionalPrincipal` 0 after every event.
        if slot.kind == Kind::Termination {
            state.contract_status = ContractStatus::Terminated;
        }
        state.status_date = slot.time;
        events.push(ContractEvent {
            event_type: slot.kind.event_type(),
            time: slot.time,
            payoff: slot.payoff,
            currency: terms.currency.clone(),
            state: state.clone(),
        });
    }
    Ok(events)
}

/// Event kinds of the equity/commodity position stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    Purchase,
    Dividend,
    Termination,
}

impl Kind {
    /// The dictionary event type of the slot.
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::Purchase => EventType::Purchase,
            Kind::Dividend => EventType::DividendPayment,
            Kind::Termination => EventType::Termination,
        }
    }

    /// The effective same-timestamp sequence rank of the slot.
    fn priority(self) -> u8 {
        sequence_rank(self.event_type())
    }
}

/// One event slot of the position stream with its precomputed payoff.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    kind: Kind,
    time: NaiveDateTime,
    payoff: Decimal,
}

/// Builds the `DV` dividend slots (techspec STK schedule and functions):
///
/// - `DV^(fix)`: when `DVNP` is set, one dividend at `DVANX` paying
///   `R(CNTRL) x QT x DVNP`;
/// - `DV`: the series `S(s, DVCL, tmax)` with `s = DVANX` (or
///   `DVANX + DVCL` after a `DV^(fix)`), each paying the observed dividend
///   `R(CNTRL) x QT x O(code, t)`. The code is `marketObjectCodeOfDividends`
///   (the STK testbed) or else the techspec's `concat(CID, "-DV")`.
///
/// `tmax` is the analysis horizon of the environment, inclusive (stk06
/// pays on its horizon 2018-01-01). A termination ends the series too,
/// exclusively: a dividend at or after `TD` belongs to the buyer. A cycle
/// with neither bound is an open-ended schedule and an error. Without a
/// cycle only `DVANX` pays. Dates follow `BDC`/`CLDR`, observed at the
/// calculation time of the convention (stk09: `SCF` pays 2017-01-01 on
/// Monday 2017-01-02). Dividends at or before the entry (when there is one)
/// or at or before the status date are not observed.
#[allow(clippy::too_many_arguments)]
fn dividend_slots(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    quantity: Decimal,
    sgn: Decimal,
    entry: Option<NaiveDateTime>,
    t0: NaiveDateTime,
    termination: Option<NaiveDateTime>,
    slots: &mut Vec<Slot>,
) -> Result<(), EngineError> {
    let Some(anchor) = terms.cycle_anchor_date_of_dividend.map(normalize_timestamp) else {
        return Ok(());
    };
    let horizon = risk.analysis_horizon().map(normalize_timestamp);
    let bdc = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let cal = terms.calendar.unwrap_or(Calendar::Nc);
    let in_window = |time: NaiveDateTime| {
        time > t0
            && entry.is_none_or(|entry| time > entry)
            && termination.is_none_or(|td| time < td)
            && horizon.is_none_or(|tmax| time <= tmax)
    };
    // Unshifted schedule dates of the observed series.
    let mut series = vec![anchor];
    if let Some(fixed) = terms.next_dividend_payment_amount {
        if in_window(anchor) {
            slots.push(Slot {
                kind: Kind::Dividend,
                time: shift_business_day(anchor, bdc, cal),
                payoff: (ck(sgn) * quantity * fixed).get()?,
            });
        }
        series.clear();
    }
    if let Some(cycle) = terms.cycle_of_dividend.as_ref() {
        let eomc = terms
            .end_of_month_convention
            .unwrap_or(EndOfMonthConvention::Sd);
        let end = match (termination, horizon) {
            (Some(td), Some(tmax)) => td.min(
                tmax.checked_add_signed(Duration::seconds(1))
                    .unwrap_or(tmax),
            ),
            (Some(td), None) => td,
            (None, Some(tmax)) => tmax
                .checked_add_signed(Duration::seconds(1))
                .unwrap_or(tmax),
            (None, None) => {
                return Err(EngineError::InvalidSchedule(
                    "the STK dividend schedule S(DVANX, DVCL, tmax) is open-ended: \
                     set terminationDate or an analysis horizon"
                        .to_string(),
                ))
            }
        };
        series = rolls_before(anchor, cycle, eomc, end)?;
        if terms.next_dividend_payment_amount.is_some() {
            // `s = DVANX + DVCL`: the anchor paid the fixed amount.
            series.retain(|time| *time != anchor);
        }
    }
    for unshifted in series {
        if !in_window(unshifted) {
            continue;
        }
        let code = match terms.market_object_code_of_dividends.as_deref() {
            Some(code) => code.to_string(),
            None => terms
                .contract_id
                .as_deref()
                .map(|id| format!("{id}-DV"))
                .ok_or(EngineError::MissingAttribute("marketObjectCodeOfDividends"))?,
        };
        let time = shift_business_day(unshifted, bdc, cal);
        let at = calculation_time(bdc, unshifted, time);
        let observed = risk
            .rate(&code, at)
            .or_else(|| risk.index(&code, at))
            .ok_or_else(|| EngineError::RiskFactorMissing {
                code: code.clone(),
                at: at.to_string(),
            })?;
        slots.push(Slot {
            kind: Kind::Dividend,
            time,
            payoff: (ck(sgn) * quantity * observed).get()?,
        });
    }
    Ok(())
}

/// The per-unit price at termination: the market observation of
/// `marketObjectCode` at `TD` (rate view, then index view), else the terms
/// price `PTD`, else the observed market value `MVO`.
fn termination_unit_price(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    td: NaiveDateTime,
) -> Result<Decimal, EngineError> {
    if let Some(code) = terms.market_object_code.as_deref() {
        if let Some(observed) = risk.rate(code, td).or_else(|| risk.index(code, td)) {
            return Ok(observed);
        }
    }
    terms
        .price_at_termination_date
        .or(terms.market_value_observed)
        .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn assert_close(actual: Decimal, expected: Decimal) {
        assert!(
            (actual - expected).abs() <= dec!(0.000000001),
            "expected {expected}, actual {actual}"
        );
    }

    fn base_terms() -> serde_json::Value {
        json!({
            "contractType": "STK",
            "contractRole": "RPA",
            "statusDate": "2016-01-01T00:00:00",
            "initialExchangeDate": "2016-01-15T00:00:00",
            "purchaseDate": "2016-01-15T00:00:00",
            "quantity": "100",
            "priceAtPurchaseDate": "50",
            "priceAtTerminationDate": "55",
            "notionalPrincipal": "5000",
            "cycleAnchorDateOfDividend": "2016-02-15T00:00:00",
            "cycleOfDividend": "P1ML1",
            "nextDividendPaymentAmount": "2.5",
            "marketObjectCodeOfDividends": "ACME_DV",
            "terminationDate": "2016-04-15T00:00:00",
            "currency": "USD"
        })
    }

    /// The observed per-share dividend 2.5 of `base_terms` from its anchor
    /// on: the techspec pays `DVNP` at `DVANX` (`DV^(fix)`) and observes
    /// every later dividend.
    fn dividends() -> StateProvider {
        StateProvider::new().with_rate("ACME_DV", t("2016-02-15T00:00:00"), dec!(2.5))
    }

    /// A daily dividend schedule over 64 years (more than 20,000 rolls)
    /// pays every dividend up to the termination: the roll limit is the
    /// engine-wide `MAX_SCHEDULE_ROLLS` with an error past it, not a silent
    /// cut at 20,000 rolls.
    #[test]
    fn long_dividend_schedules_are_not_truncated() {
        let mut raw = base_terms();
        raw["cycleAnchorDateOfDividend"] = json!("2016-02-15T00:00:00");
        raw["cycleOfDividend"] = json!("P1DL1");
        raw["terminationDate"] = json!("2080-01-01T00:00:00");
        let events = StkEngine
            .evaluate(&terms(raw), &dividends())
            .expect("events");
        let dividends: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::DividendPayment)
            .collect();
        assert!(dividends.len() > 20_002, "{} dividends", dividends.len());
        assert_eq!(
            dividends.last().expect("dividends").time,
            t("2079-12-31T00:00:00")
        );
    }

    /// Buy 100 shares at 50 (payoff -5000; no notional state), two
    /// dividends of quantity x 2.5 = 250 on the DVANX/DVCL schedule (the
    /// fixed `DVNP` at the anchor, then the observed 2.5), sale at TD at 55
    /// (payoff +5500): the brief golden.
    #[test]
    fn buy_dividends_sell_matches_the_golden() {
        let events = StkEngine
            .evaluate(&terms(base_terms()), &dividends())
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2016-01-15T00:00:00")),
                (EventType::DividendPayment, t("2016-02-15T00:00:00")),
                (EventType::DividendPayment, t("2016-03-15T00:00:00")),
                (EventType::Termination, t("2016-04-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, dec!(-5000));
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].payoff, dec!(250));
        assert_eq!(events[2].payoff, dec!(250));
        assert_eq!(events[2].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[3].payoff, dec!(5500));
        assert_eq!(events[3].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[3].state.contract_status, ContractStatus::Terminated);
    }

    /// The dividend roll stops strictly before the termination date and the
    /// sale price is the live market observation when the market object
    /// carries one (per-share price convention).
    #[test]
    fn market_observation_overrides_the_terms_price_at_sale() {
        let mut raw = base_terms();
        raw["marketObjectCode"] = json!("ACME");
        let provider = dividends().with_rate("ACME", t("2016-04-15T00:00:00"), dec!(54.5));
        let events = StkEngine.evaluate(&terms(raw), &provider).expect("events");
        let td = events.last().expect("TD");
        assert_eq!(td.event_type, EventType::Termination);
        assert_eq!(td.time, t("2016-04-15T00:00:00"));
        assert_close(td.payoff, dec!(5450));
    }

    /// Without `DVCL` only the anchor pays; the third dividend roll never
    /// exists, but the position still terminates at `TD`.
    #[test]
    fn dividend_anchor_without_cycle_pays_once() {
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("cycleOfDividend");
        raw["cycleAnchorDateOfDividend"] = json!("2016-03-15T00:00:00");
        let events = StkEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let dividends: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::DividendPayment)
            .map(|e| e.time)
            .collect();
        assert_eq!(dividends, vec![t("2016-03-15T00:00:00")]);
        assert_eq!(events.len(), 3);
    }

    /// `RPL` (short position) mirrors the payoffs.
    #[test]
    fn rpl_mirrors_the_payoff_orientation() {
        let mut raw = base_terms();
        raw["contractRole"] = json!("RPL");
        let events = StkEngine
            .evaluate(&terms(raw), &dividends())
            .expect("events");
        assert_eq!(events[0].payoff, dec!(5000));
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].payoff, dec!(-250));
        assert_eq!(events[3].payoff, dec!(-5500));
        assert_eq!(events[3].state.notional_principal, Decimal::ZERO);
    }

    /// `IED` and `PRD` on different dates: the exchange emits as a
    /// zero-payoff agreement event and the price settles at the purchase
    /// date.
    #[test]
    fn differing_ied_and_prd_move_the_price_to_the_purchase_date() {
        let mut raw = base_terms();
        raw.as_object_mut()
            .unwrap()
            .remove("cycleAnchorDateOfDividend");
        raw.as_object_mut().unwrap().remove("cycleOfDividend");
        raw["initialExchangeDate"] = json!("2016-01-10T00:00:00");
        let events = StkEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2016-01-10T00:00:00")),
                (EventType::Purchase, t("2016-01-15T00:00:00")),
                (EventType::Termination, t("2016-04-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].payoff, dec!(-5000));
    }

    /// An open position without a termination date has no terminal event,
    /// and its dividend schedule `S(DVANX, DVCL, tmax)` needs the analysis
    /// horizon `tmax`: without one the schedule is open-ended and an error.
    #[test]
    fn open_position_without_termination_stays_active() {
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("terminationDate");
        assert!(matches!(
            StkEngine.evaluate(&terms(raw.clone()), &dividends()),
            Err(EngineError::InvalidSchedule(_))
        ));
        let events = StkEngine
            .evaluate(
                &terms(raw),
                &dividends().with_analysis_horizon(t("2016-05-15T00:00:00")),
            )
            .expect("events");
        let dividend_times: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::DividendPayment)
            .map(|e| e.time)
            .collect();
        assert_eq!(
            dividend_times,
            vec![
                t("2016-02-15T00:00:00"),
                t("2016-03-15T00:00:00"),
                t("2016-04-15T00:00:00"),
                t("2016-05-15T00:00:00"),
            ]
        );
        assert_eq!(
            events.last().expect("last").state.contract_status,
            ContractStatus::Active
        );
    }

    /// Without a dividend market object code or a contract identifier there
    /// is no series to observe; a missing observation is reported.
    #[test]
    fn observed_dividends_need_a_series() {
        let mut raw = base_terms();
        raw.as_object_mut()
            .unwrap()
            .remove("marketObjectCodeOfDividends");
        assert!(matches!(
            StkEngine.evaluate(&terms(raw.clone()), &dividends()),
            Err(EngineError::MissingAttribute("marketObjectCodeOfDividends"))
        ));
        raw["contractID"] = json!("acme");
        let provider = StateProvider::new().with_rate("acme-DV", t("2016-02-15T00:00:00"), dec!(1));
        let events = StkEngine
            .evaluate(&terms(raw.clone()), &provider)
            .expect("CID-DV series");
        assert_eq!(events[2].payoff, dec!(100));
        assert!(matches!(
            StkEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::RiskFactorMissing { .. })
        ));
    }

    /// stk01: a stock without `quantity` takes the dictionary default
    /// `QT = 1`, so the purchase pays the price itself.
    #[test]
    fn quantity_defaults_to_one() {
        let events = StkEngine
            .evaluate(
                &terms(json!({
                    "contractType": "STK",
                    "contractID": "stk01",
                    "contractRole": "RPA",
                    "contractDealDate": "2012-12-28T00:00:00",
                    "statusDate": "2012-12-30T00:00:00",
                    "notionalPrincipal": "1000",
                    "currency": "USD",
                    "purchaseDate": "2013-01-01T00:00:00",
                    "priceAtPurchaseDate": "1100",
                    "endOfMonthConvention": "EOM"
                })),
                &StateProvider::new(),
            )
            .expect("stk01 evaluates");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Purchase);
        assert_eq!(events[0].payoff, dec!(-1100));
    }

    /// The dividend terms of the STK testbed's stk06 (quarterly from
    /// 2013-04-01, no termination) and stk09 (yearly from 2013-01-01,
    /// `SCF` on the Monday-to-Friday calendar).
    fn observed_dividend_terms(case: &str) -> serde_json::Value {
        let mut raw = json!({
            "contractType": "STK",
            "contractID": case,
            "contractRole": "RPA",
            "contractDealDate": "2012-12-28T00:00:00",
            "statusDate": "2012-12-30T00:00:00",
            "notionalPrincipal": "1000",
            "currency": "USD",
            "endOfMonthConvention": "EOM",
            "marketObjectCodeOfDividends": format!("{}_DV", case.to_uppercase())
        });
        if case == "stk06" {
            raw["purchaseDate"] = json!("2013-01-01T00:00:00");
            raw["priceAtPurchaseDate"] = json!("1200");
            raw["cycleAnchorDateOfDividendPayment"] = json!("2013-04-01T00:00:00");
            raw["cycleOfDividendPayment"] = json!("P3ML1");
        } else {
            raw["calendar"] = json!("MF");
            raw["businessDayConvention"] = json!("SCF");
            raw["cycleAnchorDateOfDividendPayment"] = json!("2013-01-01T00:00:00");
            raw["cycleOfDividendPayment"] = json!("P1YL1");
        }
        raw
    }

    /// stk06: the `DV` schedule is `S(DVANX, DVCL, tmax)` (techspec STK
    /// schedule), so an open position pays quarterly up to the analysis
    /// horizon, each `DV` paying the observed dividend
    /// `R(CNTRL) x O(marketObjectCodeOfDividends, t)`.
    #[test]
    fn stk06_dividends_run_to_the_analysis_horizon() {
        let mut provider = StateProvider::new().with_analysis_horizon(t("2018-01-01T00:00:00"));
        for year in 2013..=2018 {
            for month in [1, 4, 7, 10] {
                let at = t(&format!("{year}-{month:02}-01T00:00:00"));
                provider = provider.with_rate("STK06_DV", at, dec!(25));
            }
        }
        let events = StkEngine
            .evaluate(&terms(observed_dividend_terms("stk06")), &provider)
            .expect("stk06 evaluates");
        assert_eq!(events.len(), 21, "PRD and 20 quarterly dividends");
        assert_eq!(events[0].event_type, EventType::Purchase);
        assert_eq!(events[1].time, t("2013-04-01T00:00:00"));
        assert_eq!(events[20].time, t("2018-01-01T00:00:00"));
        assert!(events[1..]
            .iter()
            .all(|e| e.event_type == EventType::DividendPayment && e.payoff == dec!(25)));
    }

    /// stk09: dividend dates follow the business day convention (2017-01-01
    /// is a Sunday; `SCF` pays on Monday 2017-01-02), and stk10's varying
    /// observations are paid as observed.
    #[test]
    fn stk09_dividends_shift_to_business_days() {
        let mut provider = StateProvider::new().with_analysis_horizon(t("2018-06-01T00:00:00"));
        for (year, value) in [
            (2013, 10),
            (2014, 15),
            (2015, 5),
            (2016, 20),
            (2017, 10),
            (2018, 0),
        ] {
            provider = provider.with_rate(
                "STK09_DV",
                t(&format!("{year}-01-01T00:00:00")),
                value.into(),
            );
        }
        let events = StkEngine
            .evaluate(&terms(observed_dividend_terms("stk09")), &provider)
            .expect("stk09 evaluates");
        let got: Vec<(NaiveDateTime, Decimal)> =
            events.iter().map(|e| (e.time, e.payoff)).collect();
        assert_eq!(
            got,
            vec![
                (t("2013-01-01T00:00:00"), dec!(10)),
                (t("2014-01-01T00:00:00"), dec!(15)),
                (t("2015-01-01T00:00:00"), dec!(5)),
                (t("2016-01-01T00:00:00"), dec!(20)),
                (t("2017-01-02T00:00:00"), dec!(10)),
                (t("2018-01-01T00:00:00"), dec!(0)),
            ]
        );
    }

    /// Missing required attributes are reported.
    #[test]
    fn missing_required_attributes_are_reported() {
        // An empty bag has no purchase, sale or dividend in the window: an
        // empty stream (`quantity` defaults to 1, the purchase is optional).
        let empty = ContractTerms::new(ContractType::Stk);
        assert!(StkEngine
            .evaluate(&empty, &StateProvider::new())
            .expect("empty bag")
            .is_empty());
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("priceAtPurchaseDate");
        assert!(matches!(
            StkEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("priceAtPurchaseDate"))
        ));
    }
}
