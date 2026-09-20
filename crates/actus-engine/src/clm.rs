//! CLM: Call Money (ACTUS techspec section 7.6 "CLM: Call Money"; the full
//! LaTeX techspec section "CLM: Call Money").
//!
//! A call money deposit rolls like a PAM contract but capitalizes interest
//! at every interest cycle point: the `IPCI` series unrolls from
//! `IPANX ?? IED` while the rolled date stays strictly before the terminal
//! date, and a single `IP` event at the terminal date pays the residual
//! accrual before `MD` repays the capitalized notional. The terminal date
//! per the states table is `MD` when set; otherwise the contract is callable:
//! one observed `XD` (exercise notice, supplied through
//! [`RiskFactorProvider::observed_events`]) ends the rolling schedule and
//! settlement happens `XDN` later (`STD`), with the final accrual frozen at
//! the notice date.
//!
//! Conventions implemented here, resolved against the official testbed
//! (vendor/actus/tests/actus-tests-clm.json):
//!
//! - Events at or before the status date are not observed, including events
//!   at exactly `t0` (testbed clm01 drops the `IED`/`IPCI` at
//!   `IED == t0`, clm04 the `IPCI` at `t0`); a progressed contract starts
//!   from the raw terms states (`sgn x NT`, `IPNR`, `IPAC`) without
//!   progression accrual, so dropped schedule points leave no trace
//!   (clm03, clm04).
//! - The `RR` series mirrors PAM (unroll from `RRANX ?? IED`, long last stub
//!   with the terminal element removed); with a reset anchor but no reset
//!   cycle the series degenerates to the single anchor point (clm15).
//! - `SC*` business day conventions shift first and calculate on the shifted
//!   date, while the rate observation reads the market object at the
//!   unshifted roll date (clm10: reset calculated Friday 2015-08-14 from the
//!   Sunday roll 2015-08-16, observed value of 2015-08-16).
//! - The terminal `IP`/`MD` pair calculates on the business-day-adjusted
//!   maturity but emits at the raw maturity date (clm10: accrual ends Friday
//!   2015-09-18, events dated Sunday 2015-09-20).
//! - At a call settlement the reference emits, in order, `XD` (payoff 0), the
//!   final `IP` and `STD` (principal repayment) at the settlement date. When
//!   notice and settlement share the date the `XD` post-state carries the
//!   frozen accrual in `IPAC` (clm13, clm14); when settlement lies `XDN`
//!   later, the `XD` post-state reports `IPAC = 0` while the `IP` payoff
//!   still carries the full frozen accrual plus the pre-notice `IPAC`
//!   (clm07, clm08). No interest accrues across the notice window.

use chrono::{Duration, NaiveDateTime};
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, Cycle, CyclePeriod, CycleStub,
    DayCountConvention, EndOfMonthConvention, EventType,
};

use crate::common::{calculate_first, role_sign, stub_series};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::{sequence_rank, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::schedule::{cycle_step, shift_business_day};
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the CLM contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClmEngine;

/// Schedule entry of the CLM skeleton: one event slot with its calculation
/// time (accrual anchor input), its emission time, and the market
/// observation time of the unshifted roll date (the three coincide except
/// under business day conventions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
    obs: NaiveDateTime,
}

/// CLM event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    InterestPayment,
    InterestCapitalization,
    RateReset,
    RateResetFixed,
    Exercise,
    Settlement,
    Maturity,
}

impl Kind {
    /// The dictionary event type of the slot.
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::InterestPayment => EventType::InterestPayment,
            Kind::InterestCapitalization => EventType::InterestCapitalization,
            Kind::RateReset => EventType::RateResetVariable,
            Kind::RateResetFixed => EventType::RateResetFixed,
            Kind::Exercise => EventType::Exercise,
            Kind::Settlement => EventType::Settlement,
            Kind::Maturity => EventType::Maturity,
        }
    }

    /// The effective same-timestamp sequence rank of the slot, through the
    /// shared sequence rank (which places the call notice `XD` directly
    /// before the final interest payment, see `event::sequence_rank`).
    fn priority(self) -> u8 {
        sequence_rank(self.event_type())
    }
}

