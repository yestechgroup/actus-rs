//! FXOUT: Foreign Exchange Outright (paper section 7.11 "FXOUT: Foreign
//! Exchange Outright"; dictionary applicability `FXOUT_*`).
//!
//! An agreement to exchange two currency amounts: `NT` in `CUR` against
//! `NT2` in `CUR2`, settled at `MD` (techspec "FXOUT: Foreign Exchange
//! Outright"; SI-007). How depends on `deliverySettlement` (`DS`,
//! dictionary default `D`):
//!
//! - `DS = D` (delivery, the default): both legs settle gross at `MD`, each
//!   in its own currency: `R(CNTRL) x NT` in `CUR` and
//!   `R(CNTRL) x (-1) x NT2` in `CUR2` (techspec `STD^(1)`, `STD^(2)`). The
//!   official testbed reports both as `MD` events (fxout01, fxout07); no fx
//!   observation is needed.
//! - `DS = S` (cash): one net `STD` in `CUR` at `MD + settlementPeriod`
//!   (shifted per `BDC`), valued at the fx rate observed at `MD`:
//!
//! ```text
//! X_CURS_CUR(t) x R(CNTRL) x (NT - O_rf(i, Md) x NT2),  i = concat(CUR2, '/', CUR)
//! ```
//!
//!   read through the provider's `rate` view (fxout03: `EUR/USD` 1.2612 at
//!   `MD` 2014-10-30, `STD` 2014-10-31 pays 738.8). Fallback when the
//!   provider has no observation: the attribute `exerciseAmount` (`XA`)
//!   as the agreed settlement amount of the second leg in `CUR` terms, so
//!   the payoff is `sgn x (NT - XA)`; with neither the evaluation reports
//!   [`EngineError::RiskFactorMissing`] under the concatenated code.
//!
//! Other conventions:
//!
//! - There is no notional state (techspec states: `Md`, `Prf`, `Sd`; the
//!   testbed reports `notionalPrincipal` 0).
//! - `PRD` pays `-sgn x PPRD` at `purchaseDate`; a `terminationDate` before
//!   `MD` emits a terminal `TD` paying `sgn x PTD` (`POF_TD_STK`) in place of
//!   the settlement and reports [`ContractStatus::Terminated`]. The testbed's
//!   fxout10 expects an unsigned `TD` for `RPL`; SI-007 records it as a
//!   testbed defect.
//! - `IED` (not in the techspec FXOUT schedule; honoured when set) is the
//!   agreement: the payoff is zero, or `-sgn x premiumDiscountAtIED`.
//! - Events at or before the status date `t0` are not observed; without a
//!   maturity at or after `t0` the stream is empty.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractTerms, ContractType, DeliverySettlement, EventType};

use crate::common::role_sign;
use crate::credit_enhancement::settlement_date;
use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the FXOUT contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct FxoutEngine;

/// FXOUT event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    Purchase,
    Termination,
    /// Gross settlement of the `CUR` leg at `MD` (`DS = D`).
    FirstLeg,
    /// Gross settlement of the `CUR2` leg at `MD` (`DS = D`).
    SecondLeg,
    /// Net cash settlement in `CUR` at `MD + STP` (`DS = S`).
    NetSettlement,
}

impl Kind {
    /// The dictionary event type of the slot.
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::Purchase => EventType::Purchase,
            Kind::Termination => EventType::Termination,
            // The techspec names the gross legs STD^(1)/STD^(2); the
            // official testbed reports them as MD (fxout01, fxout07).
            Kind::FirstLeg | Kind::SecondLeg => EventType::Maturity,
            Kind::NetSettlement => EventType::Settlement,
        }
    }

    /// The effective same-timestamp sequence rank of the slot.
    fn priority(self) -> u8 {
        sequence_rank(self.event_type())
    }
}

/// One event slot of the FXOUT stream with its emission time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    time: NaiveDateTime,
}

