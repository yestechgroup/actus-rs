//! OPTNS: Option (techspec "OPTNS: Option"; SI-008).
//!
//! A cash-settled option on an observed underlying. The event stream, the
//! underlying observation and the settlement are shared with FUTUR
//! ([`crate::exercise`]): `PRD` (the premium), `MD`, `XD` fixing the
//! exercise amount `Xa` at the exercise date, and `STD` paying
//! `R(CNTRL) x Xa` after the settlement period.
//!
//! The exercise amount of the underlying price `S_t` (techspec `STF_XD`):
//!
//! - call (`OPTP = C`): `max(S_t - OPS1, 0)`;
//! - put (`OPTP = P`): `max(OPS1 - S_t, 0)`;
//! - call/put (`OPTP = CP`): `max(S_t - OPS1, 0) + max(OPS2 - S_t, 0)`,
//!   the call struck at `optionStrike1` and the put at `optionStrike2`.

use rust_decimal::Decimal;

use actus_model::enums::OptionType;
use actus_model::{ContractTerms, ContractType};

use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::exercise::evaluate_exercise;
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::EngineError;

/// Implementation of the OPTNS contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct OptnsEngine;

impl ContractEngine for OptnsEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Optns
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let option_type = terms
            .option_type
            .ok_or(EngineError::MissingAttribute("optionType"))?;
        let strike = terms
            .option_strike1
            .ok_or(EngineError::MissingAttribute("optionStrike1"))?;
        let put_strike = match option_type {
            OptionType::CallPut => Some(
                terms
                    .option_strike2
                    .ok_or(EngineError::MissingAttribute("optionStrike2"))?,
            ),
            OptionType::Call | OptionType::Put => None,
        };
        evaluate_exercise(terms, risk, &|price| {
            let call = (ck(price) - strike).get()?.max(Decimal::ZERO);
            let put = |strike: Decimal| {
                Ok::<_, EngineError>((ck(strike) - price).get()?.max(Decimal::ZERO))
            };
            match (option_type, put_strike) {
                (OptionType::Call, _) => Ok(call),
                (OptionType::Put, _) => put(strike),
                (OptionType::CallPut, Some(put_strike)) => (ck(call) + put(put_strike)?).get(),
                (OptionType::CallPut, None) => Err(EngineError::MissingAttribute("optionStrike2")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use crate::state::ContractStatus;
    use actus_model::EventType;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The OPTNS testbed's shape: a European option on AAPL, bought on
    /// 2020-01-02, with `extra` overriding the terms.
    fn option(extra: serde_json::Value) -> ContractTerms {
        let mut raw = json!({
            "contractType": "OPTNS",
            "contractID": "option",
            "contractRole": "BUY",
            "currency": "USD",
            "calendar": "NC",
            "contractDealDate": "2020-01-01T00:00:00",
            "statusDate": "2020-01-01T00:00:00",
            "purchaseDate": "2020-01-02T00:00:00",
            "priceAtPurchaseDate": "10",
            "maturityDate": "2020-09-30T00:00:00",
            "optionExerciseType": "E",
            "optionType": "C",
            "optionStrike1": " 80",
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

    /// AAPL as observed in the OPTNS testbed.
    fn aapl() -> StateProvider {
        StateProvider::new()
            .with_index("AAPL", t("2020-03-30T00:00:00"), dec!(63.70))
            .with_index("AAPL", t("2020-06-30T00:00:00"), dec!(91.20))
            .with_index("AAPL", t("2020-09-30T00:00:00"), dec!(115.81))
    }

    fn stream(events: &[ContractEvent]) -> Vec<(EventType, NaiveDateTime, Decimal)> {
        events
            .iter()
            .map(|e| (e.event_type, e.time, e.payoff))
            .collect()
    }

    /// option03: the bought call pays `max(115.81 - 80, 0) = 35.81` at `STD`
    /// after `MD` and `XD` on the maturity date; the premium is `-PPRD`.
    #[test]
    fn option03_call_settles_its_exercise_amount() {
        let events = OptnsEngine
            .evaluate(&option(json!({})), &aapl())
            .expect("events");
        assert_eq!(
            stream(&events),
            vec![
                (EventType::Purchase, t("2020-01-02T00:00:00"), dec!(-10)),
                (EventType::Maturity, t("2020-09-30T00:00:00"), Decimal::ZERO),
                (EventType::Exercise, t("2020-09-30T00:00:00"), Decimal::ZERO),
                (EventType::Settlement, t("2020-09-30T00:00:00"), dec!(35.81)),
            ]
        );
        assert!(events.iter().all(|e| e.state.notional_principal.is_zero()));
        assert_eq!(
            events.last().expect("STD").state.contract_status,
            ContractStatus::Matured
        );
    }

    /// option04 and option06: the seller receives the premium and pays the
    /// settlement, which moves by `STP` and the business day convention
    /// (2020-10-03 is a Saturday; `CSF` settles on Monday).
    #[test]
    fn option06_seller_settles_after_the_shifted_settlement_period() {
        let terms = option(json!({
            "contractRole": "SEL",
            "calendar": "MF",
            "businessDayConvention": "CSF",
            "settlementPeriod": "P3D"
        }));
        let events = OptnsEngine.evaluate(&terms, &aapl()).expect("events");
        assert_eq!(events[0].payoff, dec!(10));
        let std = events.last().expect("STD");
        assert_eq!(std.event_type, EventType::Settlement);
        assert_eq!(std.time, t("2020-10-05T00:00:00"));
        assert_eq!(std.payoff, dec!(-35.81));
    }

    /// option01, option15: an out-of-the-money option still reports `XD`
    /// and a zero `STD`; the put pays `max(80 - 63.70, 0) = 16.30`.
    #[test]
    fn option15_put_and_option01_zero_exercise() {
        let call = option(json!({"maturityDate": "2020-03-30T00:00:00"}));
        let events = OptnsEngine.evaluate(&call, &aapl()).expect("events");
        assert_eq!(events.len(), 4);
        assert_eq!(events[3].payoff, Decimal::ZERO);
        let put = option(json!({"maturityDate": "2020-03-30T00:00:00", "optionType": "P"}));
        let events = OptnsEngine.evaluate(&put, &aapl()).expect("events");
        assert_eq!(events[3].payoff, dec!(16.30));
    }

    /// option13: exercised at the status date with `XA = 10.33`, the option
    /// only has its settlement left, at `XD + P5D`.
    #[test]
    fn option13_exercised_before_the_status_date_only_settles() {
        let terms = option(json!({
            "statusDate": "2020-09-30T00:00:00",
            "exerciseDate": "2020-09-30T00:00:00",
            "exerciseAmount": "10.33",
            "settlementPeriod": "P5D"
        }));
        let events = OptnsEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        assert_eq!(
            stream(&events),
            vec![(EventType::Settlement, t("2020-10-05T00:00:00"), dec!(10.33))]
        );
    }

    /// A call/put pays the call struck at `OPS1` plus the put struck at
    /// `OPS2` (techspec `STF_XD`, `OPTP = CP`): at 115.81 with strikes 100
    /// and 120, `15.81 + 4.19 = 20`.
    #[test]
    fn call_put_uses_both_strikes() {
        let terms = option(json!({
            "optionType": "CP",
            "optionStrike1": "100",
            "optionStrike2": "120"
        }));
        let events = OptnsEngine.evaluate(&terms, &aapl()).expect("events");
        assert_eq!(events.last().expect("STD").payoff, dec!(20));
        let mut missing = terms.clone();
        missing.option_strike2 = None;
        assert!(matches!(
            OptnsEngine.evaluate(&missing, &aapl()),
            Err(EngineError::MissingAttribute("optionStrike2"))
        ));
    }

    /// A termination before maturity ends the stream with `R(CNTRL) x PTD`.
    #[test]
    fn termination_sells_the_option() {
        let terms = option(json!({
            "terminationDate": "2020-05-01T00:00:00",
            "priceAtTerminationDate": "12"
        }));
        let events = OptnsEngine.evaluate(&terms, &aapl()).expect("events");
        assert_eq!(
            stream(&events),
            vec![
                (EventType::Purchase, t("2020-01-02T00:00:00"), dec!(-10)),
                (EventType::Termination, t("2020-05-01T00:00:00"), dec!(12)),
            ]
        );
        assert_eq!(events[1].state.contract_status, ContractStatus::Terminated);
    }

    /// The underlying may also be the contract's own `marketObjectCode`;
    /// without an observation the `exerciseAmount` stands in, and without
    /// either the missing risk factor is reported.
    #[test]
    fn underlying_fallbacks_and_missing_observations() {
        let mut terms = option(json!({"marketObjectCode": "AAPL"}));
        terms.contract_structure = None;
        let events = OptnsEngine.evaluate(&terms, &aapl()).expect("events");
        assert_eq!(events.last().expect("STD").payoff, dec!(35.81));
        let mut agreed = terms.clone();
        agreed.exercise_amount = Some(dec!(7));
        let events = OptnsEngine
            .evaluate(&agreed, &StateProvider::new())
            .expect("events");
        assert_eq!(events.last().expect("STD").payoff, dec!(7));
        assert!(matches!(
            OptnsEngine.evaluate(&terms, &StateProvider::new()),
            Err(EngineError::RiskFactorMissing { .. })
        ));
        terms.market_object_code = None;
        assert!(matches!(
            OptnsEngine.evaluate(&terms, &StateProvider::new()),
            Err(EngineError::MissingAttribute("contractStructure"))
        ));
    }

    /// American and Bermudan exercise need observed `XD` events: out of
    /// scope; missing required attributes are reported.
    #[test]
    fn unsupported_and_missing_terms_are_reported() {
        let american = option(json!({"optionExerciseType": "A"}));
        assert!(matches!(
            OptnsEngine.evaluate(&american, &aapl()),
            Err(EngineError::InvalidTransition(_))
        ));
        let empty = ContractTerms::new(ContractType::Optns);
        assert!(matches!(
            OptnsEngine.evaluate(&empty, &aapl()),
            Err(EngineError::MissingAttribute("optionType"))
        ));
        let mut no_strike = option(json!({}));
        no_strike.option_strike1 = None;
        assert!(matches!(
            OptnsEngine.evaluate(&no_strike, &aapl()),
            Err(EngineError::MissingAttribute("optionStrike1"))
        ));
        let mut no_maturity = option(json!({}));
        no_maturity.maturity_date = None;
        assert!(matches!(
            OptnsEngine.evaluate(&no_maturity, &aapl()),
            Err(EngineError::MissingAttribute("maturityDate"))
        ));
        let mut no_price = option(json!({}));
        no_price.price_at_purchase_date = None;
        assert!(matches!(
            OptnsEngine.evaluate(&no_price, &aapl()),
            Err(EngineError::MissingAttribute("priceAtPurchaseDate"))
        ));
    }
}
