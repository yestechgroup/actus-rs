//! LAM: Linear Amortizer (ACTUS techspec section "LAM: Linear Amortizer").
//!
//! A LAM contract redeems its notional through a schedule of principal
//! redemption events `S(PRANX, PRCL, MD)`, each paying the next principal
//! redemption payment `PRNXT` (scaled by `Nsc`) and reducing `NT`, while
//! interest accrues on the interest calculation base amount `IPCB` and is
//! paid periodically. The event schedule covers `IED`, the `PR` redemption
//! series, the `IP` interest series (with the `IPCI` capitalization series
//! while `IPCED` is in the future), the optional `IPCB` base re-fixing
//! series (`IPCB = NTL` only), the `RR`/`RRF` rate reset series, the `SC`
//! scaling series, the purchase/termination pair `PRD`/`TD` and `MD`.
//!
//! Conventions implemented here, resolved against the official testbed:
//!
//! - Maturity inference (techspec LAM states table, `Md`): when the `MD`
//!   attribute is absent, maturity is the `ceil(NT / PRNXT)`-th element of
//!   the redemption schedule, i.e. the date the notional is fully redeemed
//!   (lam01, lam05); conversely a missing `PRNXT` defaults to `NT` divided
//!   by the number of redemption schedule elements including the maturity
//!   element (lam27, lam29 to lam31). The techspec's year-fraction count
//!   formula diverges from the reference implementation here; the fixtures
//!   follow the schedule element count.
//! - The `IPCB` base semantics: `NT` and `NTIED` re-fix the base amount to
//!   the current notional at every `PR` (and `IED`/`IPCI` for `NT`); `NTL`
//!   keeps the terms `IPCBA` value and re-fixes it to the notional only at
//!   the `IPCB` schedule points (lam16, lam17). The techspec `PR` row
//!   re-fixes only for `NT`, which contradicts the reference behaviour
//!   observed in lam18 (`NTIED` tracking the notional).
//! - Every state transition accrues `Y x IPNR x IPCB` into `IPAC` first
//!   (the techspec `PR` row omits the accrual line, but the reference
//!   carries it: the `PR` post-state in lam01 holds the period accrual that
//!   the same-timestamp `IP` then pays, dictionary sequence `PR` before
//!   `IP`). The redemption amount cannot exceed the remaining notional: the
//!   next redemption payment is capped at `|NT|` after each `PR` (lam25 pays
//!   a zero redemption once the notional is fully redeemed, while lam26
//!   keeps paying the unscaled `PRNXT` with a scaled payoff while notional
//!   remains).
//! - `PRD` suppresses emission of the same-timestamp events that precede it
//!   in the dictionary sequence while they still mutate the state (lam02:
//!   the `PR`/`IP` at the purchase date applied, `NT` post-purchase reflects
//!   the redemption, only `PRD` is reported), and keeps the accrued interest
//!   in `IPAC` after paying it into the price (lam18, lam21).
//! - `SC` re-fixes the scaling multipliers to `index(t) / SCCDD` per the
//!   scaling effect direction (lam25, lam26). The techspec `SC` row spells
//!   the update `(obs - SCIED) / SCIED`, which with the reference
//!   initialisation `Nsc(t0) = Isc(t0) = 1` is the same ratio relative to
//!   the deal-date index; the fixtures implement the ratio form.
//! - Rate resets follow the PAM machinery: the first redemption-cycle rate
//!   point after the status date becomes a fixed reset `RRF` applying
//!   `RRNXT` (lam14, the first testbed exercise of that path), the remaining
//!   points observe the market object with multiplier and spread. Schedule
//!   elements at maturity belong to `MD` and are not emitted separately
//!   (`PR`, `RR`, `SC`, `IPCB`).
//! - Events emitted before the initial exchange date mutate the state but
//!   are not reported (the ann09 reading; a preceding business day shift can
//!   place the emission of an anchor-date schedule element before the
//!   initial exchange while the calculation stays on the schedule date).

use std::collections::BTreeSet;

use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, DayCountConvention,
    EndOfMonthConvention, EventType, InterestCalculationBase, ScalingEffect,
};

use crate::common::{
    anchored_series, base_is_nt, base_tracks_notional, calculation_time, initial_base_amount,
    role_sign, stub_series, stub_series_unshifted, term_scaling_multipliers,
};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::risk::RiskFactorProvider;
use crate::schedule::cycle_step;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the LAM contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct LamEngine;

/// Schedule entry of the LAM skeleton: one event slot with its calculation
/// time (accrual anchor input) and its emission time (event time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

