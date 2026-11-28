//! LAX: Exotic Linear Amortizer (paper §7.3 "LAX: Exotic Linear Amortizer"
//! and the array schedule machinery of paper §3.2 "Array Schedule").
//!
//! A LAX contract is a LAM whose principal redemption, interest payment and
//! rate reset schedules may be array schedules: vector-valued anchors and
//! cycles define consecutive schedule segments, each unrolled with the
//! regular cycle machinery (paper §3.2: `S̃(s̃, c̃, T) = (S(s_0, c_0, s_1 -
//! c_0), S(s_1, c_1, s_2 - c_1), ..., S(s_m, c_m, T))`). Each segment
//! carries its own economics: the next principal redemption payment
//! `ARPRNXT[i]` (dictionary `arrayNextPrincipalRedemptionPayment`, falling
//! back to `PRNXT` and to the notional-over-elements default), and the
//! increase/decrease direction `ARINCDEC[i]` (dictionary
//! `arrayIncreaseDecrease`, defaulting to `DEC`).
//!
//! Segment semantics (paper §7.3, dictionary attribute descriptions):
//!
//! - `DEC` segments redeem linearly like LAM: the redemption reduces the
//!   notional, is capped at the remaining notional (the lam25 convention),
//!   and the role-signed payoff is `sgn x Nsc x redemption`.
//! - `INC` segments amortize negatively like NAM: the principal portion of
//!   the period payment is `Prnxt - Ipac`, so when the period accrual
//!   exceeds the scheduled payment the shortfall capitalizes into the
//!   notional (the nam17 mechanics carried over the role-sign conventions
//!   of the LAM state machine).
//!
//! The `IP` interest series runs on the array interest schedule
//! (`ARIPANX`/`ARIPCL`) when present, otherwise on the scalar `IPANX`/`IPCL`
//! schedule with the `IPCI` capitalization series exactly as in LAM, and
//! otherwise on the principal redemption schedule dates plus maturity (the
//! paper §7.4 alignment of interest with principal redemption). The `RR`
//! rate reset series runs on the array rate schedule (`ARRRANX`/`ARRRCL`)
//! when present, resolving `ARRATE[i]` per `ARFIXVAR` — `F` sets the fixed
//! nominal rate, `V` applies a market observation with the segment spread —
//! and otherwise on the scalar `RRANX`/`RRCL` schedule with the `RRF`
//! fixed-reset machinery of LAM. Everything else (IED, MD and its
//! inference, the IPCB and SC series, PRD/TD handling, same-timestamp
//! sequencing, progressed contracts) mirrors the LAM implementation.

use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use actus_model::enums::{ArrayFixVar, ArrayIncDec};
use actus_model::{
    BusinessDayConvention, Calendar, ContractTerms, ContractType, Cycle, DayCountConvention,
    EndOfMonthConvention, EventType,
};

use crate::common::{
    base_is_nt, base_tracks_notional, calculation_time, initial_base_amount, role_sign,
    stub_series, term_scaling_multipliers,
};
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::ContractEngine;
use crate::event::ContractEvent;
use crate::fixed_income::{
    self, interest_calculation_base_schedule, interest_schedule, rate_reset_schedule,
    scales_interest, scales_notional, scaling_index_schedule, Conventions, RateReset,
    ScheduledEvent, Timing,
};
use crate::risk::RiskFactorProvider;
use crate::schedule::{array_series, cycle_step};
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the LAX contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct LaxEngine;

/// Schedule entry of the LAX skeleton: one event slot with its calculation
/// time (accrual anchor input), its emission time (event time) and the
/// schedule segment it belongs to (`0` for slots outside an array schedule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
    segment: usize,
}

impl ScheduledEvent for Slot {
    fn event_type(&self) -> EventType {
        self.kind.event_type()
    }

    fn sequence(&self) -> u8 {
        self.kind.priority()
    }

    fn emit(&self) -> NaiveDateTime {
        self.emit
    }
}

impl Slot {
    fn at(kind: Kind, calc: NaiveDateTime, emit: NaiveDateTime) -> Slot {
        Slot {
            kind,
            calc,
            emit,
            segment: 0,
        }
    }

    /// A slot of `kind` at the calculation and emission times of `timing`.
    fn timed(kind: Kind, timing: Timing) -> Slot {
        Slot::at(kind, timing.calc, timing.emit)
    }

    fn in_segment(kind: Kind, calc: NaiveDateTime, emit: NaiveDateTime, segment: usize) -> Slot {
        Slot {
            kind,
            calc,
            emit,
            segment,
        }
    }
}

/// LAX event kinds, mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    InterestPayment,
    InterestCapitalization,
    InterestCalculationBaseFixing,
    RateReset,
    ArrayRateReset,
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
            Kind::RateReset | Kind::ArrayRateReset => EventType::RateResetVariable,
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

/// Maps a shared rate reset element onto this engine's event kind.
fn rate_reset_kind(reset: RateReset) -> Kind {
    match reset {
        RateReset::Fixed => Kind::RateResetFixed,
        RateReset::Variable => Kind::RateReset,
    }
}

impl ContractEngine for LaxEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Lax
    }

    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let ctx = Context::new(terms)?;
        fixed_income::evaluate_schedule(
            ctx.ied,
            ctx.t0,
            ctx.schedule(terms)?,
            terms,
            || progressed_initial(&ctx, terms),
            || initial_state(&ctx, terms),
            |state, slot| ctx.apply(state, slot, terms, risk),
        )
    }
}

