//! Property-based invariants of the ACTUS engine (Layer 4).
//!
//! These are IMPLEMENTATION INVARIANTS, explicitly NOT conformance tests: the
//! official testbeds (run by `crates/actus-conformance`) pin the engine to the
//! upstream Java reference on fixed inputs, while the properties here assert
//! facts that must hold for whole generated families of contracts, including
//! inputs no testbed exercises. A property failure is an engine defect even
//! when every conformance case passes, and a conformance regression cannot be
//! excused by these tests passing.
//!
//! Properties:
//!
//! - `principal_is_conserved`: for fully amortized PAM/LAM/NAM/ANN with no
//!   scaling, rate resets, capitalization or lifecycle dates, the sum of all
//!   `PR` payoffs plus the `MD` payoff equals the initial notional (relative
//!   tolerance 1e-6). The identity is the principal ledger in payoff form:
//!   every `PR` decreases `NT` by exactly its payoff and `MD` redeems the
//!   remainder, so the sum telescopes to the post-`IED` notional; it is
//!   exact modulo `Decimal` division drift because the same-timestamp `IP`
//!   (dictionary sequence 8) pays the period accrual before `MD`
//!   (sequence 19), leaving the `MD` payoff pure principal.
//! - `outstanding_principal_stays_non_negative`: post-event `NT` never
//!   crosses zero in the wrong direction; only `PR` changes `NT` between
//!   `IED` and `MD`, and redemptions never grow the outstanding principal.
//!   For NAM the generator floors `PRNXT` at 6% of notional, above the
//!   maximum per-period interest (rate <= 0.2 times the largest quarterly
//!   year fraction 92/360 gives <= 5.11%): the engine legitimately realizes
//!   negative amortization when the period payment does not cover the period
//!   interest (nam17), which violates this property by design, not by
//!   defect. The redemption cap (`pof/stf PR NAM` resolved against
//!   ann13/lam25) floors `NT` at exactly zero once the notional is
//!   exhausted.
//! - `maturity_zeroes_the_outstanding_principal`: the `MD` post-state has
//!   `NT == 0` and `IPAC == 0` exactly and status `Matured`.
//! - `event_schedule_is_monotonic`: raw schedule facts on the generated
//!   terms (`IED <= MD`, cycle anchors within `[IED, MD]`) and the evaluated
//!   event times non-decreasing, opening with `IED` and closing with `MD`.
//! - `evaluation_is_deterministic`: two evaluations of the same terms
//!   produce identical event vectors (`Decimal` exact equality).
//! - `pam_interest_payment_zeroes_accrued_interest`: every `IP` post-state
//!   carries `IPAC == 0` exactly (state-transition consistency: the `IP`
//!   transition pays and resets the accrual in one step).
//! - `annuity_amount_matches_the_equal_period_closed_form`: the pure
//!   [`actus_engine::ann::annuity_amount`] function matches the closed-form
//!   equal-period annuity `A = NT x i / (1 - (1 + i)^-m)` with
//!   `i = r x Y` per period (relative tolerance 1e-9). The closed form
//!   requires equal period day counts, so the property uses synthetic
//!   30-day periods under `A365`; unequal day counts (calendar months)
//!   break the closed form and are not exercised here.
//! - `shifted_capitalization_dates_remove_the_interest_payment`: a pinned
//!   regression for the one defect the randomized differential corpus has
//!   exposed so far (structural, deterministic — not generated).
//!
//! Case reduction: every property draws bounded integers and flags and maps
//! them through [`amortizing_case`], a pure constructor of valid
//! `ContractTerms`, so shrinking walks the same valid region of the input
//! space. No `prop_assume` filters are used: the generator's whole space is
//! admissible by construction, because over-filtering hides bugs.

use chrono::{Duration, Months, NaiveDate, NaiveDateTime};
use proptest::prelude::*;
use proptest::test_runner::Config;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use actus_engine::ann::annuity_amount;
use actus_engine::risk::StateProvider;
use actus_engine::state::ContractStatus;
use actus_engine::EngineRegistry;
use actus_model::{
    BusinessDayConvention, Calendar, ContractRole, ContractTerms, ContractType, Cycle, CyclePeriod,
    CycleStub, DayCountConvention, EndOfMonthConvention, EventType,
};

