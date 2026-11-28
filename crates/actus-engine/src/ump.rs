//! UMP: Undefined Maturity Profile (paper section 7.7 "UMP: Undefined
//! Maturity Profile"; dictionary applicability `UMP_*`).
//!
//! A contract without a scheduled maturity, driven by unscheduled events
//! observed through [`RiskFactorProvider::observed_events`] (paper section 5,
//! the `'ev'` observer `O_ev(CID, k, t)`): every observed principal
//! redemption (`PR`) repays part of the outstanding notional and every
//! observed interest payment (`PI`) pays out the accrued interest. The
//! initial exchange at `IED` moves the notional (`-sgn x NT` payoff, no
//! premium — `premiumDiscountAtIED` is not applicable to UMP).
//!
//! Conventions implemented here, resolved against the paper and the sibling
//! engines:
//!
//! - The paper's `PI` acronym does not exist in dictionary v1.4, so observed
//!   interest payments are carried as [`EventType::InterestPayment`];
//!   observed principal redemptions as [`EventType::PrincipalRedemption`].
//! - An observed `PR` redeems the state `PRNXT` amount (the terms
//!   `nextPrincipalRedemptionPayment`, held constant across redemptions so a
//!   facility with recurring fixed repayments redeems at every observed
//!   event), capped at the outstanding notional magnitude like the shared
//!   `apply_principal_redemption` helper of the amortizer family; the period
//!   accrual folds into `IPAC` before the redemption. The `PR` payoff is
//!   role-signed (`sgn x NSC x PRNXT`): a lender (`RPA`) receives redemptions.
//! - An observed `PI` pays `ISC x (IPAC + Y x IPNR x NT)` (the PAM `IP` row)
//!   and resets `IPAC`, so interest leaves as cash instead of capitalizing.
//! - Scheduled interest (`IPANX`/`IPCL`) capitalizes (techspec UMP
//!   schedule `IPCI`, `STF_IPCI_PAM`: zero payoff, `Nt += Ipac + Y x Ipnr x
//!   Nt`); observed `IP` events pay the accrual out. Scheduled interest
//!   and rate resets (`RRANX`/`RRCL`
//!   observing `MOCRR` with multiplier and spread, PAM `RR` row) run to
//!   `min(TD, Md)`, exclusive, where `Md` is the last observed principal
//!   redemption or else the analysis horizon `tmax` (techspec UMP states).
//!   With no end at all the schedule is open-ended and an error.
//! - Termination: when `terminationDate` is set, a terminal `TD` event pays
//!   `sgn x (PTD + IPAC + Y x IPNR x NT)` (the PAM `TD` row), zeroes the
//!   notional states and reports [`ContractStatus::Terminated`]. Without
//!   `TD` the stream ends after the last observed event with status
//!   [`ContractStatus::Active`] — a synthetic maturity would contradict the
//!   undefined-maturity semantics, so no terminal event exists in that case.
//! - Events at or before the status date `t0`, observed events at or before
//!   `IED` (nothing can accrue or redeem before the contract exists) and
//!   events after `TD` are not observed (the CLM analysis-window idiom).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    ContractTerms, ContractType, Cycle, DayCountConvention, EndOfMonthConvention, EventType,
};

use crate::common::role_sign;
use crate::common::rolls_before;
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the UMP contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct UmpEngine;

/// UMP event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    InterestPayment,
    /// Scheduled interest: the techspec UMP `IPCI` series capitalizes.
    InterestCapitalization,
    RateReset,
    Termination,
}

impl Kind {
    /// The dictionary event type of the slot.
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::PrincipalRedemption => EventType::PrincipalRedemption,
            Kind::InterestPayment => EventType::InterestPayment,
            Kind::InterestCapitalization => EventType::InterestCapitalization,
            Kind::RateReset => EventType::RateResetVariable,
            Kind::Termination => EventType::Termination,
        }
    }

    /// The effective same-timestamp sequence rank of the slot.
    fn priority(self) -> u8 {
        sequence_rank(self.event_type())
    }
}

