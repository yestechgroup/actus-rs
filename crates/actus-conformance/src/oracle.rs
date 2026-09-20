//! Independent oracle for the ACTUS fixed-income family (PAM, LAM, NAM,
//! ANN) for differential testing (techspec sections 7.1 "PAM: Principal At
//! Maturity", 7.2 "LAM: Linear Amortizer", 7.4 "NAM: Negative Amortizer",
//! 7.5 "ANN: Annuity" and the section 3 utilities of `docs/actus.md` /
//! `vendor/actus/techspecs/actus-techspecs.tex`).
//!
//! ## Independence rules
//!
//! This module implements the state transition functions, payoff functions
//! and schedule utilities directly from the specification text. It
//! deliberately shares nothing with `crates/actus-engine`: no helper
//! functions, no schedule, day count or state code, no `actus_engine`
//! imports. The only shared vocabulary is the `actus-model` type layer
//! (`ContractTerms`, `Cycle`, the dictionary enums and the dictionary event
//! sequence numbers of `EventType::priority`), because both sides must
//! consume the same terms and the same dictionary data to be comparable.
//!
//! ## Numerical regime (different by construction from the engine)
//!
//! The engine evaluates in variable-scale `rust_decimal` arithmetic. The
//! oracle uses hand-rolled two's-complement fixed point on `i128` with
//! truncating division:
//!
//! - money (payoffs, `NT`, `IPAC`, `PDIED`, `PRNXT`): scale 1e-9 (nano
//!   currency units, `MONEY_DIGITS = 9`);
//! - interest rates (`IPNR`, observations, `RRMLT`, `RRSP`, `RRNXT`):
//!   scale 1e-18 (`RATE_DIGITS = 18`);
//! - year fractions: scale 1e-18 (`YEAR_DIGITS = 18`).
//!
//! Every rescale, product and quotient truncates toward zero, so each
//! operation loses at most one unit in the last place of its result scale.
//! A single event chain performs at most three truncating money-scale
//! operations (year fraction, accrual product, state update), and the
//! corpus bounds contracts at 46 monthly periods with a 0.2 rate cap, so
//! the interest-on-interest compounding of state error grows by at most
//! the product of per-period growth factors `1 + r x Y` — at most
//! `(1 + 0.2 x 31/360)^46 < 2.2 < 3 = e^1` across the longest four-year
//! chain, unchanged from the v1 bound. The corpus carries at most 96 events
//! per contract (monthly cycles produce up to two events per period plus
//! resets, fixings and boundary events), so the accumulated absolute error
//! stays below `96 x 3 x 1e-9 x 3 = 8.7e-7` currency units; the
//! differential gate uses an absolute floor of `1e-5` (more than ten times
//! that bound, and six orders of magnitude below any formula-level error).
//! The annuity amount function adds only rate-scale truncations (the
//! growth factors `G_k` and their inverses at scale 1e-18), a relative
//! error around `m x 1e-18` with at most 36 remaining payments. Relative
//! to the compared magnitude the oracle error is at most about `1e-15`
//! (a 1e-18 year-fraction/rate ulp amplified by the at-most-4-year
//! accrual window) and the engine's 96-bit decimal error is below `1e-25`,
//! so a relative term of `1e-8` sits more than six orders of magnitude
//! above numerical noise while staying far below the smallest meaningful
//! divergence (a single miscounted day is `2.7e-3` relative). The derived
//! comparison bound is `|engine - oracle| <= 1e-5 + 1e-8 x max(|engine|,
//! |oracle|)` per numeric quantity; times and event types compare exactly.
//!
//! ## Scope (v2, honest)
//!
//! Covered: the full fixed-income core event set of PAM, LAM, NAM and ANN —
//! `IED`, `PR`, `PRF` (ANN, `PRNXT` attribute absent), `IP`, `IPCI`, `RR`,
//! `RRF`, `MD` — with role signs, the section 3 schedule (stubs, `EOMC`,
//! `BDC`, `NC`/`MF` calendars), the `A365`, `A360`, `30E360` day counts,
//! the LAM redemption machinery (`PRNXT` constant, capped at the remaining
//! notional), the NAM net-redemption machinery and the ANN annuity amount
//! function over the reference `PRF` fixing series.
//! Not covered (and rejected with an error when met): fees, penalties,
//! prepayment, purchase/termination, scaling, progressed contracts
//! (`IED` < `SD`), `IPCB` semantics other than the `NT` default, LAM/NAM
//! maturity inference and the LAM `PRNXT` default (the maturity and
//! redemption attributes are required), NAM without `PRNXT`, the ANN
//! amortization date horizon `AD` (the maturity attribute is required and
//! dimensions the annuity), settlement currency conversion, and day counts
//! other than the three above.
//!
//! ## Spec readings where the text required interpretation
//!
//! - The section 3.1 stub rule ("else `t_n` is removed from the schedule")
//!   is applied to the last cyclic roll, with the schedule end `T` always
//!   terminating the schedule; removing the terminal date itself would end
//!   the contract before `MD`. This is the reading the official testbeds
//!   pin (lam09 long stub, lam19 short stub).
//! - The RR schedule terminates at maturity. The terminal schedule element
//!   is the maturity date itself; neither the reference implementation nor
//!   the official testbeds emit an `RR` event at maturity, so the terminal
//!   element is not part of the RR event series.
//! - The `MD` payoff of `docs/actus.md` section 7.1 prints
//!   `Nsc - N + Isc Ipac + Feac`; the LaTeX techspec (authoritative for
//!   formula shape) reads `Nsc Nt + Isc Ipac + Feac` over the signed
//!   notional state, which is what both the engine and the testbeds
//!   implement and what this oracle implements.
//! - The section 3.4 calculation-time rule is applied uniformly: `SC*`
//!   conventions calculate on the shifted date, `CS*` conventions on the
//!   unshifted date, for every event kind including `IPCI` (techspec
//!   section "Business Day Shift Convention": "calculation of the event
//!   happens after the shift"). The engine implements the same rule for
//!   every series; its initial differential run calculated intermediate
//!   `IPCI` transitions on the unshifted date under `SC*` and lost the
//!   `IP`/`IPCI` schedule deduplication with it, an engine defect fixed
//!   against section 3.4 rather than tolerated here.
//! - The `IPCI` series terminates at `IPCED` alone when `IPCED` precedes
//!   the interest anchor, the reading the official testbeds pin (lam22,
//!   nam19, ann14 capitalize at `IPCED` alone).
//! - Events emitted before the initial exchange date mutate the state but
//!   are not reported (ann09: the 2012-12-31 fixing applies to the state
//!   and stays out of the event stream); the v1 oracle reported from the
//!   status date instead, which diverged from the engine only for
//!   schedule elements whose preceding business day shift moved the
//!   emission before the initial exchange.
//! - The effective same-timestamp order places the ANN principal fixing
//!   `PRF` after the same-timestamp rate reset (ann15, ann16: the fixing
//!   recalculates `PRNXT` from the rate the reset has just set),
//!   deviating from the v1.4 dictionary sequence 5 of `PRF`; every other
//!   kind orders by its dictionary sequence.
//! - The `RR` rate update applies `min`/`max` clamps (`RRLF`/`RRLC`/
//!   `RRPF`/`RRPC`) that the v1.4 dictionary does not model as terms; with
//!   the clamps absent the update degenerates to the observed rate times
//!   `RRMLT` plus `RRSP`, matching the engine.
//! - The LAM `PR` row keeps `PRNXT` constant and caps the next payment at
//!   the remaining notional magnitude once the notional is exhausted
//!   (lam25 pays a zero redemption after full redemption, lam26 keeps the
//!   unscaled payment while notional remains). The reported `PRNXT` state
//!   is the unsigned attribute form the LAM engine carries; NAM and ANN
//!   report the role-signed dictionary state.
//! - The NAM `PR` row nets the accrued interest: `NT+ = NT- - (PRNXT- -
//!   IPAC+)`, never recalculates the payment (nam17's negative redemptions
//!   are legitimate) and caps the redemption at the remaining notional
//!   (ann13; no NAM fixture is affected).
//! - The techspec NAM/ANN `IP` schedule row builds the series as
//!   `(u, v)` with `v = S(PRANX, PRCL, MD)`; the official fixtures pin a
//!   different machinery: nam19 emits no `IP` events during the
//!   capitalization phase and `ann01` pays interest exactly at the
//!   redemption dates plus maturity. This oracle implements the
//!   fixture-pinned machinery (the `IP` series runs from the interest
//!   anchor to maturity with the IPCI calculation dates removed), which
//!   the engine implements equally. Where that machinery places IPCI
//!   calculation dates on redemption dates (the corpus NAM shape,
//!   `IPANX = PRANX` with `IPCED` two cycles past the anchor — a shape no
//!   official fixture exercises), the `PR` netting of `IPAC` and the
//!   `IPCI` capitalization of the same accrual both apply; the strict
//!   `(u, v)` row would instead pay an `IP` event at every redemption
//!   date and leave `IPCI` a no-op there. The ambiguity is unpinned by
//!   the testbeds and documented on both sides; the engine keeps the
//!   fixture-pinned machinery.
//! - The ANN annuity amount (techspec section "Annuity Amount Function")
//!   is implemented from the spec formula
//!   `A = (n + a) x prod g_i / (1 + sum_i prod_{j>=i} g_j)` over the
//!   remaining redemption schedule `u_1..u_m` with `g_i = 1 + r x Y(u_i,
//!   u_i+1)`; the algebraically equivalent form
//!   `A = (n + a) / (1 + sum_{k=1..m-1} 1/G_k)` with
//!   `G_k = prod_{j=1..k} g_j` is what the reference implementation
//!   computes and what this oracle implements. The fixture-pinned
//!   refinements: the numerator scale is `|NT + IPAC + grown|` where
//!   `grown` accrues the live state to the next redemption time (the
//!   same-date `IP` pays the interest portion separately), the rate is the
//!   one in effect at the fixing time (future resets are not
//!   anticipated), a fixing before `IED` resolves the annuity from the
//!   attribute values without growth (ann09), and fixings exist only when
//!   the `PRNXT` attribute is absent (one the day before the first
//!   redemption emission, ann23, and one per rate reset date, ann15 —
//!   evaluated after the reset by the dictionary sequence). With the
//!   attribute present the payment stays constant (every recalculation
//!   fixture is `PRNXT`-less).