/// Relative tolerance of the principal conservation identity.
const CONSERVATION_TOLERANCE: Decimal = dec!(0.000001);

/// Relative tolerance of the annuity closed-form comparison.
const CLOSED_FORM_TOLERANCE: Decimal = dec!(0.000000001);

/// Proptest run configuration: case counts are capped so the whole suite
/// stays well under a minute; shrinking behaviour is left at the default.
fn property_config() -> Config {
    Config {
        cases: 256,
        ..Config::default()
    }
}

/// One generated fully amortized contract plus the facts the properties
/// assert about the raw terms.
#[derive(Debug, Clone)]
struct AmortizingCase {
    terms: ContractTerms,
    notional: Decimal,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
    interest_anchor: NaiveDateTime,
    principal_anchor: Option<NaiveDateTime>,
}

/// The engine registry mirroring the conformance harness registration.
fn registry() -> EngineRegistry {
    let mut engine_registry = EngineRegistry::new();
    engine_registry.register(Box::new(actus_engine::pam::PamEngine));
    engine_registry.register(Box::new(actus_engine::lam::LamEngine));
    engine_registry.register(Box::new(actus_engine::nam::NamEngine));
    engine_registry.register(Box::new(actus_engine::ann::AnnEngine));
    engine_registry
}

/// The proptest input tuple consumed by [`amortizing_case`].
type AmortizingInput = (u8, u32, u32, u32, u32, bool, u8);

/// The proptest strategy over [`AmortizingInput`].
fn amortizing_strategy() -> impl Strategy<Value = AmortizingInput> {
    (
        0u8..4,
        0u32..100_000,
        0u32..20_000,
        0u32..1_826,
        1u32..=10,
        any::<bool>(),
        0u8..6,
    )
}

/// Maps bounded proptest inputs onto valid fully amortized terms.
///
/// Notional in `[1000, 10^7]`, rate in `[0, 0.2]`, `IED` in 2020-2024,
/// maturity 1-10 years after `IED`, monthly or quarterly cycles, `RPA` role,
/// interest anchored at `IED` and principal anchored one period later. `IED`
/// precedes the status date by 30 days, so contracts never evaluate on the
/// progressed path. `nam_payment_floor` lifts the NAM payment to at least 6%
/// of notional, above the maximum per-period interest of 5.11% (rate 0.2
/// times the largest quarterly year fraction 92/360), excluding the
/// legitimate negative-amortization region (nam17) from the non-negative
/// property.
fn amortizing_case(
    (type_seed, notional_raw, rate_milli, ied_days, maturity_years, quarterly, dcc_seed): AmortizingInput,
    nam_payment_floor: bool,
) -> AmortizingCase {
    let contract_type = match type_seed {
        0 => ContractType::Pam,
        1 => ContractType::Lam,
        2 => ContractType::Nam,
        _ => ContractType::Ann,
    };
    let day_count = match dcc_seed {
        0 => DayCountConvention::A365,
        1 => DayCountConvention::A360,
        2 => DayCountConvention::Aa,
        3 => DayCountConvention::ThirtyE360,
        4 => DayCountConvention::ThirtyE360Isda,
        _ => DayCountConvention::TwentyEightE336,
    };
    let notional = Decimal::from(1000 + u64::from(notional_raw) * 100);
    let rate = Decimal::new(i64::from(rate_milli), 5);
    let ied = NaiveDate::from_ymd_opt(2020, 1, 1)
        .expect("fixed calendar date")
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        + Duration::days(i64::from(ied_days));
    let year_months = maturity_years * 12;
    let maturity = ied
        .checked_add_months(Months::new(year_months))
        .expect("maturity within the representable calendar");
    let period_months = if quarterly { 3 } else { 1 };
    let periods = year_months / period_months;
    let interest_anchor = ied;
    let principal_anchor = if contract_type == ContractType::Pam {
        None
    } else {
        Some(
            ied.checked_add_months(Months::new(period_months))
                .expect("principal anchor within the representable calendar"),
        )
    };

    let mut terms = ContractTerms::new(contract_type);
    terms.contract_role = Some(ContractRole::Rpa);
    terms.status_date = Some(ied - Duration::days(30));
    terms.initial_exchange_date = Some(ied);
    terms.maturity_date = Some(maturity);
    terms.notional_principal = Some(notional);
    terms.nominal_interest_rate = Some(rate);
    terms.day_count_convention = Some(day_count);
    terms.end_of_month_convention = Some(EndOfMonthConvention::Sd);
    terms.business_day_convention = Some(BusinessDayConvention::Nos);
    terms.calendar = Some(Calendar::Nc);
    terms.cycle_anchor_date_of_interest_payment = Some(interest_anchor);
    terms.cycle_of_interest_payment = Some(month_cycle(period_months));
    if let Some(principal_anchor) = principal_anchor {
        terms.cycle_anchor_date_of_principal_redemption = Some(principal_anchor);
        terms.cycle_of_principal_redemption = Some(month_cycle(period_months));
        if contract_type == ContractType::Lam {
            terms.next_principal_redemption_payment = Some(notional / Decimal::from(periods));
        }
        if contract_type == ContractType::Nam {
            let payment = notional / Decimal::from(periods);
            let floor = notional * Decimal::new(6, 2);
            terms.next_principal_redemption_payment = Some(if nam_payment_floor {
                payment.max(floor)
            } else {
                payment
            });
        }
    }
    AmortizingCase {
        terms,
        notional,
        ied,
        maturity,
        interest_anchor,
        principal_anchor,
    }
}