impl ContractEngine for ClmEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Clm
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let ctx = Context::new(terms, risk)?;
        let mut state = if ctx.progressed {
            ctx.progressed_initial(terms, notional)
        } else {
            ContractState::initial(terms)
        };
        let mut settlement: Option<Decimal> = None;
        let mut events = Vec::new();
        for slot in ctx.schedule(terms) {
            if !ctx.observed(slot.emit) {
                continue;
            }
            let payoff = ctx.apply(&mut state, slot, terms, risk, &mut settlement)?;
            events.push(ContractEvent {
                event_type: slot.kind.event_type(),
                time: normalize_timestamp(slot.emit),
                payoff,
                currency: terms.currency.clone(),
                state: state.clone(),
            });
        }
        Ok(events)
    }
}

/// Resolved attributes and conventions of one CLM evaluation.
struct Context {
    t0: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: Option<NaiveDateTime>,
    exercise: NaiveDateTime,
    settlement_date: NaiveDateTime,
    same_day_settlement: bool,
    progressed: bool,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
    sgn: Decimal,
}

impl Context {
    /// Resolves the attributes every CLM evaluation depends on.
    ///
    /// The terminal date comes from the states table: `MD` when set,
    /// otherwise the settlement date `XD + XDN` derived from the observed
    /// exercise notice.
    fn new(terms: &ContractTerms, risk: &dyn RiskFactorProvider) -> Result<Context, EngineError> {
        let ied = terms
            .initial_exchange_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
        let dcc = terms
            .day_count_convention
            .ok_or(EngineError::MissingAttribute("dayCountConvention"))?;
        let t0 = ContractState::initial(terms).status_date;
        let progressed = ied <= t0;
        let (exercise, settlement_date) = match terms.maturity_date.map(normalize_timestamp) {
            Some(maturity) => (maturity, maturity),
            None => {
                let notice = risk
                    .observed_events()
                    .into_iter()
                    .filter(|event| event.event_type == EventType::Exercise)
                    .map(|event| normalize_timestamp(event.time))
                    .min();
                let exercise = notice.ok_or(EngineError::MissingAttribute("exerciseDate"))?;
                let raw = terms
                    .x_day_notice
                    .as_deref()
                    .ok_or(EngineError::MissingAttribute("xDayNotice"))?;
                (exercise, notice_period(exercise, raw, terms)?)
            }
        };
        let same_day_settlement = settlement_date == exercise;
        Ok(Context {
            t0,
            ied,
            maturity: terms.maturity_date.map(normalize_timestamp),
            exercise,
            settlement_date,
            same_day_settlement,
            progressed,
            dcc,
            eomc: terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd),
            bdc: terms
                .business_day_convention
                .unwrap_or(BusinessDayConvention::Nos),
            cal: terms.calendar.unwrap_or(Calendar::Nc),
            sgn: role_sign(terms),
        })
    }

    /// Whether an event at the emission time `t` is observed: the CLM
    /// analysis window is strictly after the status date, so events at
    /// exactly `t0` are not reported (testbed clm01, clm04).
    fn observed(&self, t: NaiveDateTime) -> bool {
        t > self.t0
    }

    /// Builds the CLM event skeleton in deterministic (calculation time,
    /// sequence rank) order.
    fn schedule(&self, terms: &ContractTerms) -> Vec<Slot> {
        let mut slots = Vec::new();
        if !self.progressed {
            let (calc, emit) = self.point(self.ied);
            slots.push(Slot {
                kind: Kind::InitialExchange,
                calc,
                emit,
                obs: self.ied,
            });
        }
        self.interest_slots(terms, &mut slots);
        self.rate_reset_slots(terms, &mut slots);
        match self.maturity {
            Some(maturity) => {
                let calc = if calculate_first(self.bdc) {
                    maturity
                } else {
                    shift_business_day(maturity, self.bdc, self.cal)
                };
                slots.push(Slot {
                    kind: Kind::InterestPayment,
                    calc,
                    emit: maturity,
                    obs: maturity,
                });
                slots.push(Slot {
                    kind: Kind::Maturity,
                    calc,
                    emit: maturity,
                    obs: maturity,
                });
            }
            None => {
                slots.push(Slot {
                    kind: Kind::Exercise,
                    calc: self.exercise,
                    emit: self.exercise,
                    obs: self.exercise,
                });
                slots.push(Slot {
                    kind: Kind::InterestPayment,
                    calc: self.settlement_date,
                    emit: self.settlement_date,
                    obs: self.settlement_date,
                });
                slots.push(Slot {
                    kind: Kind::Settlement,
                    calc: self.settlement_date,
                    emit: self.settlement_date,
                    obs: self.settlement_date,
                });
            }
        }
        slots.sort_by_key(|slot| (slot.calc, slot.kind.priority()));
        slots
    }

    /// The `IPCI` capitalization series (techspec CLM schedule row IPCI).
    ///
    /// The series unrolls from the anchor `IPANX ?? IED` while the rolled
    /// date stays strictly before the terminal date; the terminal date
    /// itself belongs to the single terminal `IP` event, not to the series
    /// (testbed clm01: one-month cycle over one month yields no IPCI at
    /// all). The anchor element belongs to the series and is dropped by the
    /// analysis window when at or before the status date (clm01, clm02),
    /// while an `IED` after the status date carries a no-op `IPCI` at the
    /// exchange date (clm12).
    fn interest_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        let cycle = match terms.cycle_of_interest_payment.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        let anchor = match terms
            .cycle_anchor_date_of_interest_payment
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => self.ied,
        };
        for roll in self.roll_series(anchor, cycle) {
            let (calc, emit) = self.point(roll);
            if self.terminal_cut(calc) {
                continue;
            }
            slots.push(Slot {
                kind: Kind::InterestCapitalization,
                calc,
                emit,
                obs: roll,
            });
        }
    }

    /// The `RR` rate reset series (techspec CLM schedule row RR, "Same as
    /// PAM").
    ///
    /// The series unrolls from `RRANX ?? IED` with the PAM stub handling and
    /// the terminal element removed; with a reset anchor but no reset cycle
    /// it degenerates to the single anchor point (testbed clm15). Rate
    /// observations read the market object at the unshifted roll date even
    /// when the event calculates on the shifted date (clm10).
    fn rate_reset_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        let anchor = match (
            terms
                .cycle_anchor_date_of_rate_reset
                .map(normalize_timestamp),
            terms.cycle_of_rate_reset.as_ref(),
        ) {
            (Some(anchor), _) => anchor,
            (None, Some(_)) => self.ied,
            (None, None) => return,
        };
        let rolls = match terms.cycle_of_rate_reset.as_ref() {
            Some(cycle) => {
                let termination = self.rolling_end();
                let mut shifted =
                    stub_series(anchor, cycle, termination, self.eomc, self.bdc, self.cal);
                let mut raw = stub_series(
                    anchor,
                    cycle,
                    termination,
                    self.eomc,
                    BusinessDayConvention::Nos,
                    self.cal,
                );
                if shifted.last().map(|(_, emit)| *emit) == Some(termination) {
                    shifted.pop();
                    raw.pop();
                }
                shifted
                    .into_iter()
                    .zip(raw)
                    .map(|((calc, emit), (roll, _))| Slot {
                        kind: Kind::RateReset,
                        calc,
                        emit,
                        obs: roll,
                    })
                    .collect()
            }
            None => vec![Slot {
                kind: Kind::RateReset,
                calc: anchor,
                emit: anchor,
                obs: anchor,
            }],
        };
        for mut slot in rolls {
            if self.terminal_cut(slot.calc) {
                continue;
            }
            if terms.next_reset_rate.is_some()
                && slot.calc > self.t0
                && slot.kind == Kind::RateReset
            {
                slot.kind = Kind::RateResetFixed;
            }
            slots.push(slot);
        }
    }

    /// The terminal date the rolling series stop before: the maturity for
    /// uncalled contracts, the exercise notice for called ones.
    fn rolling_end(&self) -> NaiveDateTime {
        self.maturity.unwrap_or(self.exercise)
    }

    /// Whether a rolling schedule point at `t` is cut by the call notice
    /// (called contracts roll only strictly before the notice; clm07 keeps
    /// no reset at 2015-09-30 although the settlement lies at 2015-10-21).
    fn terminal_cut(&self, t: NaiveDateTime) -> bool {
        self.maturity.is_none() && t >= self.exercise
    }

    /// Unrolls the anchor element plus the cycle rolls strictly before the
    /// rolling end, without stub correction.
    fn roll_series(&self, anchor: NaiveDateTime, cycle: &Cycle) -> Vec<NaiveDateTime> {
        let end = self.rolling_end();
        let mut rolls = vec![anchor];
        let mut index: u64 = 0;
        loop {
            index += 1;
            let next = cycle_step(anchor, index, cycle, self.eomc);
            if next >= end {
                break;
            }
            rolls.push(next);
            if index > 20_000 {
                break;
            }
        }
        rolls
    }

    /// Resolves the (calculation, emission) pair of one roll date: `CS*`
    /// conventions calculate on the unshifted date and emit shifted, `SC*`
    /// conventions shift first and calculate on the shifted date.
    fn point(&self, roll: NaiveDateTime) -> (NaiveDateTime, NaiveDateTime) {
        if calculate_first(self.bdc) {
            (roll, shift_business_day(roll, self.bdc, self.cal))
        } else {
            let shifted = shift_business_day(roll, self.bdc, self.cal);
            (shifted, shifted)
        }
    }

    /// The states at `t0` of a progressed contract whose `IED` lies at or
    /// before the status date: the raw terms states without progression
    /// accrual, because every schedule point at or before `t0` is dropped
    /// without carrying its capitalization forward (testbed clm03, clm04).
    fn progressed_initial(&self, terms: &ContractTerms, notional: Decimal) -> ContractState {
        let mut state = ContractState::initial(terms);
        state.notional_principal = self.sgn * notional;
        state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
        state.accrued_interest = terms.accrued_interest.unwrap_or(Decimal::ZERO);
        state
    }

    /// Applies the state transition and payoff function of one slot
    /// (techspec CLM functions table: IED/PR/FP/RR/RRF/IPCI "Same as PAM",
    /// IP paying `Ipac + Y x Ipnr x Nt`) and returns the payoff.
    fn apply(
        &self,
        state: &mut ContractState,
        slot: Slot,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
        settlement: &mut Option<Decimal>,
    ) -> Result<Decimal, EngineError> {
        let y = day_count_fraction(state.status_date, slot.calc, self.dcc)?;
        let accrual = y * state.nominal_interest_rate * state.notional_principal;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                state.notional_principal = self.sgn * terms.notional_principal.unwrap_or(accrual);
                state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
                state.accrued_interest = terms.accrued_interest.unwrap_or(Decimal::ZERO);
                let premium = terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO);
                self.sgn * -(terms.notional_principal.unwrap_or_default() + premium)
            }
            Kind::InterestCapitalization => {
                state.notional_principal += state.accrued_interest + accrual;
                state.accrued_interest = Decimal::ZERO;
                Decimal::ZERO
            }
            Kind::RateReset | Kind::RateResetFixed => {
                state.accrued_interest += accrual;
                state.nominal_interest_rate = match slot.kind {
                    Kind::RateResetFixed => terms
                        .next_reset_rate
                        .ok_or(EngineError::MissingAttribute("nextResetRate"))?,
                    _ => {
                        let code = terms
                            .market_object_code_of_rate_reset
                            .as_deref()
                            .ok_or(EngineError::MissingAttribute("marketObjectCodeOfRateReset"))?;
                        let observed =
                            risk.rate(code, slot.obs)
                                .ok_or(EngineError::RiskFactorMissing {
                                    code: code.to_string(),
                                    at: slot.obs.to_string(),
                                })?;
                        let multiplier = terms.rate_multiplier.unwrap_or(Decimal::ONE);
                        let spread = terms.rate_spread.unwrap_or(Decimal::ZERO);
                        observed * multiplier + spread
                    }
                };
                Decimal::ZERO
            }
            Kind::Exercise => {
                let total = state.accrued_interest + accrual;
                *settlement = Some(total);
                state.accrued_interest = if self.same_day_settlement {
                    total
                } else {
                    Decimal::ZERO
                };
                Decimal::ZERO
            }
            Kind::InterestPayment => {
                let payoff = if self.maturity.is_none() {
                    let frozen = settlement.take().ok_or_else(|| {
                        EngineError::InvalidTransition("no called accrual".into())
                    })?;
                    state.interest_scaling_multiplier * frozen
                } else {
                    state.interest_scaling_multiplier * (state.accrued_interest + accrual)
                };
                state.accrued_interest = Decimal::ZERO;
                payoff
            }
            Kind::Settlement => {
                let payoff = state.notional_scaling_multiplier * state.notional_principal;
                state.notional_principal = Decimal::ZERO;
                state.accrued_interest = Decimal::ZERO;
                state.fee_accrued = Decimal::ZERO;
                state.contract_status = ContractStatus::Terminated;
                payoff
            }
            Kind::Maturity => {
                let payoff = state.notional_scaling_multiplier * state.notional_principal
                    + state.interest_scaling_multiplier * state.accrued_interest
                    + state.fee_accrued;
                state.notional_principal = Decimal::ZERO;
                state.accrued_interest = Decimal::ZERO;
                state.fee_accrued = Decimal::ZERO;
                state.contract_status = ContractStatus::Matured;
                payoff
            }
        };
        state.status_date = slot.calc;
        Ok(payoff)
    }
}

