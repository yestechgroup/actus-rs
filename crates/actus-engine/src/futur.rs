//! FUTUR: Future (paper §7.16, section 5 "Risk Factor Observer").
//!
//! A cash-settled future on an observed underlying market object. The
//! contract holds no schedule of its own beyond maturity: the single `MD`
//! event pays `R(CNTRL) x NT x (S_t - PFUT)` where `S_t` is the underlying
//! price observed at maturity via `O_rf(MOC, MD)` — the `marketObjectCode`
//! attribute names the underlying price series, observed through the *index*
//! namespace of the risk factor observer (the OPTNS convention).
//!
//! The agreed futures price comes from the `futuresPrice` (`PFUT`)
//! attribute. Unlike OPTNS there is no `exerciseAmount` fallback: an
//! unobserved underlying is reported as `RiskFactorMissing`.
//!
//! Delivery settlement (`DS`): only cash settlement (`D`, the default when
//! unset) is implemented — physical delivery (`S`) reports an invalid
//! transition.
//!
//! Contract role sign (dictionary Table 1): the long roles (`BUY`, `RPA`,
//! `LG`, `RFL`, `COL`, `CNO`, `UDL`, `UDLP`) carry `+1` and receive
//! `S_t - PFUT`; the short roles (`SEL`, `RPL`, `ST`/`RF`, `PFL`, `UDLM`)
//! carry `-1`.

use rust_decimal::Decimal;

use actus_model::{ContractRole, ContractTerms, ContractType, DeliverySettlement, EventType};

use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the FUTUR contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct FuturEngine;

impl ContractEngine for FuturEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Futur
    }

    /// Evaluates the cash-settled maturity payoff.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        if terms.delivery_settlement == Some(DeliverySettlement::S) {
            return Err(EngineError::InvalidTransition(
                "physical delivery is not implemented: FUTUR settles in cash only".to_string(),
            ));
        }
        let sign = role_sign(terms.contract_role);
        let maturity = terms
            .maturity_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("maturityDate"))?;
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let futures_price = terms
            .futures_price
            .ok_or(EngineError::MissingAttribute("futuresPrice"))?;
        let code = terms
            .market_object_code
            .clone()
            .ok_or(EngineError::MissingAttribute("marketObjectCode"))?;
        let underlying = risk
            .index(&code, maturity)
            .ok_or(EngineError::RiskFactorMissing {
                code,
                at: maturity.to_string(),
            })?;

        Ok(vec![ContractEvent {
            event_type: EventType::Maturity,
            time: maturity,
            payoff: sign * notional * (underlying - futures_price),
            currency: terms.currency.clone(),
            state: ContractState {
                status_date: maturity,
                contract_status: ContractStatus::Matured,
                ..ContractState::default()
            },
        }])
    }
}

/// The contract role sign of the future position (dictionary Table 1): short
/// positions carry `-1`, long positions carry `+1`.
fn role_sign(role: Option<ContractRole>) -> Decimal {
    match role {
        Some(ContractRole::Rpl)
        | Some(ContractRole::Rf)
        | Some(ContractRole::Pf)
        | Some(ContractRole::Sel)
        | Some(ContractRole::Udlm) => -Decimal::ONE,
        _ => Decimal::ONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms() -> ContractTerms {
        serde_json::from_value(json!({
            "contractType": "FUTUR",
            "contractID": "futur01",
            "contractRole": "RPA",
            "currency": "USD",
            "statusDate": "2025-01-01T00:00:00",
            "maturityDate": "2025-06-30T00:00:00",
            "notionalPrincipal": "10",
            "futuresPrice": "100",
            "marketObjectCode": "CM-WHEAT"
        }))
        .expect("terms")
    }

    fn risk(price: &str) -> StateProvider {
        StateProvider::new().with_index(
            "CM-WHEAT",
            t("2025-06-30T00:00:00"),
            price.parse().unwrap(),
        )
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    #[test]
    fn golden_pays_notional_times_underlying_minus_futures_price() {
        let events = FuturEngine
            .evaluate(&terms(), &risk("110"))
            .expect("events");
        assert_eq!(events.len(), 1);
        let md = &events[0];
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.time, t("2025-06-30T00:00:00"));
        // 10 x (110 - 100) = +100 for the long position.
        assert_eq!(md.payoff, dec!(100));
        assert_eq!(md.state.contract_status, ContractStatus::Matured);
    }

    #[test]
    fn short_position_carries_the_negative_payoff() {
        let mut future = terms();
        future.contract_role = Some(ContractRole::Sel);
        let events = FuturEngine.evaluate(&future, &risk("110")).expect("events");
        assert_eq!(events[0].payoff, dec!(-100));
    }

    #[test]
    fn losing_position_pays_negative() {
        let events = FuturEngine.evaluate(&terms(), &risk("95")).expect("events");
        assert_eq!(events[0].payoff, dec!(-50));
    }

    #[test]
    fn physical_delivery_is_out_of_scope() {
        let mut future = terms();
        future.delivery_settlement = Some(DeliverySettlement::S);
        let error = FuturEngine.evaluate(&future, &risk("110")).unwrap_err();
        assert!(matches!(error, EngineError::InvalidTransition(_)));
    }

    #[test]
    fn unobserved_underlying_is_reported() {
        let error = FuturEngine
            .evaluate(&terms(), &StateProvider::new())
            .unwrap_err();
        assert!(matches!(error, EngineError::RiskFactorMissing { .. }));
    }

    #[test]
    fn missing_attributes_are_reported() {
        let error = FuturEngine
            .evaluate(
                &serde_json::from_value(json!({"contractType": "FUTUR"})).expect("terms"),
                &risk("110"),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("maturityDate")
        ));

        let mut future = terms();
        future.market_object_code = None;
        let error = FuturEngine.evaluate(&future, &risk("110")).unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("marketObjectCode")
        ));

        let mut future = terms();
        future.futures_price = None;
        let error = FuturEngine.evaluate(&future, &risk("110")).unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("futuresPrice")
        ));
    }
}