/// A monthly `P<n>M` long-first cycle (dictionary `P<n>ML0`).
fn month_cycle(period_months: u32) -> Cycle {
    Cycle::new(period_months, CyclePeriod::Month, CycleStub::Long, 0)
        .expect("monthly cycle is well formed")
}

/// Evaluates generated terms through the full engine registry.
fn evaluate(case: &AmortizingCase) -> Vec<actus_engine::ContractEvent> {
    registry()
        .evaluate(&case.terms, &StateProvider::new())
        .expect("generated amortizing terms evaluate without error")
}

/// Whether `actual` matches `expected` within a relative Decimal tolerance.
fn within_relative(actual: Decimal, expected: Decimal, relative: Decimal) -> bool {
    let scale = actual.abs().max(expected.abs());
    (actual - expected).abs() <= relative * scale
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn principal_is_conserved(
        input in amortizing_strategy(),
    ) {
        let case = amortizing_case(input, false);
        let events = evaluate(&case);
        let first = events.first().expect("the schedule opens with the initial exchange");
        prop_assert_eq!(first.event_type, EventType::InitialExchange);
        prop_assert_eq!(first.state.notional_principal, case.notional);
        let redemption_total: Decimal = events
            .iter()
            .filter(|event| event.event_type == EventType::PrincipalRedemption)
            .map(|event| event.payoff)
            .sum();
        let last = events.last().expect("the schedule closes with maturity");
        prop_assert_eq!(last.event_type, EventType::Maturity);
        let conserved = redemption_total + last.payoff;
        prop_assert!(
            within_relative(conserved, case.notional, CONSERVATION_TOLERANCE),
            "principal not conserved: redemptions {redemption_total} + maturity payoff {} \
             vs notional {}",
            last.payoff,
            case.notional
        );
    }

    #[test]
    fn outstanding_principal_stays_non_negative(
        input in amortizing_strategy(),
    ) {
        let case = amortizing_case(input, true);
        let events = evaluate(&case);
        let mut previous = events
            .first()
            .expect("the schedule opens with the initial exchange")
            .state
            .notional_principal;
        prop_assert_eq!(previous, case.notional);
        for event in &events[1..] {
            let current = event.state.notional_principal;
            prop_assert!(
                current >= Decimal::ZERO,
                "outstanding principal crossed zero at {}: {}",
                event.time,
                current
            );
            if event.event_type == EventType::PrincipalRedemption {
                prop_assert!(
                    current <= previous,
                    "redemption grew the outstanding principal at {}: {previous} -> {current}",
                    event.time
                );
            } else if event.event_type != EventType::Maturity {
                prop_assert_eq!(current, previous, "only principal events change NT at {}", event.time);
            }
            previous = current;
        }
    }

    #[test]
    fn maturity_zeroes_the_outstanding_principal(
        input in amortizing_strategy(),
    ) {
        let case = amortizing_case(input, false);
        let events = evaluate(&case);
        let last = events.last().expect("the schedule closes with maturity");
        prop_assert_eq!(last.event_type, EventType::Maturity);
        prop_assert_eq!(last.state.notional_principal, Decimal::ZERO);
        prop_assert_eq!(last.state.accrued_interest, Decimal::ZERO);
        prop_assert_eq!(last.state.contract_status, ContractStatus::Matured);
    }

    #[test]
    fn event_schedule_is_monotonic(
        input in amortizing_strategy(),
    ) {
        let case = amortizing_case(input, false);
        prop_assert!(case.ied <= case.maturity, "maturity must not precede IED");
        prop_assert!(
            case.interest_anchor >= case.ied && case.interest_anchor <= case.maturity,
            "interest anchor must lie in [IED, MD]"
        );
        if let Some(principal_anchor) = case.principal_anchor {
            prop_assert!(
                principal_anchor >= case.ied && principal_anchor <= case.maturity,
                "principal anchor must lie in [IED, MD]"
            );
        }
        let events = evaluate(&case);
        for pair in events.windows(2) {
            prop_assert!(
                pair[0].time <= pair[1].time,
                "event times decreased: {} after {}",
                pair[1].time,
                pair[0].time
            );
        }
        prop_assert_eq!(
            events.first().expect("non-empty schedule").event_type,
            EventType::InitialExchange
        );
        prop_assert_eq!(
            events.last().expect("non-empty schedule").event_type,
            EventType::Maturity
        );
    }

    #[test]
    fn evaluation_is_deterministic(
        input in amortizing_strategy(),
    ) {
        let case = amortizing_case(input, false);
        let first_run = evaluate(&case);
        let second_run = evaluate(&case);
        prop_assert_eq!(first_run, second_run);
    }

    #[test]
    fn pam_interest_payment_zeroes_accrued_interest(
        notional_raw in 0u32..100_000,
        rate_milli in 0u32..20_000,
        ied_days in 0u32..1_826,
        maturity_years in 1u32..=10,
        quarterly in any::<bool>(),
        dcc_seed in 0u8..6,
    ) {
        let case = amortizing_case(
            (0, notional_raw, rate_milli, ied_days, maturity_years, quarterly, dcc_seed),
            false,
        );
        let events = evaluate(&case);
        for event in &events {
            if event.event_type == EventType::InterestPayment {
                prop_assert_eq!(
                    event.state.accrued_interest,
                    Decimal::ZERO,
                    "IP at {} left accrued interest behind",
                    event.time
                );
            }
        }
    }

    #[test]
    fn annuity_amount_matches_the_equal_period_closed_form(
        notional_raw in 0u32..100_000,
        rate_milli in 0u32..20_000,
        payment_count in 1u32..=120,
    ) {
        let notional = Decimal::from(1000 + u64::from(notional_raw) * 100);
        let rate = Decimal::new(i64::from(rate_milli), 5);
        let start = NaiveDate::from_ymd_opt(2020, 1, 1)
            .expect("fixed calendar date")
            .and_hms_opt(0, 0, 0)
            .expect("midnight");
        let payments: Vec<NaiveDateTime> = (1..=payment_count)
            .map(|index| start + Duration::days(30 * i64::from(index)))
            .collect();
        let amount = annuity_amount(
            notional,
            rate,
            start,
            &payments,
            DayCountConvention::A365,
        )
        .expect("equal 30 day periods form a valid day count range");
        let period_rate = rate * Decimal::from(30) / Decimal::from(365);
        let expected = if period_rate == Decimal::ZERO {
            notional / Decimal::from(payment_count)
        } else {
            let mut growth = Decimal::ONE;
            for _ in 0..payment_count {
                growth *= Decimal::ONE + period_rate;
            }
            notional * period_rate * growth / (growth - Decimal::ONE)
        };
        prop_assert!(
            within_relative(amount, expected, CLOSED_FORM_TOLERANCE),
            "annuity {amount} diverges from the closed form {expected} \
             (notional {notional}, rate {rate}, payments {payment_count})"
        );
    }
}

