//! COM: Commodity (paper section 7.10 "COM: Commodity"; dictionary
//! applicability `COM_*`).
//!
//! A physical or synthetic commodity holding: the paper defines it as
//! "similar to STK", i.e. the equity position engine minus the dividend
//! schedule. The evaluation therefore shares the STK implementation
//! ([`crate::stk::evaluate_position`]) with [`Dividends::None`]:
//!
//! - Entry: the purchase cash flow `-sgn x QT x PPRD` settles at
//!   `purchaseDate` (required), or at `initialExchangeDate` when set and
//!   coinciding; differing `IED`/`PRD` split into a zero-payoff exchange
//!   event and a purchase event carrying the price.
//! - Sale: the terminal `TD` at `terminationDate` sells at
//!   `sgn x QT x PTD` per unit, zeroing the notional state and reporting
//!   [`ContractStatus::Terminated`]. The per-unit sale price resolves as
//!   the market observation of `marketObjectCode` at `TD` (per-unit price
//!   convention, `rate` view then `index` view of the provider), then the
//!   terms `priceAtTerminationDate`, then `marketValueObserved`.
//! - The notional state is the position notional `sgn x QT x PPRD`; the
//!   `unit` attribute (`UT`, e.g. barrels) is descriptive and carries no
//!   arithmetic role.
//! - Without a termination date the stream ends after the entry with status
//!   [`ContractStatus::Active`] (an open holding has no terminal event).
//! - Events at or before the status date `t0` are not observed.

use actus_model::{ContractTerms, ContractType};

use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::stk::{evaluate_position, Dividends};
use crate::EngineError;

/// Implementation of the COM contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComEngine;

impl ContractEngine for ComEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Com
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        evaluate_position(terms, risk, Dividends::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use crate::state::ContractStatus;
    use actus_model::EventType;
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
            "contractType": "COM",
            "contractRole": "RPA",
            "statusDate": "2016-01-01T00:00:00",
            "purchaseDate": "2016-01-15T00:00:00",
            "quantity": "100",
            "unit": "bbl",
            "priceAtPurchaseDate": "50",
            "priceAtTerminationDate": "52",
            "terminationDate": "2016-06-15T00:00:00",
            "currency": "USD"
        })
    }

    /// The symmetric buy/sell golden: buy 100 units at 50 (payoff -5000,
    /// position notional +5000), sell at 52 (payoff +5200).
    #[test]
    fn buy_and_sell_match_the_golden() {
        let events = ComEngine
            .evaluate(&terms(base_terms()), &StateProvider::new())
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::Purchase, t("2016-01-15T00:00:00")),
                (EventType::Termination, t("2016-06-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, dec!(-5000));
        assert_eq!(events[0].state.notional_principal, dec!(5000));
        assert_eq!(events[1].payoff, dec!(5200));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].state.contract_status, ContractStatus::Terminated);
    }

    /// No dividend events ever occur: COM is STK minus dividends.
    #[test]
    fn dividend_attributes_are_ignored() {
        let mut raw = base_terms();
        raw["contractType"] = json!("COM");
        raw["cycleAnchorDateOfDividend"] = json!("2016-03-15T00:00:00");
        raw["cycleOfDividend"] = json!("P1ML1");
        raw["nextDividendPaymentAmount"] = json!("2.5");
        let events = ComEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert!(events
            .iter()
            .all(|e| e.event_type != EventType::DividendPayment));
        assert_eq!(events.len(), 2);
    }

    /// The sale price is the live market observation when the market object
    /// carries one (per-unit price convention, rate view before index view).
    #[test]
    fn market_observation_overrides_the_terms_price_at_sale() {
        let mut raw = base_terms();
        raw["marketObjectCode"] = json!("BRENT");
        let provider =
            StateProvider::new().with_rate("BRENT", t("2016-06-15T00:00:00"), dec!(51.5));
        let events = ComEngine.evaluate(&terms(raw), &provider).expect("events");
        assert_close(events.last().expect("TD").payoff, dec!(5150));
    }

    /// `RPL` (short position) mirrors the payoffs and the notional state.
    #[test]
    fn rpl_mirrors_the_payoff_orientation() {
        let mut raw = base_terms();
        raw["contractRole"] = json!("RPL");
        let events = ComEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(events[0].payoff, dec!(5000));
        assert_eq!(events[0].state.notional_principal, dec!(-5000));
        assert_eq!(events[1].payoff, dec!(-5200));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
    }

    /// Differing `IED` and `PRD` split into a zero-payoff exchange event and
    /// a purchase event carrying the price.
    #[test]
    fn differing_ied_and_prd_move_the_price_to_the_purchase_date() {
        let mut raw = base_terms();
        raw["initialExchangeDate"] = json!("2016-01-10T00:00:00");
        let events = ComEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(events[0].event_type, EventType::InitialExchange);
        assert_eq!(events[0].time, t("2016-01-10T00:00:00"));
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, dec!(5000));
        assert_eq!(events[1].event_type, EventType::Purchase);
        assert_eq!(events[1].payoff, dec!(-5000));
    }

    /// An open holding without a termination date has no terminal event and
    /// missing required attributes are reported.
    #[test]
    fn open_position_and_missing_attributes() {
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("terminationDate");
        let events = ComEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state.contract_status, ContractStatus::Active);

        let empty = ContractTerms::new(ContractType::Com);
        assert!(matches!(
            ComEngine.evaluate(&empty, &StateProvider::new()),
            Err(EngineError::MissingAttribute("quantity"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("priceAtPurchaseDate");
        assert!(matches!(
            ComEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("priceAtPurchaseDate"))
        ));
    }
}