use chrono::{Datelike, NaiveDate, NaiveDateTime, Weekday};
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractRole, ContractTerms, ContractType, Cycle, CyclePeriod,
    DayCountConvention, EndOfMonthConvention, EventType,
};

/// Money fixed point: nano currency units.
const MONEY_DIGITS: u32 = 9;
/// Rate fixed point: 1e-18 rate units.
const RATE_DIGITS: u32 = 18;

const RATE_ONE: i128 = 1_000_000_000_000_000_000;
const YEAR_ONE: i128 = 1_000_000_000_000_000_000;

/// One oracle event: dictionary event type, emission time, payoff and the
/// post-event `NT`/`IPNR`/`IPAC`/`PRNXT` states, all numerics in their fixed
/// point scales. The returned stream is ordered by (event time, effective
/// sequence rank — see [`sequence_rank`]) per the techspec section "Event
/// Sequence"; state transitions themselves are applied in schedule
/// calculation order, so a reported event's post-event state can reflect
/// transitions of same-window events that sort after it by emission time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleEvent {
    /// Event type `k` (dictionary acronym via `EventType`).
    pub event_type: EventType,
    /// Emission time `t` (the shifted date under a shift convention).
    pub time: NaiveDateTime,
    /// Payoff `c`, money fixed point, role-signed.
    pub payoff: i128,
    /// Post-event notional principal `NT`, money fixed point, role-signed.
    pub notional: i128,
    /// Post-event nominal interest rate `IPNR`, rate fixed point.
    pub rate: i128,
    /// Post-event accrued interest `IPAC`, money fixed point.
    pub accrued: i128,
    /// Post-event next principal redemption payment `PRNXT`, money fixed
    /// point (unsigned for LAM, role-signed for NAM and ANN; zero for PAM).
    pub next_principal_redemption: i128,
}

/// Converts oracle money fixed point to `Decimal` for comparison and
/// diagnostics.
#[must_use]
pub fn money_to_decimal(value: i128) -> Decimal {
    Decimal::from_i128_with_scale(value, MONEY_DIGITS)
}

/// Converts oracle rate fixed point to `Decimal` for diagnostics.
#[must_use]
pub fn rate_to_decimal(value: i128) -> Decimal {
    Decimal::from_i128_with_scale(value, RATE_DIGITS)
}

/// Rescales a `Decimal` mantissa from its own decimal exponent to the
/// target fixed point exponent, truncating toward zero.
fn rescale(value: Decimal, target_digits: u32) -> Result<i128, String> {
    let mantissa = value.mantissa();
    let source_digits = value.scale();
    let scaled = if target_digits >= source_digits {
        checked_mul(mantissa, 10i128.pow(target_digits - source_digits))?
    } else {
        mantissa / 10i128.pow(source_digits - target_digits)
    };
    Ok(scaled)
}

fn checked_mul(left: i128, right: i128) -> Result<i128, String> {
    left.checked_mul(right)
        .ok_or_else(|| format!("fixed point overflow: {left} x {right}"))
}

fn checked_div(left: i128, right: i128) -> Result<i128, String> {
    left.checked_div(right)
        .ok_or_else(|| format!("fixed point division by zero: {left} / {right}"))
}

/// Converts a terms/observation decimal to money fixed point.
fn money_of(value: Decimal) -> Result<i128, String> {
    rescale(value, MONEY_DIGITS)
}

/// Converts a terms/observation decimal to rate fixed point.
fn rate_of(value: Decimal) -> Result<i128, String> {
    rescale(value, RATE_DIGITS)
}

/// Evaluates a PAM contract independently of the engine.
///
/// `risk` is the observed rate series for the terms' rate reset market
/// object, ascending by observation time and read as a step function (the
/// latest observation at or before the requested time applies).
pub fn evaluate_pam(
    terms: &ContractTerms,
    risk: &[(NaiveDateTime, Decimal)],
) -> Result<Vec<OracleEvent>, String> {
    evaluate_family(terms, risk, Family::Pam)
}

/// Evaluates a LAM contract independently of the engine (see
/// [`evaluate_pam`] for the risk series convention).
pub fn evaluate_lam(
    terms: &ContractTerms,
    risk: &[(NaiveDateTime, Decimal)],
) -> Result<Vec<OracleEvent>, String> {
    evaluate_family(terms, risk, Family::Lam)
}

