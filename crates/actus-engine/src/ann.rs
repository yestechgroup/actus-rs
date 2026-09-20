//! ANN: Annuity (ACTUS techspec section "ANN: Annuity").
//!
//! An ANN contract pays a constant total periodic amount: every `PR` event
//! redeems the principal as the residual `PRNXT - IPAC` of the period
//! payment (the NAM payoff, techspec `pof/stf PR NAM`) while the same-date
//! `IP` event pays the interest portion out of the same amount. When the
//! `PRNXT` attribute is absent the annuity amount is derived from the
//! contract itself (the techspec states table leaves the `Prnxt(t0)` formula
//! as a `todo`; the closed form below is reverse-engineered from the
//! official testbed and validated on all 20 PRNXT-less fixtures). The
//! annuity is recalculated by the principal fixing events `PRF` that the
//! reference schedules one day before the first redemption and after every
//! rate reset, so variable-rate contracts re-dimension their payment at
//! each `RR` (ann15, ann16, ann24, ann25).
//!
//! The amortization date attribute `AD` bounds the annuity: the payment
//! stream is dimensioned to fully amortize the notional at `AD`, while the
//! contract events are cut at maturity `MD` — with both attributes present
//! and `MD < AD` the contract matures early paying the remaining notional
//! as a balloon (ann12). With `MD` absent, `AD` is the maturity.
//!
//! Conventions implemented here, resolved against the official testbed:
//!
//! - The annuity amount at a recalculation time `s` (techspec `A(s, T, n, a,
//!   r)` section "Annuity Amount Function", with the accrued-interest term
//!   resolved to zero because the interest portion is paid separately by the
//!   same-date `IP` event):
//!   `PRNXT = NT(s) / sum_i Q(u_i)^-1`, where the payment times `u_1..u_m`
//!   are the remaining PR schedule elements after `s` (the schedule
//!   `S(PRANX, PRCL, T*)` with `T* = AD` when present, else `MD`),
//!   `Q(u_i) = prod_{j<i} (1 + r x Y(u_j, u_{j+1}))` accumulates the
//!   per-period simple-interest growth, and the chain starts one full cycle
//!   before `u_1` (floored at `IED`), so the first payment is discounted by
//!   exactly one period of growth (ann01: 434.866594118346; ann09: the
//!   zero-length first period leaves the leading weight at 1,
//!   400.071084163282). The rate is the one in effect at `s` — after the
//!   same-date `RR`/`RRF` — applied flat to all remaining periods: future
//!   resets are not anticipated (the ann15 fixing at 2013-02-28 fixes
//!   472.772962 at 8% although the April reset is already scheduled).
//! - Maturity inference (techspec ANN states table `Md`, "Same as NAM"):
//!   with `MD` and `AD` absent, `n = ceil(NT / PRNXT)` redemption periods
//!   amortize the notional and maturity is the shifted `n`-th element of
//!   the redemption schedule (ann11: 2013-09-01, ann28: 2013-09-02, the
//!   `SCF` shift of the Saturday roll). The element count form is shared
//!   with LAM; the NAM net-redemption count is indistinguishable on the
//!   fixtures but the shifted emission date requires the redemption
//!   schedule's own times.
//! - The `PRF` schedule exists only when `PRNXT` is absent: one fixing one
//!   day before the first redemption emission (ann23: 2013-01-31) plus one
//!   at every rate reset date, evaluated after the reset (ann15). A fixing
//!   before `IED` mutates the state but is not reported (ann09: the
//!   2012-12-31 fixing); it resolves `PRNXT` from the attribute notional
//!   and rate because the exchange has not happened yet. A fixing before
//!   `PRD` is suppressed by the purchase window like any earlier event
//!   (ann18). `PRF` pays zero and accrues interest like any other event.
//! - Same-timestamp ordering places the fixing after the rate reset (see
//!   [`crate::event::sequence_rank`]), so the recalculated payment uses the
//!   rate the reset has just set.
//! - The `PR` payoff/state transition, `IED`, `IP`, `IPCI`, `IPCB`, `RR`,
//!   `RRF`, `PRD`, `TD` and `MD` rows follow the NAM/LAM machinery; the `PR`
//!   row is extracted to [`crate::common::apply_principal_redemption`].
//!   Schedule elements at maturity belong to `MD` and are not emitted
//!   separately (`PR`, `RR`); the `IP` series keeps its maturity element
//!   and pays the final accrual before `MD` (ann01, ann14).

