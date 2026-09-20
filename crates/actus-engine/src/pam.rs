//! PAM: Principal At Maturity (ACTUS techspec section "PAM: Principal At
//! Maturity").
//!
//! A PAM contract exchanges a notional at `IED` and `MD` and pays periodic
//! interest on the constant notional. The event schedule covers `IED`, the
//! `IPCI` capitalization series while `IPCED` is in the future, the `IP`
//! interest series anchored at `IPANX`, the `RR` rate reset series anchored
//! at `RRANX`, the purchase/termination pair `PRD`/`TD` and `MD`.
//!
//! Conventions implemented here, resolved against the official testbed:
//!
//! - Contract role sign: `RPA` carries sign `+1`, `RPL` sign `-1`; the
//!   notional state `NT` is signed (`sgn x NT`), the `IED` payoff is
//!   `-sgn x (NT + PDIED)` and `IP` payoffs are `IPAC + Y x IPNR x NT`, so a
//!   lender (RPA) pays out principal and receives interest (testbed pam01,
//!   pam03).
//! - `CS*` business day conventions calculate payoffs and accrual anchors on
//!   the unshifted schedule and emit events at the shifted dates; `SC*`
//!   conventions shift first and calculate on the shifted dates (pam06 to
//!   pam11).
//! - Every state transition advances the accrual anchor `SD` to the event
//!   calculation time, so a same-timestamp `IP` before `RR` (dictionary event
//!   sequence) accrues with the pre-reset rate and the reset applies to the
//!   next accrual period (pam21 to pam24).
//! - `TD` zeroes `NT`/`IPAC` but keeps `IPNR`, and suppresses all events
//!   after it (pam12, pam20); the techspec's `TD` row resets `IPNR`, which
//!   the reference implementation does not do.
//! - When `PRD` is set, events before the purchase date are not observed
//!   (pam12, pam20); when `IED` lies before the status date the contract is
//!   evaluated progressed from `t0` with the states-at-t0 initialisation
//!   (pam13).

use std::collections::BTreeSet;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, DayCountConvention,
    EndOfMonthConvention, EventType,
};

use crate::common::{
    anchored_series, calculation_time, role_sign, stub_series, stub_series_unshifted,
};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the PAM contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct PamEngine;

/// Schedule entry of the PAM skeleton: one event slot with its calculation
/// time (accrual anchor input) and its emission time (event time; the two
/// diverge only under `CS*` business day conventions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

/// PAM event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    InterestPayment,
    InterestCapitalization,
    RateReset,
    RateResetFixed,
    Purchase,
    Termination,
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
            Kind::Purchase => EventType::Purchase,
            Kind::Termination => EventType::Termination,
            Kind::Maturity => EventType::Maturity,
        }
    }

    /// The dictionary event sequence number driving same-timestamp order.
    fn priority(self) -> u8 {
        self.event_type().priority()
    }
}