/// One event slot of the UMP stream: the observed or scheduled kind and its
/// emission time (no business day convention is applied; UMP has no
/// schedule-driven shifting needs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    time: NaiveDateTime,
}

impl ContractEngine for UmpEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Ump
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let ied = terms
            .initial_exchange_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
        let dcc = terms
            .day_count_convention
            .ok_or(EngineError::MissingAttribute("dayCountConvention"))?;
        let t0 = ContractState::initial(terms).status_date;
        let termination = terms.termination_date.map(normalize_timestamp);
        let sgn = role_sign(terms);
        let ctx = Context {
            dcc,
            sgn,
            ied,
            t0,
            termination,
            horizon: risk.analysis_horizon().map(normalize_timestamp),
        };

        let mut slots: Vec<Slot> = Vec::new();
        if ied > t0 {
            slots.push(Slot {
                kind: Kind::InitialExchange,
                time: ied,
            });
        }
        let mut observed: Vec<(NaiveDateTime, EventType)> = Vec::new();
        for event in risk.observed_events() {
            let time = normalize_timestamp(event.time);
            let kind = match event.event_type {
                EventType::PrincipalRedemption => Kind::PrincipalRedemption,
                EventType::InterestPayment => Kind::InterestPayment,
                _ => continue,
            };
            if time <= ied || time <= t0 {
                continue;
            }
            if termination.is_some_and(|td| time > td) {
                continue;
            }
            observed.push((time, kind.event_type()));
            slots.push(Slot { kind, time });
        }
        if let Some(td) = termination {
            if td > t0 && td > ied {
                slots.push(Slot {
                    kind: Kind::Termination,
                    time: td,
                });
            }
        }
        ctx.scheduled_slots(terms, &mut slots, &observed)?;
        slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

        let mut state = ContractState::initial(terms);
        state.notional_principal = (ck(sgn) * notional).get()?;
        state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
        // IPAC is unsigned; the state is role-signed like the notional.
        state.accrued_interest =
            (ck(sgn) * terms.accrued_interest.unwrap_or(Decimal::ZERO)).get()?;
        let mut events = Vec::new();
        for slot in slots {
            let payoff = ctx.apply(&mut state, slot, terms, risk)?;
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

/// Resolved attributes and conventions of one UMP evaluation.
struct Context {
    dcc: DayCountConvention,
    sgn: Decimal,
    ied: NaiveDateTime,
    t0: NaiveDateTime,
    termination: Option<NaiveDateTime>,
    /// The analysis horizon `tmax` of the environment.
    horizon: Option<NaiveDateTime>,
}

impl Context {
    /// The scheduled `IPCI` and `RR` series (techspec UMP schedule rows IPCI
    /// and RR): unrolled from `IPANX`/`RRANX ?? IED` by `IPCL`/`RRCL` while
    /// the rolled date stays strictly before the end of the schedule,
    /// `min(TD, Md)`, observing `MOCRR` at the roll date. `Md` is the last
    /// observed principal redemption, or else the analysis horizon `tmax`
    /// (techspec UMP states); the horizon is exclusive (ump04 ends its
    /// yearly series in 2017 for a horizon of 2018-01-01). A cycle with no
    /// end is open-ended and an error. A schedule point shared with an
    /// observed event of the same type is dropped in favor of the observed
    /// one.
    fn scheduled_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
        observed: &[(NaiveDateTime, EventType)],
    ) -> Result<(), EngineError> {
        let maturity = observed
            .iter()
            .filter(|(_, event_type)| *event_type == EventType::PrincipalRedemption)
            .map(|(time, _)| *time)
            .max()
            .or(self.horizon);
        let end = match (self.termination, maturity) {
            (Some(td), Some(md)) => Some(td.min(md)),
            (td, md) => td.or(md),
        };
        let series = [
            (
                terms.cycle_of_interest_payment.as_ref(),
                terms
                    .cycle_anchor_date_of_interest_payment
                    .map(normalize_timestamp)
                    .unwrap_or(self.ied),
                EventType::InterestCapitalization,
                Kind::InterestCapitalization,
            ),
            (
                terms.cycle_of_rate_reset.as_ref(),
                terms
                    .cycle_anchor_date_of_rate_reset
                    .map(normalize_timestamp)
                    .unwrap_or(self.ied),
                EventType::RateResetVariable,
                Kind::RateReset,
            ),
        ];
        for (cycle, anchor, event_type, kind) in series {
            let Some(cycle) = cycle else {
                continue;
            };
            let Some(end) = end else {
                return Err(EngineError::InvalidSchedule(
                    "the UMP schedule S(s, IPCL, Md) is open-ended: set terminationDate \
                     or an analysis horizon"
                        .to_string(),
                ));
            };
            for time in roll_series(anchor, cycle, Some(end))? {
                if time <= self.ied || time <= self.t0 || time >= end {
                    continue;
                }
                if observed.contains(&(time, event_type)) {
                    continue;
                }
                slots.push(Slot { kind, time });
            }
        }
        Ok(())
    }

    /// Applies the state transition and payoff function of one slot and
    /// returns the role-signed payoff.
    fn apply(
        &self,
        state: &mut ContractState,
        slot: Slot,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Decimal, EngineError> {
        let y = day_count_fraction(state.status_date, slot.time, self.dcc)?;
        let accrual = (ck(y) * state.nominal_interest_rate * state.notional_principal).get()?;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                let notional = terms
                    .notional_principal
                    .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
                state.notional_principal = (ck(self.sgn) * notional).get()?;
                state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
                state.accrued_interest =
                    (ck(self.sgn) * terms.accrued_interest.unwrap_or(Decimal::ZERO)).get()?;
                (-ck(self.sgn) * notional).get()?
            }
            Kind::PrincipalRedemption => {
                state.accrued_interest = (ck(state.accrued_interest) + accrual).get()?;
                let mut payoff = (ck(state.notional_scaling_multiplier)
                    * self.sgn
                    * state.next_principal_redemption_payment)
                    .get()?;
                if payoff.abs() > state.notional_principal.abs() {
                    payoff = state.notional_principal;
                }
                state.notional_principal = (ck(state.notional_principal) - payoff).get()?;
                payoff
            }
            Kind::InterestPayment => {
                let payoff = (state.interest_scaling_multiplier
                    * (ck(state.accrued_interest) + accrual))
                    .get()?;
                state.accrued_interest = Decimal::ZERO;
                payoff
            }
            Kind::InterestCapitalization => {
                // STF_IPCI_PAM: the accrued interest joins the notional.
                state.notional_principal =
                    (ck(state.notional_principal) + state.accrued_interest + accrual).get()?;
                state.accrued_interest = Decimal::ZERO;
                Decimal::ZERO
            }
            Kind::RateReset => {
                state.accrued_interest = (ck(state.accrued_interest) + accrual).get()?;
                let code = terms
                    .market_object_code_of_rate_reset
                    .as_deref()
                    .ok_or(EngineError::MissingAttribute("marketObjectCodeOfRateReset"))?;
                let observed =
                    risk.rate(code, slot.time)
                        .ok_or(EngineError::RiskFactorMissing {
                            code: code.to_string(),
                            at: slot.time.to_string(),
                        })?;
                let multiplier = terms.rate_multiplier.unwrap_or(Decimal::ONE);
                let spread = terms.rate_spread.unwrap_or(Decimal::ZERO);
                state.nominal_interest_rate = (ck(observed) * multiplier + spread).get()?;
                Decimal::ZERO
            }
            Kind::Termination => {
                let price = terms
                    .price_at_termination_date
                    .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                let payoff =
                    (ck(self.sgn) * (ck(price) + state.accrued_interest + accrual)).get()?;
                state.notional_principal = Decimal::ZERO;
                state.accrued_interest = Decimal::ZERO;
                state.fee_accrued = Decimal::ZERO;
                state.contract_status = ContractStatus::Terminated;
                payoff
            }
        };
        state.status_date = slot.time;
        Ok(payoff)
    }
}

