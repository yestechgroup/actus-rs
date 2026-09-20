//! NAM: Negative Amortizer (ACTUS techspec section "NAM: Negative
//! Amortizer").
//!
//! A NAM contract pays a constant total periodic amount: every `PR` event
//! redeems the principal as the residual `PRNXT - IPAC` of the period
//! payment (so the same-date `IP` event pays the interest portion out of the
//! same amount), and when the period interest exceeds the payment the
//! redemption turns negative and the notional grows (true negative
//! amortization, nam17). Interest accrues on the interest calculation base
//! amount `IPCB` exactly as in LAM; the `IPCI` capitalization series runs
//! while `IPCED` is in the future and folds the accrued interest into the
//! notional (nam19, nam22), after which the contract amortizes like a LAM.
//! The event schedule covers `IED`, the `PR` redemption series, the `IP`
//! interest series (with the `IPCI` series replacing it up to `IPCED`), the
//! optional `IPCB` base re-fixing series (`IPCB = NTL` only), the `RR`/`RRF`
//! rate reset series, the purchase/termination pair `PRD`/`TD` and `MD`.
//!
//! Conventions implemented here, resolved against the official testbed:
//!
//! - The `PR` payoff and state transition (techspec NAM functions table, PR
//!   row) with the contract role mirror: the reference implementation treats
//!   an `RPL` contract as the role-signed mirror of the `RPA` economics
//!   (nam02, nam04), so with the states carrying the role sign
//!   (`Prnxt = sgn PRNXT`, `Ipac` accrued on the signed base `Ipcb =
//!   sgn NT`) the payoff is `Nsc x (Prnxt - Ipac(t+))` and the notional
//!   reduces by the payoff, `Nt(t+) = Nt(t-) - (Prnxt - Ipac(t+))`. The
//!   redemption is never capped at the remaining notional and `Prnxt` is
//!   never recalculated: nam17 pays negative redemptions while the notional
//!   grows, and nam19/nam22 keep `PRNXT + IPAC` constant at the terms amount
//!   across the capitalization.
//! - Maturity inference (techspec NAM states table, `Md`): when the `MD`
//!   attribute is absent, `n = ceil(NT / (PRNXT - NT x Y(t-, t-+PRCL) x
//!   IPNR))` redemption periods amortize the notional and maturity is the
//!   `n`-th element of the redemption schedule (the anchor itself counts as
//!   the first), i.e. the anchor rolled `n - 1` times (nam15: 2013-12-01,
//!   nam19: 2014-08-01). The year fraction uses the contract day count
//!   convention and the terms rate; the denominator is the net principal
//!   portion of the first period payment.
//! - The `IPCI` capitalization STF is LAM's: `NT += IPAC`, `IPAC` resets and
//!   the `NT` base semantics re-fixes `Ipcb` to the capitalized notional
//!   (nam19 capitalizes 236.46 accrued since `IED` into `NT = 5236.46`).
//!   Contrary to the summary in the wave handover, `PRNXT` is not
//!   recalculated from the new notional: the testbed keeps the terms amount
//!   as the total periodic payment.
//! - The initial accrued interest at `IED` is the role-signed terms value
//!   (`sgn x IPAC_attrs`, nam04 initializes `Ipac = -200` for `RPL` with
//!   `IPAC_attrs = 200`); the pre-accrual fallback from an interest anchor
//!   before `IED` accrues on the signed notional.
//! - Rate resets follow the PAM machinery: with `RRNXT` set the first
//!   redemption-cycle rate point after the status date becomes a fixed reset
//!   `RRF` applying `RRNXT` (nam13), the remaining points observe the market
//!   object with multiplier and spread. Schedule elements at maturity belong
//!   to `MD` and are not emitted separately (`PR`, `RR`, `IPCB`); the `IP`
//!   series keeps its maturity element and pays the final accrual before
//!   `MD` (nam14, nam19, nam22).