impl ContractEngine for PamEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Pam
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let ctx = Context::new(terms)?;
        let slots = ctx.schedule(terms);
        let progressed = ctx.ied < ctx.t0;
        let mut state = if progressed {
            progressed_initial(&ctx, terms)?
        } else {
            ContractState::initial(terms)
        };
        let mut events = Vec::new();
        for slot in slots {
            if slot.kind == Kind::InitialExchange && progressed {
                continue;
            }
            let payoff = ctx.apply(&mut state, slot, terms, risk)?;
            let reported = slot.emit >= ctx.ied;
            if !reported || !ctx.observed(slot.emit, terms) {
                continue;
            }
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

/// Resolved attributes and conventions of one PAM evaluation.
struct Context {
    t0: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
    sgn: Decimal,
}

impl Context {
    /// Resolves the attributes every PAM evaluation depends on.
    fn new(terms: &ContractTerms) -> Result<Context, EngineError> {
        let ied = terms
            .initial_exchange_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
        let maturity = terms
            .maturity_date
            .ok_or(EngineError::MissingAttribute("maturityDate"))?;
        let dcc = terms
            .day_count_convention
            .ok_or(EngineError::MissingAttribute("dayCountConvention"))?;
        Ok(Context {
            t0: ContractState::initial(terms).status_date,
            ied,
            maturity,
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

    /// Whether an event at `t` is observed: the analysis window starts at the
    /// status date (progressed contracts) and at the purchase date when `PRD`
    /// is set, and ends at termination when `TD` is set.
    fn observed(&self, t: NaiveDateTime, terms: &ContractTerms) -> bool {
        if t < self.t0 {
            return false;
        }
        if let Some(prd) = terms.purchase_date {
            if t < prd {
                return false;
            }
        }
        if let Some(td) = terms.termination_date {
            if t > td {
                return false;
            }
        }
        true
    }

    /// Builds the PAM event skeleton in deterministic (calculation time,
    /// dictionary sequence) order.
    fn schedule(&self, terms: &ContractTerms) -> Vec<Slot> {
        let mut slots = vec![
            Slot {
                kind: Kind::InitialExchange,
                calc: self.ied,
                emit: self.ied,
            },
            Slot {
                kind: Kind::Maturity,
                calc: self.maturity,
                emit: self.maturity,
            },
        ];
        if let Some(prd) = terms.purchase_date {
            slots.push(Slot {
                kind: Kind::Purchase,
                calc: prd,
                emit: prd,
            });
        }
        if let Some(td) = terms.termination_date {
            slots.push(Slot {
                kind: Kind::Termination,
                calc: td,
                emit: td,
            });
        }
        if terms.nominal_interest_rate.is_some() {
            self.interest_slots(terms, &mut slots);
            self.rate_reset_slots(terms, &mut slots);
        }
        slots.sort_by_key(|slot| (slot.calc, slot.kind.priority()));
        slots
    }

    /// The `IPCI` and `IP` series (techspec PAM schedule rows IPCI and IP).
    ///
    /// Both series unroll from the interest cycle anchor `IPANX`; the `IPCI`
    /// series runs to the capitalization end date `IPCED` (rolls strictly
    /// before it, `IPCED` itself appended unconditionally, pinned by lam22
    /// when `IPCED` precedes the anchor), and the `IP` series runs to
    /// maturity with the capitalization dates removed, so capitalization
    /// replaces interest payment while `IPCED` is in the future (testbed
    /// pam18, pam19). The removal deduplicates on the unshifted schedule
    /// dates: under `SC*` conventions the emission dates of the two series
    /// diverge independently (a rolled `IP` date shifts, the `IPCED` schedule
    /// end never shifts), so only the unshifted dates identify the shared
    /// schedule points.
    fn interest_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        let anchor = match terms
            .cycle_anchor_date_of_interest_payment
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => return,
        };
        let cycle = match terms.cycle_of_interest_payment.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        let ipci = match terms.capitalization_end_date.map(normalize_timestamp) {
            Some(ipced) if ipced < self.maturity => {
                let series = anchored_series(anchor, cycle, ipced, self.eomc, self.bdc, self.cal);
                let anchors: BTreeSet<NaiveDateTime> =
                    series.iter().map(|(unshifted, _)| *unshifted).collect();
                for (unshifted, emit) in series {
                    slots.push(Slot {
                        kind: Kind::InterestCapitalization,
                        calc: calculation_time(self.bdc, unshifted, emit),
                        emit,
                    });
                }
                anchors
            }
            _ => BTreeSet::new(),
        };
        for (unshifted, emit) in
            stub_series_unshifted(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
        {
            if ipci.contains(&unshifted) {
                continue;
            }
            slots.push(Slot {
                kind: Kind::InterestPayment,
                calc: calculation_time(self.bdc, unshifted, emit),
                emit,
            });
        }
    }

    /// The `RR` rate reset series (techspec PAM schedule row RR).
    ///
    /// The series unrolls from `RRANX` to maturity; the maturity element
    /// itself is not part of the series (a rate reset at maturity is
    /// meaningless and does not occur in the testbeds). With `RRNXT` set, the
    /// first schedule point after the status date becomes a fixed reset `RRF`
    /// applying `RRNXT` instead of a market observation.
    fn rate_reset_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        let anchor = match terms
            .cycle_anchor_date_of_rate_reset
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => return,
        };
        let cycle = match terms.cycle_of_rate_reset.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        let mut series = stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal);
        if series.last().map(|(_, emit)| *emit) == Some(self.maturity) {
            series.pop();
        }
        let mut fixed_pending = terms.next_reset_rate.is_some();
        for (calc, emit) in series {
            let kind = if fixed_pending && calc > self.t0 {
                fixed_pending = false;
                Kind::RateResetFixed
            } else {
                Kind::RateReset
            };
            slots.push(Slot { kind, calc, emit });
        }
    }

    /// Applies the state transition and payoff function of one slot
    /// (techspec PAM functions table) and returns the payoff.
    fn apply(
        &self,
        state: &mut ContractState,
        slot: Slot,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Decimal, EngineError> {
        let yf = day_count_fraction(state.status_date, slot.calc, self.dcc)?;
        let accrual = yf * state.nominal_interest_rate * state.notional_principal;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                let notional = terms
                    .notional_principal
                    .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
                let premium = terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO);
                state.notional_principal = self.sgn * notional;
                state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
                state.accrued_interest = initial_accrued(terms, state, slot.calc, self)?;
                self.sgn * -(notional + premium)
            }
            Kind::InterestPayment => {
                let payoff = state.interest_scaling_multiplier * (state.accrued_interest + accrual);
                state.accrued_interest = Decimal::ZERO;
                payoff
            }
            Kind::InterestCapitalization => {
                state.notional_principal += state.accrued_interest + accrual;
                state.accrued_interest = Decimal::ZERO;
                Decimal::ZERO
            }
            Kind::RateReset => {
                state.accrued_interest += accrual;
                let code = terms
                    .market_object_code_of_rate_reset
                    .as_deref()
                    .ok_or(EngineError::MissingAttribute("marketObjectCodeOfRateReset"))?;
                let observed =
                    risk.rate(code, slot.calc)
                        .ok_or(EngineError::RiskFactorMissing {
                            code: code.to_string(),
                            at: slot.calc.to_string(),
                        })?;
                let multiplier = terms.rate_multiplier.unwrap_or(Decimal::ONE);
                let spread = terms.rate_spread.unwrap_or(Decimal::ZERO);
                state.nominal_interest_rate = observed * multiplier + spread;
                Decimal::ZERO
            }
            Kind::RateResetFixed => {
                state.accrued_interest += accrual;
                state.nominal_interest_rate = terms
                    .next_reset_rate
                    .ok_or(EngineError::MissingAttribute("nextResetRate"))?;
                Decimal::ZERO
            }
            Kind::Purchase => {
                let price = terms
                    .price_at_purchase_date
                    .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                state.accrued_interest += accrual;
                self.sgn * -(price + state.accrued_interest)
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

/// The accrued interest state at `IED` (techspec PAM functions table, IED
/// row): the terms value when present, otherwise the accrual since `IPANX`
/// when the anchor precedes the initial exchange, otherwise zero.
fn initial_accrued(
    terms: &ContractTerms,
    state: &ContractState,
    ied: NaiveDateTime,
    ctx: &Context,
) -> Result<Decimal, EngineError> {
    if let Some(accrued) = terms.accrued_interest {
        return Ok(accrued);
    }
    if let Some(anchor) = terms.cycle_anchor_date_of_interest_payment {
        if anchor < ied {
            let yf = day_count_fraction(anchor, ied, ctx.dcc)?;
            return Ok(yf * state.notional_principal * state.nominal_interest_rate);
        }
    }
    Ok(Decimal::ZERO)
}

/// Builds the states-at-t0 of a progressed contract whose `IED` precedes the
/// status date (techspec PAM states table): the notional and rate are live
/// (`sgn x NT`, `IPNR`), and the accrued interest accrues from the last
/// interest schedule point before the status date, or is taken from the
/// terms when carried.
fn progressed_initial(ctx: &Context, terms: &ContractTerms) -> Result<ContractState, EngineError> {
    let mut state = ContractState::initial(terms);
    let notional = terms
        .notional_principal
        .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
    state.notional_principal = ctx.sgn * notional;
    state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
    state.accrued_interest = match terms.nominal_interest_rate {
        None => Decimal::ZERO,
        Some(_) => match terms.accrued_interest {
            Some(accrued) => accrued,
            None => {
                let accrual = progressed_accrued(ctx, terms, state.notional_principal)?;
                accrual.unwrap_or(Decimal::ZERO)
            }
        },
    };
    Ok(state)
}

/// The accrued interest of a progressed contract from the last interest
/// schedule point `t-` before the status date; `None` when no schedule point
/// precedes the status date.
fn progressed_accrued(
    ctx: &Context,
    terms: &ContractTerms,
    notional: Decimal,
) -> Result<Option<Decimal>, EngineError> {
    let anchor = match terms
        .cycle_anchor_date_of_interest_payment
        .map(normalize_timestamp)
    {
        Some(anchor) => anchor,
        None => return Ok(None),
    };
    let cycle = match terms.cycle_of_interest_payment.as_ref() {
        Some(cycle) => cycle,
        None => return Ok(None),
    };
    let previous = stub_series(anchor, cycle, ctx.maturity, ctx.eomc, ctx.bdc, ctx.cal)
        .into_iter()
        .map(|(calc, _)| calc)
        .filter(|t| *t < ctx.t0)
        .max();
    match previous {
        Some(t_minus) => {
            let yf = day_count_fraction(t_minus, ctx.t0, ctx.dcc)?;
            Ok(Some(
                yf * notional * terms.nominal_interest_rate.unwrap_or(Decimal::ZERO),
            ))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use chrono::NaiveDateTime;
    use serde_json::json;
    use std::str::FromStr;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn risk() -> StateProvider {
        StateProvider::new()
    }

    #[test]
    fn ied_payoff_is_role_signed_and_notional_state_flips() {
        let rpa = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "dayCountConvention": "A365"
        }));
        let rpl = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPL",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "dayCountConvention": "A365"
        }));

        let rpa_events = PamEngine.evaluate(&rpa, &risk()).expect("rpa");
        let ied = rpa_events
            .iter()
            .find(|e| e.event_type == EventType::InitialExchange)
            .expect("IED");
        assert_eq!(ied.payoff, Decimal::from(-3000));
        assert_eq!(ied.state.notional_principal, Decimal::from(3000));

        let rpl_events = PamEngine.evaluate(&rpl, &risk()).expect("rpl");
        let ied = rpl_events
            .iter()
            .find(|e| e.event_type == EventType::InitialExchange)
            .expect("IED");
        assert_eq!(ied.payoff, Decimal::from(3000));
        assert_eq!(ied.state.notional_principal, Decimal::from(-3000));
        let md = rpl_events.last().expect("MD");
        assert_eq!(md.payoff, Decimal::from(-3000));
    }

    #[test]
    fn premium_discount_shifts_the_ied_payoff() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "premiumDiscountAtIED": "-200",
            "dayCountConvention": "A365"
        }));
        let events = PamEngine.evaluate(&terms, &risk()).expect("events");
        let ied = events
            .iter()
            .find(|e| e.event_type == EventType::InitialExchange)
            .expect("IED");
        assert_eq!(ied.payoff, Decimal::from(-2800));
    }

    #[test]
    fn rate_reset_applies_market_observation_multiplier_and_spread() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-06-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "cycleAnchorDateOfRateReset": "2013-02-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "marketObjectCodeOfRateReset": "USD_SWP",
            "rateMultiplier": "2.5",
            "rateSpread": "0.02",
            "dayCountConvention": "30E360"
        }));
        let observed = StateProvider::new().with_rate(
            "USD_SWP",
            t("2013-02-01T00:00:00"),
            Decimal::new(9827160493827161, 18),
        );
        let events = PamEngine.evaluate(&terms, &observed).expect("events");
        let reset = events
            .iter()
            .find(|e| e.event_type == EventType::RateResetVariable)
            .expect("RR");
        let expected_rate = Decimal::from_str("0.0445679012345679").expect("rate");
        assert!((reset.state.nominal_interest_rate - expected_rate).abs() < Decimal::new(1, 12));
    }

    #[test]
    fn purchase_suppresses_the_ied_and_termination_suppresses_the_maturity() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfInterestPayment": "2013-01-31T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "purchaseDate": "2013-01-30T00:00:00",
            "priceAtPurchaseDate": "1000",
            "terminationDate": "2013-10-17T00:00:00",
            "priceAtTerminationDate": "2900",
            "dayCountConvention": "A365"
        }));
        let events = PamEngine.evaluate(&terms, &risk()).expect("events");
        assert_eq!(
            events.first().expect("first").event_type,
            EventType::Purchase
        );
        assert_eq!(
            events.last().expect("last").event_type,
            EventType::Termination
        );
        assert!(!events
            .iter()
            .any(|e| e.event_type == EventType::InitialExchange
                || e.event_type == EventType::Maturity));
        let prd = events.first().expect("PRD");
        let expected_accrued = Decimal::from(29) / Decimal::from(365) * Decimal::from(300);
        assert!((prd.state.accrued_interest - expected_accrued).abs() < Decimal::new(1, 6));
    }

    #[test]
    fn progressed_contract_starts_from_the_status_date_state() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2012-11-09T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfInterestPayment": "2013-01-09T00:00:00",
            "cycleOfInterestPayment": "P3ML0",
            "dayCountConvention": "AA"
        }));
        let events = PamEngine.evaluate(&terms, &risk()).expect("events");
        assert!(!events
            .iter()
            .any(|e| e.event_type == EventType::InitialExchange));
        let first = events.first().expect("first");
        assert_eq!(first.event_type, EventType::InterestPayment);
        assert_eq!(first.time, t("2013-01-09T00:00:00"));
        assert_eq!(first.state.notional_principal, Decimal::from(3000));
        let expected = (Decimal::from(2) / Decimal::from(366)
            + Decimal::from(8) / Decimal::from(365))
            * Decimal::from(300);
        assert!((first.payoff - expected).abs() < Decimal::new(1, 9));
    }

    #[test]
    fn end_of_day_maturity_calculates_on_the_raw_date_and_emits_midnight() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-12-31T23:59:59",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365"
        }));
        let events = PamEngine.evaluate(&terms, &risk()).expect("events");
        let ip = events
            .iter()
            .rev()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("final IP");
        assert_eq!(ip.time, t("2014-01-01T00:00:00"));
        let expected = Decimal::from(61) / Decimal::from(365) * Decimal::from(300);
        assert!((ip.payoff - expected).abs() < Decimal::new(1, 6));
    }

    #[test]
    fn capitalization_series_replaces_interest_until_the_capitalization_end() {
        let terms = terms(json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-07-01T00:00:00",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "capitalizationEndDate": "2013-03-01T00:00:00",
            "dayCountConvention": "A365"
        }));
        let events = PamEngine.evaluate(&terms, &risk()).expect("events");
        let ipcis: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestCapitalization)
            .map(|e| e.time)
            .collect();
        assert_eq!(
            ipcis,
            vec![
                t("2013-01-01T00:00:00"),
                t("2013-02-01T00:00:00"),
                t("2013-03-01T00:00:00")
            ]
        );
        let ips: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.time)
            .collect();
        assert_eq!(
            ips,
            vec![
                t("2013-04-01T00:00:00"),
                t("2013-05-01T00:00:00"),
                t("2013-06-01T00:00:00"),
                t("2013-07-01T00:00:00")
            ]
        );
    }
}