/// Evaluates a NAM contract independently of the engine (see
/// [`evaluate_pam`] for the risk series convention).
pub fn evaluate_nam(
    terms: &ContractTerms,
    risk: &[(NaiveDateTime, Decimal)],
) -> Result<Vec<OracleEvent>, String> {
    evaluate_family(terms, risk, Family::Nam)
}

/// Evaluates an ANN contract independently of the engine (see
/// [`evaluate_pam`] for the risk series convention).
pub fn evaluate_ann(
    terms: &ContractTerms,
    risk: &[(NaiveDateTime, Decimal)],
) -> Result<Vec<OracleEvent>, String> {
    evaluate_family(terms, risk, Family::Ann)
}

/// The fixed-income contract family the oracle evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Pam,
    Lam,
    Nam,
    Ann,
}

impl Family {
    fn of(terms: &ContractTerms) -> Result<Family, String> {
        match terms.contract_type {
            ContractType::Pam => Ok(Family::Pam),
            ContractType::Lam => Ok(Family::Lam),
            ContractType::Nam => Ok(Family::Nam),
            ContractType::Ann => Ok(Family::Ann),
            other => Err(format!("oracle scope excludes {other}")),
        }
    }
}

/// Evaluates one fixed-income contract independently of the engine.
fn evaluate_family(
    terms: &ContractTerms,
    risk: &[(NaiveDateTime, Decimal)],
    family: Family,
) -> Result<Vec<OracleEvent>, String> {
    let resolved = Family::of(terms)?;
    if resolved != family {
        return Err(format!(
            "oracle scope is {family:?}, got {}",
            terms.contract_type
        ));
    }
    if terms.scaling_effect.is_some()
        || terms.notional_scaling_multiplier.is_some()
        || terms.interest_scaling_multiplier.is_some()
    {
        return Err("oracle scope excludes scaling terms".to_string());
    }
    if terms.purchase_date.is_some() || terms.termination_date.is_some() {
        return Err("oracle scope excludes purchase and termination".to_string());
    }
    let dcc = terms
        .day_count_convention
        .ok_or("dayCountConvention required")?;
    match dcc {
        DayCountConvention::A365 | DayCountConvention::A360 | DayCountConvention::ThirtyE360 => {}
        other => return Err(format!("oracle day count scope excludes {other}")),
    }
    let ied = terms
        .initial_exchange_date
        .ok_or("initialExchangeDate required")?;
    let maturity = terms.maturity_date.ok_or("maturityDate required")?;
    let t0 = terms
        .status_date
        .or(terms.contract_deal_date)
        .unwrap_or(ied);
    if ied < t0 {
        return Err("oracle scope excludes progressed contracts".to_string());
    }
    let amortizing = matches!(family, Family::Lam | Family::Nam | Family::Ann);
    let (pranx, prcl) = if amortizing {
        (
            Some(
                terms
                    .cycle_anchor_date_of_principal_redemption
                    .ok_or("cycleAnchorDateOfPrincipalRedemption required")?,
            ),
            Some(
                terms
                    .cycle_of_principal_redemption
                    .ok_or("cycleOfPrincipalRedemption required")?,
            ),
        )
    } else {
        (None, None)
    };
    if matches!(family, Family::Lam | Family::Nam)
        && terms.next_principal_redemption_payment.is_none()
    {
        return Err(
            "oracle scope requires nextPrincipalRedemptionPayment for LAM and NAM".to_string(),
        );
    }
    let eomc = terms
        .end_of_month_convention
        .unwrap_or(EndOfMonthConvention::Sd);
    let bdc = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let calendar = terms.calendar.unwrap_or(Calendar::Nc);
    let sgn: i128 = match terms.contract_role {
        Some(ContractRole::Rpl) => -1,
        _ => 1,
    };

    let redemption_times = pranx
        .zip(prcl.as_ref())
        .map(|(anchor, cycle)| {
            schedule_series(anchor, cycle, maturity, eomc, bdc, calendar)
                .into_iter()
                .map(|(unshifted, _)| unshifted)
                .collect::<Vec<NaiveDateTime>>()
        })
        .unwrap_or_default();

    let mut slots = vec![
        Slot {
            kind: Kind::InitialExchange,
            calc: ied,
            emit: ied,
        },
        Slot {
            kind: Kind::Maturity,
            calc: maturity,
            emit: maturity,
        },
    ];
    let mut first_redemption_emit = None;
    if let (Some(anchor), Some(cycle)) = (pranx.as_ref(), prcl.as_ref()) {
        for (unshifted, emit) in schedule_series(*anchor, cycle, maturity, eomc, bdc, calendar) {
            if unshifted == maturity {
                continue;
            }
            if first_redemption_emit.is_none() {
                first_redemption_emit = Some(emit);
            }
            slots.push(Slot {
                kind: Kind::PrincipalRedemption,
                calc: calculation_time(bdc, unshifted, emit),
                emit,
            });
        }
    }
    let mut resets = Vec::new();
    if terms.nominal_interest_rate.is_some() {
        interest_slots(terms, maturity, eomc, bdc, calendar, &mut slots)?;
        resets = reset_slots(terms, t0, maturity, eomc, bdc, calendar, &mut slots)?;
    }
    if family == Family::Ann && terms.next_principal_redemption_payment.is_none() {
        if let Some(first) = first_redemption_emit {
            let fixing = first - chrono::Duration::days(1);
            slots.push(Slot {
                kind: Kind::PrincipalFixing,
                calc: fixing,
                emit: fixing,
            });
        }
        for reset in &resets {
            slots.push(Slot {
                kind: Kind::PrincipalFixing,
                calc: reset.calc,
                emit: reset.emit,
            });
        }
    }
    slots.sort_by_key(|slot| (slot.calc, sequence_rank(slot.kind.event_type())));

    let initial_redemption = match family {
        Family::Ann => match terms.next_principal_redemption_payment {
            Some(prnxt) => sgn * money_of(prnxt)?,
            None => {
                sgn * annuity_amount(
                    terms,
                    dcc,
                    &redemption_times,
                    t0,
                    money_of(
                        terms
                            .notional_principal
                            .ok_or("notionalPrincipal required")?,
                    )?,
                    money_of(terms.accrued_interest.unwrap_or(Decimal::ZERO))?,
                    rate_of(terms.nominal_interest_rate.unwrap_or(Decimal::ZERO))?,
                    false,
                )?
            }
        },
        Family::Lam | Family::Nam => match terms.next_principal_redemption_payment {
            Some(prnxt) => money_of(prnxt)?,
            None => 0,
        },
        Family::Pam => 0,
    };
    let mut state = State {
        notional: 0,
        rate: 0,
        accrued: 0,
        next_principal_redemption: initial_redemption,
        status_date: t0,
    };
    let mut events = Vec::new();
    for slot in slots {
        let year = year_fraction(state.status_date, slot.calc, dcc)?;
        let accrual = interest(year, state.rate, state.notional)?;
        let payoff = match slot.kind {
            Kind::InitialExchange => {
                let notional = money_of(
                    terms
                        .notional_principal
                        .ok_or("notionalPrincipal required")?,
                )?;
                let premium = money_of(terms.premium_discount_at_ied.unwrap_or(Decimal::ZERO))?;
                state.notional = sgn * notional;
                state.rate = rate_of(terms.nominal_interest_rate.unwrap_or(Decimal::ZERO))?;
                state.accrued = initial_accrued(terms, ied, dcc, state.notional, state.rate)?;
                match family {
                    Family::Lam => {
                        state.next_principal_redemption = money_of(
                            terms
                                .next_principal_redemption_payment
                                .ok_or("nextPrincipalRedemptionPayment required")?,
                        )?;
                    }
                    Family::Nam => {
                        state.next_principal_redemption = sgn
                            * money_of(
                                terms
                                    .next_principal_redemption_payment
                                    .ok_or("nextPrincipalRedemptionPayment required")?,
                            )?;
                    }
                    _ => {}
                }
                sgn * -checked_add(notional, premium)?
            }
            Kind::PrincipalRedemption if family == Family::Lam => {
                state.accrued = checked_add(state.accrued, accrual)?;
                let redemption = state.next_principal_redemption;
                state.notional = checked_sub(state.notional, sgn * redemption)?;
                state.next_principal_redemption = redemption.min(state.notional.abs());
                sgn * redemption
            }
            Kind::PrincipalRedemption => {
                state.accrued = checked_add(state.accrued, accrual)?;
                let mut payoff = checked_sub(state.next_principal_redemption, state.accrued)?;
                if payoff.abs() > state.notional.abs() {
                    payoff = state.notional;
                }
                state.notional = checked_sub(state.notional, payoff)?;
                payoff
            }
            Kind::PrincipalFixing => {
                state.accrued = checked_add(state.accrued, accrual)?;
                state.next_principal_redemption = if slot.calc < ied {
                    sgn * annuity_amount(
                        terms,
                        dcc,
                        &redemption_times,
                        slot.calc,
                        money_of(
                            terms
                                .notional_principal
                                .ok_or("notionalPrincipal required")?,
                        )?,
                        money_of(terms.accrued_interest.unwrap_or(Decimal::ZERO))?,
                        rate_of(terms.nominal_interest_rate.unwrap_or(Decimal::ZERO))?,
                        false,
                    )?
                } else {
                    sgn * annuity_amount(
                        terms,
                        dcc,
                        &redemption_times,
                        slot.calc,
                        state.notional,
                        state.accrued,
                        state.rate,
                        true,
                    )?
                };
                0
            }
            Kind::Interest => {
                let payoff = checked_add(state.accrued, accrual)?;
                state.accrued = 0;
                payoff
            }
            Kind::Capitalization => {
                state.notional = checked_add(state.notional, checked_add(state.accrued, accrual)?)?;
                state.accrued = 0;
                0
            }
            Kind::Reset => {
                state.accrued = checked_add(state.accrued, accrual)?;
                let code = terms
                    .market_object_code_of_rate_reset
                    .as_deref()
                    .ok_or("marketObjectCodeOfRateReset required for resets")?;
                let observed = rate_of(observe(risk, code, slot.calc)?)?;
                let multiplier = rate_of(terms.rate_multiplier.unwrap_or(Decimal::ONE))?;
                let spread = rate_of(terms.rate_spread.unwrap_or(Decimal::ZERO))?;
                let updated = checked_div(checked_mul(observed, multiplier)?, RATE_ONE)?;
                state.rate = checked_add(updated, spread)?;
                0
            }
            Kind::ResetFixed => {
                state.accrued = checked_add(state.accrued, accrual)?;
                state.rate = rate_of(terms.next_reset_rate.ok_or("nextResetRate required")?)?;
                0
            }
            Kind::Maturity => {
                let payoff = checked_add(state.notional, state.accrued)?;
                state.notional = 0;
                state.accrued = 0;
                payoff
            }
        };
        state.status_date = slot.calc;
        if slot.emit >= ied {
            events.push(OracleEvent {
                event_type: slot.kind.event_type(),
                time: slot.emit,
                payoff,
                notional: state.notional,
                rate: state.rate,
                accrued: state.accrued,
                next_principal_redemption: state.next_principal_redemption,
            });
        }
    }
    events.sort_by(|left, right| {
        (left.time, sequence_rank(left.event_type))
            .cmp(&(right.time, sequence_rank(right.event_type)))
    });
    Ok(events)
}

