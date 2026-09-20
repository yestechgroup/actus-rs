//! CSH: Cash (ACTUS techspec section "CSH: Cash").
//!
//! A plain cash transfer carries a notional from `t0` on; it has no
//! exchange schedule of its own. The techspec defines the states-at-t0 as
//! `NT(t0) = sgn x NT` (role-signed notional), `IPNR = IPAC = 0` and the
//! accrual anchor `SD` at the status date; the only schedule row is the
//! contingent analysis event `AD` ("Same as PAM"), whose payoff is zero and
//! whose state transition function only advances `SD(t+) = t`.
//!
//! The analysis dates are external to the terms: the evaluation environment
//! supplies the observed event stream ([`RiskFactorProvider::observed_events`],
//! the testbed `eventsObserved` entries), and the engine replays one `AD`
//! event per observed analysis date, mirroring the upstream reference
//! behaviour. The testbed `to` field bounds the reported stream and is
//! applied by the harness, not by the engine.
//!
//! Conventions implemented here, resolved against the official testbed:
//!
//! - Contract role sign: `RPA` carries sign `+1`, `RPL` sign `-1`, so the
//!   notional state after the analysis event is `+NT` for the real side and
//!   `-NT` for the mirror side (testbed csh01, csh02).
//! - The event time is the normalised observation time: `23:59:59` denotes
//!   midnight of the following day (testbed csh04).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractTerms, ContractType, EventType};

use crate::common::role_sign;
use crate::daycount::normalize_timestamp;
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::state::ContractState;
use crate::EngineError;

/// Implementation of the CSH contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CshEngine;

impl ContractEngine for CshEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Csh
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let mut state = ContractState::initial(terms);
        state.notional_principal = role_sign(terms) * notional;
        let mut analyses: Vec<NaiveDateTime> = risk
            .observed_events()
            .into_iter()
            .filter(|event| event.event_type == EventType::Monitoring)
            .map(|event| normalize_timestamp(event.time))
            .collect();
        analyses.sort();
        analyses.dedup();
        let mut events = Vec::new();
        for time in analyses {
            state.status_date = time;
            events.push(ContractEvent {
                event_type: EventType::Monitoring,
                time,
                payoff: Decimal::ZERO,
                currency: terms.currency.clone(),
                state: state.clone(),
            });
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use serde_json::json;

    use crate::ObservedEvent;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn observed(time: &str) -> ObservedEvent {
        ObservedEvent {
            time: t(time),
            event_type: EventType::Monitoring,
        }
    }

    fn terms(role: &str, currency: &str, notional: &str) -> ContractTerms {
        serde_json::from_value(json!({
            "contractType": "CSH",
            "statusDate": "2015-07-15T00:00:00",
            "contractRole": role,
            "currency": currency,
            "notionalPrincipal": notional
        }))
        .expect("terms")
    }

    #[test]
    fn analysis_event_reports_zero_payoff_and_role_signed_notional() {
        let terms = terms("RPA", "CHF", "1000");
        let risk = StateProvider::new().with_observed_event(observed("2015-07-30T00:00:00"));
        let events = CshEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Monitoring);
        assert_eq!(events[0].time, t("2015-07-30T00:00:00"));
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::from(1000));
        assert_eq!(events[0].state.nominal_interest_rate, Decimal::ZERO);
        assert_eq!(events[0].state.accrued_interest, Decimal::ZERO);
        assert_eq!(events[0].currency.as_deref(), Some("CHF"));
    }

    #[test]
    fn rpl_flips_the_notional_state() {
        let terms = terms("RPL", "USD", "1200");
        let risk = StateProvider::new().with_observed_event(observed("2015-07-31T00:00:00"));
        let events = CshEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::from(-1200));
    }

    #[test]
    fn analysis_date_advances_the_status_date_anchor() {
        let terms = terms("RPA", "CHF", "2000");
        let risk = StateProvider::new().with_observed_event(observed("2015-08-20T00:00:00"));
        let events = CshEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events[0].state.status_date, t("2015-08-20T00:00:00"));
        assert_eq!(events[0].state.notional_principal, Decimal::from(2000));
    }

    #[test]
    fn end_of_day_observation_normalises_to_next_midnight() {
        let terms = terms("RPL", "USD", "2500");
        let risk = StateProvider::new().with_observed_event(observed("2015-08-20T23:59:59"));
        let events = CshEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events[0].time, t("2015-08-21T00:00:00"));
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::from(-2500));
    }

    #[test]
    fn without_observed_events_no_analysis_event_occurs() {
        let terms = terms("RPA", "CHF", "1000");
        let events = CshEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        assert!(events.is_empty());
    }

    #[test]
    fn missing_notional_is_reported() {
        let terms = ContractTerms::new(ContractType::Csh);
        let err = CshEngine
            .evaluate(&terms, &StateProvider::new())
            .unwrap_err();
        assert!(matches!(
            err,
            EngineError::MissingAttribute("notionalPrincipal")
        ));
    }
}
