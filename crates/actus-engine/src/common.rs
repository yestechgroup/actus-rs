//! Primitives shared by the contract type implementations (extracted from
//! the PAM implementation; the LAM and NAM implementations reuse them
//! unchanged).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractRole, ContractTerms, Cycle, EndOfMonthConvention,
    InterestCalculationBase,
};

use crate::schedule::{cycle_step, generate_schedule, shift_business_day};
use crate::state::ContractState;

/// The contract role sign (techspec section "Contract Role Sign
/// Convention").
///
/// `RPL` mirrors `RPA`: the notional state and the payoffs flip sign. All
/// other roles act as `RPA` in the fixed income testbeds.
pub fn role_sign(terms: &ContractTerms) -> Decimal {
    match terms.contract_role {
        Some(ContractRole::Rpl) => -Decimal::ONE,
        _ => Decimal::ONE,
    }
}

/// Whether the business day convention calculates before shifting (`CS*`).
///
/// `CS*` conventions calculate the payoff on the unshifted schedule date and
/// emit the event at the shifted date; `SC*` conventions shift first and
/// calculate on the shifted date (techspec section "Business Day Shift
/// Convention").
pub fn calculate_first(bdc: BusinessDayConvention) -> bool {
    matches!(
        bdc,
        BusinessDayConvention::Csf | BusinessDayConvention::Csmf | BusinessDayConvention::Csp
    )
}

/// The i-th unrolled cycle increment from the anchor (i starts at 1) with
/// the End Of Month Shift Convention applied.
pub fn roll(
    anchor: NaiveDateTime,
    index: u64,
    cycle: &Cycle,
    eomc: EndOfMonthConvention,
) -> NaiveDateTime {
    cycle_step(anchor, index, cycle, eomc)
}

/// Unrolls one cyclic series into (calculation time, emission time) pairs
/// where the schedule end always belongs to the schedule.
///
/// The series rolls from `anchor` by `cycle` while the rolled date is
/// strictly before `termination`, then appends `termination` without stub
/// correction and without business day shifting (the techspec schedule
/// element `t_m = T` is not adjusted). All other dates are shifted per `bdc`
/// into the emission time, keeping the unshifted date as calculation time, so
/// `CS*` conventions calculate on the raw schedule date and emit at the
/// shifted date. A termination at or before the anchor yields the
/// single-element schedule `[termination]`: the schedule end belongs to the
/// schedule even when the anchor rolled past it.
pub fn anchored_series(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    if termination <= anchor {
        return vec![(termination, termination)];
    }
    let mut calc = vec![anchor];
    let mut index: u64 = 0;
    loop {
        index += 1;
        let next = roll(anchor, index, cycle, eomc);
        if next >= termination {
            break;
        }
        calc.push(next);
    }
    calc.push(termination);
    let last = calc.len() - 1;
    calc.into_iter()
        .enumerate()
        .map(|(position, t)| {
            let emit = if position < last {
                shift_business_day(t, bdc, cal)
            } else {
                t
            };
            (t, emit)
        })
        .collect()
}

/// The state transition input time of one schedule element under `bdc`.
///
/// `CS*` conventions calculate on the unshifted schedule date and emit at
/// the shifted date; `SC*` conventions shift first and calculate on the
/// shifted date (techspec section "Business Day Shift Convention"). The two
/// times coincide whenever the element is a business day.
pub fn calculation_time(
    bdc: BusinessDayConvention,
    unshifted: NaiveDateTime,
    shifted: NaiveDateTime,
) -> NaiveDateTime {
    if calculate_first(bdc) {
        unshifted
    } else {
        shifted
    }
}

/// Unrolls one cyclic series with the cycle stub indicator applied into
/// (unshifted schedule date, emission time) pairs (techspec section
/// "Schedule").
///
/// The unshifted dates come from the schedule generated under `NOS`, the
/// emission times from the schedule generated under `bdc`; the two schedules
/// pair positionally because the business day shift moves each rolled date
/// independently and never moves the schedule end. The unshifted dates are
/// the identity keys of the schedule elements: series that must not
/// duplicate a date (the `IP` series against the `IPCI` capitalization
/// dates) deduplicate on them, independent of the convention.
pub fn stub_series_unshifted(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let unshifted = generate_schedule(
        anchor,
        cycle,
        termination,
        eomc,
        BusinessDayConvention::Nos,
        cal,
    );
    let emit = generate_schedule(anchor, cycle, termination, eomc, bdc, cal);
    unshifted.into_iter().zip(emit).collect()
}