fn checked_add(left: i128, right: i128) -> Result<i128, String> {
    left.checked_add(right)
        .ok_or_else(|| format!("fixed point overflow: {left} + {right}"))
}

fn checked_sub(left: i128, right: i128) -> Result<i128, String> {
    left.checked_sub(right)
        .ok_or_else(|| format!("fixed point overflow: {left} - {right}"))
}

/// The truncated rate-times-year-fraction product at rate fixed point
/// (`r x Y`, dimensionless at scale 1e-18).
fn weighted(year: i128, rate: i128) -> Result<i128, String> {
    checked_div(checked_mul(year, rate)?, RATE_ONE)
}

/// The effective same-timestamp sequence rank of an event type.
///
/// The [`EventType::priority`] dictionary values with the one deviation the
/// official testbeds pin inside this family: the annuity principal fixing
/// `PRF` evaluates after the same-timestamp rate reset (ann15, ann16 order
/// `PR, IP, RR/RRF, PRF`, because the fixing recalculates `PRNXT` from the
/// rate the reset has just set), deviating from the v1.4 dictionary
/// sequence 5 of `PRF`.
fn sequence_rank(event_type: EventType) -> u8 {
    match event_type {
        EventType::PrincipalPaymentAmountFixing => EventType::RateResetVariable.priority() + 1,
        other => other.priority(),
    }
}

/// The annuity amount function (techspec section "Annuity Amount Function")
/// over the remaining redemption schedule, in money fixed point.
///
/// `times` are the raw (unshifted) redemption schedule times including the
/// schedule end; the series strictly after `at` dimensions the payment as
/// `scale / (1 + sum_{k=1..m-1} 1/G_k)` with
/// `G_k = prod_{j=1..k} (1 + r x Y(u_j, u_j+1))`, algebraically the spec
/// form `(n + a) x prod g_i / (1 + sum_i prod_{j>=i} g_j)` (see the module
/// spec-readings section). With `grow_accrued` the accrued interest is
/// grown to the next redemption time at `rate` first; a fixing before the
/// initial exchange resolves the annuity from the caller-supplied
/// attribute values without growth. The result is unsigned.
#[allow(clippy::too_many_arguments)]
fn annuity_amount(
    terms: &ContractTerms,
    dcc: DayCountConvention,
    times: &[NaiveDateTime],
    at: NaiveDateTime,
    notional: i128,
    accrued: i128,
    rate: i128,
    grow_accrued: bool,
) -> Result<i128, String> {
    let remaining: Vec<NaiveDateTime> = times.iter().copied().filter(|time| *time > at).collect();
    let next = remaining
        .first()
        .ok_or_else(|| "annuity recalculation has no remaining redemption payments".to_string())?;
    let grown = if grow_accrued {
        interest(year_fraction(at, *next, dcc)?, rate, notional)?
    } else {
        0
    };
    let scale = checked_add(checked_add(notional, accrued)?, grown)?.abs();
    let mut weights = YEAR_ONE;
    let mut growth = RATE_ONE;
    for pair in remaining.windows(2) {
        let factor = checked_add(
            RATE_ONE,
            weighted(year_fraction(pair[0], pair[1], dcc)?, rate)?,
        )?;
        growth = checked_div(checked_mul(growth, factor)?, RATE_ONE)?;
        weights = checked_add(
            weights,
            checked_div(checked_mul(YEAR_ONE, YEAR_ONE)?, growth)?,
        )?;
    }
    if weights == 0 {
        return Err(format!(
            "annuity payment schedule carries no discount weights at {} for {}",
            at, terms.contract_type
        ));
    }
    checked_div(checked_mul(scale, YEAR_ONE)?, weights)
}

