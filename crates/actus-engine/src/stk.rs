//! STK: Stock (paper section 7.9 "STK: Stock"; dictionary applicability
//! `STK_*`).
//!
//! An equity position. The economic driver is the share count `quantity`
//! (`QT`): every payoff multiplies the quantity by a per-share price, and
//! the notional state carries the resulting position notional
//! `sgn x QT x PPRD`. The dictionary lists `notionalPrincipal` as required
//! for STK, but it is descriptive there (the position notional at entry);
//! the engine derives it from `quantity x priceAtPurchaseDate`, which is
//! the only reading consistent with the quantity-based dividend and sale
//! payoffs.
//!
//! Conventions implemented here, resolved against the paper and the sibling
//! engines:
//!
//! - Entry: the purchase cash flow `-sgn x QT x PPRD` settles at
//!   `purchaseDate` (`PRD`, required by the applicability) or, when
//!   `initialExchangeDate` is set, at the exchange date if both coincide.
//!   When `IED` and `PRD` differ, `IED` emits as a zero-payoff agreement
//!   event and the price settles at `PRD`.
//! - Dividends (`DV`): the `DVANX`/`DVCL` schedule (paper: "Dividend events
//!   based on `DVANX`, `DVCL`") pays `sgn x QT x DVNP` per period, reading
//!   `nextDividendPaymentAmount` as the fixed per-period amount multiplied
//!   by the share count. The series unrolls from the anchor while strictly
//!   before the termination date; without `DVCL` only the anchor pays.
//!   `DVNP` missing pays zero (the schedule point is still reported).
//! - Sale: the terminal `TD` at `terminationDate` sells the position at
//!   `sgn x QT x PTD`, zeroes the notional and reports
//!   [`ContractStatus::Terminated`]. The per-unit sale price resolves as
//!   the market observation of `marketObjectCode` at `TD` (paper: "Payoffs
//!   driven by market observations"; convention: the market object carries
//!   the per-share price, observed through the provider's `rate` view and
//!   falling back to its `index` view), then `priceAtTerminationDate`, then
//!   `marketValueObserved`; with none present the evaluation reports
//!   [`EngineError::MissingAttribute`].
//! - Without a termination date the stream ends after the last dividend
//!   with status [`ContractStatus::Active`] (an open position has no
//!   terminal event). The ex-dividend date `DVEX` (no-accrual window) is
//!   out of scope: dividends are attributed to their schedule dates.
//! - Events at or before the status date `t0` are not observed (the CLM
//!   analysis-window idiom).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractTerms, ContractType, EndOfMonthConvention, EventType};