use std::collections::BTreeSet;

use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

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
use crate::event::{sequence_rank, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::schedule::{cycle_step, generate_schedule, shift_business_day};
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the ANN contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnnEngine;

/// Schedule entry of the ANN skeleton: one event slot with its calculation
/// time (accrual anchor input) and its emission time (event time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

/// ANN event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    PrincipalFixing,
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
            Kind::PrincipalFixing => EventType::PrincipalPaymentAmountFixing,
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

    /// The effective dictionary event sequence number driving same-timestamp
    /// order.
    fn rank(self) -> u8 {
        sequence_rank(self.event_type())
    }
}

impl ContractEngine for AnnEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Ann
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
            if !reported || !ctx.observed(slot.emit, slot.kind.rank(), terms) {
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

/// Resolved attributes and conventions of one ANN evaluation.
struct Context {
    t0: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
    dcc: DayCountConvention,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
    sgn: Decimal,
    next_principal_redemption: Option<Decimal>,
    base_tracks_notional: bool,
    redemption_times: Vec<NaiveDateTime>,
}

impl Context {
    /// Resolves the attributes every ANN evaluation depends on, including
    /// the maturity (attribute, amortization date, or inferred from the
    /// redemption schedule) and the annuity schedule the payment stream is
    /// dimensioned over (techspec ANN states table, `Prnxt`).
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
            None => match terms.amortization_date {
                Some(amortization_date) if amortization_date > ied => amortization_date,
                _ => inferred_maturity(terms, notional, pranx, prcl, bdc, cal, eomc)?,
            },
        };
        let amortization_end = match terms.amortization_date {
            Some(amortization_date) if amortization_date > ied => amortization_date,
            _ => maturity,
        };
        let redemption_times = generate_schedule(
            pranx,
            prcl,
            amortization_end,
            eomc,
            BusinessDayConvention::Nos,
            cal,
        );
        if redemption_times.is_empty() {
            return Err(EngineError::InvalidTransition(
                "principal redemption schedule is empty".to_string(),
            ));
        }
        let next_principal_redemption = terms.next_principal_redemption_payment;
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
            redemption_times,
        })
    }

    /// Whether an event at `t` is observed: the analysis window starts at the
    /// status date (progressed contracts) and at the purchase date when
    /// `PRD` is set, and ends at termination when `TD` is set. At the
    /// purchase date itself the events preceding `PRD` in the dictionary
    /// sequence mutate the state but are not reported.
    fn observed(&self, t: NaiveDateTime, rank: u8, terms: &ContractTerms) -> bool {
        if t < self.t0 {
            return false;
        }
        if let Some(prd) = terms.purchase_date {
            if t < prd || (t == prd && rank < EventType::Purchase.priority()) {
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

    /// Builds the ANN event skeleton in deterministic (calculation time,
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
        let first_redemption = self.principal_redemption_slots(terms, &mut slots);
        let rate_resets = if terms.nominal_interest_rate.is_some() {
            self.interest_slots(terms, &mut slots);
            self.rate_reset_slots(terms, &mut slots)
        } else {
            Vec::new()
        };
        self.principal_fixing_slots(terms, first_redemption, &rate_resets, &mut slots);
        self.interest_calculation_base_slots(terms, &mut slots);
        slots.sort_by_key(|slot| (slot.calc, slot.kind.rank()));
        slots
    }

    /// The `PR` principal redemption series (techspec ANN schedule row PR,
    /// "Same as LAM"): `S(PRANX, PRCL, MD)` without the maturity element,
    /// which the maturity event supersedes. Returns the emission time of the
    /// first redemption for the initial fixing placement.
    fn principal_redemption_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
    ) -> Option<NaiveDateTime> {
        let anchor = terms
            .cycle_anchor_date_of_principal_redemption
            .map(normalize_timestamp)?;
        let cycle = terms.cycle_of_principal_redemption.as_ref()?;
        let mut first = None;
        for (calc, emit) in stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)
        {
            if calc == self.maturity {
                continue;
            }
            if first.is_none() {
                first = Some(emit);
            }
            slots.push(Slot {
                kind: Kind::PrincipalRedemption,
                calc,
                emit,
            });
        }
        first
    }

    /// The `IPCI` and `IP` series (techspec ANN schedule rows IPCI and IP).
    ///
    /// Both series unroll from the interest cycle anchor `IPANX`; the `IPCI`
    /// series runs to the capitalization end date `IPCED` (the capitalization
    /// end always belongs to the schedule, even when it precedes the
    /// interest anchor: ann14 capitalizes at `IPCED` alone), and the `IP`
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

    /// The `RR`/`RRF` rate reset series (techspec ANN schedule row RR,
    /// "Same as PAM").
    ///
    /// The series unrolls from `RRANX` to maturity without the maturity
    /// element; with `RRNXT` set, the first schedule point after the status
    /// date becomes a fixed reset `RRF` applying `RRNXT` instead of a market
    /// observation (ann16). Returns the reset slots for the fixing series.
    fn rate_reset_slots(&self, terms: &ContractTerms, slots: &mut Vec<Slot>) -> Vec<Slot> {
        let anchor = match terms
            .cycle_anchor_date_of_rate_reset
            .map(normalize_timestamp)
        {
            Some(anchor) => anchor,
            None => return Vec::new(),
        };
        let cycle = match terms.cycle_of_rate_reset.as_ref() {
            Some(cycle) => cycle,
            None => return Vec::new(),
        };
        let mut series = stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal);
        if series.last().map(|(_, emit)| *emit) == Some(self.maturity) {
            series.pop();
        }
        let mut resets = Vec::new();
        let mut fixed_pending = terms.next_reset_rate.is_some();
        for (calc, emit) in series {
            let kind = if fixed_pending && calc > self.t0 {
                fixed_pending = false;
                Kind::RateResetFixed
            } else {
                Kind::RateReset
            };
            let slot = Slot { kind, calc, emit };
            slots.push(slot);
            resets.push(slot);
        }
        resets
    }

    /// The `PRF` principal fixing series, present only when the `PRNXT`
    /// attribute is absent: one fixing one day before the first redemption
    /// emission and one fixing at every rate reset (evaluated after the
    /// reset by the sequence rank, ann15).
    fn principal_fixing_slots(
        &self,
        terms: &ContractTerms,
        first_redemption: Option<NaiveDateTime>,
        rate_resets: &[Slot],
        slots: &mut Vec<Slot>,
    ) {
        if terms.next_principal_redemption_payment.is_some() {
            return;
        }
        if let Some(first) = first_redemption {
            let fixing = first - chrono::Duration::days(1);
            slots.push(Slot {
                kind: Kind::PrincipalFixing,
                calc: fixing,
                emit: fixing,
            });
        }
        for reset in rate_resets {
            slots.push(Slot {
                kind: Kind::PrincipalFixing,
                calc: reset.calc,
                emit: reset.emit,
            });
        }
    }

    /// The `IPCB` interest calculation base fixing series (techspec ANN
    /// schedule row IPCB, "Same as LAM"): defined only for `IPCB = NTL`,
    /// unrolled from `IPCBANX` to maturity without the maturity element.
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

    /// Recalculates the next principal redemption payment (techspec ANN
    /// functions table, `Prnxt(t+) = A(...)` rows).
    ///
    /// Before the initial exchange the attribute notional and accrued are
    /// used without the accrual growth (the annuity is fixed from the
    /// attribute values while the contract has not started yet, ann09);
    /// afterwards the live signed state drives the formula and the accrued
    /// interest is grown to the next redemption time. The result is
    /// role-signed.
    fn recalculate_annuity(
        &self,
        state: &ContractState,
        terms: &ContractTerms,
        at: NaiveDateTime,
    ) -> Result<Decimal, EngineError> {
        if at < self.ied {
            let notional = terms
                .notional_principal
                .ok_or(EngineError::MissingAttribute("notionalPrincipal"))?;
            let accrued = terms.accrued_interest.unwrap_or(Decimal::ZERO);
            let rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
            self.annuity_payment(notional, accrued, rate, at, false)
                .map(|amount| self.sgn * amount)
        } else {
            self.annuity_payment(
                state.notional_principal,
                state.accrued_interest,
                state.nominal_interest_rate,
                at,
                true,
            )
            .map(|amount| self.sgn * amount)
        }
    }

    /// The annuity payment dimensioned over the remaining redemption
    /// schedule (techspec "Annuity Amount Function" `A(s, T, n, a, r)`,
    /// implemented as the reference `AnnuityUtils.annuityPayment`).
    ///
    /// The remaining schedule carries the raw (unshifted) redemption times
    /// strictly after the fixing time `at`, maturity element included. With
    /// `grow_accrued` the accrued interest is first grown to the next
    /// redemption time at the live rate. The payment is the scale
    /// `|NT + Ipac|` divided by the annuity weight sum
    /// `1 + Σ_{k=1..m-1} 1/G_k` over the payment-period growth factors
    /// `G_k = Π_{j=1..k}(1 + rate x Y(t_{j-1}, t_j))`. The inputs are the
    /// signed state (or attribute) values, the result is unsigned.
    fn annuity_payment(
        &self,
        notional: Decimal,
        accrued: Decimal,
        rate: Decimal,
        at: NaiveDateTime,
        grow_accrued: bool,
    ) -> Result<Decimal, EngineError> {
        let times: Vec<NaiveDateTime> = self
            .redemption_times
            .iter()
            .copied()
            .filter(|time| *time > at)
            .collect();
        let next = times
            .first()
            .copied()
            .ok_or(EngineError::InvalidTransition(
                "annuity recalculation has no remaining redemption payments".to_string(),
            ))?;
        let grown = if grow_accrued {
            let yf = day_count_fraction(at, next, self.dcc)?;
            notional * rate * yf
        } else {
            Decimal::ZERO
        };
        let scale = (notional + accrued + grown).abs();
        let mut weights = Decimal::ONE;
        let mut growth = Decimal::ONE;
        for pair in times.windows(2) {
            let yf = day_count_fraction(pair[0], pair[1], self.dcc)?;
            growth *= Decimal::ONE + rate * yf;
            weights += Decimal::ONE / growth;
        }
        Ok(scale / weights)
    }

    /// Applies the state transition and payoff function of one slot
    /// (techspec ANN functions table) and returns the payoff. Every
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
                if let Some(prnxt) = self.next_principal_redemption {
                    state.next_principal_redemption_payment = self.sgn * prnxt;
                }
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
            Kind::PrincipalFixing => {
                state.accrued_interest += accrual;
                state.next_principal_redemption_payment =
                    self.recalculate_annuity(state, terms, slot.calc)?;
                Decimal::ZERO
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

/// The annuity amount dimensioned to fully redeem `notional` over the
/// remaining payment times (techspec section "Annuity Amount Function",
/// resolved against the official testbed).
///
/// Each period grows the outstanding balance by the simple-interest factor
/// `1 + r x Y(t_{i-1}, t_i)`; the annuity is the amount whose discounted
/// value under the same growth factors repays exactly `notional`, including
/// the residual payment at the schedule end:
///
/// ```text
/// A = NT / sum_i 1/Q(u_i),  Q(u_i) = prod_{j<=i} (1 + r x Y(u_{j-1}, u_j))
/// ```
///
/// with `u_0 = start` one full cycle before the first remaining payment.
/// A zero-length first period (start equal to the first payment) leaves the
/// leading weight at exactly 1 (ann09).
///
/// Public so the property suite can test the pure function against the
/// closed-form equal-period annuity
/// `A = NT x i / (1 - (1 + i)^-m)` with `i = r x Y` per period.
pub fn annuity_amount(
    notional: Decimal,
    rate: Decimal,
    start: NaiveDateTime,
    payments: &[NaiveDateTime],
    dcc: DayCountConvention,
) -> Result<Decimal, EngineError> {
    let mut growth = Decimal::ONE;
    let mut weights = Decimal::ZERO;
    let mut previous = start;
    for payment in payments {
        let yf = day_count_fraction(previous, *payment, dcc)?;
        growth *= Decimal::ONE + rate * yf;
        weights += Decimal::ONE / growth;
        previous = *payment;
    }
    if weights == Decimal::ZERO {
        return Err(EngineError::InvalidTransition(
            "annuity payment schedule carries no discount weights".to_string(),
        ));
    }
    Ok(notional / weights)
}

/// The maturity inferred from the redemption schedule when the `MD` and `AD`
/// attributes are absent (techspec ANN states table, `Md`).
///
/// `n = ceil(NT / PRNXT)` redemption periods amortize the notional and the
/// maturity is the `n`-th element of the redemption schedule under the
/// business day convention (ann11: 2013-09-01; ann28: the `SCF` shift of
/// the Saturday roll to 2013-09-02).
fn inferred_maturity(
    terms: &ContractTerms,
    notional: Decimal,
    pranx: NaiveDateTime,
    prcl: &actus_model::Cycle,
    bdc: BusinessDayConvention,
    cal: Calendar,
    eomc: EndOfMonthConvention,
) -> Result<NaiveDateTime, EngineError> {
    let prnxt = terms
        .next_principal_redemption_payment
        .ok_or(EngineError::MissingAttribute(
            "nextPrincipalRedemptionPayment",
        ))?;
    let redemptions = (notional / prnxt)
        .ceil()
        .to_u64()
        .ok_or(EngineError::InvalidTransition(
            "notional principal is not redeemable by the next principal redemption payment"
                .to_string(),
        ))?;
    let steps = redemptions.saturating_sub(1);
    let unshifted = if steps == 0 {
        pranx
    } else {
        cycle_step(pranx, steps, prcl, eomc)
    };
    Ok(shift_business_day(unshifted, bdc, cal))
}

/// The accrued interest state at `IED` (techspec ANN functions table, IED
/// row, "Same as LAM"): the role-signed terms value when present, otherwise
/// the accrual since `IPANX` on the signed notional when the anchor precedes
/// the initial exchange, otherwise zero.
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

/// Builds the pre-initial-exchange-date state (techspec ANN states table,
/// `Ipcb` row: `t0 < IED` yields a zeroed calculation base): the annuity
/// amount (role signed) and scaling multipliers are resolved from the terms
/// or the annuity formula, the monetary and rate states stay zero until
/// `IED`.
fn initial_state(ctx: &Context, terms: &ContractTerms) -> ContractState {
    let mut state = ContractState::initial(terms);
    state.next_principal_redemption_payment = initial_annuity(ctx, terms);
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state
}

/// Builds the states-at-t0 of a progressed contract whose `IED` precedes the
/// status date (techspec ANN states table): the notional, rate, annuity
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
    state.next_principal_redemption_payment = initial_annuity(ctx, terms);
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

/// The role-signed annuity amount at `t0` (techspec ANN states table,
/// `Prnxt`): the `PRNXT` attribute when present, otherwise the annuity
/// formula over the redemption schedule remaining at the status date with
/// the attribute notional, accrued interest and rate, without the accrual
/// growth (the attribute-value convention of the pre-initial-exchange
/// fixing).
fn initial_annuity(ctx: &Context, terms: &ContractTerms) -> Decimal {
    if let Some(prnxt) = ctx.next_principal_redemption {
        return ctx.sgn * prnxt;
    }
    let notional = terms.notional_principal.unwrap_or(Decimal::ZERO);
    let accrued = terms.accrued_interest.unwrap_or(Decimal::ZERO);
    let rate = terms.nominal_interest_rate.unwrap_or(Decimal::ZERO);
    ctx.annuity_payment(notional, accrued, rate, ctx.t0, false)
        .map(|amount| ctx.sgn * amount)
        .unwrap_or(Decimal::ZERO)
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
        let tolerance = dec!(0.000000001);
        assert!(
            (actual - expected).abs() < tolerance,
            "actual {actual} vs expected {expected}"
        );
    }

    fn ann15_terms() -> ContractTerms {
        terms(json!({
            "contractType": "ANN",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-03-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "amortizationDate": "2014-01-01T00:00:00",
            "cycleAnchorDateOfRateReset": "2013-04-01T00:00:00",
            "cycleOfRateReset": "P3ML1",
            "rateSpread": "0.1",
            "marketObjectCodeOfRateReset": "USD.SWP",
            "dayCountConvention": "A365"
        }))
    }

    fn ann15_risk() -> StateProvider {
        StateProvider::new()
            .with_rate(
                "USD.SWP",
                t("2013-04-01T00:00:00"),
                dec!(0.010567901234567901),
            )
            .with_rate(
                "USD.SWP",
                t("2013-07-01T00:00:00"),
                dec!(0.011679012345679012),
            )
            .with_rate(
                "USD.SWP",
                t("2013-10-01T00:00:00"),
                dec!(0.012790123456790127),
            )
    }

    #[test]
    fn annuity_amount_matches_the_hand_computed_ann01_payment() {
        let amount = annuity_amount(
            dec!(5000),
            dec!(0.08),
            t("2013-01-01T00:00:00"),
            &[
                t("2013-02-01T00:00:00"),
                t("2013-03-01T00:00:00"),
                t("2013-04-01T00:00:00"),
                t("2013-05-01T00:00:00"),
                t("2013-06-01T00:00:00"),
                t("2013-07-01T00:00:00"),
                t("2013-08-01T00:00:00"),
                t("2013-09-01T00:00:00"),
                t("2013-10-01T00:00:00"),
                t("2013-11-01T00:00:00"),
                t("2013-12-01T00:00:00"),
                t("2014-01-01T00:00:00"),
            ],
            DayCountConvention::A365,
        )
        .expect("annuity");
        assert_close(amount, dec!(434.866594118346));
    }

    #[test]
    fn zero_length_first_period_leaves_the_leading_weight_at_one() {
        let amount = annuity_amount(
            dec!(5000),
            dec!(0.08),
            t("2013-01-01T00:00:00"),
            &[
                t("2013-01-01T00:00:00"),
                t("2013-02-01T00:00:00"),
                t("2013-03-01T00:00:00"),
                t("2013-04-01T00:00:00"),
                t("2013-05-01T00:00:00"),
                t("2013-06-01T00:00:00"),
                t("2013-07-01T00:00:00"),
                t("2013-08-01T00:00:00"),
                t("2013-09-01T00:00:00"),
                t("2013-10-01T00:00:00"),
                t("2013-11-01T00:00:00"),
                t("2013-12-01T00:00:00"),
                t("2014-01-01T00:00:00"),
            ],
            DayCountConvention::A365,
        )
        .expect("annuity");
        assert_close(amount, dec!(400.071084163282));
    }

    #[test]
    fn fixing_before_first_redemption_sets_the_annuity_from_the_terms() {
        let events = AnnEngine
            .evaluate(&ann15_terms(), &ann15_risk())
            .expect("events");
        let fixing = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalPaymentAmountFixing)
            .expect("initial PRF");
        assert_eq!(fixing.time, t("2013-02-28T00:00:00"));
        assert_eq!(fixing.payoff, Decimal::ZERO);
        assert_close(fixing.state.accrued_interest, dec!(29.58904109589041));
        assert_close(
            fixing.state.next_principal_redemption_payment,
            dec!(472.772962074754183),
        );
        let redemption = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(redemption.time, t("2013-03-01T00:00:00"));
        assert_close(redemption.payoff, dec!(442.08803056790487));
        assert_close(redemption.state.accrued_interest, dec!(30.684931506849313));
    }

    #[test]
    fn rate_reset_redimensions_the_annuity_from_the_reset_rate() {
        let events = AnnEngine
            .evaluate(&ann15_terms(), &ann15_risk())
            .expect("events");
        let reset = events
            .iter()
            .find(|e| e.event_type == EventType::RateResetVariable)
            .expect("first RR");
        assert_eq!(reset.time, t("2013-04-01T00:00:00"));
        assert_close(reset.state.nominal_interest_rate, dec!(0.1105679012345679));
        let fixing = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalPaymentAmountFixing)
            .nth(1)
            .expect("PRF after the first RR");
        assert_eq!(fixing.time, t("2013-04-01T00:00:00"));
        assert_close(
            fixing.state.next_principal_redemption_payment,
            dec!(478.73903628688925),
        );
        let redemption = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .nth(2)
            .expect("PR after the recalculation");
        assert_eq!(redemption.time, t("2013-05-01T00:00:00"));
        assert_close(redemption.payoff, dec!(441.3327838664801));
    }

    #[test]
    fn amortization_date_bounds_the_annuity_while_maturity_cuts_the_events() {
        let terms = terms(json!({
            "contractType": "ANN",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-15T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML1",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "amortizationDate": "2014-01-01T00:00:00",
            "dayCountConvention": "A365"
        }));
        let events = AnnEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let redemptions: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .collect();
        assert_eq!(redemptions.len(), 10);
        assert_eq!(
            redemptions.last().expect("last PR").time,
            t("2013-11-01T00:00:00")
        );
        assert_close(
            redemptions[0].state.next_principal_redemption_payment,
            dec!(434.866594118346),
        );
        let maturity = events.last().expect("balloon maturity");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.time, t("2013-11-15T00:00:00"));
        assert_close(maturity.payoff, dec!(861.136153461425));
    }

    #[test]
    fn fixing_before_the_initial_exchange_fixes_from_the_attributes() {
        let terms = terms(json!({
            "contractType": "ANN",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-01-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
            "cycleOfInterestPayment": "P1ML1",
            "amortizationDate": "2014-01-01T00:00:00",
            "dayCountConvention": "A365"
        }));
        let events = AnnEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        assert!(!events
            .iter()
            .any(|e| e.event_type == EventType::PrincipalPaymentAmountFixing));
        let redemption = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("PR at IED");
        assert_eq!(redemption.time, t("2013-01-01T00:00:00"));
        assert_close(redemption.payoff, dec!(400.071084163282));
    }

    #[test]
    fn fixing_pays_zero_and_accrues_interest() {
        let terms = terms(json!({
            "contractType": "ANN",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2012-12-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "accruedInterest": "0",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "amortizationDate": "2014-01-01T00:00:00",
            "dayCountConvention": "A365"
        }));
        let events = AnnEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let fixing = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalPaymentAmountFixing)
            .expect("PRF");
        assert_eq!(fixing.time, t("2013-01-31T00:00:00"));
        assert_eq!(fixing.payoff, Decimal::ZERO);
        assert_eq!(fixing.state.notional_principal, dec!(5000));
        assert_close(fixing.state.accrued_interest, dec!(32.8767123287));
        assert_close(
            fixing.state.next_principal_redemption_payment,
            dec!(434.866594118346),
        );
    }

    #[test]
    fn maturity_is_inferred_and_shifted_to_a_business_day() {
        let terms = terms(json!({
            "contractType": "ANN",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "nextPrincipalRedemptionPayment": "700",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "30E360",
            "calendar": "MF",
            "businessDayConvention": "SCF",
            "endOfMonthConvention": "EOM"
        }));
        let events = AnnEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let redemptions: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .collect();
        assert_eq!(redemptions.len(), 7);
        let maturity = events.last().expect("maturity");
        assert_eq!(maturity.event_type, EventType::Maturity);
        assert_eq!(maturity.time, t("2013-09-02T00:00:00"));
        assert_close(maturity.payoff, dec!(239.2687482002));
        let interest = events
            .iter()
            .rev()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("final IP");
        assert_eq!(interest.time, t("2013-09-02T00:00:00"));
        assert_close(interest.payoff, dec!(1.6482958209));
    }

    #[test]
    fn role_mirror_flips_the_annuity_and_redemptions() {
        let terms = terms(json!({
            "contractType": "ANN",
            "contractRole": "RPL",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "cycleAnchorDateOfPrincipalRedemption": "2013-02-01T00:00:00",
            "cycleOfPrincipalRedemption": "P1ML0",
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "amortizationDate": "2014-01-01T00:00:00",
            "dayCountConvention": "A365"
        }));
        let events = AnnEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let ied = events.first().expect("IED");
        assert_eq!(ied.payoff, Decimal::from(5000));
        assert_eq!(ied.state.notional_principal, Decimal::from(-5000));
        let fixing = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalPaymentAmountFixing)
            .expect("PRF");
        assert_close(
            fixing.state.next_principal_redemption_payment,
            dec!(-434.866594118346),
        );
        let redemption = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_close(redemption.payoff, dec!(-400.8939913786));
    }
}
