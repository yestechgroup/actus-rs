//! OPTNS: Option (paper §7.15, section 5 "Risk Factor Observer").
//!
//! A cash-settled European option on an observed underlying market object.
//! The event stream is:
//!
//! - `IED` (only when `premiumDiscountAtIED` (`PDIED`) is set): the premium
//!   is paid by the holder at the initial exchange date, payoff
//!   `-R(CNTRL) x PDIED` (the premium flows from the option holder to the
//!   counterparty, mirroring the `IED` premium orientation of PAM).
//! - `XD` (only when `exerciseDate` (`XD`) is set): the exercise decision at
//!   `t_XD`, zero payoff. Only European exercise is supported: when
//!   `optionExerciseType` (`OPXT`) is set to anything other than `E` the
//!   evaluation reports an invalid transition (American/Bermudan exercise
//!   windows are out of scope).
//! - `MD`: the terminal payoff `R(CNTRL) x NT x max(±(S_t - OPS1), 0)` with
//!   `+` for a call and `-` for a put (`optionType`, `OPTP`); the underlying
//!   price `S_t` is observed at the exercise date (`XD`, else the maturity)
//!   via `O_rf(MOC, t)` — the `marketObjectCode` attribute names the
//!   underlying price series, observed through the *index* namespace of the
//!   risk factor observer (the same convention the CEC engine uses for
//!   observed market values).
//!
//! Observation fallback: when the provider lacks the observation, the
//! `exerciseAmount` (`XA`) attribute is used as the terminal payoff if set
//! (already intrinsic, hence not combined with the strike); otherwise the
//! evaluation reports `RiskFactorMissing`.
//!
//! Contract role sign (dictionary Table 1): the buyer/long roles (`BUY`,
//! `RPA`, `LG`, `RFL`, `COL`, `CNO`, `UDL`, `UDLP`) carry `+1`; the short
//! roles (`SEL`, `RPL`, `ST`/`RF`, `PFL`, `UDLM`) carry `-1`.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::enums::{OptionExerciseType, OptionType};
use actus_model::{ContractRole, ContractTerms, ContractType, EventType};

use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the OPTNS contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct OptnsEngine;

impl ContractEngine for OptnsEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Optns
    }

    /// Evaluates the premium, exercise and intrinsic-value events.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let sign = role_sign(terms.contract_role);
        let maturity = terms
            .maturity_date
            .or(terms.option_exercise_end_date)
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("maturityDate"))?;
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let strike = terms
            .option_strike1
            .ok_or(EngineError::MissingAttribute("optionStrike1"))?;
        let option_type = terms
            .option_type
            .ok_or(EngineError::MissingAttribute("optionType"))?;
        if let Some(exercise_type) = terms.option_exercise_type {
            if exercise_type != OptionExerciseType::European {
                return Err(EngineError::InvalidTransition(format!(
                    "unsupported optionExerciseType {exercise_type}: only European (E) exercise \
                     is implemented"
                )));
            }
        }

        let mut events = Vec::new();
        if let Some(premium) = terms.premium_discount_at_ied {
            let ied = terms
                .initial_exchange_date
                .map(normalize_timestamp)
                .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
            events.push(ContractEvent {
                event_type: EventType::InitialExchange,
                time: ied,
                payoff: -sign * premium,
                currency: terms.currency.clone(),
                state: event_state(sign * premium, ied, ContractStatus::Active),
            });
        }

        let exercise = terms.exercise_date.map(normalize_timestamp);
        if let Some(xd) = exercise {
            events.push(ContractEvent {
                event_type: EventType::Exercise,
                time: xd,
                payoff: Decimal::ZERO,
                currency: terms.currency.clone(),
                state: event_state(Decimal::ZERO, xd, ContractStatus::Active),
            });
        }

        let observation_time = exercise.unwrap_or(maturity);
        let payoff =
            match intrinsic_value(terms, risk, option_type, notional, strike, observation_time) {
                Some(value) => value,
                None => {
                    return Err(EngineError::RiskFactorMissing {
                        code: terms.market_object_code.clone().unwrap_or_default(),
                        at: observation_time.to_string(),
                    })
                }
            };
        events.push(ContractEvent {
            event_type: EventType::Maturity,
            time: maturity,
            payoff: sign * payoff,
            currency: terms.currency.clone(),
            state: event_state(Decimal::ZERO, maturity, ContractStatus::Matured),
        });
        Ok(events)
    }
}

/// The intrinsic value at the observation time.
///
/// `Some(NT x max(±(S_t - OPS1), 0))` when the underlying market object is
/// observed; `Some(XA)` when the provider lacks the observation but the
/// exercise amount attribute carries the terminal payoff; `None` when
/// neither is available.
fn intrinsic_value(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    option_type: OptionType,
    notional: Decimal,
    strike: Decimal,
    at: NaiveDateTime,
) -> Option<Decimal> {
    if let Some(code) = terms.market_object_code.as_deref() {
        if let Some(underlying) = risk.index(code, at) {
            let direction = match option_type {
                OptionType::Call => Decimal::ONE,
                OptionType::Put => -Decimal::ONE,
                OptionType::CallPut => {
                    return Some(notional * call_put_intrinsic(underlying, strike))
                }
            };
            let intrinsic = (direction * (underlying - strike)).max(Decimal::ZERO);
            return Some(notional * intrinsic);
        }
    }
    terms.exercise_amount
}