use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::{ContractState, ContractStatus};
use crate::ump::position_sign;
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
    let quantity = terms
        .quantity
        .ok_or(EngineError::MissingAttribute("quantity"))?;
    let entry_price = terms
        .price_at_purchase_date
        .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
    let sgn = position_sign(terms);
    let t0 = ContractState::initial(terms).status_date;
    let ied = terms.initial_exchange_date.map(normalize_timestamp);
    let purchase = terms.purchase_date.map(normalize_timestamp);
    let termination = terms.termination_date.map(normalize_timestamp);
    let entry_time = ied
        .or(purchase)
        .ok_or(EngineError::MissingAttribute("purchaseDate"))?;

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
                    -sgn * quantity * entry_price
                },
            });
        }
    }
    if let Some(prd) = purchase {
        if prd > t0 && price_moves_with_purchase {
            slots.push(Slot {
                kind: Kind::Purchase,
                time: prd,
                payoff: -sgn * quantity * entry_price,
            });
        }
    }
    if dividends == Dividends::Paid {
        dividend_slots(
            terms,
            quantity,
            sgn,
            entry_time,
            t0,
            termination,
            &mut slots,
        );
    }
    if let Some(td) = termination {
        if td > t0 {
            let price = termination_unit_price(terms, risk, td)?;
            slots.push(Slot {
                kind: Kind::Termination,
                time: td,
                payoff: sgn * quantity * price,
            });
        }
    }
    slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

    let mut state = ContractState::initial(terms);
    let mut events = Vec::new();
    for slot in slots {
        match slot.kind {
            Kind::InitialExchange | Kind::Purchase => {
                state.notional_principal = sgn * quantity * entry_price;
            }
            Kind::Termination => {
                state.notional_principal = Decimal::ZERO;
                state.contract_status = ContractStatus::Terminated;
            }
            Kind::Dividend => {}
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

/// Builds the `DV` dividend slots (paper section 7.9, "DV Schedule"):
/// unrolls from `DVANX` by `DVCL` while strictly before the termination
/// date; without a cycle the anchor pays once. Dividends at or before the
/// entry or at or before the status date are not observed.
fn dividend_slots(
    terms: &ContractTerms,
    quantity: Decimal,
    sgn: Decimal,
    entry: NaiveDateTime,
    t0: NaiveDateTime,
    termination: Option<NaiveDateTime>,
    slots: &mut Vec<Slot>,
) {
    let Some(anchor) = terms.cycle_anchor_date_of_dividend.map(normalize_timestamp) else {
        return;
    };
    let amount = terms.next_dividend_payment_amount.unwrap_or(Decimal::ZERO);
    let payoff = sgn * quantity * amount;
    let rolls = match terms.cycle_of_dividend.as_ref() {
        Some(cycle) => {
            let eomc = terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd);
            let mut rolls = vec![anchor];
            if let Some(termination) = termination {
                let mut index: u64 = 0;
                loop {
                    index += 1;
                    let next = cycle_step(anchor, index, cycle, eomc);
                    if next >= termination {
                        break;
                    }
                    rolls.push(next);
                    if index > 20_000 {
                        break;
                    }
                }
            }
            rolls
        }
        None => vec![anchor],
    };
    for time in rolls {
        if time <= entry || time <= t0 {
            continue;
        }
        if termination.is_some_and(|td| time >= td) {
            continue;
        }
        slots.push(Slot {
            kind: Kind::Dividend,
            time,
            payoff,
        });
    }
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
            "terminationDate": "2016-04-15T00:00:00",
            "currency": "USD"
        })
    }

    /// Buy 100 shares at 50 (payoff -5000, position notional +5000), two
    /// dividends of quantity x 2.5 = 250 on the DVANX/DVCL schedule, sale at
    /// TD at 55 (payoff +5500): the brief golden.
    #[test]
    fn buy_dividends_sell_matches_the_golden() {
        let events = StkEngine
            .evaluate(&terms(base_terms()), &StateProvider::new())
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
        assert_eq!(events[0].state.notional_principal, dec!(5000));
        assert_eq!(events[1].payoff, dec!(250));
        assert_eq!(events[2].payoff, dec!(250));
        assert_eq!(events[2].state.notional_principal, dec!(5000));
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
        let provider = StateProvider::new().with_rate("ACME", t("2016-04-15T00:00:00"), dec!(54.5));
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

    /// `RPL` (short position) mirrors the payoffs and the notional state.
    #[test]
    fn rpl_mirrors_the_payoff_orientation() {
        let mut raw = base_terms();
        raw["contractRole"] = json!("RPL");
        let events = StkEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(events[0].payoff, dec!(5000));
        assert_eq!(events[0].state.notional_principal, dec!(-5000));
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
        assert_eq!(events[0].state.notional_principal, dec!(5000));
        assert_eq!(events[1].payoff, dec!(-5000));
    }

    /// An open position without a termination date has no terminal event.
    #[test]
    fn open_position_without_termination_stays_active() {
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("terminationDate");
        let events = StkEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(
            events.last().expect("last").event_type,
            EventType::DividendPayment
        );
        assert_eq!(
            events.last().expect("last").state.contract_status,
            ContractStatus::Active
        );
    }

    /// Missing required attributes are reported.
    #[test]
    fn missing_required_attributes_are_reported() {
        let empty = ContractTerms::new(ContractType::Stk);
        assert!(matches!(
            StkEngine.evaluate(&empty, &StateProvider::new()),
            Err(EngineError::MissingAttribute("quantity"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("priceAtPurchaseDate");
        assert!(matches!(
            StkEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("priceAtPurchaseDate"))
        ));
    }
}
