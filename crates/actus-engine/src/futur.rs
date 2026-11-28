//! FUTUR: Future (techspec "FUTUR: Future"; SI-008).
//!
//! A future on an observed underlying. The schedule and states are "Same as
//! OPTNS", so the event stream, the underlying observation and the
//! settlement are shared with OPTNS ([`crate::exercise`]): `PRD`, `MD`,
//! `XD` fixing the exercise amount `Xa = S_t - PFUT` (techspec
//! `STF_XD_FUTUR`; negative for a losing long), and `STD` paying
//! `R(CNTRL) x Xa` after the settlement period.
//!
//! The techspec FUTUR functions do not branch on `deliverySettlement`:
//! every future settles the amount `Xa`, the testbed's `DS = S` (cash, the
//! dictionary's `cashSettlement`) as well as physical delivery `DS = D`,
//! which is not modelled beyond that amount.

use actus_model::{ContractTerms, ContractType};

use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::exercise::evaluate_exercise;
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::EngineError;

/// Implementation of the FUTUR contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct FuturEngine;

impl ContractEngine for FuturEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Futur
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let futures_price = terms
            .futures_price
            .ok_or(EngineError::MissingAttribute("futuresPrice"))?;
        evaluate_exercise(terms, risk, &|price| (ck(price) - futures_price).get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use actus_model::EventType;
    use chrono::NaiveDateTime;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The FUTUR testbed's shape: a future on AAPL at 80, bought on
    /// 2020-01-02 for 10, with `extra` overriding the terms.
    fn future(extra: serde_json::Value) -> ContractTerms {
        let mut raw = json!({
            "contractType": "FUTUR",
            "contractID": "future",
            "contractRole": "BUY",
            "currency": "USD",
            "calendar": "NC",
            "contractDealDate": "2020-01-01T00:00:00",
            "statusDate": "2020-01-01T00:00:00",
            "purchaseDate": "2020-01-02T00:00:00",
            "priceAtPurchaseDate": "10",
            "maturityDate": "2020-03-30T00:00:00",
            "futuresPrice": " 80",
            "settlementPeriod": "P0D",
            "deliverySettlement": "S",
            "contractStructure": [{
                "object": {"marketObjectCode": "AAPL"},
                "referenceType": "MOC",
                "referenceRole": "UDL"
            }]
        });
        for (key, value) in extra.as_object().expect("object") {
            raw[key] = value.clone();
        }
        serde_json::from_value(raw).expect("terms")
    }

    fn aapl() -> StateProvider {
        StateProvider::new()
            .with_index("AAPL", t("2020-03-30T00:00:00"), dec!(63.70))
            .with_index("AAPL", t("2020-09-30T00:00:00"), dec!(115.81))
    }

    fn stream(events: &[ContractEvent]) -> Vec<(EventType, NaiveDateTime, Decimal)> {
        events
            .iter()
            .map(|e| (e.event_type, e.time, e.payoff))
            .collect()
    }

    /// future01: the long future settles `S - PFUT = 63.70 - 80 = -16.30`
    /// in cash (`DS = S`) at `STD`, after `MD` and `XD`.
    #[test]
    fn future01_settles_a_loss_in_cash() {
        let events = FuturEngine
            .evaluate(&future(json!({})), &aapl())
            .expect("events");
        assert_eq!(
            stream(&events),
            vec![
                (EventType::Purchase, t("2020-01-02T00:00:00"), dec!(-10)),
                (EventType::Maturity, t("2020-03-30T00:00:00"), Decimal::ZERO),
                (EventType::Exercise, t("2020-03-30T00:00:00"), Decimal::ZERO),
                (
                    EventType::Settlement,
                    t("2020-03-30T00:00:00"),
                    dec!(-16.30)
                ),
            ]
        );
        assert!(events.iter().all(|e| e.state.notional_principal.is_zero()));
    }

    /// future07: the short future settles `-(115.81 - 80)` after `P3D`,
    /// moved off the weekend by `CSP` on the Monday-to-Friday calendar.
    #[test]
    fn future07_short_settles_on_the_preceding_business_day() {
        let terms = future(json!({
            "contractRole": "SEL",
            "maturityDate": "2020-09-30T00:00:00",
            "calendar": "MF",
            "businessDayConvention": "CSP",
            "settlementPeriod": "P3D"
        }));
        let events = FuturEngine.evaluate(&terms, &aapl()).expect("events");
        assert_eq!(events[0].payoff, dec!(10));
        let std = events.last().expect("STD");
        assert_eq!(std.time, t("2020-10-02T00:00:00"));
        assert_eq!(std.payoff, dec!(-35.81));
    }

    /// future13: exercised at the status date with `XA = 10.33`, only the
    /// settlement at `XD + P5D` is left.
    #[test]
    fn future13_exercised_before_the_status_date_only_settles() {
        let terms = future(json!({
            "statusDate": "2020-09-30T00:00:00",
            "maturityDate": "2020-09-30T00:00:00",
            "exerciseDate": "2020-09-30T00:00:00",
            "exerciseAmount": "10.33",
            "settlementPeriod": "P5D"
        }));
        let events = FuturEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        assert_eq!(
            stream(&events),
            vec![(EventType::Settlement, t("2020-10-05T00:00:00"), dec!(10.33))]
        );
    }

    /// The agreed futures price is required; an unobserved underlying is
    /// reported.
    #[test]
    fn missing_terms_and_observations_are_reported() {
        let mut no_price = future(json!({}));
        no_price.futures_price = None;
        assert!(matches!(
            FuturEngine.evaluate(&no_price, &aapl()),
            Err(EngineError::MissingAttribute("futuresPrice"))
        ));
        assert!(matches!(
            FuturEngine.evaluate(&future(json!({})), &StateProvider::new()),
            Err(EngineError::RiskFactorMissing { .. })
        ));
    }
}