/// Midnight of a fixed calendar date.
fn datetime(year: i32, month: u32, day: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("fixed calendar date")
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
}

/// Regression test for the `SC*` `IPCI`/`IP` schedule defect the randomized
/// differential corpus exposed (`crates/actus-conformance`,
/// `tests/differential.rs`): the `IP` series deduplicated against the
/// capitalization dates using shifted emission dates while the `IPCI`
/// series carried unshifted calculation times, so a business-day-shifted
/// capitalization date left a duplicate `IP` event in the schedule (which
/// paid the period accrual out) and the `IPCI` at `IPCED` capitalized only
/// the two-day tail. Per `docs/actus.md` section 3.4 ("calculation of the
/// event happens after the shift") both series now calculate on the shifted
/// date and deduplicate on the unshifted schedule dates.
#[test]
fn shifted_capitalization_dates_remove_the_interest_payment() {
    let mut terms = ContractTerms::new(ContractType::Pam);
    terms.contract_role = Some(ContractRole::Rpa);
    terms.status_date = Some(datetime(2030, 10, 4));
    terms.initial_exchange_date = Some(datetime(2030, 10, 27));
    terms.maturity_date = Some(datetime(2032, 3, 27));
    terms.notional_principal = Some(dec!(1000000));
    terms.nominal_interest_rate = Some(dec!(0.1));
    terms.day_count_convention = Some(DayCountConvention::A360);
    terms.end_of_month_convention = Some(EndOfMonthConvention::Sd);
    terms.business_day_convention = Some(BusinessDayConvention::Scmp);
    terms.calendar = Some(Calendar::Mf);
    terms.cycle_anchor_date_of_interest_payment = Some(datetime(2030, 10, 27));
    terms.cycle_of_interest_payment =
        Some(Cycle::new(3, CyclePeriod::Month, CycleStub::Short, 1).expect("quarterly cycle"));
    terms.capitalization_end_date = Some(datetime(2031, 4, 27));

    let events = registry()
        .evaluate(&terms, &StateProvider::new())
        .expect("terms evaluate without error");

    // 2030-10-27 is a Sunday: the anchor point shifts to Friday 2030-10-25.
    // That emission precedes the initial exchange date, so the event mutates
    // the state but is not reported (the ann09 pre-IED non-reporting
    // reading); in particular the defective duplicate `IP` next to the
    // capitalization is no longer observable at this date either. The
    // deduplication property stays pinned by the shifted April roll and the
    // capitalization growth assertions below, which sit after the initial
    // exchange.
    let anchor: Vec<_> = events
        .iter()
        .filter(|event| event.time == datetime(2030, 10, 25))
        .collect();
    assert_eq!(anchor.len(), 0, "events at the shifted anchor: {anchor:?}");
    assert!(
        events
            .iter()
            .all(|event| event.time >= datetime(2030, 10, 27)),
        "no event is emitted before the initial exchange date"
    );

    // 2031-04-27 (the capitalization end) is also a Sunday: no `IP` at the
    // shifted roll 2031-04-25, and the `IPCI` sits at the unshifted schedule
    // end (the terminal element is never shifted).
    assert!(!events.iter().any(|event| {
        event.event_type == EventType::InterestPayment && event.time == datetime(2031, 4, 25)
    }));
    let april = events
        .iter()
        .find(|event| {
            event.event_type == EventType::InterestCapitalization
                && event.time == datetime(2031, 4, 27)
        })
        .expect("capitalization at the unshifted IPCED");
    assert_eq!(april.state.accrued_interest, Decimal::ZERO);

    // The April capitalization folds a full quarter of interest (90 days,
    // A360, on the capitalized notional) instead of the defective two-day
    // tail: the notional grows from the January capitalization by far more
    // than a tail-only capitalization would add.
    let january = events
        .iter()
        .find(|event| {
            event.event_type == EventType::InterestCapitalization
                && event.time == datetime(2031, 1, 27)
        })
        .expect("capitalization at the January roll");
    let growth = april.state.notional_principal.abs() - january.state.notional_principal.abs();
    assert!(
        growth > dec!(20000),
        "capitalization added only {growth} — the accrual was paid out by a duplicate IP"
    );
}