/// The `IPAC` state at `IED` (techspec PAM IED state transition): the terms
/// value when carried, otherwise the accrual over `Y(IPANX, IED)` when the
/// interest anchor precedes the initial exchange, otherwise zero.
fn initial_accrued(
    terms: &ContractTerms,
    ied: NaiveDateTime,
    dcc: DayCountConvention,
    notional: i128,
    rate: i128,
) -> Result<i128, String> {
    if let Some(carried) = terms.accrued_interest {
        return money_of(carried);
    }
    if let Some(anchor) = terms.cycle_anchor_date_of_interest_payment {
        if anchor < ied {
            let year = year_fraction(anchor, ied, dcc)?;
            return interest(year, rate, notional);
        }
    }
    Ok(0)
}

/// One schedule slot: a calculation time (state transition input) and an
/// emission time (event time); the two diverge only under shift
/// conventions that calculate before the shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    kind: Kind,
    calc: NaiveDateTime,
    emit: NaiveDateTime,
}

/// PAM/LAM/NAM/ANN event kinds mapped onto the dictionary event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InitialExchange,
    PrincipalRedemption,
    PrincipalFixing,
    Interest,
    Capitalization,
    Reset,
    ResetFixed,
    Maturity,
}

impl Kind {
    fn event_type(self) -> EventType {
        match self {
            Kind::InitialExchange => EventType::InitialExchange,
            Kind::PrincipalRedemption => EventType::PrincipalRedemption,
            Kind::PrincipalFixing => EventType::PrincipalPaymentAmountFixing,
            Kind::Interest => EventType::InterestPayment,
            Kind::Capitalization => EventType::InterestCapitalization,
            Kind::Reset => EventType::RateResetVariable,
            Kind::ResetFixed => EventType::RateResetFixed,
            Kind::Maturity => EventType::Maturity,
        }
    }
}

/// Interest calculation state (`NT`, `IPNR`, `IPAC`, `PRNXT`, `SD`), fixed
/// point.
struct State {
    notional: i128,
    rate: i128,
    accrued: i128,
    next_principal_redemption: i128,
    status_date: NaiveDateTime,
}

/// Appends the `IPCI` and `IP` series (techspec schedule rows IPCI/IP, PAM
/// shape shared by the amortizing types; see the module spec-readings
/// section for the fixture-pinned IP reading).
///
/// The IPCI series runs from the interest anchor to the capitalization end
/// date, whose date always terminates the series; the IP series runs from
/// the anchor to maturity with the IPCI calculation dates removed, so
/// capitalization replaces the interest payment while `IPCED` is in the
/// future.
fn interest_slots(
    terms: &ContractTerms,
    maturity: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    calendar: Calendar,
    slots: &mut Vec<Slot>,
) -> Result<(), String> {
    let anchor = terms
        .cycle_anchor_date_of_interest_payment
        .ok_or("oracle scope requires cycleAnchorDateOfInterestPayment")?;
    let cycle = terms
        .cycle_of_interest_payment
        .ok_or("oracle scope requires cycleOfInterestPayment")?;
    let mut capitalization_dates = Vec::new();
    if let Some(ipced) = terms.capitalization_end_date {
        if ipced < maturity {
            for (calc, emit) in capped_series(anchor, &cycle, ipced, eomc, bdc, calendar) {
                capitalization_dates.push(calc);
                slots.push(Slot {
                    kind: Kind::Capitalization,
                    calc: calculation_time(bdc, calc, emit),
                    emit,
                });
            }
        }
    }
    for (calc, emit) in schedule_series(anchor, &cycle, maturity, eomc, bdc, calendar) {
        if capitalization_dates.contains(&calc) {
            continue;
        }
        slots.push(Slot {
            kind: Kind::Interest,
            calc: calculation_time(bdc, calc, emit),
            emit,
        });
    }
    Ok(())
}

/// Appends the `RR` and `RRF` series (techspec schedule rows RR/RRF, "Same
/// as PAM" for the amortizing types) and returns the slots appended.
///
/// The reset series rolls from `RRANX` to maturity; its terminal element is
/// the maturity date itself and is not an event (see module reading notes).
/// With `RRNXT` carried, the first series point after the status date
/// becomes a fixed reset applying `RRNXT` instead of a market observation.
fn reset_slots(
    terms: &ContractTerms,
    t0: NaiveDateTime,
    maturity: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    calendar: Calendar,
    slots: &mut Vec<Slot>,
) -> Result<Vec<Slot>, String> {
    let anchor = match terms.cycle_anchor_date_of_rate_reset {
        Some(anchor) => anchor,
        None => return Ok(Vec::new()),
    };
    let cycle = match terms.cycle_of_rate_reset {
        Some(cycle) => cycle,
        None => return Ok(Vec::new()),
    };
    let mut series = schedule_series(anchor, &cycle, maturity, eomc, bdc, calendar);
    if series.last().map(|(_, emit)| *emit) == Some(maturity) {
        series.pop();
    }
    let mut fixed_pending = terms.next_reset_rate.is_some();
    let mut resets = Vec::new();
    for (calc, emit) in series {
        let kind = if fixed_pending && calc > t0 {
            fixed_pending = false;
            Kind::ResetFixed
        } else {
            Kind::Reset
        };
        let slot = Slot {
            kind,
            calc: calculation_time(bdc, calc, emit),
            emit,
        };
        slots.push(slot);
        resets.push(slot);
    }
    Ok(resets)
}

/// Whether the convention calculates before the shift (`CS*` family and
/// `NOS`); `SC*` conventions calculate after the shift (techspec section
/// "Business Day Shift Convention").
fn calculates_after_shift(bdc: BusinessDayConvention) -> bool {
    matches!(
        bdc,
        BusinessDayConvention::Scf
            | BusinessDayConvention::Scmf
            | BusinessDayConvention::Scp
            | BusinessDayConvention::Scmp
    )
}

/// The state transition input time of an event under `bdc`.
fn calculation_time(
    bdc: BusinessDayConvention,
    unshifted: NaiveDateTime,
    shifted: NaiveDateTime,
) -> NaiveDateTime {
    if calculates_after_shift(bdc) {
        shifted
    } else {
        unshifted
    }
}

/// The ACTUS schedule `S(anchor, cycle, termination)` with stub correction
/// and business day adjustment (techspec section "Schedule", section 3.1).
///
/// Rolls from the anchor while strictly before the termination date; an
/// overshooting roll is removed under a long last stub (the final period is
/// extended to the termination date), kept under a short last stub (the
/// final period is short); the termination date always terminates the
/// schedule and is never shifted. Returns unshifted calculation dates
/// paired with shifted emission dates.
fn schedule_series(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    calendar: Calendar,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    if termination <= anchor {
        return vec![(anchor, anchor)];
    }
    let mut rolled = vec![anchor];
    let mut index: u64 = 0;
    let overshot = loop {
        index += 1;
        let next = roll(anchor, index, cycle, eomc);
        if next >= termination {
            break next > termination;
        }
        rolled.push(next);
    };
    if overshot && is_long_stub(cycle) && rolled.len() > 1 {
        rolled.pop();
    }
    rolled.push(termination);
    let last = rolled.len() - 1;
    rolled
        .into_iter()
        .enumerate()
        .map(|(position, t)| {
            let emit = if position < last {
                shift(t, bdc, calendar)
            } else {
                t
            };
            (t, emit)
        })
        .collect()
}