/// Unrolls one cyclic series into (calculation time, emission time) pairs
/// with the cycle stub indicator applied (techspec section "Schedule").
///
/// The shared schedule primitive generates the emission times: a long last
/// stub (dictionary `0`) removes the overshooting roll so the final period is
/// extended to `termination`, a short last stub (`1`) keeps it and appends
/// the termination. Under `CS*` conventions the calculation times are the
/// unshifted schedule (generated with `NOS`) paired with the shifted emission
/// times; every other convention calculates where it emits.
pub fn stub_series(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    stub_series_unshifted(anchor, cycle, termination, eomc, bdc, cal)
        .into_iter()
        .map(|(unshifted, shifted)| (calculation_time(bdc, unshifted, shifted), shifted))
        .collect()
}

/// Whether the interest calculation base amount tracks the notional.
///
/// `NT` (and, per the reference behaviour the testbed fixtures expose, the
/// otherwise static `NTIED`) re-fix the base amount to the notional at every
/// principal redemption; `NTL` keeps the fixed amount until an `IPCB`
/// schedule point re-fixes it (techspec LAM states table, `Ipcb`).
pub fn base_tracks_notional(terms: &ContractTerms) -> bool {
    !matches!(
        terms.interest_calculation_base,
        Some(InterestCalculationBase::Ntl)
    )
}

/// Whether the interest calculation base semantics is `NT` (the default).
///
/// `NT` re-fixes the base amount to the notional at `IED`, `IPCI` and (per
/// the reference behaviour the testbed fixtures expose) tracks it through
/// the `PR` series together with `NTIED`.
pub fn base_is_nt(terms: &ContractTerms) -> bool {
    matches!(
        terms.interest_calculation_base,
        None | Some(InterestCalculationBase::Nt)
    )
}

/// The scaling multipliers carried by the terms (`SCIP`/`SCNT`), defaulting
/// to the unscaled unit multiplier (techspec states tables, `Isc`/`Nsc`).
pub fn term_scaling_multipliers(terms: &ContractTerms) -> (Decimal, Decimal) {
    (
        terms.interest_scaling_multiplier.unwrap_or(Decimal::ONE),
        terms.notional_scaling_multiplier.unwrap_or(Decimal::ONE),
    )
}

/// The interest calculation base amount at `IED` (techspec states tables,
/// `Ipcb`): the signed notional for `NT` semantics (the default), otherwise
/// the signed terms amount.
pub fn initial_base_amount(terms: &ContractTerms, notional: Decimal, sgn: Decimal) -> Decimal {
    match terms.interest_calculation_base {
        None | Some(InterestCalculationBase::Nt) => sgn * notional,
        Some(_) => sgn * terms.interest_calculation_base_amount.unwrap_or(notional),
    }
}

/// The `PR` payoff and state transition shared by NAM and ANN (techspec
/// NAM functions table, PR row: `pof/stf PR NAM`).
///
/// The payoff is `Nsc x (Prnxt - Ipac(t+))` after the period accrual has
/// been added to `Ipac` by the caller, capped at the remaining notional
/// magnitude: ann13 redeems only the outstanding principal once the fixed
/// annuity exceeds it and pays zero redemptions afterwards, while every NAM
/// fixture is unaffected because their net redemptions never exceed the
/// notional (nam17's negative redemptions pass the cap unchanged). The
/// notional reduces by the payoff and the `NT`/`NTIED` base semantics
/// re-fixes the calculation base to the reduced notional. `Prnxt` is never
/// recalculated here (nam17 pays negative redemptions while the notional
/// grows).
pub fn apply_principal_redemption(
    state: &mut ContractState,
    base_tracks_notional: bool,
) -> Decimal {
    let payoff = state.notional_scaling_multiplier
        * (state.next_principal_redemption_payment - state.accrued_interest);
    let payoff = if payoff.abs() > state.notional_principal.abs() {
        state.notional_principal
    } else {
        payoff
    };
    state.notional_principal -= payoff;
    if base_tracks_notional {
        state.interest_calculation_base_amount = state.notional_principal;
    }
    payoff
}