/// LAM event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    InterestPayment,
    InterestCapitalization,
    InterestCalculationBaseFixing,
    RateReset,
    RateResetFixed,
    ScalingIndexFixing,
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
            Kind::ScalingIndexFixing => EventType::ScalingIndexFixing,
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

/// Whether the scaling effect drives the notional scaling multiplier
/// (techspec `SCEF = [x]N[x]`).
fn scales_notional(effect: Option<ScalingEffect>) -> bool {
    matches!(
        effect,
        Some(ScalingEffect::Notional) | Some(ScalingEffect::InterestAndNotional)
    )
}

/// Whether the scaling effect drives the interest scaling multiplier
/// (techspec `SCEF = I[x][x]`).
fn scales_interest(effect: Option<ScalingEffect>) -> bool {
    matches!(
        effect,
        Some(ScalingEffect::Interest) | Some(ScalingEffect::InterestAndNotional)
    )
}

impl ContractEngine for LamEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Lam
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

/// Resolved attributes and conventions of one LAM evaluation.
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
    /// Resolves the attributes every LAM evaluation depends on, including
    /// the maturity inference and the default redemption amount.
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
        let maturity = match terms.maturity_date.map(normalize_timestamp) {
            Some(maturity) => maturity,
            None => {
                let prnxt = terms.next_principal_redemption_payment.ok_or(
                    EngineError::MissingAttribute("nextPrincipalRedemptionPayment"),
                )?;
                let redemptions = (notional / prnxt)
                    .ceil()
                    .to_u64()
                    .ok_or(EngineError::InvalidTransition(
                    "notional principal is not redeemable by the next principal redemption payment"
                        .to_string(),
                ))?;
                let steps = redemptions.saturating_sub(1);
                normalize_timestamp(cycle_step(pranx, steps, prcl, eomc))
            }
        };
        let next_principal_redemption = match terms.next_principal_redemption_payment {
            Some(prnxt) => prnxt,
            None => {
                let elements = stub_series(pranx, prcl, maturity, eomc, bdc, cal).len();
                if elements == 0 {
                    return Err(EngineError::InvalidTransition(
                        "principal redemption schedule is empty".to_string(),
                    ));
                }
                notional / Decimal::from(elements as u64)
            }
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

    /// Builds the LAM event skeleton in deterministic (calculation time,
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
        self.scaling_index_slots(terms, &mut slots);
        self.interest_calculation_base_slots(terms, &mut slots);
        slots.sort_by_key(|slot| (slot.calc, slot.kind.priority()));
        slots
    }

    /// The `PR` principal redemption series (techspec LAM schedule row PR):
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

    /// The `IPCI` and `IP` series (techspec LAM schedule rows IPCI and IP).
    ///
    /// Both series unroll from the interest cycle anchor `IPANX`; the `IPCI`
    /// series runs to the capitalization end date `IPCED` (the capitalization
    /// end always belongs to the schedule), and the `IP` series runs to
    /// maturity with the capitalization dates removed, so capitalization
    /// replaces interest payment while `IPCED` is in the future (lam24). The
    /// removal deduplicates on the unshifted schedule dates: under `SC*`
    /// conventions the emission dates of the two series diverge
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

    /// The `RR`/`RRF` rate reset series (techspec LAM schedule row RR).
    ///
    /// The series unrolls from `RRANX` to maturity without the maturity
    /// element; with `RRNXT` set, the first schedule point after the status
    /// date becomes a fixed reset `RRF` applying `RRNXT` instead of a market
    /// observation (lam14).
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

    /// The `SC` scaling index series (techspec LAM schedule row SC):
    /// `S(SCANX, SCCL, MD)` without the maturity element.
    fn scaling_index_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) {
        if !scales_notional(terms.scaling_effect) && !scales_interest(terms.scaling_effect) {
            return;
        }
        let anchor = match terms
            .cycle_anchor_date_of_scaling_index
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => return,
        };
        let cycle = match terms.cycle_of_scaling_index.as_ref() {
            Some(cycle) => cycle,
            None => return,
        };
        for (calc, emit) in stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
        {
            if calc == self.maturity {
                continue;
            }
            slots.push(Slot {
                kind: Kind::ScalingIndexFixing,
                calc,
                emit,
            });
        }
    }

    /// The `IPCB` interest calculation base fixing series (techspec LAM
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
    /// (techspec LAM functions table) and returns the payoff. Every
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
                state.next_principal_redemption_payment = self.next_principal_redemption;
                let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
                state.interest_scaling_multiplier = interest_scaling;
                state.notional_scaling_multiplier = notional_scaling;
                state.interest_calculation_base_amount =
                    initial_base_amount(terms, notional, self.sgn);
                self.sgn * -(notional + premium)
            }
            Kind::PrincipalRedemption => {
                state.accrued_interest += accrual;
                let redemption = state.next_principal_redemption_payment;
                state.notional_principal -= self.sgn * redemption;
                if self.base_tracks_notional {
                    state.interest_calculation_base_amount = state.notional_principal;
                }
                state.next_principal_redemption_payment =
                    redemption.min(state.notional_principal.abs());
                self.sgn * state.notional_scaling_multiplier * redemption
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
            Kind::ScalingIndexFixing => {
                state.accrued_interest += accrual;
                let code = terms.market_object_code_of_scaling_index.as_deref().ok_or(
                    EngineError::MissingAttribute("marketObjectCodeOfScalingIndex"),
                )?;
                let observed =
                    risk.index(code, slot.calc)
                        .ok_or(EngineError::RiskFactorMissing {
                            code: code.to_string(),
                            at: slot.calc.to_string(),
                        })?;
                let deal_date_index = terms.scaling_index_at_contract_deal_date.ok_or(
                    EngineError::MissingAttribute("scalingIndexAtContractDealDate"),
                )?;
                let factor = observed / deal_date_index;
                if scales_notional(terms.scaling_effect) {
                    state.notional_scaling_multiplier = factor;
                }
                if scales_interest(terms.scaling_effect) {
                    state.interest_scaling_multiplier = factor;
                }
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

/// The accrued interest state at `IED` (techspec LAM functions table, IED
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

/// Builds the pre-initial-exchange-date state (techspec LAM states table,
/// `Ipcb` row: `t0 < IED` yields a zeroed calculation base): the redemption
/// amount and scaling multipliers are already resolved from the terms, the
/// monetary and rate states stay zero until `IED`.
fn initial_state(ctx: &Context, terms: &ContractTerms) -> ContractState {
    let mut state = ContractState::initial(terms);
    state.next_principal_redemption_payment = ctx.next_principal_redemption;
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state
}

/// Builds the states-at-t0 of a progressed contract whose `IED` precedes the
/// status date (techspec LAM states table): the notional, rate, redemption
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
    state.next_principal_redemption_payment = ctx.next_principal_redemption;
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state.interest_calculation_base_amount = initial_base_amount(terms, notional, ctx.sgn);
    state.accrued_interest = match terms.nominal_interest_rate {
        None => Decimal::ZERO,
        Some(_) => match terms.accrued_interest {
            Some(accrued) => accrued,
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

    #[test]
    fn principal_redemption_pays_prnxt_and_carries_the_period_accrual() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
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
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.time, t("2013-02-01T00:00:00"));
        assert_eq!(pr.payoff, Decimal::from(500));
        assert_eq!(pr.state.notional_principal, Decimal::from(4500));
        assert_close(pr.state.accrued_interest, dec!(33.972602739726));
        let ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("first IP");
        assert_eq!(ip.time, t("2013-02-01T00:00:00"));
        assert_close(ip.payoff, dec!(33.972602739726));
        assert_eq!(ip.state.accrued_interest, Decimal::ZERO);
    }

    #[test]
    fn maturity_is_inferred_from_the_redemption_schedule() {
        let terms = terms(json!({
            "contractType": "LAM",
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
            "interestCalculationBase": "NT"
        }));
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let maturity = events.last().expect("maturity");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.time, t("2013-11-01T00:00:00"));
        assert_eq!(maturity.payoff, Decimal::from(500));
        assert_eq!(maturity.state.notional_principal, Decimal::ZERO);
    }

    #[test]
    fn missing_prnxt_defaults_to_notional_over_schedule_elements() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        }));
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.payoff, Decimal::from(500));
        assert_eq!(
            pr.state.next_principal_redemption_payment,
            Decimal::from(500)
        );
    }

    #[test]
    fn scaling_fixing_rescales_redemption_and_interest_payoffs() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-08-01T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-06-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "100",
            "cycleAnchorDateOfInterestPayment": "2013-06-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT",
            "scalingEffect": "INO",
            "marketObjectCodeOfScalingIndex": "USA.CPI",
            "scalingIndexAtContractDealDate": "100",
            "cycleAnchorDateOfScalingIndex": "2013-05-01T00:00:00",
            "cycleOfScalingIndex": "P2ML1"
        }));
        let observed =
            StateProvider::new().with_index("USA.CPI", t("2013-05-01T00:00:00"), dec!(300));
        let events = LamEngine.evaluate(&terms, &observed).expect("events");
        let sc = events
            .iter()
            .find(|e| e.event_type == EventType::ScalingIndexFixing)
            .expect("first SC");
        assert_eq!(sc.time, t("2013-05-01T00:00:00"));
        assert_eq!(sc.payoff, Decimal::ZERO);
        assert_eq!(sc.state.notional_scaling_multiplier, Decimal::from(3));
        assert_eq!(sc.state.interest_scaling_multiplier, Decimal::from(3));
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.time, t("2013-06-01T00:00:00"));
        assert_eq!(pr.payoff, Decimal::from(300));
        assert_eq!(pr.state.notional_principal, Decimal::from(900));
        let ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("first IP");
        assert_close(ip.payoff, dec!(99.2876712328767123287671233));
    }

    #[test]
    fn ntl_base_stays_fixed_until_the_ipcb_schedule_refixes_it() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-09-15T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML1",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NTL",
            "interestCalculationBaseAmount": "6000",
            "cycleAnchorDateOfInterestCalculationBase": "2013-05-01T00:00:00",
            "cycleOfInterestCalculationBase": "P2ML1"
        }));
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let first_pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_close(first_pr.state.accrued_interest, dec!(40.7671232876712));
        let ipcb = events
            .iter()
            .find(|e| e.event_type == EventType::InterestCalculationBaseFixing)
            .expect("first IPCB");
        assert_eq!(ipcb.time, t("2013-05-01T00:00:00"));
        assert_eq!(ipcb.payoff, Decimal::ZERO);
        assert_eq!(
            ipcb.state.interest_calculation_base_amount,
            Decimal::from(3000)
        );
        let refixed_pr = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .nth(4)
            .expect("PR at 2013-06-01");
        assert_eq!(refixed_pr.time, t("2013-06-01T00:00:00"));
        assert_close(refixed_pr.state.accrued_interest, dec!(20.383561643835616));
    }

    #[test]
    fn ntied_base_tracks_the_notional_at_principal_redemptions() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-15T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NTIED",
            "interestCalculationBaseAmount": "5000"
        }));
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let second_pr = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .nth(1)
            .expect("PR at 2013-03-01");
        assert_eq!(second_pr.time, t("2013-03-01T00:00:00"));
        assert_close(second_pr.state.accrued_interest, dec!(27.6164383561644));
    }

    #[test]
    fn next_reset_rate_becomes_a_fixed_reset_before_market_resets() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
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
            "rateMultiplier": "1",
            "rateSpread": "0.1",
            "marketObjectCodeOfRateReset": "USD.SWP"
        }));
        let observed = StateProvider::new()
            .with_rate("USD.SWP", t("2013-07-01T00:00:00"), dec!(0.000892839506173))
            .with_rate("USD.SWP", t("2013-10-01T00:00:00"), dec!(0.000981234567901));
        let events = LamEngine.evaluate(&terms, &observed).expect("events");
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
            dec!(0.100892839506173),
        );
    }

    #[test]
    fn purchase_applies_but_suppresses_the_preceding_same_day_events() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.1",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML1",
            "nextPrincipalRedemptionPayment": "500",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "purchaseDate": "2013-06-01T00:00:00",
            "priceAtPurchaseDate": "-5100",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        }));
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let prd = events.first().expect("purchase");
        assert_eq!(prd.event_type, EventType::Purchase);
        assert_eq!(prd.time, t("2013-06-01T00:00:00"));
        assert_eq!(prd.payoff, Decimal::from(5100));
        assert_eq!(prd.state.notional_principal, Decimal::from(2500));
        assert_eq!(prd.state.accrued_interest, Decimal::ZERO);
        assert!(!events.iter().any(|e| {
            (e.event_type == EventType::PrincipalRedemption
                || e.event_type == EventType::InterestPayment)
                && e.time == t("2013-06-01T00:00:00")
        }));
    }

    #[test]
    fn real_position_long_flips_the_notional_and_payoffs() {
        let terms = terms(json!({
            "contractType": "LAM",
            "contractRole": "RPL",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
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
        let events = LamEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let ied = events.first().expect("IED");
        assert_eq!(ied.payoff, Decimal::from(5000));
        assert_eq!(ied.state.notional_principal, Decimal::from(-5000));
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.payoff, Decimal::from(-500));
        assert_eq!(pr.state.notional_principal, Decimal::from(-4500));
        let maturity = events.last().expect("MD");
        assert_eq!(maturity.payoff, Decimal::from(-500));
    }
}