/// Resolved attributes and conventions of one LAX evaluation.
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
    principal_array: Option<(Vec<NaiveDateTime>, Vec<Cycle>)>,
    interest_array: Option<(Vec<NaiveDateTime>, Vec<Cycle>)>,
    rate_array: Option<(Vec<NaiveDateTime>, Vec<Cycle>)>,
    payments: Option<Vec<Decimal>>,
    increase_decrease: Option<Vec<ArrayIncDec>>,
}

impl Context {
    /// The schedule conventions shared with [`crate::fixed_income`].
    fn conventions(&self) -> Conventions {
        Conventions {
            t0: self.t0,
            maturity: self.maturity,
            dcc: self.dcc,
            eomc: self.eomc,
            bdc: self.bdc,
            cal: self.cal,
        }
    }

    /// Resolves the attributes every LAX evaluation depends on, including
    /// the maturity inference and the default redemption amount (both the
    /// LAM conventions; the scalar `PRANX`/`PRCL` pair is required only when
    /// the array attributes are absent).
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
        let principal_array = match (
            terms
                .array_cycle_anchor_date_of_principal_redemption
                .as_ref()
                .map(|anchors| {
                    anchors
                        .iter()
                        .map(|anchor| normalize_timestamp(*anchor))
                        .collect::<Vec<_>>()
                }),
            terms.array_cycle_of_principal_redemption.as_ref(),
        ) {
            (Some(anchors), Some(cycles)) if !anchors.is_empty() && !cycles.is_empty() => {
                Some((anchors, cycles.clone()))
            }
            _ => None,
        };
        let scalar_anchor = terms
            .cycle_anchor_date_of_principal_redemption
            .map(normalize_timestamp);
        let scalar_cycle = terms.cycle_of_principal_redemption;
        if principal_array.is_none() {
            scalar_anchor.ok_or(EngineError::MissingAttribute(
                "cycleAnchorDateOfPrincipalRedemption",
            ))?;
            scalar_cycle
                .as_ref()
                .ok_or(EngineError::MissingAttribute("cycleOfPrincipalRedemption"))?;
        }
        let maturity = match terms.maturity_date.map(normalize_timestamp) {
            Some(maturity) => maturity,
            None => {
                let prnxt = terms.next_principal_redemption_payment.ok_or(
                    EngineError::MissingAttribute("nextPrincipalRedemptionPayment"),
                )?;
                let anchor = scalar_anchor.ok_or(EngineError::MissingAttribute(
                    "cycleAnchorDateOfPrincipalRedemption",
                ))?;
                let cycle = scalar_cycle
                    .as_ref()
                    .ok_or(EngineError::MissingAttribute("cycleOfPrincipalRedemption"))?;
                let redemptions = (notional / prnxt)
                    .ceil()
                    .to_u64()
                    .ok_or(EngineError::InvalidTransition(
                    "notional principal is not redeemable by the next principal redemption payment"
                        .to_string(),
                ))?;
                let steps = redemptions.saturating_sub(1);
                normalize_timestamp(cycle_step(anchor, steps, cycle, eomc)?)
            }
        };
        let next_principal_redemption = match terms.next_principal_redemption_payment {
            Some(prnxt) => prnxt,
            None => {
                // The LAM default: the notional divided by the number of
                // redemption schedule elements including the maturity
                // element (lam27 to lam31). The array schedule replaces the
                // scalar series in the element count when present.
                let elements = match (&principal_array, &scalar_anchor, &scalar_cycle) {
                    (Some((anchors, cycles)), _, _) => array_series(
                        anchors,
                        cycles,
                        maturity,
                        eomc,
                        BusinessDayConvention::Nos,
                        cal,
                    )?
                    .len(),
                    (_, Some(anchor), Some(cycle)) => {
                        stub_series(*anchor, cycle, maturity, eomc, bdc, cal)?.len()
                    }
                    _ => {
                        return Err(EngineError::MissingAttribute(
                            "nextPrincipalRedemptionPayment",
                        ))
                    }
                };
                if elements == 0 {
                    return Err(EngineError::InvalidTransition(
                        "principal redemption schedule is empty".to_string(),
                    ));
                }
                notional / Decimal::from(elements as u64)
            }
        };
        let interest_array = match (
            terms
                .array_cycle_anchor_date_of_interest_payment
                .as_ref()
                .map(|anchors| {
                    anchors
                        .iter()
                        .map(|anchor| normalize_timestamp(*anchor))
                        .collect::<Vec<_>>()
                }),
            terms.array_cycle_of_interest_payment.as_ref(),
        ) {
            (Some(anchors), Some(cycles)) if !anchors.is_empty() && !cycles.is_empty() => {
                Some((anchors, cycles.clone()))
            }
            _ => None,
        };
        let rate_array = match (
            terms
                .array_cycle_anchor_date_of_rate_reset
                .as_ref()
                .map(|anchors| {
                    anchors
                        .iter()
                        .map(|anchor| normalize_timestamp(*anchor))
                        .collect::<Vec<_>>()
                }),
            terms.array_cycle_of_rate_reset.as_ref(),
        ) {
            (Some(anchors), Some(cycles)) if !anchors.is_empty() && !cycles.is_empty() => {
                Some((anchors, cycles.clone()))
            }
            _ => None,
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
            principal_array,
            interest_array,
            rate_array,
            payments: terms.array_next_principal_redemption_payment.clone(),
            increase_decrease: terms.array_increase_decrease.clone(),
        })
    }

    /// Builds the LAX event skeleton in deterministic (calculation time,
    /// dictionary sequence) order.
    fn schedule(&self, terms: &ContractTerms) -> Result<Vec<Slot>, EngineError> {
        let mut slots = vec![
            Slot::at(Kind::InitialExchange, self.ied, self.ied),
            Slot::at(Kind::Maturity, self.maturity, self.maturity),
        ];
        if let Some(prd) = terms.purchase_date {
            slots.push(Slot::at(Kind::Purchase, prd, prd));
        }
        if let Some(td) = terms.termination_date {
            slots.push(Slot::at(Kind::Termination, td, td));
        }
        let principal_pairs = self.principal_redemption_slots(terms, &mut slots)?;
        if terms.nominal_interest_rate.is_some() {
            self.interest_slots(terms, &principal_pairs, &mut slots)?;
            self.rate_reset_slots(terms, &mut slots)?;
        }
        self.scaling_index_slots(terms, &mut slots)?;
        self.interest_calculation_base_slots(terms, &mut slots)?;
        slots.sort_by_key(|slot| (slot.calc, slot.kind.priority()));
        Ok(slots)
    }

    /// The (unshifted schedule date, emission time) pairs of one array
    /// schedule ending at the maturity: the array schedule is generated
    /// under `NOS` for the unshifted dates and under `bdc` for the emission
    /// times, paired positionally (the shift never changes the element
    /// count) — the same construction as `stub_series_unshifted`.
    fn array_pairs(
        &self,
        anchors: &[NaiveDateTime],
        cycles: &[Cycle],
    ) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>, EngineError> {
        let unshifted = array_series(
            anchors,
            cycles,
            self.maturity,
            self.eomc,
            BusinessDayConvention::Nos,
            self.cal,
        )?;
        let shifted = array_series(
            anchors,
            cycles,
            self.maturity,
            self.eomc,
            self.bdc,
            self.cal,
        )?;
        Ok(unshifted
            .into_iter()
            .zip(shifted)
            .map(|((unshifted, _), (_, emitted))| (unshifted, emitted))
            .collect())
    }

    /// The schedule segment a date belongs to: the last anchor at or before
    /// the date (`0` before the first anchor).
    fn segment_of(anchors: &[NaiveDateTime], t: NaiveDateTime) -> usize {
        anchors
            .iter()
            .enumerate()
            .filter(|(_, anchor)| **anchor <= t)
            .map(|(index, _)| index)
            .max()
            .unwrap_or(0)
    }

    /// The `PR` principal redemption series.
    ///
    /// With the array attributes `ARPRANX`/`ARPRCL` present the series is
    /// the array schedule (paper §3.2) to maturity; the maturity element
    /// belongs to `MD` and is not emitted, mirroring the LAM rule. The
    /// returned (schedule date, emission time) pairs are shared with the
    /// interest series for the fallback alignment (paper §7.4). Without the
    /// array attributes the series is the LAM scalar series
    /// `S(PRANX, PRCL, MD)` without the maturity element.
    fn principal_redemption_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
    ) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>, EngineError> {
        if let Some((anchors, cycles)) = &self.principal_array {
            let pairs = self.array_pairs(anchors, cycles)?;
            for (unshifted, emit) in &pairs {
                let calc = calculation_time(self.bdc, *unshifted, *emit);
                if calc >= self.maturity {
                    continue;
                }
                slots.push(Slot::in_segment(
                    Kind::PrincipalRedemption,
                    calc,
                    *emit,
                    Self::segment_of(anchors, *unshifted),
                ));
            }
            return Ok(pairs);
        }
        let anchor = match terms.cycle_anchor_date_of_principal_redemption {
            Some(anchor) => normalize_timestamp(anchor),
            None => return Ok(Vec::new()),
        };
        let cycle = match terms.cycle_of_principal_redemption.as_ref() {
            Some(cycle) => cycle,
            None => return Ok(Vec::new()),
        };
        let pairs = stub_series(anchor, cycle, self.maturity, self.eomc, self.bdc, self.cal)?;
        for (calc, emit) in &pairs {
            if *calc == self.maturity {
                continue;
            }
            slots.push(Slot::at(Kind::PrincipalRedemption, *calc, *emit));
        }
        Ok(pairs)
    }

    /// The `IPCI` and `IP` series.
    ///
    /// With the array attributes `ARIPANX`/`ARIPCL` present the `IP` series
    /// is the array interest schedule to maturity (the maturity element is
    /// kept: like in LAM, the final accrual is paid at or before `MD`).
    /// Otherwise with scalar `IPANX`/`IPCL` the LAM machinery applies: the
    /// `IPCI` series runs to the capitalization end date `IPCED` and the
    /// `IP` series runs to maturity with the capitalization dates removed,
    /// deduplicated on the unshifted schedule dates. Without any interest
    /// attributes the interest accrual is paid on the principal redemption
    /// schedule dates plus maturity (the paper §7.4 alignment), so the
    /// period accrual never folds silently into the `MD` payoff.
    fn interest_slots(
        &self,
        terms: &ContractTerms,
        principal_pairs: &[(NaiveDateTime, NaiveDateTime)],
        slots: &mut Vec<Slot>,
    ) -> Result<(), EngineError> {
        if let Some((anchors, cycles)) = &self.interest_array {
            for (unshifted, emit) in self.array_pairs(anchors, cycles)? {
                let calc = calculation_time(self.bdc, unshifted, emit);
                if calc > self.maturity {
                    continue;
                }
                slots.push(Slot::at(Kind::InterestPayment, calc, emit));
            }
            return Ok(());
        }
        let anchor = terms
            .cycle_anchor_date_of_interest_payment
            .map(normalize_timestamp);
        let cycle = terms.cycle_of_interest_payment;
        if anchor.is_none() || cycle.is_none() {
            for (unshifted, emit) in principal_pairs {
                let calc = calculation_time(self.bdc, *unshifted, *emit);
                if calc > self.maturity {
                    continue;
                }
                slots.push(Slot::at(Kind::InterestPayment, calc, *emit));
            }
            // The maturity element pays the final accrual before `MD`; when
            // the redemption pairs already end at maturity this is the same
            // schedule point, so no extra slot is added.
            let maturity_in_pairs = principal_pairs
                .iter()
                .any(|(unshifted, _)| *unshifted == self.maturity);
            if !maturity_in_pairs && self.maturity > self.ied {
                slots.push(Slot::at(
                    Kind::InterestPayment,
                    self.maturity,
                    self.maturity,
                ));
            }
            return Ok(());
        }
        let schedule = interest_schedule(&self.conventions(), terms)?;
        for timing in schedule.capitalization {
            slots.push(Slot::timed(Kind::InterestCapitalization, timing));
        }
        for timing in schedule.payment {
            slots.push(Slot::timed(Kind::InterestPayment, timing));
        }
        Ok(())
    }

    /// The `RR` rate reset series.
    ///
    /// With the array attributes `ARRRANX`/`ARRRCL` present the series is
    /// the array rate schedule to maturity without the maturity element;
    /// each slot resolves `ARRATE[segment]` per `ARFIXVAR` at application
    /// time. Otherwise the LAM machinery applies: the scalar series unrolls
    /// from `RRANX`, and with `RRNXT` set the first schedule point after the
    /// status date becomes a fixed reset `RRF` (lam14).
    fn rate_reset_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
    ) -> Result<(), EngineError> {
        if let Some((anchors, cycles)) = &self.rate_array {
            let mut pairs = self.array_pairs(anchors, cycles)?;
            if pairs.last().map(|(_, emit)| *emit) == Some(self.maturity) {
                pairs.pop();
            }
            for (unshifted, emit) in pairs {
                let calc = calculation_time(self.bdc, unshifted, emit);
                if calc > self.maturity {
                    continue;
                }
                slots.push(Slot::in_segment(
                    Kind::ArrayRateReset,
                    calc,
                    emit,
                    Self::segment_of(anchors, unshifted),
                ));
            }
            return Ok(());
        }
        for (reset, timing) in rate_reset_schedule(&self.conventions(), terms)? {
            slots.push(Slot::timed(rate_reset_kind(reset), timing));
        }
        Ok(())
    }

    /// The `SC` scaling index series (as in LAM): `S(SCANX, SCCL, MD)`
    /// without the maturity element.
    fn scaling_index_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
    ) -> Result<(), EngineError> {
        for timing in scaling_index_schedule(&self.conventions(), terms)? {
            slots.push(Slot::timed(Kind::ScalingIndexFixing, timing));
        }
        Ok(())
    }

    /// The `IPCB` interest calculation base fixing series (as in LAM):
    /// defined only for `IPCB = NTL`, unrolled from `IPCBANX` to maturity
    /// without the maturity element.
    fn interest_calculation_base_slots(
        &self,
        terms: &ContractTerms,
        slots: &mut Vec<Slot>,
    ) -> Result<(), EngineError> {
        for timing in interest_calculation_base_schedule(&self.conventions(), terms)? {
            slots.push(Slot::timed(Kind::InterestCalculationBaseFixing, timing));
        }
        Ok(())
    }

    /// The scheduled principal redemption payment of one segment
    /// (`ARPRNXT[segment]`, falling back to the carried `PRNXT` state and,
    /// through its initialisation, to the notional-over-elements default).
    fn scheduled_redemption(&self, segment: usize, state: &ContractState) -> Decimal {
        match &self.payments {
            Some(payments) if !payments.is_empty() => {
                let index = segment.min(payments.len() - 1);
                payments[index]
            }
            _ => state.next_principal_redemption_payment,
        }
    }

    /// Whether a segment increases the notional (`ARINCDEC[segment] = INC`);
    /// segments default to `DEC` when the attribute is absent.
    fn segment_increases(&self, segment: usize) -> bool {
        match &self.increase_decrease {
            Some(segments) if !segments.is_empty() => {
                let index = segment.min(segments.len() - 1);
                segments[index] == ArrayIncDec::Inc
            }
            _ => false,
        }
    }

    /// The `ARRATE` value of one segment, clamped to the last element.
    fn array_rate(&self, terms: &ContractTerms, segment: usize) -> Option<Decimal> {
        let rates = terms.array_rate.as_ref()?;
        if rates.is_empty() {
            return None;
        }
        Some(rates[segment.min(rates.len() - 1)])
    }

    /// Applies the state transition and payoff function of one slot
    /// (techspec LAM functions table, with the LAX segment semantics of
    /// paper §7.3 for `PR` and `RR`) and returns the payoff. Every
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
                state.accrued_interest =
                    fixed_income::initial_accrued(terms, state, slot.calc, self.dcc, self.sgn)?;
                state.next_principal_redemption_payment = self.scheduled_redemption(0, state);
                let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
                state.interest_scaling_multiplier = interest_scaling;
                state.notional_scaling_multiplier = notional_scaling;
                state.interest_calculation_base_amount =
                    initial_base_amount(terms, notional, self.sgn);
                self.sgn * -(notional + premium)
            }
            Kind::PrincipalRedemption => {
                state.accrued_interest += accrual;
                let scheduled = self.scheduled_redemption(slot.segment, state);
                if self.segment_increases(slot.segment) {
                    // NAM semantics: the net principal portion of the period
                    // payment is `Prnxt - Ipac`; when the period accrual
                    // exceeds the payment the redemption turns negative and
                    // the notional grows by the shortfall (nam17), capped at
                    // the remaining notional magnitude (ann13). The accrued
                    // interest enters the payoff by magnitude so the `RPL`
                    // role mirrors the `RPA` economics.
                    let net = state.notional_scaling_multiplier
                        * (scheduled - self.sgn * state.accrued_interest);
                    let payoff = if net.abs() > state.notional_principal.abs() {
                        state.notional_principal
                    } else {
                        net
                    };
                    state.notional_principal -= payoff;
                    if self.base_tracks_notional {
                        state.interest_calculation_base_amount = state.notional_principal;
                    }
                    state.next_principal_redemption_payment = scheduled;
                    payoff
                } else {
                    // LAM semantics: ordinary linear redemption, capped at
                    // the remaining notional (lam25).
                    let redemption = scheduled.min(state.notional_principal.abs());
                    state.notional_principal -= self.sgn * redemption;
                    if self.base_tracks_notional {
                        state.interest_calculation_base_amount = state.notional_principal;
                    }
                    state.next_principal_redemption_payment =
                        scheduled.min(state.notional_principal.abs());
                    self.sgn * state.notional_scaling_multiplier * redemption
                }
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
            Kind::ArrayRateReset => {
                state.accrued_interest += accrual;
                match terms.array_fixed_variable {
                    Some(ArrayFixVar::Variable) => {
                        // The segment rate is the spread on top of the
                        // reference rate (the applicable `RRSP`).
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
                        let spread = self
                            .array_rate(terms, slot.segment)
                            .unwrap_or(terms.rate_spread.unwrap_or(Decimal::ZERO));
                        state.nominal_interest_rate = observed * multiplier + spread;
                    }
                    // `F` and the attribute default: the segment rate is the
                    // new fixed nominal interest rate.
                    _ => {
                        let rate = self
                            .array_rate(terms, slot.segment)
                            .or(terms.next_reset_rate)
                            .ok_or(EngineError::MissingAttribute("arrayRate"))?;
                        state.nominal_interest_rate = rate;
                    }
                }
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

/// Builds the pre-initial-exchange-date state (techspec LAM states table,
/// `Ipcb` row: `t0 < IED` yields a zeroed calculation base): the redemption
/// amount and scaling multipliers are already resolved from the terms, the
/// monetary and rate states stay zero until `IED`.
fn initial_state(ctx: &Context, terms: &ContractTerms) -> ContractState {
    let mut state = ContractState::initial(terms);
    // The scalar default first, then the array payment of the first segment
    // when `ARPRNXT` is present.
    state.next_principal_redemption_payment = ctx.next_principal_redemption;
    state.next_principal_redemption_payment = ctx.scheduled_redemption(0, &state);
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
    // The scalar default first, then the array payment of the first segment
    // when `ARPRNXT` is present.
    state.next_principal_redemption_payment = ctx.next_principal_redemption;
    state.next_principal_redemption_payment = ctx.scheduled_redemption(0, &state);
    let (interest_scaling, notional_scaling) = term_scaling_multipliers(terms);
    state.interest_scaling_multiplier = interest_scaling;
    state.notional_scaling_multiplier = notional_scaling;
    state.interest_calculation_base_amount = initial_base_amount(terms, notional, ctx.sgn);
    state.accrued_interest = match terms.nominal_interest_rate {
        None => Decimal::ZERO,
        Some(_) => match terms.accrued_interest {
            Some(accrued) => ctx.sgn * accrued,
            None => fixed_income::progressed_accrued(
                &ctx.conventions(),
                terms,
                state.interest_calculation_base_amount,
            )?
            .unwrap_or(Decimal::ZERO),
        },
    };
    Ok(state)
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

    /// The terms of the lam01 fixture; used as the LAM reference contract
    /// for the equivalence checks.
    fn lam_reference_json(contract_type: &str) -> serde_json::Value {
        json!({
            "contractType": contract_type,
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
        })
    }

    #[test]
    fn scalar_dec_lax_reproduces_the_lam_event_vector() {
        let lam_terms = terms(lam_reference_json("LAM"));
        let lax_terms = terms(lam_reference_json("LAX"));
        let lam_events = crate::lam::LamEngine
            .evaluate(&lam_terms, &StateProvider::new())
            .expect("lam events");
        let lax_events = LaxEngine
            .evaluate(&lax_terms, &StateProvider::new())
            .expect("lax events");
        assert_eq!(lax_events, lam_events);
        assert_eq!(lam_events.len(), 21);
    }

    #[test]
    fn single_segment_array_dec_lax_reproduces_the_lam_event_vector() {
        let mut raw = lam_reference_json("LAX");
        raw["arrayCycleAnchorDateOfPrincipalRedemption"] = json!(["2013-02-01T00:00:00"]);
        raw["arrayCycleOfPrincipalRedemption"] = json!(["P1ML0"]);
        raw["arrayNextPrincipalRedemptionPayment"] = json!(["500"]);
        raw["arrayIncreaseDecrease"] = json!(["DEC"]);
        let lam_terms = terms(lam_reference_json("LAM"));
        let lax_terms = terms(raw);
        let lam_events = crate::lam::LamEngine
            .evaluate(&lam_terms, &StateProvider::new())
            .expect("lam events");
        let lax_events = LaxEngine
            .evaluate(&lax_terms, &StateProvider::new())
            .expect("lax events");
        assert_eq!(lax_events, lam_events);
    }

    /// Two-segment hand-computed contract (paper §3.2 array schedule,
    /// segment economics per §7.3): NT = 1440 at IED 2026-01-01, MD
    /// 2028-01-01, zero rate. Segment 0 anchors 2026-01-01 with `P1ML0` and
    /// `ARPRNXT = 100`, terminating one cycle before the 2027-01-01 anchor,
    /// so twelve monthly PR of 100 run 2026-01-01..2026-12-01. Segment 1
    /// anchors 2027-01-01 with `P3ML1` and `ARPRNXT = 60`, keeping the
    /// overshooting roll, so quarterly PR of 60 run 2027-01-01, 04-01,
    /// 07-01, 10-01 and MD supersedes the 2028-01-01 schedule end.
    #[test]
    fn two_segment_contract_pays_the_per_segment_redemptions() {
        let terms = terms(json!({
            "contractType": "LAX",
            "contractRole": "RPA",
            "statusDate": "2025-12-30T00:00:00",
            "initialExchangeDate": "2026-01-01T00:00:00",
            "maturityDate": "2028-01-01T00:00:00",
            "notionalPrincipal": "1440",
            "nominalInterestRate": "0",
            "arrayCycleAnchorDateOfPrincipalRedemption":
                ["2026-01-01T00:00:00", "2027-01-01T00:00:00"],
            "arrayCycleOfPrincipalRedemption": ["P1ML0", "P3ML1"],
            "arrayNextPrincipalRedemptionPayment": ["100", "60"],
            "arrayIncreaseDecrease": ["DEC", "DEC"],
            "dayCountConvention": "A365",
            "endOfMonthConvention": "SD",
            "businessDayConvention": "NOS",
            "calendar": "NC"
        }));
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let prs: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .collect();
        assert_eq!(prs.len(), 16);
        let times: Vec<NaiveDateTime> = prs.iter().map(|e| e.time).collect();
        assert_eq!(times[0], t("2026-01-01T00:00:00"));
        assert_eq!(times[11], t("2026-12-01T00:00:00"));
        assert_eq!(times[12], t("2027-01-01T00:00:00"));
        assert_eq!(times[13], t("2027-04-01T00:00:00"));
        assert_eq!(times[14], t("2027-07-01T00:00:00"));
        assert_eq!(times[15], t("2027-10-01T00:00:00"));
        // Segment 0 pays 12 x 100 = 1200, segment 1 pays 4 x 60 = 240; the
        // notional is redeemed exactly at the last PR.
        assert_eq!(prs[0].payoff, Decimal::from(100));
        assert_eq!(prs[0].state.notional_principal, Decimal::from(1340));
        assert_eq!(prs[11].payoff, Decimal::from(100));
        assert_eq!(prs[11].state.notional_principal, Decimal::from(240));
        assert_eq!(prs[12].payoff, Decimal::from(60));
        assert_eq!(prs[12].state.notional_principal, Decimal::from(180));
        assert_eq!(prs[15].payoff, Decimal::from(60));
        assert_eq!(prs[15].state.notional_principal, Decimal::ZERO);
        let redemption_total: Decimal = prs.iter().map(|e| e.payoff).sum();
        assert_eq!(redemption_total, Decimal::from(1440));
        // Zero rate: the IED carries the principal, the maturity redeems the
        // remainder (zero) and the IP events carry zero payoffs.
        assert_eq!(events.first().expect("IED").payoff, Decimal::from(-1440));
        assert_eq!(events.last().expect("MD").payoff, Decimal::ZERO);
        assert_eq!(events.last().expect("MD").time, t("2028-01-01T00:00:00"));
        // Without any interest attributes the IP series aligns with the PR
        // schedule dates plus maturity (paper §7.4).
        let ips: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.time)
            .collect();
        assert_eq!(ips.len(), 17);
        assert_eq!(ips[0], t("2026-01-01T00:00:00"));
        assert_eq!(ips[16], t("2028-01-01T00:00:00"));
    }

    /// An `INC` segment with a scheduled payment below the period accrual
    /// amortizes negatively: with NT = 5000 at 8% `A365` the accrual of the
    /// 31-day first period is 5000 x 0.08 x 31/365 = 33.9726027397260, the
    /// scheduled `ARPRNXT = 30` falls short by 3.9726027397260, so the PR
    /// payoff is -3.9726027397260 and the notional grows by the shortfall to
    /// 5003.9726027397260.
    #[test]
    fn inc_segment_capitalizes_the_accrual_shortfall() {
        let terms = terms(json!({
            "contractType": "LAX",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2013-11-01T00:00:00",
            "notionalPrincipal": "5000",
            "nominalInterestRate": "0.08",
            "arrayCycleAnchorDateOfPrincipalRedemption": ["2013-02-01T00:00:00"],
            "arrayCycleOfPrincipalRedemption": ["P1ML0"],
            "arrayNextPrincipalRedemptionPayment": ["30"],
            "arrayIncreaseDecrease": ["INC"],
            "cycleAnchorDateOfInterestPayment": "2013-02-01T00:00:00",
            "cycleOfInterestPayment": "P1ML0",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        }));
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let pr = events
            .iter()
            .find(|e| e.event_type == EventType::PrincipalRedemption)
            .expect("first PR");
        assert_eq!(pr.time, t("2013-02-01T00:00:00"));
        assert_close(pr.payoff, dec!(-3.9726027397260));
        assert_close(pr.state.notional_principal, dec!(5003.9726027397260));
        assert_close(pr.state.accrued_interest, dec!(33.9726027397260));
        assert_eq!(
            pr.state.next_principal_redemption_payment,
            Decimal::from(30)
        );
        // The same-date IP still pays the period accrual (the dictionary
        // sequence PR -> IP), so the total period cash is the ARPRNXT of 30.
        let ip = events
            .iter()
            .find(|e| e.event_type == EventType::InterestPayment)
            .expect("first IP");
        assert_close(ip.payoff, dec!(33.9726027397260));
        assert_eq!(ip.state.accrued_interest, Decimal::ZERO);
    }

    /// A pure-`DEC` LAX redeems the initial notional exactly: the sum of the
    /// `PR` payoffs telescopes to the post-IED notional because every `PR`
    /// decreases `NT` by its capped payoff.
    #[test]
    fn dec_redemptions_conserve_the_initial_notional() {
        let terms = terms(json!({
            "contractType": "LAX",
            "contractRole": "RPA",
            "statusDate": "2025-12-30T00:00:00",
            "initialExchangeDate": "2026-01-01T00:00:00",
            "maturityDate": "2028-01-01T00:00:00",
            "notionalPrincipal": "1440",
            "nominalInterestRate": "0",
            "arrayCycleAnchorDateOfPrincipalRedemption":
                ["2026-01-01T00:00:00", "2027-01-01T00:00:00"],
            "arrayCycleOfPrincipalRedemption": ["P1ML0", "P3ML1"],
            "arrayNextPrincipalRedemptionPayment": ["100", "60"],
            "arrayIncreaseDecrease": ["DEC", "DEC"],
            "dayCountConvention": "A365"
        }));
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let redemption_total: Decimal = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .map(|e| e.payoff)
            .sum();
        assert_eq!(redemption_total, Decimal::from(1440));
        let maturity = events.last().expect("MD");
        assert_eq!(maturity.state.notional_principal, Decimal::ZERO);
        assert_eq!(maturity.payoff, Decimal::ZERO);
    }

    /// An array rate schedule with `ARFIXVAR = F` resets the nominal rate to
    /// the per-segment `ARRATE` element: segment 0 (`P2ML0` to one cycle
    /// before the 2026-07-01 anchor) resets at 0.05 on 2026-01-01, 03-01,
    /// 05-01, segment 1 resets at 0.09 on 2026-07-01 (the maturity element
    /// of the schedule belongs to `MD`).
    #[test]
    fn array_rate_reset_applies_the_segment_rate() {
        let terms = terms(json!({
            "contractType": "LAX",
            "contractRole": "RPA",
            "statusDate": "2025-12-30T00:00:00",
            "initialExchangeDate": "2026-01-01T00:00:00",
            "maturityDate": "2027-01-01T00:00:00",
            "notionalPrincipal": "1200",
            "nominalInterestRate": "0.08",
            "arrayCycleAnchorDateOfPrincipalRedemption": ["2026-01-01T00:00:00"],
            "arrayCycleOfPrincipalRedemption": ["P3ML1"],
            "arrayNextPrincipalRedemptionPayment": ["100"],
            "arrayCycleAnchorDateOfRateReset":
                ["2026-01-01T00:00:00", "2026-07-01T00:00:00"],
            "arrayCycleOfRateReset": ["P2ML0", "P6ML0"],
            "arrayRate": ["0.05", "0.09"],
            "arrayFixedVariable": "F",
            "cycleAnchorDateOfInterestPayment": "2026-01-01T00:00:00",
            "cycleOfInterestPayment": "P3ML1",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        }));
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let resets: Vec<&ContractEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::RateResetVariable)
            .collect();
        assert_eq!(resets.len(), 4);
        assert_eq!(resets[0].time, t("2026-01-01T00:00:00"));
        assert_eq!(resets[0].state.nominal_interest_rate, dec!(0.05));
        assert_eq!(resets[1].time, t("2026-03-01T00:00:00"));
        assert_eq!(resets[1].state.nominal_interest_rate, dec!(0.05));
        assert_eq!(resets[2].time, t("2026-05-01T00:00:00"));
        assert_eq!(resets[2].state.nominal_interest_rate, dec!(0.05));
        assert_eq!(resets[3].time, t("2026-07-01T00:00:00"));
        assert_eq!(resets[3].state.nominal_interest_rate, dec!(0.09));
        assert!(resets.iter().all(|e| e.payoff == Decimal::ZERO));
    }

    /// A one-year LAX with quarterly array redemptions and a two-segment
    /// rate array; `extra` overrides or adds attributes.
    fn array_rate_contract(extra: serde_json::Value) -> ContractTerms {
        let mut raw = json!({
            "contractType": "LAX",
            "contractRole": "RPA",
            "statusDate": "2025-12-30T00:00:00",
            "initialExchangeDate": "2026-01-01T00:00:00",
            "maturityDate": "2027-01-01T00:00:00",
            "notionalPrincipal": "1200",
            "nominalInterestRate": "0.08",
            "arrayCycleAnchorDateOfPrincipalRedemption": ["2026-01-01T00:00:00"],
            "arrayCycleOfPrincipalRedemption": ["P3ML1"],
            "arrayNextPrincipalRedemptionPayment": ["100"],
            "arrayCycleAnchorDateOfRateReset":
                ["2026-01-01T00:00:00", "2026-07-01T00:00:00"],
            "arrayCycleOfRateReset": ["P2ML0", "P6ML0"],
            "arrayRate": ["0.05", "0.09"],
            "cycleAnchorDateOfInterestPayment": "2026-01-01T00:00:00",
            "cycleOfInterestPayment": "P3ML1",
            "dayCountConvention": "A365",
            "interestCalculationBase": "NT"
        });
        for (key, value) in extra.as_object().expect("object") {
            raw[key] = value.clone();
        }
        terms(raw)
    }

    #[test]
    fn variable_array_rate_adds_the_segment_spread_to_the_observed_rate() {
        let terms = array_rate_contract(json!({
            "arrayFixedVariable": "V",
            "marketObjectCodeOfRateReset": "IDX",
            "rateMultiplier": "2"
        }));
        let risk = StateProvider::new()
            .with_rate("IDX", t("2026-01-01T00:00:00"), dec!(0.01))
            .with_rate("IDX", t("2026-07-01T00:00:00"), dec!(0.02));
        let events = LaxEngine.evaluate(&terms, &risk).expect("events");
        let rates: Vec<(NaiveDateTime, Decimal)> = events
            .iter()
            .filter(|e| e.event_type == EventType::RateResetVariable)
            .map(|e| (e.time, e.state.nominal_interest_rate))
            .collect();
        assert_eq!(
            rates,
            vec![
                // Segment 0: 2 x 0.01 + 0.05.
                (t("2026-01-01T00:00:00"), dec!(0.07)),
                (t("2026-03-01T00:00:00"), dec!(0.07)),
                (t("2026-05-01T00:00:00"), dec!(0.07)),
                // Segment 1: 2 x 0.02 + 0.09.
                (t("2026-07-01T00:00:00"), dec!(0.13)),
            ]
        );
    }

    #[test]
    fn variable_array_rate_needs_a_market_object_and_an_observation() {
        let missing_code = array_rate_contract(json!({ "arrayFixedVariable": "V" }));
        assert!(matches!(
            LaxEngine.evaluate(&missing_code, &StateProvider::new()),
            Err(EngineError::MissingAttribute("marketObjectCodeOfRateReset"))
        ));
        let unobserved = array_rate_contract(json!({
            "arrayFixedVariable": "V",
            "marketObjectCodeOfRateReset": "IDX"
        }));
        match LaxEngine.evaluate(&unobserved, &StateProvider::new()) {
            Err(EngineError::RiskFactorMissing { code, .. }) => assert_eq!(code, "IDX"),
            other => panic!("expected RiskFactorMissing, got {other:?}"),
        }
    }

    #[test]
    fn interest_array_schedules_interest_on_the_array_dates() {
        let mut terms = array_rate_contract(json!({
            "arrayCycleAnchorDateOfInterestPayment":
                ["2026-01-01T00:00:00", "2026-07-01T00:00:00"],
            "arrayCycleOfInterestPayment": ["P6ML1", "P1ML1"]
        }));
        terms.array_cycle_anchor_date_of_rate_reset = None;
        terms.array_cycle_of_rate_reset = None;
        terms.array_rate = None;
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let ip: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.time)
            .collect();
        // Segment 0 is semiannual (one IP at 2026-01-01, the anchor, then
        // segment 1 takes over monthly from 2026-07-01 to maturity).
        assert_eq!(ip.first(), Some(&t("2026-01-01T00:00:00")));
        assert!(ip.contains(&t("2026-07-01T00:00:00")));
        assert!(ip.contains(&t("2026-08-01T00:00:00")));
        assert!(ip.contains(&t("2026-12-01T00:00:00")));
        assert!(!ip.contains(&t("2026-04-01T00:00:00")));
        assert!(ip.iter().all(|time| *time <= t("2027-01-01T00:00:00")));
        // Interest is paid on every IP event; total accrual is positive.
        let paid: Decimal = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .sum();
        assert!(paid > Decimal::ZERO);
    }

    #[test]
    fn without_an_interest_cycle_interest_follows_the_redemption_dates() {
        let mut terms = array_rate_contract(json!({}));
        terms.cycle_anchor_date_of_interest_payment = None;
        terms.cycle_of_interest_payment = None;
        terms.array_cycle_anchor_date_of_rate_reset = None;
        terms.array_cycle_of_rate_reset = None;
        terms.array_rate = None;
        let events = LaxEngine
            .evaluate(&terms, &StateProvider::new())
            .expect("events");
        let pr: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::PrincipalRedemption)
            .map(|e| e.time)
            .collect();
        let ip: Vec<NaiveDateTime> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.time)
            .collect();
        assert!(!pr.is_empty());
        for time in &pr {
            assert!(ip.contains(time), "IP expected at redemption date {time}");
        }
        // The final accrual is paid at maturity.
        assert_eq!(ip.last(), Some(&t("2027-01-01T00:00:00")));
    }

    #[test]
    fn missing_redemption_terms_are_reported_not_panicked() {
        let mut no_payment = array_rate_contract(json!({}));
        no_payment.array_next_principal_redemption_payment = None;
        no_payment.array_cycle_anchor_date_of_principal_redemption = None;
        no_payment.array_cycle_of_principal_redemption = None;
        no_payment.maturity_date = None;
        assert!(matches!(
            LaxEngine.evaluate(&no_payment, &StateProvider::new()),
            Err(EngineError::MissingAttribute(_))
        ));
        let mut no_ied = array_rate_contract(json!({}));
        no_ied.initial_exchange_date = None;
        assert!(matches!(
            LaxEngine.evaluate(&no_ied, &StateProvider::new()),
            Err(EngineError::MissingAttribute("initialExchangeDate"))
        ));
    }
}