impl ContractEngine for FxoutEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Fxout
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let notional2 = terms
            .notional_principal2
            .ok_or(EngineError::MissingAttribute("notionalPrincipal2"))?;
        let maturity = terms
            .maturity_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("maturityDate"))?;
        let sgn = role_sign(terms);
        let t0 = ContractState::initial(terms).status_date;

        let mut slots: Vec<Slot> = Vec::new();
        if let Some(ied) = terms.initial_exchange_date.map(normalize_timestamp) {
            if ied > t0 {
                slots.push(Slot {
                    kind: Kind::InitialExchange,
                    time: ied,
                });
            }
        }
        if let Some(prd) = terms.purchase_date.map(normalize_timestamp) {
            if prd > t0 {
                slots.push(Slot {
                    kind: Kind::Purchase,
                    time: prd,
                });
            }
        }
        if let Some(td) = terms.termination_date.map(normalize_timestamp) {
            if td > t0 && td < maturity {
                slots.push(Slot {
                    kind: Kind::Termination,
                    time: td,
                });
            }
        }
        let terminated = slots.iter().any(|slot| slot.kind == Kind::Termination);
        let net = terms.delivery_settlement == Some(DeliverySettlement::S);
        if !terminated && maturity > t0 {
            if net {
                slots.push(Slot {
                    kind: Kind::NetSettlement,
                    time: settlement_date(terms, maturity)?,
                });
            } else {
                // Inserted in this order; the stable sort keeps CUR first.
                for kind in [Kind::FirstLeg, Kind::SecondLeg] {
                    slots.push(Slot {
                        kind,
                        time: maturity,
                    });
                }
            }
        }
        slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

        // FXOUT carries no notional state: the techspec states are `Md`,
        // `Prf` and `Sd`, and the official testbed reports
        // `notionalPrincipal` 0 after every event.
        let mut state = ContractState::initial(terms);
        let mut events = Vec::new();
        let currency2 = terms.currency2.clone();
        for slot in slots {
            let mut currency = terms.currency.clone();
            let payoff = match slot.kind {
                Kind::InitialExchange => {
                    (-ck(sgn) * terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO)).get()?
                }
                Kind::Purchase => {
                    let price = terms
                        .price_at_purchase_date
                        .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                    (-ck(sgn) * price).get()?
                }
                Kind::Termination => {
                    // POF_TD_STK = R(CNTRL) x PTD. fxout10 (RPL) expects +100,
                    // a testbed defect (SI-007): the same function is
                    // role-signed in stk05 and com04.
                    let price = terms
                        .price_at_termination_date
                        .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                    state.fee_accrued = Decimal::ZERO;
                    state.contract_status = ContractStatus::Terminated;
                    (ck(sgn) * price).get()?
                }
                Kind::FirstLeg => {
                    state.contract_status = ContractStatus::Matured;
                    (ck(sgn) * notional).get()?
                }
                Kind::SecondLeg => {
                    currency.clone_from(&currency2);
                    state.fee_accrued = Decimal::ZERO;
                    state.contract_status = ContractStatus::Matured;
                    (-ck(sgn) * notional2).get()?
                }
                Kind::NetSettlement => {
                    let rate = settlement_rate(terms, risk, maturity)?;
                    let payoff = (ck(sgn) * (ck(notional) - ck(rate) * notional2)).get()?;
                    state.fee_accrued = Decimal::ZERO;
                    state.contract_status = ContractStatus::Matured;
                    payoff
                }
            };
            state.status_date = slot.time;
            events.push(ContractEvent {
                event_type: slot.kind.event_type(),
                time: slot.time,
                payoff,
                currency,
                state: state.clone(),
            });
        }
        Ok(events)
    }
}