/// The IPCI series `S(anchor, cycle, IPCED)`: rolls strictly before the
/// capitalization end date, which itself always terminates the series
/// (techspec PAM schedule row IPCI) and is never shifted. An `IPCED` at or
/// before the anchor leaves the capitalization end as the single element,
/// the testbed-pinned reading (lam22, nam19, ann14).
fn capped_series(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    capitalization_end: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    calendar: Calendar,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    if capitalization_end <= anchor {
        return vec![(capitalization_end, capitalization_end)];
    }
    let mut rolled = vec![anchor];
    let mut index: u64 = 0;
    loop {
        index += 1;
        let next = roll(anchor, index, cycle, eomc);
        if next >= capitalization_end {
            break;
        }
        rolled.push(next);
    }
    rolled.push(capitalization_end);
    let last = rolled.len() - 1;
    rolled
        .into_iter()
        .enumerate()
        .map(|(position, t)| {
            let emit = if position < last {
                shift(t, bdc, calendar)
            } else {
                t
            };
            (t, emit)
        })
        .collect()
}

/// Whether the cycle carries the long last stub indicator (dictionary
/// trailing digit `0`; `1` is the short last stub).
fn is_long_stub(cycle: &Cycle) -> bool {
    cycle.index() == 0
}

/// The `index`-th cycle increment from the anchor (`index` starts at 1),
/// with the end-of-month snapping rule applied (techspec sections "Schedule"
/// and "End Of Month Shift Convention").
fn roll(
    anchor: NaiveDateTime,
    index: u64,
    cycle: &Cycle,
    eomc: EndOfMonthConvention,
) -> NaiveDateTime {
    let steps = u32::try_from(index).unwrap_or(u32::MAX);
    match cycle.period() {
        CyclePeriod::Day => {
            anchor + chrono::Duration::days(i64::from(cycle.length()) * i64::from(steps))
        }
        CyclePeriod::Week => {
            anchor + chrono::Duration::days(i64::from(cycle.length()) * 7 * i64::from(steps))
        }
        CyclePeriod::Month => month_shift(
            anchor,
            u64::from(cycle.length()) * u64::from(steps),
            eomc,
            cycle,
        ),
        CyclePeriod::Year => month_shift(
            anchor,
            u64::from(cycle.length()) * 12 * u64::from(steps),
            eomc,
            cycle,
        ),
    }
}

/// Calendar month arithmetic from the anchor: the target year/month derive
/// from the anchor month plus the offset, the anchor day of month carries
/// over clamped to the target month length; under an active end-of-month
/// convention the result snaps to the last day of the target month.
///
/// The snapping rule activates only when the anchor is the last day of a
/// month with fewer than 31 days and the cycle is month or year based
/// (techspec section "End Of Month Shift Convention").
fn month_shift(
    anchor: NaiveDateTime,
    months: u64,
    eomc: EndOfMonthConvention,
    cycle: &Cycle,
) -> NaiveDateTime {
    let total = i64::from(anchor.year()) * 12 + i64::from(anchor.month() - 1) + months as i64;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) as u32) + 1;
    let month_based = matches!(cycle.period(), CyclePeriod::Month | CyclePeriod::Year);
    let snapping = month_based
        && eomc == EndOfMonthConvention::Eom
        && anchor.day() == days_in_month(i64::from(anchor.year()), anchor.month())
        && days_in_month(i64::from(anchor.year()), anchor.month()) < 31;
    if snapping {
        let day = days_in_month(year, month);
        midnight(year as i32, month, day)
    } else {
        let day = anchor.day().min(days_in_month(year, month));
        midnight(year as i32, month, day)
    }
}