/// Adds the `XDN` notice period to the exercise time (techspec states
/// table, settlement sup of the observed event set).
///
/// Day and week periods shift by whole days (the testbed uses `P0D` and
/// `P31D`); month and year periods roll one cycle step with the End Of Month
/// Shift Convention of the terms.
fn notice_period(
    exercise: NaiveDateTime,
    raw: &str,
    terms: &ContractTerms,
) -> Result<NaiveDateTime, EngineError> {
    let upper = raw.trim().to_ascii_uppercase();
    let body = upper
        .strip_prefix('P')
        .ok_or_else(|| EngineError::InvalidTransition(format!("invalid xDayNotice: {raw}")))?;
    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
    let period = &body[digits.len()..];
    let n: i64 = digits
        .parse()
        .map_err(|_| EngineError::InvalidTransition(format!("invalid xDayNotice: {raw}")))?;
    match period {
        "D" => exercise
            .checked_add_signed(Duration::days(n))
            .ok_or_else(|| EngineError::InvalidTransition("xDayNotice overflow".into())),
        "W" => exercise
            .checked_add_signed(Duration::weeks(n))
            .ok_or_else(|| EngineError::InvalidTransition("xDayNotice overflow".into())),
        "M" | "Y" => {
            let cycle_period = if period == "M" {
                CyclePeriod::Month
            } else {
                CyclePeriod::Year
            };
            let cycle = Cycle::new(n.max(1) as u32, cycle_period, CycleStub::Short, 0)
                .map_err(|e| EngineError::InvalidTransition(e.to_string()))?;
            let eomc = terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd);
            Ok(cycle_step(exercise, 1, &cycle, eomc))
        }
        _ => Err(EngineError::InvalidTransition(format!(
            "invalid xDayNotice period: {raw}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use crate::ObservedEvent;
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

    fn tolerance() -> Decimal {
        dec!(0.000000001)
    }

    fn assert_close(actual: Decimal, expected: Decimal) {
        assert!(
            (actual - expected).abs() <= tolerance(),
            "expected {expected}, actual {actual}"
        );
    }

    /// The clm02 fixture: interest capitalizes at every ten-day cycle point
    /// on the growing notional, hand-checked first step
    /// `NT(1) = -1000 x (1 + 0.12 x 10/365) = -1003.287671232876712...`,
    /// with the residual two-day accrual paid at maturity and the
    /// capitalized notional repaid by `MD`.
    #[test]
    fn family_a_capitalization_chain_matches_the_reference() {
        let terms = terms(json!({
            "contractType": "CLM",
            "contractRole": "RPL",
            "statusDate": "2015-07-20T00:00:00",
            "initialExchangeDate": "2015-07-20T00:00:00",
            "maturityDate": "2015-09-20T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "cycleOfInterestPayment": "P10DL1",
            "calendar": "NC",
            "endOfMonthConvention": "SD",
            "dayCountConvention": "AA",
            "currency": "CHF"
        }));
        let events = ClmEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let times: Vec<NaiveDateTime> = events.iter().map(|e| e.time).collect();
        assert_eq!(
            times,
            vec![
                t("2015-07-30T00:00:00"),
                t("2015-08-09T00:00:00"),
                t("2015-08-19T00:00:00"),
                t("2015-08-29T00:00:00"),
                t("2015-09-08T00:00:00"),
                t("2015-09-18T00:00:00"),
                t("2015-09-20T00:00:00"),
                t("2015-09-20T00:00:00")
            ]
        );
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![
                EventType::InterestCapitalization,
                EventType::InterestCapitalization,
                EventType::InterestCapitalization,
                EventType::InterestCapitalization,
                EventType::InterestCapitalization,
                EventType::InterestCapitalization,
                EventType::InterestPayment,
                EventType::Maturity
            ]
        );
        assert_close(events[0].state.notional_principal, dec!(-1003.28767123288));
        assert_close(events[5].state.notional_principal, dec!(-1019.88887159849));
        for capitalization in &events[..6] {
            assert_eq!(capitalization.payoff, Decimal::ZERO);
            assert_eq!(capitalization.state.accrued_interest, Decimal::ZERO);
        }
        assert_close(events[6].payoff, dec!(-0.670611860777087));
        assert_close(events[6].state.notional_principal, dec!(-1019.88887159849));
        assert_close(events[7].payoff, dec!(-1019.88887159849));
        assert_eq!(events[7].state.notional_principal, Decimal::ZERO);
    }

    /// The clm13 fixture: a called contract with same-day settlement emits
    /// `XD` (payoff 0, the frozen 33-day accrual in `IPAC`), then `IP`
    /// paying that accrual, then `STD` repaying the notional, in that order
    /// at the shared settlement timestamp.
    #[test]
    fn family_b_terminal_emits_exercise_interest_settlement_in_order() {
        let terms = terms(json!({
            "contractType": "CLM",
            "contractRole": "RPA",
            "statusDate": "2015-08-20T00:00:00",
            "initialExchangeDate": "2015-08-22T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "xDayNotice": "P0D",
            "calendar": "NC",
            "endOfMonthConvention": "SD",
            "dayCountConvention": "AA",
            "currency": "CHF"
        }));
        let risk = StateProvider::new().with_observed_event(ObservedEvent {
            time: t("2015-09-24T00:00:00"),
            event_type: EventType::Exercise,
        });
        let events = ClmEngine.evaluate(&terms, &risk).expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2015-08-22T00:00:00")),
                (EventType::Exercise, t("2015-09-24T00:00:00")),
                (EventType::InterestPayment, t("2015-09-24T00:00:00")),
                (EventType::Settlement, t("2015-09-24T00:00:00"))
            ]
        );
        assert_eq!(events[0].payoff, dec!(-1000));
        assert_eq!(events[1].payoff, Decimal::ZERO);
        assert_close(events[1].state.accrued_interest, dec!(10.8493150684932));
        assert_close(events[2].payoff, dec!(10.8493150684932));
        assert_eq!(events[2].state.accrued_interest, Decimal::ZERO);
        assert_eq!(events[3].payoff, dec!(1000));
        assert_eq!(events[3].state.notional_principal, Decimal::ZERO);
    }

    /// The clm12 fixture: with `IED` after the status date the exchange
    /// carries a same-day no-op `IPCI` (dictionary order `IED` before
    /// `IPCI`), and events at the status date itself are still suppressed.
    #[test]
    fn initial_exchange_after_the_status_date_carries_a_noop_capitalization() {
        let terms = terms(json!({
            "contractType": "CLM",
            "contractRole": "RPA",
            "statusDate": "2015-08-20T00:00:00",
            "initialExchangeDate": "2015-08-22T00:00:00",
            "maturityDate": "2015-09-20T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "cycleOfInterestPayment": "P1ML1",
            "calendar": "NC",
            "endOfMonthConvention": "SD",
            "dayCountConvention": "AA",
            "currency": "CHF"
        }));
        let events = ClmEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2015-08-22T00:00:00")),
                (EventType::InterestCapitalization, t("2015-08-22T00:00:00")),
                (EventType::InterestPayment, t("2015-09-20T00:00:00")),
                (EventType::Maturity, t("2015-09-20T00:00:00"))
            ]
        );
        assert_eq!(events[1].payoff, Decimal::ZERO);
        assert_eq!(events[1].state.notional_principal, dec!(1000));
        assert_close(events[2].payoff, dec!(9.53424657534247));
    }

    /// The clm10 fixture: under `SCP` with the Monday-to-Friday calendar the
    /// reset rolls on Sunday 2015-08-16, calculates on Friday 2015-08-14 and
    /// still observes the 2015-08-16 market object; the terminal `IP`/`MD`
    /// accrue to the preceding business day 2015-09-18 but stay dated at the
    /// Sunday maturity.
    #[test]
    fn shift_first_conventions_calculate_shifted_and_observe_unshifted() {
        let terms = terms(json!({
            "contractType": "CLM",
            "contractRole": "RPL",
            "statusDate": "2015-08-05T00:00:00",
            "initialExchangeDate": "2015-07-20T00:00:00",
            "maturityDate": "2015-09-20T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "cycleAnchorDateOfInterestPayment": "2015-08-01T00:00:00",
            "cycleOfInterestPayment": "P2WL1",
            "cycleAnchorDateOfRateReset": "2015-08-01T00:00:00",
            "cycleOfRateReset": "P15DL1",
            "marketObjectCodeOfRateReset": "EUR_Prim",
            "businessDayConvention": "SCP",
            "calendar": "MF",
            "endOfMonthConvention": "EOM",
            "dayCountConvention": "AA",
            "currency": "EUR"
        }));
        let risk = StateProvider::new()
            .with_rate(
                "EUR_Prim",
                t("2015-08-16T00:00:00"),
                Decimal::from_str_exact("0.00897530864197531").expect("rate"),
            )
            .with_rate(
                "EUR_Prim",
                t("2015-08-31T00:00:00"),
                Decimal::from_str_exact("0.00897530864197531").expect("rate"),
            )
            .with_rate(
                "EUR_Prim",
                t("2015-09-15T00:00:00"),
                Decimal::from_str_exact("0.00897530864197531").expect("rate"),
            );
        let events = ClmEngine.evaluate(&terms, &risk).expect("events");
        let first_reset = events
            .iter()
            .find(|e| e.event_type == EventType::RateResetVariable)
            .expect("RR");
        assert_eq!(first_reset.time, t("2015-08-14T00:00:00"));
        assert_close(
            first_reset.state.nominal_interest_rate,
            dec!(0.00897530864197531),
        );
        assert_close(
            first_reset.state.notional_principal,
            dec!(-1002.95890410959),
        );
        let ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("IP");
        assert_eq!(ip.time, t("2015-09-20T00:00:00"));
        assert_close(ip.payoff, dec!(-0.172757405636449));
        let md = events.last().expect("MD");
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.time, t("2015-09-20T00:00:00"));
        assert_close(md.payoff, dec!(-1003.64957705672));
    }

    /// The clm15 fixture: a reset anchor without a reset cycle degenerates
    /// to the single anchor reset; the accrued interest resets to the
    /// observed rate for the remainder of the lifetime.
    #[test]
    fn reset_anchor_without_cycle_is_a_single_reset() {
        let terms = terms(json!({
            "contractType": "CLM",
            "contractRole": "RPL",
            "statusDate": "2015-08-20T00:00:00",
            "initialExchangeDate": "2015-08-22T00:00:00",
            "maturityDate": "2015-12-31T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.12",
            "cycleAnchorDateOfRateReset": "2015-09-01T00:00:00",
            "marketObjectCodeOfRateReset": "USD_Treasury",
            "calendar": "NC",
            "endOfMonthConvention": "SD",
            "dayCountConvention": "AA",
            "currency": "USD"
        }));
        let risk = StateProvider::new().with_rate(
            "USD_Treasury",
            t("2015-09-01T00:00:00"),
            Decimal::from_str_exact("0.00962962963").expect("rate"),
        );
        let events = ClmEngine.evaluate(&terms, &risk).expect("events");
        let types: Vec<(EventType, NaiveDateTime)> =
            events.iter().map(|e| (e.event_type, e.time)).collect();
        assert_eq!(
            types,
            vec![
                (EventType::InitialExchange, t("2015-08-22T00:00:00")),
                (EventType::RateResetVariable, t("2015-09-01T00:00:00")),
                (EventType::InterestPayment, t("2015-12-31T00:00:00")),
                (EventType::Maturity, t("2015-12-31T00:00:00"))
            ]
        );
        assert_close(events[1].state.accrued_interest, dec!(-3.28767123287671));
        assert_close(events[1].state.nominal_interest_rate, dec!(0.00962962963));
        assert_close(events[2].payoff, dec!(-6.4799594116));
        assert_close(events[3].payoff, dec!(-1000));
    }
}