use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::BTreeSet;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, DayCountConvention,
    EndOfMonthConvention, EventType, InterestCalculationBase,
};

use crate::common::{
    anchored_series, apply_principal_redemption, base_is_nt, base_tracks_notional,
    calculation_time, initial_base_amount, role_sign, stub_series, stub_series_unshifted,
    term_scaling_multipliers,
};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the NAM contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct NamEngine;

/// Schedule entry of the NAM skeleton: one event slot with its calculation
/// time (accrual anchor input) and its emission time (event time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

/// NAM event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    InterestPayment,
    InterestCapitalization,
    InterestCalculationBaseFixing,
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
            Kind::PrincipalRedemption => EventType::PrincipalRedemption,
            Kind::InterestPayment => EventType::InterestPayment,
            Kind::InterestCapitalization => EventType::InterestCapitalization,
            Kind::InterestCalculationBaseFixing => EventType::InterestCalculationBaseFixing,
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

impl ContractEngine for NamEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Nam
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
            initial_state(&ctx, terms)
        };
        let mut events = Vec::new();
        for slot in slots {
            if slot.kind == Kind::InitialExchange && progressed {
                continue;
            }
            let payoff = ctx.apply(&mut state, slot, terms, risk)?;
            let reported = slot.emit >= ctx.ied;
            if !reported || !ctx.observed(slot.emit, slot.kind.priority(), terms) {
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

/// Resolved attributes and conventions of one NAM evaluation.
struct Context {
    t0: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
    sgn: Decimal,
    next_principal_redemption: Decimal,
    base_tracks_notional: bool,
}

impl Context {
    /// Resolves the attributes every NAM evaluation depends on, including
    /// the maturity inference from the net redemption schedule and the
    /// required next principal redemption payment (techspec NAM states
    /// table, `Prnxt`).
    fn new(terms: &ContractTerms) -> Result<Context, EngineError> {
        let ied = terms
            .initial_exchange_date
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
        let notional = terms
            .notional_principal
            .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
        let dcc = terms
            .day_count_convention
            .ok_or(EngineError::MissingAttribute("dayCountConvention"))?;
        let eomc = terms
            .end_of_month_convention
            .unwrap_or(EndOfMonthConvention::Sd);
        let bdc = terms
            .business_day_convention
            .unwrap_or(BusinessDayConvention::Nos);
        let cal = terms.calendar.unwrap_or(Calendar::Nc);
        let pranx = terms
            .cycle_anchor_date_of_principal_redemption
            .map(normalize_timestamp)
            .ok_or(EngineError::MissingAttribute(
                "cycleAnchorDateOfPrincipalRedemption",
            ))?;
        let prcl = terms
            .cycle_of_principal_redemption
            .as_ref()
            .ok_or(EngineError::MissingAttribute("cycleOfPrincipalRedemption"))?;
        let next_principal_redemption =
            terms
                .next_principal_redemption_payment
                .ok_or(EngineError::MissingAttribute(
                    "nextPrincipalRedemptionPayment",
                ))?;
        let maturity = match terms.maturity_date.map(normalize_timestamp) {
            Some(maturity) => maturity,
            None => inferred_maturity(terms, notional, pranx, prcl, ied, dcc, eomc)?,
        };
        Ok(Context {
            t0: ContractState::initial(terms).status_date,
            ied,
            maturity,
            dcc,
            eomc,
            bdc,
            cal,
            sgn: role_sign(terms),
            next_principal_redemption,
            base_tracks_notional: base_tracks_notional(terms),
        })
    }

    /// Whether an event at `t` is observed: the analysis window starts at the
    /// status date (progressed contracts) and at the purchase date when
    /// `PRD` is set, and ends at termination when `TD` is set. At the
    /// purchase date itself the events preceding `PRD` in the dictionary
    /// sequence mutate the state but are not reported.
    fn observed(&self, t: NaiveDateTime, priority: u8, terms: &ContractTerms) -> bool {
        if t < self.t0 {
            return false;
        }
        if let Some(prd) = terms.purchase_date {
            if t < prd || (t == prd && priority < EventType::Purchase.priority()) {
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

    /// Builds the NAM event skeleton in deterministic (calculation time,
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
        self.principal_redemption_slots(terms, &mut slots);
        if terms.nominal_interest_rate.is_some() {
            self.interest_slots(terms, &mut slots);
            self.rate_reset_slots(terms, &mut slots);
        }
        self.interest_calculation_base_slots(terms, &mut slots);
        slots.sort_by_key(|slot| (slot.calc, slot.kind.priority()));
        slots
    }

    /// The `PR` principal redemption series (techspec NAM schedule row PR):
    /// `S(PRANX, PRCL, MD)` without the maturity element, which the
    /// maturity event supersedes.
    fn principal_redemption_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        let anchor = match terms.cycle_anchor_date_of_principal_redemption {
            Some(anchor) => normalize_timestamp(anchor),
            None => return,
        };
        let cycle = match terms.cycle_of_principal_redemption.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        for (calc, emit) in stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
        {
            if calc == self.maturity {
                continue;
            }
            slots.push(Slot {
                kind: Kind::PrincipalRedemption,
                calc,
                emit,
            });
        }
    }

    /// The `IPCI` and `IP` series (techspec NAM schedule rows IPCI and IP).
    ///
    /// Both series unroll from the interest cycle anchor `IPANX`; the `IPCI`
    /// series runs to the capitalization end date `IPCED` (the capitalization
    /// end always belongs to the schedule, even when it precedes the
    /// interest anchor: nam19 capitalizes at `IPCED` alone), and the `IP`
    /// series runs to maturity with the capitalization dates removed, so
    /// capitalization replaces interest payment while `IPCED` is in the
    /// future. The removal deduplicates on the unshifted schedule dates:
    /// under `SC*` conventions the emission dates of the two series diverge
    /// independently, so only the unshifted dates identify the shared
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

    /// The `RR`/`RRF` rate reset series (techspec NAM schedule row RR).
    ///
    /// The series unrolls from `RRANX` to maturity without the maturity
    /// element; with `RRNXT` set, the first schedule point after the status
    /// date becomes a fixed reset `RRF` applying `RRNXT` instead of a market
    /// observation (nam13).
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

    /// The `IPCB` interest calculation base fixing series (techspec NAM
    /// schedule row IPCB): defined only for `IPCB = NTL`, unrolled from
    /// `IPCBANX` to maturity without the maturity element.
    fn interest_calculation_base_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        if terms.interest_calculation_base != Some(InterestCalculationBase::Ntl) {
            return;
        }
        let anchor = match terms
            .cycle_anchor_date_of_interest_calculation_base
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => return,
        };
        let cycle = match terms.cycle_of_interest_calculation_base.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        for (calc, emit) in stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
        {
            if calc == self.maturity {
                continue;
            }
            slots.push(Slot {
                kind: Kind::InterestCalculationBaseFixing,
                calc,
                emit,
            });
        }
    }

    /// Applies the state transition and payoff function of one slot
    /// (techspec NAM functions table) and returns the payoff. Every
    /// transition first accrues `Y x IPNR x IPCB` into `IPAC`.
    fn apply(
        &self,
        state: &mut ContractState,
        slot: Slot,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Decimal, EngineError> {
        let yf = day_count_fraction(state.status_date, slot.calc, self.dcc)?;
        let accrual = yf * state.nominal_interest_rate * state.interest_calculation_base_amount;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                let notional = terms
                    .notional_principal
                    .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
                let premium = terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO);
                state.notional_principal = self.sgn * notional;
                state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
                state.accrued_interest = initial_accrued(terms, state, slot.calc, self)?;
                state.next_principal_redemption_payment = self.sgn * self.next_principal_redemption;
                let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
                state.interest_scaling_multiplier = interest_scaling;
                state.notional_scaling_multiplier = notional_scaling;
                state.interest_calculation_base_amount =
                    initial_base_amount(terms, notional, self.sgn);
                self.sgn * -(notional + premium)
            }
            Kind::PrincipalRedemption => {
                state.accrued_interest += accrual;
                apply_principal_redemption(state, self.base_tracks_notional)
            }
            Kind::InterestPayment => {
                state.accrued_interest += accrual;
                let payoff = state.interest_scaling_multiplier * state.accrued_interest;
                state.accrued_interest = Decimal::ZERO;
                state.last_interest_payment_date = Some(slot.calc);
                payoff
            }
            Kind::InterestCapitalization => {
                state.accrued_interest += accrual;
                state.notional_principal += state.accrued_interest;
                state.accrued_interest = Decimal::ZERO;
                if base_is_nt(terms) {
                    state.interest_calculation_base_amount = state.notional_principal;
                }
                Decimal::ZERO
            }
            Kind::InterestCalculationBaseFixing => {
                state.accrued_interest += accrual;
                state.interest_calculation_base_amount = state.notional_principal;
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
                state.accrued_interest += accrual;
                let price = terms
                    .price_at_purchase_date
                    .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
                self.sgn * -(price + state.accrued_interest)
            }
            Kind::Termination => {
                state.accrued_interest += accrual;
                let price = terms
                    .price_at_termination_date
                    .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
                let payoff = self.sgn * (price + state.accrued_interest);
                state.notional_principal = Decimal::ZERO;
                state.accrued_interest = Decimal::ZERO;
                state.fee_accrued = Decimal::ZERO;
                state.interest_calculation_base_amount = Decimal::ZERO;
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
                state.interest_calculation_base_amount = Decimal::ZERO;
                state.contract_status = ContractStatus::Matured;
                payoff
            }
        };
        state.status_date = slot.calc;
        Ok(payoff)
    }
}

