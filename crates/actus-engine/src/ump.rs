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
//! - Scheduled interest (`IPANX`/`IPCL`) and rate resets (`RRANX`/`RRCL`
//!   observing `MOCRR` with multiplier and spread, PAM `RR` row) are honored
//!   only up to the termination date: the contract has no maturity, so the
//!   series would be unbounded and are not generated without `TD`.
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
    ContractRole, ContractTerms, ContractType, Cycle, DayCountConvention, EndOfMonthConvention,
    EventType,
};

use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// The contract role sign of the market instrument engines (paper section
/// 3.7 "Contract Role Sign Convention", Table 1).
///
/// The shared `role_sign` helper covers the fixed-income `RPA`/`RPL` pair;
/// the contingent and market instrument engines (UMP, STK, COM, FXOUT) also
/// face the short and seller orientations, which map to `-1` per Table 1
/// (`RPL`, `RF`/`ST`, `PF`, `SEL`, `UDLM`). All other roles act as `+1`.
pub(crate) fn position_sign(terms: &ContractTerms) -> Decimal {
    match terms.contract_role {
        Some(
            ContractRole::Rpl
            | ContractRole::Rf
            | ContractRole::Pf
            | ContractRole::Sel
            | ContractRole::Udlm,
        ) => -Decimal::ONE,
        _ => Decimal::ONE,
    }
}

/// Implementation of the UMP contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct UmpEngine;

/// UMP event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    InterestPayment,
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
        let sgn = position_sign(terms);
        let ctx = Context {
            dcc,
            sgn,
            ied,
            t0,
            termination,
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
        ctx.scheduled_slots(terms, &mut slots, &observed);
        slots.sort_by_key(|slot| (slot.time, slot.kind.priority()));

        let mut state = ContractState::initial(terms);
        state.notional_principal = sgn * notional;
        state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
        state.accrued_interest = terms.accrued_interest.unwrap_or(Decimal::ZERO);
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
}

impl Context {
    /// The scheduled `IP` and `RR` series (techspec PAM schedule rows IP and
    /// RR): unrolled from `IPANX`/`RRANX ?? IED` by `IPCL`/`RRCL` while the
    /// rolled date stays strictly before the termination date, observing
    /// `MOCRR` at the roll date. The series exist only when `TD` bounds them
    /// (the contract has no maturity, so they would be unbounded otherwise);
    /// a schedule point shared with an observed event of the same type is
    /// dropped in favor of the observed one.
    fn scheduled_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
        observed: &[(NaiveDateTime, EventType)],
    ) {
        let series = [
            (
                terms.cycle_of_interest_payment.as_ref(),
                terms
                    .cycle_anchor_date_of_interest_payment
                    .map(normalize_timestamp)
                    .unwrap_or(self.ied),
                EventType::InterestPayment,
                Kind::InterestPayment,
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
            let Some(termination) = self.termination else {
                continue;
            };
            for time in roll_series(anchor, cycle, Some(termination)) {
                if time <= self.ied || time <= self.t0 || time >= termination {
                    continue;
                }
                if observed.contains(&(time, event_type)) {
                    continue;
                }
                slots.push(Slot { kind, time });
            }
        }
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
        let accrual = y * state.nominal_interest_rate * state.notional_principal;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                let notional = terms
                    .notional_principal
                    .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
                state.notional_principal = self.sgn * notional;
                state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
                state.accrued_interest = terms.accrued_interest.unwrap_or(Decimal::ZERO);
                -self.sgn * notional
            }
            Kind::PrincipalRedemption => {
                state.accrued_interest += accrual;
                let mut payoff = state.notional_scaling_multiplier
                    * self.sgn
                    * state.next_principal_redemption_payment;
                if payoff.abs() > state.notional_principal.abs() {
                    payoff = state.notional_principal;
                }
                state.notional_principal -= payoff;
                payoff
            }
            Kind::InterestPayment => {
                let payoff = state.interest_scaling_multiplier * (state.accrued_interest + accrual);
                state.accrued_interest = Decimal::ZERO;
                payoff
            }
            Kind::RateReset => {
                state.accrued_interest += accrual;
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
                state.nominal_interest_rate = observed * multiplier + spread;
                Decimal::ZERO
            }
            Kind::Termination => {
                let price = terms
                    .price_at_termination_date
                    .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                let payoff = self.sgn * (price + state.accrued_interest + accrual);
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
) -> Vec<NaiveDateTime> {
    let Some(termination) = termination else {
        return vec![anchor];
    };
    let eomc = EndOfMonthConvention::Sd;
    let mut rolls = vec![anchor];
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
    rolls
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
    /// termination date; the terminal accrual starts from the last schedule
    /// point.
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
                (EventType::InterestPayment, t("2016-02-15T00:00:00")),
                (EventType::Termination, t("2016-03-15T00:00:00")),
            ]
        );
        // 1000 x 0.12 x 31/365.
        assert_close(events[1].payoff, dec!(10.191780821917808));
        // 1000 + 1000 x 0.12 x 29/365.
        assert_close(events[2].payoff, dec!(1009.5342465753425));
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
}
