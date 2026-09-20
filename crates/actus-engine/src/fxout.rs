//! FXOUT: Foreign Exchange Outright (paper section 7.11 "FXOUT: Foreign
//! Exchange Outright"; dictionary applicability `FXOUT_*`).
//!
//! An agreement to exchange two currency amounts: `NT` in `CUR` against
//! `NT2` in `CUR2`, settled at `MD`. Per the paper the settlement payoff is
//!
//! ```text
//! X_CURS_CUR(t) x R(CNTRL) x (NT - O_rf(i, Md_t) x NT2),  i = concat(CUR2, '/', CUR)
//! ```
//!
//! so the holder with `R(CNTRL) = +1` (`RPA`) receives the `CUR` leg net of
//! the `CUR2` leg converted into `CUR` at the observed rate.
//!
//! Conventions implemented here, resolved against the paper and the
//! evaluation brief:
//!
//! - The fx observation `O_rf(i, MD)` is read through the provider's `rate`
//!   view under the market object code `concat(CUR2, "/", CUR)` (e.g.
//!   `USD/EUR` for `CUR = EUR`, `CUR2 = USD`).
//! - Fallback when the provider has no observation: the required attribute
//!   `exerciseAmount` (`XA`) is interpreted as the agreed settlement amount
//!   of the second leg expressed in `CUR` terms, so the payoff is
//!   `sgn x (NT - XA)` — a forward struck away from the market. With
//!   neither an observation nor `XA` the evaluation reports
//!   [`EngineError::RiskFactorMissing`] under the concatenated code.
//! - `IED` is the agreement: the payoff is zero, or
//!   `-sgn x premiumDiscountAtIED` when `PDIED` is set (not part of the
//!   FXOUT applicability list but honored when present).
//! - `PRD`/`TD` are honored with the PAM idiom (both are in the FXOUT
//!   applicability): a secondary-market purchase pays `-sgn x PPRD` at
//!   `purchaseDate`, and a set `terminationDate` emits a terminal `TD`
//!   paying `sgn x PTD`, zeroing the notional state and reporting
//!   [`ContractStatus::Terminated`] in place of the `MD` settlement.
//! - `settlementPeriod` (`STP`, dictionary default `P0D`) and the business
//!   day convention are out of scope: `MD` emits and observes at the raw
//!   maturity date, which reproduces the default behaviour.
//! - Events at or before the status date `t0` are not observed; without a
//!   maturity at or after `t0` the stream is empty.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractTerms, ContractType, EventType};

use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::ump::position_sign;
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
    Maturity,
}

impl Kind {
    /// The dictionary event type of the slot.
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::Purchase => EventType::Purchase,
            Kind::Termination => EventType::Termination,
            Kind::Maturity => EventType::Maturity,
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
        let sgn = position_sign(terms);
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
        if !terminated && maturity > t0 {
            slots.push(Slot {
                kind: Kind::Maturity,
                time: maturity,
            });
        }
        slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

        let mut state = ContractState::initial(terms);
        state.notional_principal = sgn * notional;
        let mut events = Vec::new();
        for slot in slots {
            let payoff = match slot.kind {
                Kind::InitialExchange => {
                    -sgn * terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO)
                }
                Kind::Purchase => {
                    let price = terms
                        .price_at_purchase_date
                        .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                    -sgn * price
                }
                Kind::Termination => {
                    let price = terms
                        .price_at_termination_date
                        .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                    state.notional_principal = Decimal::ZERO;
                    state.fee_accrued = Decimal::ZERO;
                    state.contract_status = ContractStatus::Terminated;
                    sgn * price
                }
                Kind::Maturity => {
                    let payoff =
                        sgn * (notional - settlement_rate(terms, risk, maturity)? * notional2);
                    state.notional_principal = Decimal::ZERO;
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
                currency: terms.currency.clone(),
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
    Ok(agreed / notional2)
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
            "currency2": "USD"
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
                (EventType::Maturity, t("2016-07-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, dec!(1000000));
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
        assert_eq!(events[0].state.notional_principal, dec!(-1000000));
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
}