/// The maturity inferred from the net redemption schedule when the `MD`
/// attribute is absent (techspec NAM states table, `Md`).
///
/// The anchor of the redemption schedule (`t-`) rolled `n - 1` times by the
/// redemption cycle, with `n = ceil(NT / (PRNXT - NT x Y(t-, t-+PRCL) x
/// IPNR))`: the denominator is the net principal portion of one period
/// payment, i.e. the payment minus one period of interest on the notional.
/// The anchor fallback chain uses `PRANX` when at or after the status date
/// and the first redemption roll after `IED` otherwise.
fn inferred_maturity(
    terms: &ContractTerms,
    notional: Decimal,
    pranx: NaiveDateTime,
    prcl: &actus_model::Cycle,
    ied: NaiveDateTime,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
) -> Result<NaiveDateTime, EngineError> {
    let prnxt = terms
        .next_principal_redemption_payment
        .ok_or(EngineError::MissingAttribute(
            "nextPrincipalRedemptionPayment",
        ))?;
    let t0 = ContractState::initial(terms).status_date;
    let t_minus = if pranx >= t0 {
        pranx
    } else {
        normalize_timestamp(cycle_step(ied, 1, prcl, eomc))
    };
    let period_end = normalize_timestamp(cycle_step(t_minus, 1, prcl, eomc));
    let yf = day_count_fraction(t_minus, period_end, dcc)?;
    let period_interest = notional * yf * terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
    let net_redemption = prnxt - period_interest;
    if net_redemption <= Decimal::ZERO {
        return Err(EngineError::InvalidTransition(
            "notional principal is not redeemable: the period interest exhausts the payment"
                .to_string(),
        ));
    }
    let redemptions =
        (notional / net_redemption)
            .ceil()
            .to_u64()
            .ok_or(EngineError::InvalidTransition(
                "notional principal is not redeemable by the net redemption amount".to_string(),
            ))?;
    let steps = redemptions.saturating_sub(1);
    Ok(normalize_timestamp(cycle_step(pranx, steps, prcl, eomc)))
}