/// The settlement rate applied to the second leg: the fx observation under
/// `concat(CUR2, "/", CUR)` at the maturity date, else the agreed
/// `exerciseAmount` (`XA`) as the second-leg settlement amount in `CUR`
/// terms (which is equivalent to observing `XA / NT2`).
fn settlement_rate(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    maturity: NaiveDateTime,
) -> Result<Decimal, EngineError> {
    let currency = terms.currency.clone().unwrap_or_default();
    let currency2 = terms
        .currency2
        .clone()
        .ok_or(EngineError::MissingAttribute("currency2"))?;
    let code = format!("{currency2}/{currency}");
    if let Some(observed) = risk.rate(&code, maturity) {
        return Ok(observed);
    }
    let notional2 = terms
        .notional_principal2
        .ok_or(EngineError::MissingAttribute("notionalPrincipal2"))?;
    let agreed = terms
        .exercise_amount
        .ok_or(EngineError::RiskFactorMissing {
            code,
            at: maturity.to_string(),
        })?;
    (ck(agreed) / notional2).get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use chrono::NaiveDateTime;
    use rust_decimal::Decimal;
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
            "contractType": "FXOUT",
            "contractRole": "RPA",
            "statusDate": "2016-01-01T00:00:00",
            "initialExchangeDate": "2016-01-15T00:00:00",
            "maturityDate": "2016-07-15T00:00:00",
            "notionalPrincipal": "1000000",
            "notionalPrincipal2": "920000",
            "currency": "EUR",
            "currency2": "USD",
            "deliverySettlement": "S"
        })
    }

    /// The market-observed settlement: fx `USD/EUR` = 1.08 at `MD` converts
    /// the USD leg into 1.08 x 920000 = 993600 EUR, so the `RPA` holder
    /// receives 1000000 - 993600 = 6400 EUR (paper §7.11 payoff, R=+1).
    #[test]
    fn market_observed_settlement_pays_the_net_leg() {
        let risk = StateProvider::new().with_rate("USD/EUR", t("2016-07-15T00:00:00"), dec!(1.08));
        let events = FxoutEngine
            .evaluate(&terms(base_terms()), &risk)
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2016-01-15T00:00:00")),
                (EventType::Settlement, t("2016-07-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
        assert_close(events[1].payoff, dec!(6400));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].state.contract_status, ContractStatus::Matured);
    }

    /// `RPL` mirrors the settlement payoff; the observation is a step
    /// function, so a rate quoted before `MD` still applies at `MD`.
    #[test]
    fn rpl_mirrors_the_settlement_payoff() {
        let mut raw = base_terms();
        raw["contractRole"] = json!("RPL");
        let risk = StateProvider::new().with_rate("USD/EUR", t("2016-01-10T00:00:00"), dec!(1.08));
        let events = FxoutEngine.evaluate(&terms(raw), &risk).expect("events");
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
        assert_close(events[1].payoff, dec!(-6400));
    }

    /// Without a market observation the agreed `exerciseAmount` (the second
    /// leg settlement in `CUR` terms) settles the contract; a set
    /// `premiumDiscountAtIED` is paid at the agreement.
    #[test]
    fn exercise_amount_fallback_settles_the_agreed_leg() {
        let mut raw = base_terms();
        raw["exerciseAmount"] = json!("950000");
        raw["premiumDiscountAtIED"] = json!("100");
        let events = FxoutEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(events[0].payoff, dec!(-100));
        assert_close(events[1].payoff, dec!(50000));
        assert_eq!(events[1].state.contract_status, ContractStatus::Matured);
    }

    /// Neither an observation nor `XA` is a missing risk factor, reported
    /// under the concatenated market object code.
    #[test]
    fn missing_observation_and_exercise_amount_is_an_error() {
        let err = FxoutEngine
            .evaluate(&terms(base_terms()), &StateProvider::new())
            .unwrap_err();
        match err {
            EngineError::RiskFactorMissing { code, at } => {
                assert_eq!(code, "USD/EUR");
                assert_eq!(at, t("2016-07-15T00:00:00").to_string());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// A secondary-market purchase pays the price, and a termination before
    /// the maturity supplants the settlement with the termination price.
    #[test]
    fn purchase_and_termination_follow_the_pam_idiom() {
        let mut raw = base_terms();
        raw["purchaseDate"] = json!("2016-02-01T00:00:00");
        raw["priceAtPurchaseDate"] = json!("5000");
        raw["terminationDate"] = json!("2016-06-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("20000");
        let events = FxoutEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![
                EventType::InitialExchange,
                EventType::Purchase,
                EventType::Termination
            ]
        );
        assert_eq!(events[1].payoff, dec!(-5000));
        assert_eq!(events[2].payoff, dec!(20000));
        assert_eq!(events[2].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[2].state.contract_status, ContractStatus::Terminated);
    }

    /// Missing required attributes are reported.
    #[test]
    fn missing_required_attributes_are_reported() {
        let empty = ContractTerms::new(ContractType::Fxout);
        assert!(matches!(
            FxoutEngine.evaluate(&empty, &StateProvider::new()),
            Err(EngineError::MissingAttribute("notionalPrincipal"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("notionalPrincipal2");
        assert!(matches!(
            FxoutEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("notionalPrincipal2"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("maturityDate");
        assert!(matches!(
            FxoutEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("maturityDate"))
        ));
    }

    /// fxout08: FXOUT has no notional state (techspec FXOUT states: `Md`,
    /// `Prf`, `Sd`); the testbed reports `notionalPrincipal` 0 after the
    /// purchase and the termination.
    #[test]
    fn fxout08_has_no_notional_state() {
        let events = FxoutEngine
            .evaluate(
                &terms(json!({
                    "contractType": "FXOUT",
                    "contractID": "fxout08",
                    "contractRole": "RPA",
                    "currency": "USD",
                    "currency2": "EUR",
                    "maturityDate": "2014-12-31T23:59:59",
                    "notionalPrincipal": "2000",
                    "notionalPrincipal2": "1000",
                    "statusDate": "2013-12-30T00:00:00",
                    "purchaseDate": "2014-04-01T00:00:00",
                    "priceAtPurchaseDate": "50",
                    "terminationDate": "2014-09-30T23:59:59",
                    "priceAtTerminationDate": "100"
                })),
                &StateProvider::new(),
            )
            .expect("fxout08 evaluates");
        let got: Vec<(EventType, Decimal, Decimal)> = events
            .iter()
            .map(|e| (e.event_type, e.payoff, e.state.notional_principal))
            .collect();
        assert_eq!(
            got,
            vec![
                (EventType::Purchase, dec!(-50), Decimal::ZERO),
                (EventType::Termination, dec!(100), Decimal::ZERO),
            ]
        );
    }

    fn fxout_testbed_terms(role: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut raw = json!({
            "contractType": "FXOUT",
            "contractRole": role,
            "currency": "USD",
            "currency2": "EUR",
            "maturityDate": "2014-12-31T23:59:59",
            "notionalPrincipal": "2000",
            "notionalPrincipal2": "1000",
            "statusDate": "2013-12-30T00:00:00"
        });
        for (key, value) in extra.as_object().expect("object") {
            raw[key] = value.clone();
        }
        raw
    }

    /// fxout01: with `deliverySettlement` unset (dictionary default `D`,
    /// delivery) both legs settle gross at `MD`, each in its own currency:
    /// `R(CNTRL) x NT` in `CUR` and `R(CNTRL) x (-1) x NT2` in `CUR2`
    /// (techspec `STD^(1)`, `STD^(2)`; the testbed names them `MD`). No
    /// fx observation is needed.
    #[test]
    fn fxout01_delivery_settles_both_legs_gross() {
        let events = FxoutEngine
            .evaluate(
                &terms(fxout_testbed_terms("RPA", json!({}))),
                &StateProvider::new(),
            )
            .expect("fxout01 evaluates");
        let got: Vec<(EventType, NaiveDateTime, Decimal, Option<String>)> = events
            .iter()
            .map(|e| (e.event_type, e.time, e.payoff, e.currency.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    EventType::Maturity,
                    t("2015-01-01T00:00:00"),
                    dec!(2000),
                    Some("USD".into())
                ),
                (
                    EventType::Maturity,
                    t("2015-01-01T00:00:00"),
                    dec!(-1000),
                    Some("EUR".into())
                ),
            ]
        );
    }

    /// fxout03: `DS = S` settles net in `CUR` at `MD + settlementPeriod`
    /// (`STD`), at the rate observed at `MD`:
    /// `R(CNTRL) x (NT - O(CUR2/CUR, MD) x NT2) = 2000 - 1.2612 x 1000`.
    #[test]
    fn fxout03_cash_settles_net_after_the_settlement_period() {
        let raw = fxout_testbed_terms(
            "RPA",
            json!({
                "maturityDate": "2014-10-30T00:00:00",
                "deliverySettlement": "S",
                "settlementPeriod": "P1DL0"
            }),
        );
        let risk =
            StateProvider::new().with_rate("EUR/USD", t("2014-10-30T00:00:00"), dec!(1.2612));
        let events = FxoutEngine
            .evaluate(&terms(raw), &risk)
            .expect("fxout03 evaluates");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Settlement);
        assert_eq!(events[0].time, t("2014-10-31T00:00:00"));
        assert_eq!(events[0].payoff, dec!(738.8));
        assert_eq!(events[0].currency.as_deref(), Some("USD"));
    }
}