/// The intrinsic value of a call/put combination: long the call strike and
/// the put strike (`OPS1`/`OPS2`, dictionary: strike price and put price of
/// the call/put).
fn call_put_intrinsic(underlying: Decimal, strike: Decimal) -> Decimal {
    (underlying - strike).max(Decimal::ZERO) + (strike - underlying).max(Decimal::ZERO)
}

/// The contract role sign of the option holder (dictionary Table 1): short
/// positions carry `-1`, long/buyer positions carry `+1`.
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

/// Builds a post-event state carrying the given notional at the event time.
fn event_state(notional: Decimal, time: NaiveDateTime, status: ContractStatus) -> ContractState {
    ContractState {
        notional_principal: notional,
        status_date: time,
        contract_status: status,
        ..ContractState::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The golden option: NT 100, strike 100, ITM call observed at 110.
    fn call(moc: bool) -> ContractTerms {
        let mut raw = json!({
            "contractType": "OPTNS",
            "contractID": "optns01",
            "contractRole": "RPA",
            "currency": "USD",
            "statusDate": "2025-01-01T00:00:00",
            "maturityDate": "2025-12-31T00:00:00",
            "notionalPrincipal": "100",
            "optionType": "C",
            "optionStrike1": "100",
            "exerciseDate": "2025-06-30T00:00:00"
        });
        if moc {
            raw["marketObjectCode"] = json!("EQ-ABC");
        }
        terms(raw)
    }

    fn risk(price: &str) -> StateProvider {
        StateProvider::new().with_index("EQ-ABC", t("2025-06-30T00:00:00"), price.parse().unwrap())
    }

    #[test]
    fn itm_call_pays_intrinsic_value_at_maturity() {
        let events = OptnsEngine
            .evaluate(&call(true), &risk("110"))
            .expect("events");
        assert_eq!(events.len(), 2);
        let xd = &events[0];
        assert_eq!(xd.event_type, EventType::Exercise);
        assert_eq!(xd.time, t("2025-06-30T00:00:00"));
        assert_eq!(xd.payoff, Decimal::ZERO);
        let md = &events[1];
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.time, t("2025-12-31T00:00:00"));
        assert_eq!(md.payoff, dec!(1000));
        assert_eq!(md.state.contract_status, ContractStatus::Matured);
    }

    #[test]
    fn itm_put_pays_intrinsic_value() {
        let mut option = call(true);
        option.option_type = Some(OptionType::Put);
        let events = OptnsEngine.evaluate(&option, &risk("90")).expect("events");
        assert_eq!(events.last().expect("MD").payoff, dec!(1000));
    }

    #[test]
    fn seller_orientation_flips_the_payoff() {
        let mut option = call(true);
        option.contract_role = Some(ContractRole::Sel);
        let events = OptnsEngine.evaluate(&option, &risk("110")).expect("events");
        assert_eq!(events.last().expect("MD").payoff, dec!(-1000));
    }

    #[test]
    fn otm_option_pays_zero() {
        let events = OptnsEngine
            .evaluate(&call(true), &risk("90"))
            .expect("events");
        assert_eq!(events.last().expect("MD").payoff, Decimal::ZERO);
    }

    #[test]
    fn premium_is_paid_at_the_initial_exchange() {
        let mut option = call(true);
        option.premium_discount_at_ied = Some(dec!(50));
        option.initial_exchange_date = Some(t("2025-01-01T00:00:00"));
        let events = OptnsEngine.evaluate(&option, &risk("110")).expect("events");
        assert_eq!(events.len(), 3);
        let ied = &events[0];
        assert_eq!(ied.event_type, EventType::InitialExchange);
        assert_eq!(ied.time, t("2025-01-01T00:00:00"));
        assert_eq!(ied.payoff, dec!(-50));
    }

    #[test]
    fn exercise_amount_is_the_terminal_fallback_without_observation() {
        let mut option = call(false);
        option.exercise_amount = Some(dec!(250));
        let events = OptnsEngine
            .evaluate(&option, &StateProvider::new())
            .expect("events");
        assert_eq!(events.last().expect("MD").payoff, dec!(250));
    }

    #[test]
    fn unobserved_underlying_without_fallback_is_reported() {
        let error = OptnsEngine
            .evaluate(&call(true), &StateProvider::new())
            .unwrap_err();
        assert!(matches!(error, EngineError::RiskFactorMissing { .. }));
    }

    #[test]
    fn non_european_exercise_is_out_of_scope() {
        let mut option = call(true);
        option.option_exercise_type = Some(OptionExerciseType::American);
        option.cycle_anchor_date_of_optionality = Some(t("2025-01-01T00:00:00"));
        option.option_exercise_end_date = Some(t("2025-12-31T00:00:00"));
        let error = OptnsEngine.evaluate(&option, &risk("110")).unwrap_err();
        assert!(matches!(error, EngineError::InvalidTransition(_)));
    }

    #[test]
    fn missing_required_attributes_are_reported() {
        let error = OptnsEngine
            .evaluate(
                &terms(json!({"contractType": "OPTNS", "statusDate": "2025-01-01T00:00:00"})),
                &risk("110"),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("maturityDate")
        ));

        let error = OptnsEngine
            .evaluate(&call(false), &StateProvider::new())
            .unwrap_err();
        assert!(matches!(error, EngineError::RiskFactorMissing { .. }));
    }
}