/// The accrued interest state at `IED` (techspec NAM functions table, IED
/// row): the role-signed terms value when present, otherwise the accrual
/// since `IPANX` on the signed notional when the anchor precedes the initial
/// exchange, otherwise zero.
fn initial_accrued(
    terms: &ContractTerms,
    state: &ContractState,
    ied: NaiveDateTime,
    ctx: &Context,
) -> Result<Decimal, EngineError> {
    if let Some(accrued) = terms.accrued_interest {
        return Ok(ctx.sgn * accrued);
    }
    if let Some(anchor) = terms.cycle_anchor_date_of_interest_payment {
        if anchor < ied {
            let yf = day_count_fraction(anchor, ied, ctx.dcc)?;
            return Ok(yf * state.notional_principal * state.nominal_interest_rate);
        }
    }
    Ok(Decimal::ZERO)
}

/// Builds the pre-initial-exchange-date state (techspec NAM states table,
/// `Ipcb` row: `t0 < IED` yields a zeroed calculation base): the redemption
/// amount (role signed) and scaling multipliers are already resolved from
/// the terms, the monetary and rate states stay zero until `IED`.
fn initial_state(ctx: &Context, terms: &ContractTerms) -> ContractState {
    let mut state = ContractState::initial(terms);
    state.next_principal_redemption_payment = ctx.sgn * ctx.next_principal_redemption;
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state
}