fn midnight(year: i32, month: u32, day: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("oracle dates are representable")
        .and_hms_opt(0, 0, 0)
        .expect("midnight is representable")
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// The business day shift (techspec section "Business Day Shift
/// Convention"): `NOS` never shifts; following/preceding walk to the next
/// or previous business day of the calendar; the modified variants fall
/// back to the opposite direction when the shift would cross a month
/// boundary. `NC` counts every day a business day, `MF` Monday to Friday.
fn shift(t: NaiveDateTime, bdc: BusinessDayConvention, calendar: Calendar) -> NaiveDateTime {
    if bdc == BusinessDayConvention::Nos || is_business_day(t, calendar) {
        return t;
    }
    let original_month = t.month();
    match bdc {
        BusinessDayConvention::Scf | BusinessDayConvention::Csf => {
            walk(t, calendar, Direction::Following)
        }
        BusinessDayConvention::Scmf | BusinessDayConvention::Csmf => {
            modified(t, calendar, Direction::Following, original_month)
        }
        BusinessDayConvention::Scp | BusinessDayConvention::Csp => {
            walk(t, calendar, Direction::Preceding)
        }
        BusinessDayConvention::Scmp => modified(t, calendar, Direction::Preceding, original_month),
        BusinessDayConvention::Nos => t,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Following,
    Preceding,
}

fn modified(
    t: NaiveDateTime,
    calendar: Calendar,
    primary: Direction,
    original_month: u32,
) -> NaiveDateTime {
    let shifted = walk(t, calendar, primary);
    if shifted.month() != original_month {
        let fallback = match primary {
            Direction::Following => Direction::Preceding,
            Direction::Preceding => Direction::Following,
        };
        walk(t, calendar, fallback)
    } else {
        shifted
    }
}

fn walk(t: NaiveDateTime, calendar: Calendar, direction: Direction) -> NaiveDateTime {
    let mut candidate = t;
    loop {
        if is_business_day(candidate, calendar) {
            return candidate;
        }
        candidate = match direction {
            Direction::Following => candidate + chrono::Duration::days(1),
            Direction::Preceding => candidate - chrono::Duration::days(1),
        };
    }
}

fn is_business_day(t: NaiveDateTime, calendar: Calendar) -> bool {
    match calendar {
        Calendar::Nc => true,
        Calendar::Mf => !matches!(t.weekday(), Weekday::Sat | Weekday::Sun),
    }
}

/// The year fraction `Y(start, end, DCC)` for the covered conventions
/// (techspec section "Year Fraction Convention"; the convention formulas
/// are the standard act/365, act/360 and 30E/360 eurobond definitions).
/// Result in year fixed point; truncating.
fn year_fraction(
    start: NaiveDateTime,
    end: NaiveDateTime,
    dcc: DayCountConvention,
) -> Result<i128, String> {
    if end < start {
        return Err(format!("year fraction undefined: {start} > {end}"));
    }
    let days = i128::from((end - start).num_days());
    match dcc {
        DayCountConvention::A365 => checked_div(checked_mul(days, YEAR_ONE)?, 365),
        DayCountConvention::A360 => checked_div(checked_mul(days, YEAR_ONE)?, 360),
        DayCountConvention::ThirtyE360 => {
            let d1 = i128::from(start.day().min(30));
            let d2 = i128::from(end.day().min(30));
            let month_span = i64::from(end.year() - start.year()) * 12 + i64::from(end.month())
                - i64::from(start.month());
            let span = i128::from(month_span * 30) + d2 - d1;
            checked_div(checked_mul(span, YEAR_ONE)?, 360)
        }
        other => Err(format!("oracle day count scope excludes {other}")),
    }
}

/// The interest accrual `Y x IPNR x NT` in money fixed point; every
/// intermediate truncates toward zero (see the module numerical regime).
fn interest(year: i128, rate: i128, notional: i128) -> Result<i128, String> {
    checked_div(checked_mul(weighted(year, rate)?, notional)?, YEAR_ONE)
}

/// Step-function risk factor observation: the latest series point at or
/// before `at` (techspec section "Risk Factor Observer").
fn observe(
    risk: &[(NaiveDateTime, Decimal)],
    market_object_code: &str,
    at: NaiveDateTime,
) -> Result<Decimal, String> {
    risk.iter()
        .rev()
        .find(|(time, _)| *time <= at)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("risk factor {market_object_code} not observed at {at}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn dec(raw: &str) -> Decimal {
        Decimal::from_str(raw).expect("decimal")
    }

    #[test]
    fn year_fractions_follow_the_covered_conventions() {
        let a365 = year_fraction(
            t("2013-01-01T00:00:00"),
            t("2013-02-01T00:00:00"),
            DayCountConvention::A365,
        )
        .expect("a365");
        assert_eq!(a365, 31 * YEAR_ONE / 365);
        let a360 = year_fraction(
            t("2013-01-01T00:00:00"),
            t("2013-02-01T00:00:00"),
            DayCountConvention::A360,
        )
        .expect("a360");
        assert_eq!(a360, 31 * YEAR_ONE / 360);
        let thirty = year_fraction(
            t("2013-01-31T00:00:00"),
            t("2013-02-28T00:00:00"),
            DayCountConvention::ThirtyE360,
        )
        .expect("30e360");
        assert_eq!(thirty, 28 * YEAR_ONE / 360);
        let full = year_fraction(
            t("2013-01-01T00:00:00"),
            t("2014-01-01T00:00:00"),
            DayCountConvention::ThirtyE360,
        )
        .expect("30e360 full year");
        assert_eq!(full, YEAR_ONE);
    }

    #[test]
    fn long_stub_removes_the_overshooting_roll_and_short_keeps_it() {
        let long = schedule_series(
            t("2013-02-01T00:00:00"),
            &cycle_of(1, CyclePeriod::Month, 0),
            t("2013-11-15T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        let dates: Vec<NaiveDateTime> = long.iter().map(|(calc, _)| *calc).collect();
        assert_eq!(
            dates,
            vec![
                t("2013-02-01T00:00:00"),
                t("2013-03-01T00:00:00"),
                t("2013-04-01T00:00:00"),
                t("2013-05-01T00:00:00"),
                t("2013-06-01T00:00:00"),
                t("2013-07-01T00:00:00"),
                t("2013-08-01T00:00:00"),
                t("2013-09-01T00:00:00"),
                t("2013-10-01T00:00:00"),
                t("2013-11-15T00:00:00"),
            ]
        );
        let short = schedule_series(
            t("2013-01-31T00:00:00"),
            &cycle_of(2, CyclePeriod::Week, 1),
            t("2013-03-10T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        let short_dates: Vec<NaiveDateTime> = short.iter().map(|(calc, _)| *calc).collect();
        assert_eq!(
            short_dates,
            vec![
                t("2013-01-31T00:00:00"),
                t("2013-02-14T00:00:00"),
                t("2013-02-28T00:00:00"),
                t("2013-03-10T00:00:00"),
            ]
        );
    }

    fn cycle_of(length: u32, period: CyclePeriod, stub_digit: u8) -> Cycle {
        actus_model::Cycle::new(
            length,
            period,
            if stub_digit == 0 {
                actus_model::CycleStub::Long
            } else {
                actus_model::CycleStub::Short
            },
            stub_digit,
        )
        .expect("test cycle")
    }

    #[test]
    fn end_of_month_snaps_short_month_anchors_to_month_ends() {
        let rolled = schedule_series(
            t("2013-02-28T00:00:00"),
            &cycle_of(1, CyclePeriod::Month, 1),
            t("2013-07-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        let dates: Vec<NaiveDateTime> = rolled.iter().map(|(calc, _)| *calc).collect();
        assert_eq!(
            dates,
            vec![
                t("2013-02-28T00:00:00"),
                t("2013-03-31T00:00:00"),
                t("2013-04-30T00:00:00"),
                t("2013-05-31T00:00:00"),
                t("2013-06-30T00:00:00"),
                t("2013-07-01T00:00:00"),
            ]
        );
        let same_day = schedule_series(
            t("2013-02-28T00:00:00"),
            &cycle_of(1, CyclePeriod::Month, 1),
            t("2013-07-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        let same_day_dates: Vec<NaiveDateTime> = same_day.iter().map(|(calc, _)| *calc).collect();
        assert_eq!(
            same_day_dates,
            vec![
                t("2013-02-28T00:00:00"),
                t("2013-03-28T00:00:00"),
                t("2013-04-28T00:00:00"),
                t("2013-05-28T00:00:00"),
                t("2013-06-28T00:00:00"),
                t("2013-07-01T00:00:00"),
            ]
        );
    }

    #[test]
    fn shift_conventions_walk_the_calendar_as_specified() {
        let saturday = t("2013-03-02T00:00:00");
        assert_eq!(
            shift(saturday, BusinessDayConvention::Scf, Calendar::Mf),
            t("2013-03-04T00:00:00")
        );
        assert_eq!(
            shift(saturday, BusinessDayConvention::Scp, Calendar::Mf),
            t("2013-03-01T00:00:00")
        );
        let month_end_saturday = t("2013-03-30T00:00:00");
        assert_eq!(
            shift(
                month_end_saturday,
                BusinessDayConvention::Scmf,
                Calendar::Mf
            ),
            t("2013-03-29T00:00:00")
        );
        assert_eq!(
            shift(
                month_end_saturday,
                BusinessDayConvention::Scmp,
                Calendar::Mf
            ),
            t("2013-03-29T00:00:00")
        );
        let first_on_saturday = t("2013-06-01T00:00:00");
        assert_eq!(
            shift(first_on_saturday, BusinessDayConvention::Scp, Calendar::Mf),
            t("2013-05-31T00:00:00")
        );
        assert_eq!(
            shift(first_on_saturday, BusinessDayConvention::Scmp, Calendar::Mf),
            t("2013-06-03T00:00:00")
        );
        assert_eq!(
            shift(saturday, BusinessDayConvention::Nos, Calendar::Mf),
            saturday
        );
        assert_eq!(
            shift(saturday, BusinessDayConvention::Scf, Calendar::Nc),
            saturday
        );
    }

    #[test]
    fn accrual_truncates_in_nano_units() {
        let year = YEAR_ONE / 2;
        let accrued =
            interest(year, rate_of(dec("0.1")).expect("rate"), 1_000_000_000_000).expect("accrual");
        assert_eq!(accrued, 50_000_000_000);
    }

    fn dec_oracle_terms(contract_type: ContractType) -> ContractTerms {
        let mut terms = ContractTerms::new(contract_type);
        terms.contract_role = Some(ContractRole::Rpa);
        terms.status_date = Some(t("2030-01-01T00:00:00"));
        terms.initial_exchange_date = Some(t("2030-01-01T00:00:00"));
        terms.notional_principal = Some(dec("1000"));
        terms.nominal_interest_rate = Some(dec("0.12"));
        terms.day_count_convention = Some(DayCountConvention::A365);
        terms.end_of_month_convention = Some(EndOfMonthConvention::Sd);
        terms.calendar = Some(Calendar::Nc);
        terms.business_day_convention = Some(BusinessDayConvention::Nos);
        terms.currency = Some("USD".to_string());
        terms
    }

    fn amortizing_terms(contract_type: ContractType, maturity: &str) -> ContractTerms {
        let mut terms = dec_oracle_terms(contract_type);
        terms.maturity_date = Some(t(maturity));
        terms.cycle_anchor_date_of_principal_redemption = Some(t("2030-02-01T00:00:00"));
        terms.cycle_of_principal_redemption = Some(cycle_of(1, CyclePeriod::Month, 0));
        terms.cycle_anchor_date_of_interest_payment = Some(t("2030-02-01T00:00:00"));
        terms.cycle_of_interest_payment = Some(cycle_of(1, CyclePeriod::Month, 0));
        terms
    }

    fn event_of<'a>(events: &'a [OracleEvent], event_type: EventType, at: &str) -> &'a OracleEvent {
        let time = t(at);
        events
            .iter()
            .find(|event| event.event_type == event_type && event.time == time)
            .unwrap_or_else(|| panic!("no {event_type} at {at} in {events:?}"))
    }

    #[test]
    fn lam_redemption_reduces_the_notional_and_keeps_the_payment() {
        let mut terms = amortizing_terms(ContractType::Lam, "2030-04-01T00:00:00");
        terms.next_principal_redemption_payment = Some(dec("100"));
        let events = evaluate_lam(&terms, &[]).expect("lam evaluation");
        assert_eq!(events.len(), 7, "IED, PR/IP x2, IP at MD, MD: {events:?}");
        let ied = event_of(&events, EventType::InitialExchange, "2030-01-01T00:00:00");
        assert_eq!(ied.payoff, -1_000_000_000_000);
        assert_eq!(ied.notional, 1_000_000_000_000);
        assert_eq!(ied.next_principal_redemption, 100_000_000_000);
        let pr = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-02-01T00:00:00",
        );
        assert_eq!(pr.payoff, 100_000_000_000);
        assert_eq!(pr.notional, 900_000_000_000);
        assert_eq!(pr.accrued, 10_191_780_821);
        assert_eq!(pr.next_principal_redemption, 100_000_000_000);
        let ip = event_of(&events, EventType::InterestPayment, "2030-02-01T00:00:00");
        assert_eq!(ip.payoff, 10_191_780_821);
        assert_eq!(ip.accrued, 0);
        let pr2 = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-03-01T00:00:00",
        );
        assert_eq!(pr2.accrued, 8_284_931_506);
        assert_eq!(pr2.notional, 800_000_000_000);
        let md_ip = event_of(&events, EventType::InterestPayment, "2030-04-01T00:00:00");
        assert_eq!(md_ip.payoff, 8_153_424_657);
        let md = event_of(&events, EventType::Maturity, "2030-04-01T00:00:00");
        assert_eq!(md.payoff, 800_000_000_000);
        assert_eq!(md.notional, 0);
    }

    #[test]
    fn lam_redemption_caps_at_the_exhausted_notional() {
        let mut terms = amortizing_terms(ContractType::Lam, "2030-05-01T00:00:00");
        terms.notional_principal = Some(dec("100"));
        terms.next_principal_redemption_payment = Some(dec("60"));
        let events = evaluate_lam(&terms, &[]).expect("lam evaluation");
        let pr = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-02-01T00:00:00",
        );
        assert_eq!(pr.payoff, 60_000_000_000);
        assert_eq!(pr.notional, 40_000_000_000);
        assert_eq!(pr.next_principal_redemption, 40_000_000_000);
        let pr2 = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-03-01T00:00:00",
        );
        assert_eq!(pr2.payoff, 40_000_000_000);
        assert_eq!(pr2.notional, 0);
        assert_eq!(pr2.next_principal_redemption, 0);
        let pr3 = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-04-01T00:00:00",
        );
        assert_eq!(pr3.payoff, 0);
        assert_eq!(pr3.accrued, 0);
        let md = event_of(&events, EventType::Maturity, "2030-05-01T00:00:00");
        assert_eq!(md.payoff, 0);
    }

    #[test]
    fn nam_redemption_nets_the_accrued_interest_without_recalculating() {
        let mut terms = amortizing_terms(ContractType::Nam, "2030-04-01T00:00:00");
        terms.next_principal_redemption_payment = Some(dec("10"));
        let events = evaluate_nam(&terms, &[]).expect("nam evaluation");
        assert_eq!(events.len(), 7, "IED, PR/IP x2, IP at MD, MD: {events:?}");
        let ied = event_of(&events, EventType::InitialExchange, "2030-01-01T00:00:00");
        assert_eq!(ied.next_principal_redemption, 10_000_000_000);
        let pr = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-02-01T00:00:00",
        );
        assert_eq!(pr.accrued, 10_191_780_821);
        assert_eq!(pr.payoff, -191_780_821);
        assert_eq!(pr.notional, 1_000_191_780_821);
        assert_eq!(pr.next_principal_redemption, 10_000_000_000);
        let pr2 = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-03-01T00:00:00",
        );
        assert_eq!(pr2.accrued, 9_207_244_886);
        assert_eq!(pr2.payoff, 792_755_114);
        assert_eq!(pr2.notional, 999_399_025_707);
        let md = event_of(&events, EventType::Maturity, "2030-04-01T00:00:00");
        assert_eq!(md.payoff, 999_399_025_707);
    }

    #[test]
    fn ann_fixing_dimensions_the_annuity_over_the_remaining_schedule() {
        let mut terms = amortizing_terms(ContractType::Ann, "2030-04-01T00:00:00");
        terms.notional_principal = Some(dec("1200"));
        let events = evaluate_ann(&terms, &[]).expect("ann evaluation");
        assert_eq!(
            events.len(),
            8,
            "IED, PRF, PR/IP x2, IP at MD, MD: {events:?}"
        );
        let ied = event_of(&events, EventType::InitialExchange, "2030-01-01T00:00:00");
        assert_eq!(ied.payoff, -1_200_000_000_000);
        assert_eq!(
            ied.next_principal_redemption, 403_801_108_102,
            "annuity at t0 over the ungrown attribute values"
        );
        let prf = event_of(
            &events,
            EventType::PrincipalPaymentAmountFixing,
            "2030-01-31T00:00:00",
        );
        assert_eq!(prf.payoff, 0);
        assert_eq!(prf.accrued, 11_835_616_438);
        assert_eq!(
            prf.next_principal_redemption, 407_916_560_491,
            "1200 plus the 1d growth to the next redemption, over 1 + 1/G1 + 1/G2 with G over 28d/31d"
        );
        let pr = event_of(
            &events,
            EventType::PrincipalRedemption,
            "2030-02-01T00:00:00",
        );
        assert_eq!(pr.accrued, 12_230_136_985);
        assert_eq!(pr.payoff, 395_686_423_506);
        assert_eq!(pr.notional, 804_313_576_494);
        let ip = event_of(&events, EventType::InterestPayment, "2030-02-01T00:00:00");
        assert_eq!(ip.payoff, 12_230_136_985);
        let md = event_of(&events, EventType::Maturity, "2030-04-01T00:00:00");
        assert_eq!(md.notional, 0);
    }
}