/// Unrolls the anchor plus the cycle rolls strictly before the termination
/// date, without stub correction and with a runaway guard (the CLM idiom).
fn roll_series(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: Option<NaiveDateTime>,
) -> Result<Vec<NaiveDateTime>, EngineError> {
    let Some(termination) = termination else {
        return Ok(vec![anchor]);
    };
    rolls_before(anchor, cycle, EndOfMonthConvention::Sd, termination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use crate::ObservedEvent;
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
            "contractType": "UMP",
            "contractRole": "RPA",
            "statusDate": "2016-01-01T00:00:00",
            "initialExchangeDate": "2016-01-15T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "nextPrincipalRedemptionPayment": "400",
            "dayCountConvention": "A365",
            "currency": "CHF"
        })
    }

    /// A daily interest schedule over 60 years (more than 20,000 rolls) runs
    /// to the termination instead of stopping silently after 20,000 rolls.
    #[test]
    fn long_interest_schedules_are_not_truncated() {
        let mut raw = base_terms();
        raw["cycleAnchorDateOfInterestPayment"] = json!("2016-01-15T00:00:00");
        raw["cycleOfInterestPayment"] = json!("P1DL1");
        raw["terminationDate"] = json!("2076-01-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("1000");
        let events = UmpEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let last_ip = events
            .iter()
            .rfind(|e| e.event_type == EventType::InterestCapitalization)
            .expect("scheduled interest capitalizations");
        assert_eq!(last_ip.time, t("2076-01-14T00:00:00"));
    }

    /// A carried `accruedInterest` (`IPAC`) is an unsigned attribute; the
    /// state is role-signed like the notional (actus-core
    /// `UndefinedMaturityProfile` initial states: `roleSign x IPAC`, kept by
    /// `STF_IED_PAM`; the same rule as PAM/LAM/NAM, #27). The `RPL` mirror
    /// then negates every payoff and signed state of the `RPA` contract.

    #[test]
    fn carried_accrued_interest_is_role_signed() {
        let mut raw = base_terms();
        raw["accruedInterest"] = json!("10");
        raw["terminationDate"] = json!("2016-09-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("1000");
        let risk = StateProvider::new()
            .with_observed_event(observed("2016-06-15T00:00:00", EventType::InterestPayment));
        let lender = UmpEngine.evaluate(&terms(raw.clone()), &risk).expect("RPA");
        raw["contractRole"] = json!("RPL");
        let borrower = UmpEngine.evaluate(&terms(raw), &risk).expect("RPL");

        assert_eq!(lender[0].event_type, EventType::InitialExchange);
        assert_eq!(lender[0].state.accrued_interest, dec!(10));
        assert_eq!(borrower[0].state.accrued_interest, dec!(-10));
        assert_eq!(lender.len(), borrower.len());
        for (a, b) in lender.iter().zip(&borrower) {
            assert_eq!(a.event_type, b.event_type);
            assert_eq!(b.state.notional_principal, -a.state.notional_principal);
            assert_eq!(b.state.accrued_interest, -a.state.accrued_interest);
            if a.event_type != EventType::Termination {
                // TD pays `sgn x (PTD + Ipac)` with a signed Ipac (techspec
                // PAM TD row), which is not a pure mirror.
                assert_eq!(b.payoff, -a.payoff, "{:?}", a.event_type);
            }
        }
    }

    fn observed(time: &str, event_type: EventType) -> ObservedEvent {
        ObservedEvent {
            time: t(time),
            event_type,
        }
    }

    /// IED exchanges the notional, the observed PR redeems `PRNXT = 400`
    /// (1000 -> 600) while interest keeps accruing into `IPAC`, and the
    /// observed PI pays the accumulated accrual in cash.
    #[test]
    fn observed_redemption_and_interest_pay_follow_the_state_machinery() {
        let terms = terms(base_terms());
        let risk = StateProvider::new()
            .with_observed_event(observed(
                "2016-03-15T00:00:00",
                EventType::PrincipalRedemption,
            ))
            .with_observed_event(observed("2016-06-15T00:00:00", EventType::InterestPayment));
        let events = UmpEngine.evaluate(&terms, &risk).expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2016-01-15T00:00:00")),
                (EventType::PrincipalRedemption, t("2016-03-15T00:00:00")),
                (EventType::InterestPayment, t("2016-06-15T00:00:00")),
            ]
        );
        assert_eq!(events[0].payoff, dec!(-1000));
        assert_eq!(events[0].state.notional_principal, dec!(1000));
        assert_eq!(events[1].payoff, dec!(400));
        assert_eq!(events[1].state.notional_principal, dec!(600));
        // 1000 x 0.12 x 60/365 (leap-year actual days Jan 15 -> Mar 15).
        assert_close(events[1].state.accrued_interest, dec!(19.726027397260274));
        // 19.726... + 600 x 0.12 x 92/365 (Mar 15 -> Jun 15).
        assert_close(events[2].payoff, dec!(37.873972602739726));
        assert_eq!(events[2].state.accrued_interest, Decimal::ZERO);
        assert_eq!(events[2].state.notional_principal, dec!(600));
        assert_eq!(events[2].state.contract_status, ContractStatus::Active);
    }

    /// A redemption exceeding the outstanding notional redeems only the
    /// remaining principal (the amortizer cap idiom).
    #[test]
    fn redemption_is_capped_at_the_outstanding_notional() {
        let terms = terms(base_terms());
        let risk = StateProvider::new()
            .with_observed_event(observed(
                "2016-03-15T00:00:00",
                EventType::PrincipalRedemption,
            ))
            .with_observed_event(observed(
                "2016-06-15T00:00:00",
                EventType::PrincipalRedemption,
            ))
            .with_observed_event(observed(
                "2016-09-15T00:00:00",
                EventType::PrincipalRedemption,
            ));
        let events = UmpEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events[1].payoff, dec!(400));
        assert_eq!(events[1].state.notional_principal, dec!(600));
        assert_eq!(events[2].payoff, dec!(400));
        assert_eq!(events[2].state.notional_principal, dec!(200));
        assert_eq!(events[3].payoff, dec!(200));
        assert_eq!(events[3].state.notional_principal, Decimal::ZERO);
    }

    /// Without observed events the schedule is the IED alone and the
    /// contract stays active: an undefined maturity has no terminal event.
    #[test]
    fn without_observations_only_the_exchange_occurs() {
        let terms = terms(base_terms());
        let events = UmpEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::InitialExchange);
        assert_eq!(events[0].time, t("2016-01-15T00:00:00"));
        assert_eq!(events[0].payoff, dec!(-1000));
        assert_eq!(events[0].state.notional_principal, dec!(1000));
        assert_eq!(events[0].state.contract_status, ContractStatus::Active);
    }

    /// A set termination date ends the stream with a `TD` paying the
    /// termination price plus the frozen accrual.
    #[test]
    fn termination_settles_the_termination_price_plus_accrual() {
        let mut raw = base_terms();
        raw["terminationDate"] = json!("2016-09-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("500");
        let events = UmpEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![EventType::InitialExchange, EventType::Termination]
        );
        // 500 + 1000 x 0.12 x 244/365 (Jan 15 -> Sep 15).
        assert_close(events[1].payoff, dec!(580.21917808219178));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
        assert_eq!(events[1].state.contract_status, ContractStatus::Terminated);
    }

    /// With `IPANX`/`IPCL` set the scheduled interest series runs up to the
    /// termination date and capitalizes (`IPCI`); the terminal accrual
    /// starts from the last schedule point, on the capitalized notional.
    #[test]
    fn scheduled_interest_runs_to_the_termination_date() {
        let mut raw = base_terms();
        raw["cycleAnchorDateOfInterestPayment"] = json!("2016-01-15T00:00:00");
        raw["cycleOfInterestPayment"] = json!("P1ML1");
        raw["terminationDate"] = json!("2016-03-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("1000");
        let events = UmpEngine
            .evaluate(&terms(raw), &StateProvider::new())
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2016-01-15T00:00:00")),
                (EventType::InterestCapitalization, t("2016-02-15T00:00:00")),
                (EventType::Termination, t("2016-03-15T00:00:00")),
            ]
        );
        // IPCI pays nothing: 1000 + 1000 x 0.12 x 31/365 joins the notional.
        assert_eq!(events[1].payoff, Decimal::ZERO);
        assert_close(
            events[1].state.notional_principal,
            dec!(1010.191780821917808219178082),
        );
        // PTD + Nt x 0.12 x 29/365 on the capitalized notional.
        assert_close(events[2].payoff, dec!(1009.631417526740476637267780));
    }

    /// The scheduled rate reset series observes `MOCRR` at the roll date;
    /// the reset rate accrues from the roll to the termination.
    #[test]
    fn scheduled_rate_resets_observe_the_market_object() {
        let mut raw = base_terms();
        raw["cycleAnchorDateOfRateReset"] = json!("2016-02-15T00:00:00");
        raw["cycleOfRateReset"] = json!("P1ML1");
        raw["marketObjectCodeOfRateReset"] = json!("EUR_Prim");
        raw["rateSpread"] = json!("0.01");
        raw["terminationDate"] = json!("2016-03-15T00:00:00");
        raw["priceAtTerminationDate"] = json!("1000");
        let risk = StateProvider::new().with_rate("EUR_Prim", t("2016-02-15T00:00:00"), dec!(0.06));
        let events = UmpEngine.evaluate(&terms(raw), &risk).expect("events");
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![
                EventType::InitialExchange,
                EventType::RateResetVariable,
                EventType::Termination
            ]
        );
        assert_close(events[1].state.nominal_interest_rate, dec!(0.07));
        // 1000 + 1000 x 0.12 x 31/365 (accrued into IPAC at the reset)
        //      + 1000 x 0.07 x 29/365 (accrual at the reset rate).
        assert_close(events[2].payoff, dec!(1015.7534246575342));
    }

    /// `RPL` mirrors the payoffs and the notional state.
    #[test]
    fn rpl_mirrors_the_payoff_orientation() {
        let mut raw = base_terms();
        raw["contractRole"] = json!("RPL");
        let risk = StateProvider::new().with_observed_event(observed(
            "2016-03-15T00:00:00",
            EventType::PrincipalRedemption,
        ));
        let events = UmpEngine.evaluate(&terms(raw), &risk).expect("events");
        assert_eq!(events[0].payoff, dec!(1000));
        assert_eq!(events[0].state.notional_principal, dec!(-1000));
        assert_eq!(events[1].payoff, dec!(-400));
        assert_eq!(events[1].state.notional_principal, dec!(-600));
    }

    /// Missing required attributes are reported.
    #[test]
    fn missing_required_attributes_are_reported() {
        let empty = ContractTerms::new(ContractType::Ump);
        assert!(matches!(
            UmpEngine.evaluate(&empty, &StateProvider::new()),
            Err(EngineError::MissingAttribute("notionalPrincipal"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("initialExchangeDate");
        assert!(matches!(
            UmpEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("initialExchangeDate"))
        ));
        let mut raw = base_terms();
        raw.as_object_mut().unwrap().remove("dayCountConvention");
        assert!(matches!(
            UmpEngine.evaluate(&terms(raw), &StateProvider::new()),
            Err(EngineError::MissingAttribute("dayCountConvention"))
        ));
    }

    /// The UMP testbed's ump04 shape (`RPL`, yearly interest from 2014).
    fn ump04(extra: serde_json::Value) -> ContractTerms {
        let mut raw = json!({
            "contractType": "UMP",
            "contractID": "ump04",
            "statusDate": "2012-12-15T00:00:00",
            "contractRole": "RPL",
            "currency": "USD",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "  1000",
            "cycleAnchorDateOfInterestPayment": "2014-01-01T00:00:00",
            "cycleOfInterestPayment": "P1YL1",
            "nominalInterestRate": "0.01",
            "dayCountConvention": "30E360",
            "xDayNotice": "P1M"
        });
        for (key, value) in extra.as_object().expect("object") {
            raw[key] = value.clone();
        }
        serde_json::from_value(raw).expect("terms")
    }

    /// ump04: without observed `PR` events, `Md = tmax` (techspec UMP
    /// states), so the scheduled interest runs to the analysis horizon,
    /// exclusive: 2014-2017 for a horizon of 2018-01-01.
    #[test]
    fn ump04_scheduled_interest_runs_to_the_analysis_horizon() {
        let risk = StateProvider::new().with_analysis_horizon(t("2018-01-01T00:00:00"));
        let events = UmpEngine
            .evaluate(&ump04(json!({})), &risk)
            .expect("ump04 evaluates");
        let times: Vec<NaiveDateTime> = events.iter().skip(1).map(|e| e.time).collect();
        assert_eq!(
            times,
            vec![
                t("2014-01-01T00:00:00"),
                t("2015-01-01T00:00:00"),
                t("2016-01-01T00:00:00"),
                t("2017-01-01T00:00:00"),
            ]
        );
        // Neither a termination nor a horizon: the schedule is open-ended.
        assert!(matches!(
            UmpEngine.evaluate(&ump04(json!({})), &StateProvider::new()),
            Err(EngineError::InvalidSchedule(_))
        ));
    }

    /// ump02, ump04: scheduled interest capitalizes (techspec UMP schedule
    /// `IPCI`, `STF_IPCI_PAM`): zero payoff, `Nt += Ipac + Y x Ipnr x Nt`.
    /// ump02's first period is 24 days of 30E/360 at 1 %: 1000.6666...;
    /// ump04 (`RPL`) compounds yearly: -1010, -1020.1, ...
    #[test]
    fn ump02_scheduled_interest_capitalizes() {
        let terms = ump04(json!({
            "contractRole": "RPA",
            "cycleAnchorDateOfInterestPayment": "2013-01-25T00:00:00",
            "cycleOfInterestPayment": "P1ML1"
        }));
        let risk = StateProvider::new().with_analysis_horizon(t("2014-01-01T00:00:00"));
        let events = UmpEngine.evaluate(&terms, &risk).expect("ump02 evaluates");
        assert_eq!(events.len(), 13);
        let first = &events[1];
        assert_eq!(first.event_type, EventType::InterestCapitalization);
        assert_eq!(first.payoff, Decimal::ZERO);
        assert_eq!(first.state.accrued_interest, Decimal::ZERO);
        assert_eq!(
            first.state.notional_principal.round_dp(9),
            dec!(1000.666666667)
        );
        let risk = StateProvider::new().with_analysis_horizon(t("2018-01-01T00:00:00"));
        let events = UmpEngine
            .evaluate(&ump04(json!({})), &risk)
            .expect("ump04 evaluates");
        let notionals: Vec<Decimal> = events
            .iter()
            .skip(1)
            .map(|e| e.state.notional_principal.round_dp(5))
            .collect();
        assert_eq!(
            notionals,
            vec![
                dec!(-1010),
                dec!(-1020.1),
                dec!(-1030.301),
                dec!(-1040.60401)
            ]
        );
    }
}