/// Builds the states-at-t0 of a progressed contract whose `IED` precedes the
/// status date (techspec NAM states table): the notional, rate, redemption
/// amount and calculation base are live, the scaling multipliers come from
/// the terms, and the accrued interest accrues from the last interest
/// schedule point before the status date or is taken from the terms when
/// carried.
fn progressed_initial(ctx: &Context, terms: &ContractTerms) -> Result<ContractState, EngineError> {
    let mut state = ContractState::initial(terms);
    let notional = terms
        .notional_principal
        .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
    state.notional_principal = ctx.sgn * notional;
    state.nominal_interest_rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
    state.next_principal_redemption_payment = ctx.sgn * ctx.next_principal_redemption;
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state.interest_calculation_base_amount = initial_base_amount(terms, notional, ctx.sgn);
    state.accrued_interest = match terms.nominal_interest_rate {
        None => Decimal::ZERO,
        Some(_) => match terms.accrued_interest {
            Some(accrued) => ctx.sgn * accrued,
            None => progressed_accrued(ctx, terms, state.interest_calculation_base_amount)?
                .unwrap_or(Decimal::ZERO),
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
    base_amount: Decimal,
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
                yf * base_amount * terms.nominal_interest_rate.unwrap_or(Decimal::ZERO),
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
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn assert_close(actual: Decimal, expected: Decimal) {
        let tolerance = dec!(0.0000000001);
        assert!(
            (actual - expected).abs() < tolerance,
            "actual {actual} vs expected {expected}"
        );
    }

    fn nam19_risk() -> StateProvider {
        StateProvider::new()
            .with_rate(
                "LIBOR_USD",
                t("2013-04-01T00:00:00"),
                dec!(0.0105679012345679),
            )
            .with_rate(
                "LIBOR_USD",
                t("2013-07-01T00:00:00"),
                dec!(0.0116790123456790),
            )
            .with_rate(
                "LIBOR_USD",
                t("2013-10-01T00:00:00"),
                dec!(0.0127901234567901),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-01-01T00:00:00"),
                dec!(0.0139012345679012),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-04-01T00:00:00"),
                dec!(0.0150123456790123),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-07-01T00:00:00"),
                dec!(0.0161234567901234),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-10-01T00:00:00"),
                dec!(0.0172345679012345),
            )
    }

    #[test]
    fn principal_redemption_pays_the_net_principal_portion() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-12-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        }));
        let events = NamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.time, t("2013-02-01T00:00:00"));
        assert_close(pr.payoff, dec!(466.0273972602740));
        assert_close(pr.state.notional_principal, dec!(4533.9726027397260));
        assert_close(pr.state.accrued_interest, dec!(33.9726027397260));
        assert_eq!(
            pr.state.next_principal_redemption_payment,
            Decimal::from(500)
        );
        let ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("first IP");
        assert_close(ip.payoff, dec!(33.9726027397260));
        assert_eq!(ip.state.accrued_interest, Decimal::ZERO);
    }

    #[test]
    fn payment_shortfall_amortizes_negatively() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2016-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "40",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "cycleAnchorDateOfRateReset": "2014-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.12",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let risk = StateProvider::new()
            .with_rate(
                "LIBOR_USD",
                t("2014-04-01T00:00:00"),
                dec!(0.0150123456790123),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-07-01T00:00:00"),
                dec!(0.0161234567901234),
            )
            .with_rate(
                "LIBOR_USD",
                t("2014-10-01T00:00:00"),
                dec!(0.0172345679012345),
            )
            .with_rate(
                "LIBOR_USD",
                t("2015-01-01T00:00:00"),
                dec!(0.0183456790123456),
            )
            .with_rate(
                "LIBOR_USD",
                t("2015-04-01T00:00:00"),
                dec!(0.0194567901234568),
            )
            .with_rate(
                "LIBOR_USD",
                t("2015-07-01T00:00:00"),
                dec!(0.0205679012345678),
            )
            .with_rate(
                "LIBOR_USD",
                t("2015-10-01T00:00:00"),
                dec!(0.0216790123456790),
            );
        let events = NamEngine.evaluate(&terms, &risk).expect("events");
        let pr = events
            .iter()
            .rev()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("last PR");
        assert_eq!(pr.time, t("2015-12-01T00:00:00"));
        assert_close(pr.payoff, dec!(-21.0037048092));
        assert_close(pr.state.notional_principal, dec!(5259.6888816489));
    }

    #[test]
    fn capitalization_folds_accrued_interest_into_the_notional() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-10-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML1",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-10-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "capitalizationEndDate": "2013-07-01T00:00:00",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let events = NamEngine.evaluate(&terms, &nam19_risk()).expect("events");
        let ipcis: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestCapitalization)
            .collect();
        assert_eq!(ipcis.len(), 1);
        assert_eq!(ipcis[0].time, t("2013-07-01T00:00:00"));
        assert_close(ipcis[0].state.notional_principal, dec!(5236.4613563335));
        assert_eq!(ipcis[0].state.accrued_interest, Decimal::ZERO);
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR after capitalization");
        assert_eq!(pr.time, t("2013-10-01T00:00:00"));
        assert_close(pr.state.accrued_interest, dec!(147.402357771153));
        assert_close(pr.payoff, dec!(352.597642228847));
        assert_eq!(
            pr.state.next_principal_redemption_payment,
            Decimal::from(500)
        );
    }

    #[test]
    fn capitalization_end_replaces_interest_until_ipced() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-09-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-10-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-09-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "capitalizationEndDate": "2013-09-01T00:00:00",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let events = NamEngine.evaluate(&terms, &nam19_risk()).expect("events");
        assert_eq!(
            events
                .iter()
                .find(|e| e.event_type == EventType::InterestCapitalization)
                .expect("IPCI")
                .time,
            t("2013-09-01T00:00:00")
        );
        assert!(!events
            .iter()
            .any(|e| e.event_type == EventType::InterestPayment
                && e.time == t("2013-09-01T00:00:00")));
        let first_ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("first IP after capitalization");
        assert_eq!(first_ip.time, t("2013-10-01T00:00:00"));
        let ipcis: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestCapitalization)
            .map(|e| e.time)
            .collect();
        assert_eq!(ipcis.len(), 1);
    }

    #[test]
    fn maturity_is_inferred_from_the_net_redemption_schedule() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-10-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML1",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-10-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "capitalizationEndDate": "2013-07-01T00:00:00",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let events = NamEngine.evaluate(&terms, &nam19_risk()).expect("events");
        let maturity = events.last().expect("maturity");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.time, t("2014-08-01T00:00:00"));
        let ip = events
            .iter()
            .rev()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("final IP");
        assert_eq!(ip.time, t("2014-08-01T00:00:00"));
        assert_close(ip.payoff, dec!(6.326348387518158));
        assert_close(maturity.payoff, dec!(641.4522304807997));
    }

    #[test]
    fn maturity_inference_uses_the_period_interest_on_the_notional() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NTL",
            "interestCalculationBaseAmount": "5000",
            "cycleAnchorDateOfInterestCalculationBase": "2013-05-01T00:00:00",
            "cycleOfInterestCalculationBase": "P2ML1",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let events = NamEngine.evaluate(&terms, &nam19_risk()).expect("events");
        let maturity = events.last().expect("maturity");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.time, t("2013-12-01T00:00:00"));
        assert_close(maturity.payoff, dec!(267.276508843091));
    }

    #[test]
    fn role_long_mirror_flips_initial_accrued_and_redemptions() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPL",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2016-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "accruedInterest": "200",
            "cycleAnchorDateOfPrincipalRedemption": "2013-04-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-04-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "terminationDate": "2013-12-15T00:00:00",
            "priceAtTerminationDate": "1800",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let risk = StateProvider::new()
            .with_rate("LIBOR_USD", t("2013-04-01T00:00:00"), Decimal::ZERO)
            .with_rate(
                "LIBOR_USD",
                t("2013-07-01T00:00:00"),
                dec!(0.0116790123456790),
            )
            .with_rate(
                "LIBOR_USD",
                t("2013-10-01T00:00:00"),
                dec!(0.0127901234567901),
            );
        let events = NamEngine.evaluate(&terms, &risk).expect("events");
        let ied = events.first().expect("IED");
        assert_eq!(ied.payoff, Decimal::from(5000));
        assert_eq!(ied.state.notional_principal, Decimal::from(-5000));
        assert_eq!(ied.state.accrued_interest, Decimal::from(-200));
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_close(pr.payoff, dec!(-201.369863013699));
        assert_close(pr.state.notional_principal, dec!(-4798.630136986301));
        let td = events.last().expect("termination");
        assert_eq!(td.event_type, EventType::Termination);
        assert_close(td.payoff, dec!(-1795.575842425654));
        assert_eq!(td.state.notional_principal, Decimal::ZERO);
    }

    #[test]
    fn next_reset_rate_becomes_a_fixed_reset_before_market_resets() {
        let terms = terms(json!({
            "contractType": "NAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-12-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "nextResetRate": "0.06",
            "rateSpread": "0.10",
            "marketObjectCodeOfRateReset": "LIBOR_USD"
        }));
        let risk = StateProvider::new()
            .with_rate(
                "LIBOR_USD",
                t("2013-04-01T00:00:00"),
                dec!(0.0105679012345679),
            )
            .with_rate(
                "LIBOR_USD",
                t("2013-07-01T00:00:00"),
                dec!(0.011654320987605),
            )
            .with_rate(
                "LIBOR_USD",
                t("2013-10-01T00:00:00"),
                dec!(0.012765432098765),
            );
        let events = NamEngine.evaluate(&terms, &risk).expect("events");
        let fixed = events
            .iter()
            .find(|e| e.event_type == EventType::RateResetFixed)
            .expect("RRF");
        assert_eq!(fixed.time, t("2013-04-01T00:00:00"));
        assert_eq!(fixed.state.nominal_interest_rate, dec!(0.06));
        let variable = events
            .iter()
            .find(|e| e.event_type == EventType::RateResetVariable)
            .expect("first RR");
        assert_eq!(variable.time, t("2013-07-01T00:00:00"));
        assert_close(
            variable.state.nominal_interest_rate,
            dec!(0.111654320987605),
        );
    }
}
