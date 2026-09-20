//! Deterministic randomized contract corpus for differential testing.
//!
//! Generates valid, randomized [`ContractTerms`] for the fixed-income family
//! (PAM, LAM, NAM, ANN) plus the risk factor observation series a floating
//! rate contract needs, from a single `u64` seed. Generation is fully
//! in-memory and deterministic (splitmix64, no external RNG dependency): the
//! same seed always produces the same case list, which the regression test
//! pins as a hash so accidental corpus drift is caught.
//!
//! ## Coverage (per the differential plan)
//!
//! - fixed and floating rates (`RR` market observations with multiplier and
//!   spread, plus `RRNXT` fixed resets);
//! - with and without capitalization (`IPCED` on PAM and NAM);
//! - `RPA` and `RPL` contract roles;
//! - `EOM` and `SD` end-of-month conventions (techspec section "End Of Month
//!   Shift Convention");
//! - `NC` and `MF` calendars with the shift/calculate business day
//!   conventions (`NOS`, `SCF`, `SCMF`, `CSF`, `CSMF`, `SCP`, `SCMP`);
//! - day counts `A365`, `A360`, `30E360`;
//! - long (`0`) and short (`1`) last stubs on all cycles;
//! - `PDIED` premium/discount and carried `IPAC` on a subset of cases.
//!
//! ## Exclusions (v1, deliberate and documented)
//!
//! - scaling (`SCEF`/`SCIP`/`SCNT`/`SCIXSD`) — the oracle does not model the
//!   `SC` event in v1;
//! - fees (`FER`/`FECL`/`FEANX`/`FEAC`/`FEB`), prepayment and penalty
//!   (`OPANX`/`OPCL`/`PPEF`/`PYRT`/`PYTP`), purchase/termination
//!   (`PRD`/`PPRD`/`TD`/`PTD`) and progressed contracts (`IED` < `SD`) —
//!   outside the v1 PAM oracle scope;
//! - zero-coupon terms (`IPNR` absent) and `fixingDays`;
//! - day counts `Aa`, `30E360ISDA`, `28E336`;
//! - interest calculation base semantics other than the `NT` default.
//!
//! PAM capitalization cases anchor the interest schedule on a day of month
//! at most 28 (never February 28/29), so `IPCED` is an exact unclamped roll
//! of the interest cycle under either end-of-month convention; this keeps
//! the two readings of the techspec IP-schedule anchor (section 7.1 builds
//! the IP series from `IPED`-anchored rolls, the engine anchors on `IPANX`
//! and removes the IPCI calculation dates) exactly equivalent.
//!
//! LAM/NAM/ANN cases are generated valid (attribute shape mirrors the
//! vendored testbeds). ANN cases carry the `PRNXT` attribute on roughly 60
//! percent of the slice (a constant payment, the ann01-style shape) and omit
//! it on the rest (the ann09/ann15-style shape), so the oracle v2 gate
//! exercises the techspec annuity amount function through the reference `PRF`
//! fixing series on the `PRNXT`-less share. No other mix change was needed:
//! the amortization date balloon horizon (`AD`) stays out of the corpus
//! because the annuity function is fully testable dimensioned to the
//! maturity attribute, and the `AD` balloon path remains covered by the
//! official ann12 fixture.
//!
//! The NAM shape places the capitalization dates on redemption dates
//! (`IPANX = PRANX`, `IPCED` two cycles past the anchor) — a contract shape
//! no official fixture exercises. The v2 differential gate covers it; the
//! spec ambiguity it exposes (whether an `IP` event fires next to the
//! redemption at a capitalization date) is resolved through the
//! fixture-pinned replacement machinery and documented in the oracle module
//! docs and the engine's LAM/NAM/ANN reading notes.

use std::fmt;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractRole, ContractTerms, ContractType, Cycle, CyclePeriod,
    CycleStub, DayCountConvention, EndOfMonthConvention,
};

/// Seed of the pinned regression corpus (mnemonic spelling of "ACTUS" within
/// the hex alphabet).
pub const CORPUS_SEED: u64 = 0xAC705;

/// Total number of cases the differential gate generates; case `i % 4 == 0`
/// is a PAM case, so this yields 512 PAM cases for the v1 oracle gate.
pub const DIFFERENTIAL_CASES: usize = 2048;

/// One generated corpus case: contract terms plus the observed risk factor
/// series (empty for fixed rate terms).
#[derive(Debug, Clone)]
pub struct CorpusCase {
    /// Stable case identifier, e.g. `corpus-0007-pam`.
    pub id: String,
    /// Generated contract terms.
    pub terms: ContractTerms,
    /// Observed rate series for the terms' `marketObjectCodeOfRateReset`
    /// (ascending by observation time; empty when the rate is fixed).
    pub risk_factors: Vec<(NaiveDateTime, Decimal)>,
}

/// FNV-1a 64 hash over the serialized corpus (id, terms, risk factors).
///
/// The pinned value in [`corpus_is_byte_stable_for_the_pinned_seed`] makes
/// any change to generation logic or corpus composition visible in review.
#[must_use]
pub fn corpus_hash(cases: &[CorpusCase]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for case in cases {
        feed(&mut hash, case.id.as_bytes());
        feed(&mut hash, &[0x00]);
        feed(
            &mut hash,
            serde_json::to_vec(&case.terms)
                .expect("terms serialize")
                .as_slice(),
        );
        feed(&mut hash, &[0x01]);
        for (time, value) in &case.risk_factors {
            feed(&mut hash, time.to_string().as_bytes());
            feed(&mut hash, &[0x02]);
            feed(
                &mut hash,
                serde_json::to_vec(value)
                    .expect("decimal serializes")
                    .as_slice(),
            );
        }
        feed(&mut hash, &[0x03]);
    }
    hash
}

fn feed(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Generates `count` deterministic corpus cases from `seed`.
///
/// Case kinds cycle PAM, LAM, NAM, ANN; every generator consumes the shared
/// splitmix64 stream in a fixed order, so the output is a pure function of
/// `(seed, count)`.
#[must_use]
pub fn generate(seed: u64, count: usize) -> Vec<CorpusCase> {
    let mut rng = SplitMix64::new(seed);
    let mut cases = Vec::with_capacity(count);
    for index in 0..count {
        let contract_type = match index % 4 {
            0 => ContractType::Pam,
            1 => ContractType::Lam,
            2 => ContractType::Nam,
            _ => ContractType::Ann,
        };
        let (terms, risk_factors) = match contract_type {
            ContractType::Pam => build_pam(&mut rng),
            ContractType::Lam => build_amortizing(&mut rng, ContractType::Lam),
            ContractType::Nam => build_amortizing(&mut rng, ContractType::Nam),
            _ => build_amortizing(&mut rng, ContractType::Ann),
        };
        cases.push(CorpusCase {
            id: format!("corpus-{index:04}-{}", ContractTypeName(contract_type)),
            terms,
            risk_factors,
        });
    }
    cases
}

/// `fmt::Display` adapter for contract type names inside case identifiers.
struct ContractTypeName(ContractType);

impl fmt::Display for ContractTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ContractType::Pam => f.write_str("pam"),
            ContractType::Lam => f.write_str("lam"),
            ContractType::Nam => f.write_str("nam"),
            ContractType::Ann => f.write_str("ann"),
            other => write!(f, "{other}"),
        }
    }
}

/// splitmix64 pseudo-random stream (Steele, Lea, Flood 2014); the corpus
/// RNG of record — small, dependency-free and fully deterministic.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let index = self.below(items.len() as u64) as usize;
        &items[index]
    }

    fn flip(&mut self) -> bool {
        self.below(2) == 1
    }

    fn decimal(&mut self, mantissa_lo: i64, mantissa_hi: i64, scale: u32) -> Decimal {
        Decimal::new(
            self.range(mantissa_lo as u64, mantissa_hi as u64) as i64,
            scale,
        )
    }
}

fn midnight(year: i32, month: u32, day: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("corpus dates are representable")
        .and_hms_opt(0, 0, 0)
        .expect("midnight is representable")
}

fn add_days(t: NaiveDateTime, days: i64) -> NaiveDateTime {
    t + Duration::days(days)
}

/// Calendar month addition from the anchor (day clamped to the target month
/// length); used for anchor placement only, never for schedule rolls.
fn add_months(t: NaiveDateTime, months: u32) -> NaiveDateTime {
    let total = i64::from(t.year()) * 12 + i64::from(t.month() - 1) + i64::from(months);
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) as u32) + 1;
    let day = t.day().min(days_in_month(year, month));
    midnight(year as i32, month, day)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        _ => 28,
    }
}

fn is_leap(year: i64) -> bool {
    let year = year as i32;
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn months_of(period: CyclePeriod, length: u32) -> u32 {
    match period {
        CyclePeriod::Year => length * 12,
        CyclePeriod::Month => length,
        CyclePeriod::Week => length * 7 / 4,
        CyclePeriod::Day => length,
    }
}

fn cycle_of(length: u32, period: CyclePeriod, stub_digit: u8) -> Cycle {
    let stub = if stub_digit == 0 {
        CycleStub::Long
    } else {
        CycleStub::Short
    };
    Cycle::new(length, period, stub, stub_digit).expect("corpus cycles are well formed")
}

/// Builds one randomized PAM case (techspec section "PAM: Principal At
/// Maturity" attribute set, within the documented corpus exclusions).
fn build_pam(rng: &mut SplitMix64) -> (ContractTerms, Vec<(NaiveDateTime, Decimal)>) {
    let ied = add_days(midnight(2030, 1, 1), rng.range(0, 1400) as i64);
    let status_date = add_days(ied, -(rng.range(15, 24) as i64));
    let maturity = add_months(ied, rng.range(8, 46) as u32);
    let mut terms = base_terms(rng, ContractType::Pam, status_date, ied, maturity);
    interest_schedule_block(rng, &mut terms, ied, maturity);
    premium_discount_block(rng, &mut terms);
    rate_reset_assignment(rng, &mut terms, ied, maturity)
}

/// Builds the `IPANX`/`IPCL`/`IPCED` block of a PAM case.
///
/// Without capitalization the anchor sits on the initial exchange date or a
/// few days before it (exercising the `Y(IPANX, IED)` accrued interest of
/// the IED state transition) and always after the status date, so no
/// schedule point precedes the analysis window. With capitalization the
/// anchor's day of month is forced to 28 or below (and off February 28/29)
/// so that `IPCED` is an exact unclamped roll of the interest cycle under
/// both readings of the techspec anchor rule.
fn interest_schedule_block(
    rng: &mut SplitMix64,
    terms: &mut ContractTerms,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
) {
    let capitalizing = rng.below(100) < 30;
    let (length, period) = if capitalizing {
        *rng.pick(&[(1u32, CyclePeriod::Month), (3u32, CyclePeriod::Month)])
    } else {
        *rng.pick(&[
            (1u32, CyclePeriod::Month),
            (3u32, CyclePeriod::Month),
            (6u32, CyclePeriod::Month),
            (1u32, CyclePeriod::Year),
        ])
    };
    let stub = rng.below(2) as u8;
    terms.cycle_of_interest_payment = Some(cycle_of(length, period, stub));

    let mut anchor = if capitalizing {
        let day = ied.day().min(28);
        let safe_day = if ied.month() == 2 && day >= 28 {
            27
        } else {
            day
        };
        midnight(ied.year(), ied.month(), safe_day)
    } else if rng.flip() {
        ied
    } else {
        add_days(ied, -(rng.range(1, 10) as i64))
    };
    if capitalizing && anchor >= maturity {
        anchor = ied;
    }
    terms.cycle_anchor_date_of_interest_payment = Some(anchor);

    if capitalizing {
        let cycle_months = months_of(period, length);
        for rolls in [2u32, 3] {
            let capitalization_end = add_months(anchor, rolls * cycle_months);
            if capitalization_end < maturity {
                terms.capitalization_end_date = Some(capitalization_end);
                break;
            }
        }
    }

    if (capitalization_is_active(terms) || anchor >= ied) && rng.below(100) < 15 {
        let notional = terms.notional_principal.expect("notional set");
        let rate = terms.nominal_interest_rate.expect("rate set");
        terms.accrued_interest =
            Some((notional * rate * Decimal::new(rng.range(0, 1_000) as i64, 7)).round_dp(6));
    }
}

/// Whether the terms activate the IPCI series (`IPCED` strictly before
/// maturity, matching the engine's activation condition).
fn capitalization_is_active(terms: &ContractTerms) -> bool {
    match (terms.capitalization_end_date, terms.maturity_date) {
        (Some(ipced), Some(md)) => ipced < md,
        _ => false,
    }
}

/// Adds a `PDIED` premium or discount on roughly 40 percent of the cases.
fn premium_discount_block(rng: &mut SplitMix64, terms: &mut ContractTerms) {
    if rng.below(100) < 40 {
        let notional = terms.notional_principal.expect("notional set");
        let fraction = Decimal::new(rng.range(100, 500) as i64, 4);
        let signed = if rng.flip() { fraction } else { -fraction };
        terms.premium_discount_at_ied = Some((notional * signed).round_dp(2));
    }
}

/// Assigns the floating rate `RR` block and observation series, or leaves
/// the terms fixed rate with an empty series.
fn rate_reset_assignment(
    rng: &mut SplitMix64,
    terms: &mut ContractTerms,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
) -> (ContractTerms, Vec<(NaiveDateTime, Decimal)>) {
    if !rng.flip() {
        return (terms.clone(), Vec::new());
    }
    let (length, period) = *rng.pick(&[
        (3u32, CyclePeriod::Month),
        (6u32, CyclePeriod::Month),
        (1u32, CyclePeriod::Year),
    ]);
    let stub = rng.below(2) as u8;
    let cycle = cycle_of(length, period, stub);
    let offset = (rng.below(2) as u32) * months_of(period, length);
    let mut anchor = add_months(ied, offset);
    if anchor >= maturity {
        anchor = ied;
    }
    terms.cycle_anchor_date_of_rate_reset = Some(anchor);
    terms.cycle_of_rate_reset = Some(cycle);
    terms.market_object_code_of_rate_reset = Some("CORPUS-RATE-RF".to_string());
    if rng.flip() {
        terms.rate_multiplier =
            Some(*rng.pick(&[Decimal::new(75, 2), Decimal::ONE, Decimal::new(125, 2)]));
    }
    if rng.flip() {
        terms.rate_spread =
            Some(*rng.pick(&[Decimal::new(5, 4), Decimal::new(1, 2), Decimal::new(125, 4)]));
    }
    if rng.below(100) < 34 {
        terms.next_reset_rate = Some(rng.decimal(100, 2_000, 4));
    }
    let observations = observation_series(rng, ied, maturity);
    (terms.clone(), observations)
}

/// The observed rate series of a floating case: a 30-day grid starting a
/// week before the initial exchange date and ending a week after maturity,
/// so every shift-adjusted reset calculation time (a preceding shift can
/// move an event before the initial exchange date) finds an observation at
/// or before itself under the step-function lookup.
fn observation_series(
    rng: &mut SplitMix64,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
) -> Vec<(NaiveDateTime, Decimal)> {
    let mut series = Vec::new();
    let mut at = add_days(ied, -7);
    let horizon = add_days(maturity, 7);
    while at <= horizon {
        let basis_points = 300 + rng.range(0, 900);
        series.push((at, Decimal::new(basis_points as i64, 4)));
        at = add_days(at, 30);
    }
    series
}

/// Shared attribute block of every corpus case: dates, role, notional,
/// rate, day count, conventions and currency.
fn base_terms(
    rng: &mut SplitMix64,
    contract_type: ContractType,
    status_date: NaiveDateTime,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
) -> ContractTerms {
    let mut terms = ContractTerms::new(contract_type);
    terms.contract_role = Some(*rng.pick(&[ContractRole::Rpa, ContractRole::Rpl]));
    terms.status_date = Some(status_date);
    terms.contract_deal_date = Some(add_days(status_date, -2));
    terms.initial_exchange_date = Some(ied);
    terms.maturity_date = Some(maturity);
    terms.notional_principal = Some(Decimal::new(rng.range(1_000, 2_000_000) as i64, 0));
    terms.nominal_interest_rate = Some(rng.decimal(50, 2_000, 4));
    terms.day_count_convention = Some(*rng.pick(&[
        DayCountConvention::A365,
        DayCountConvention::A360,
        DayCountConvention::ThirtyE360,
    ]));
    if rng.flip() {
        terms.end_of_month_convention = Some(EndOfMonthConvention::Eom);
    } else {
        terms.end_of_month_convention = Some(EndOfMonthConvention::Sd);
    }
    if rng.flip() {
        terms.calendar = Some(Calendar::Nc);
        terms.business_day_convention = Some(BusinessDayConvention::Nos);
    } else {
        terms.calendar = Some(Calendar::Mf);
        terms.business_day_convention = Some(*rng.pick(&[
            BusinessDayConvention::Nos,
            BusinessDayConvention::Scf,
            BusinessDayConvention::Scmf,
            BusinessDayConvention::Csf,
            BusinessDayConvention::Csmf,
            BusinessDayConvention::Scp,
            BusinessDayConvention::Scmp,
        ]));
    }
    terms.currency = Some("USD".to_string());
    terms
}

/// Builds one randomized LAM, NAM or ANN case.
///
/// Attribute shape mirrors the amortizing testbeds: a principal redemption
/// schedule (`PRANX`/`PRCL`/`PRNXT`) with the interest cycle anchored on the
/// redemption anchor; NAM adds capitalization until `IPCED` (negative
/// amortization). These cases feed later oracle waves — the v1 differential
/// gate covers PAM.
fn build_amortizing(
    rng: &mut SplitMix64,
    contract_type: ContractType,
) -> (ContractTerms, Vec<(NaiveDateTime, Decimal)>) {
    let ied = add_days(midnight(2030, 1, 1), rng.range(0, 1400) as i64);
    let status_date = add_days(ied, -(rng.range(5, 20) as i64));
    let maturity = add_months(ied, rng.range(10, 36) as u32);
    let mut terms = base_terms(rng, contract_type, status_date, ied, maturity);

    let (length, period) = *rng.pick(&[(1u32, CyclePeriod::Month), (3u32, CyclePeriod::Month)]);
    let stub = rng.below(2) as u8;
    let redemption_cycle = cycle_of(length, period, stub);
    let redemption_anchor = add_months(ied, rng.range(1, 3) as u32);
    terms.cycle_anchor_date_of_principal_redemption = Some(redemption_anchor);
    terms.cycle_of_principal_redemption = Some(redemption_cycle);
    terms.cycle_anchor_date_of_interest_payment = Some(redemption_anchor);
    terms.cycle_of_interest_payment = Some(redemption_cycle);

    let total_months = (i64::from(maturity.year()) * 12 + i64::from(maturity.month()))
        - (i64::from(ied.year()) * 12 + i64::from(ied.month()));
    let notional = terms.notional_principal.expect("notional set");
    match contract_type {
        ContractType::Lam => {
            let periods = (total_months / i64::from(months_of(period, length))).max(1);
            terms.next_principal_redemption_payment =
                Some((notional / Decimal::from(periods)).round_dp(2));
        }
        ContractType::Nam => {
            let rate = terms.nominal_interest_rate.expect("rate set");
            let monthly_interest = notional * rate / Decimal::from(12);
            terms.next_principal_redemption_payment =
                Some((monthly_interest / Decimal::from(2)).round_dp(2));
            let capitalization_end = add_months(redemption_anchor, 2 * months_of(period, length));
            if capitalization_end < maturity {
                terms.capitalization_end_date = Some(capitalization_end);
            }
        }
        _ => {
            if rng.below(100) < 40 {
                terms.next_principal_redemption_payment =
                    Some((notional * Decimal::new(25, 3)).round_dp(2));
            }
        }
    }

    if !rng.flip() {
        return (terms, Vec::new());
    }
    let (anchor, cycle, market_object_code, multiplier, spread, next_reset) =
        rate_reset_attributes(rng, ied, maturity);
    terms.cycle_anchor_date_of_rate_reset = Some(anchor);
    terms.cycle_of_rate_reset = Some(cycle);
    terms.market_object_code_of_rate_reset = Some(market_object_code);
    if let Some(multiplier) = multiplier {
        terms.rate_multiplier = Some(multiplier);
    }
    if let Some(spread) = spread {
        terms.rate_spread = Some(spread);
    }
    if let Some(next_reset) = next_reset {
        terms.next_reset_rate = Some(next_reset);
    }
    let observations = observation_series(rng, ied, maturity);
    (terms, observations)
}

/// Builds the `RR` attributes of a floating rate case.
///
/// The anchor precedes maturity; `RRNXT` appears on roughly a third of the
/// floating cases so both `RR` and `RRF` events occur. Observations sit on a
/// 30-day grid from the initial exchange date through maturity; the engine's
/// [`actus_engine::StateProvider`] and the oracle's own lookup both read them
/// as step functions, so a value at or before every reset calculation time is
/// guaranteed.
fn rate_reset_attributes(
    rng: &mut SplitMix64,
    ied: NaiveDateTime,
    maturity: NaiveDateTime,
) -> (
    NaiveDateTime,
    Cycle,
    String,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
) {
    let (length, period) = *rng.pick(&[
        (3u32, CyclePeriod::Month),
        (6u32, CyclePeriod::Month),
        (1u32, CyclePeriod::Year),
    ]);
    let stub = rng.below(2) as u8;
    let cycle = cycle_of(length, period, stub);
    let offset = (rng.below(2) as u32) * months_of(period, length);
    let mut anchor = add_months(ied, offset);
    if anchor >= maturity {
        anchor = ied;
    }
    let multiplier = if rng.flip() {
        None
    } else {
        Some(*rng.pick(&[Decimal::new(75, 2), Decimal::ONE, Decimal::new(125, 2)]))
    };
    let spread = if rng.flip() {
        None
    } else {
        Some(*rng.pick(&[Decimal::new(5, 4), Decimal::new(1, 2), Decimal::new(125, 4)]))
    };
    let next_reset = if rng.below(100) < 34 {
        Some(rng.decimal(100, 2_000, 4))
    } else {
        None
    };
    (
        anchor,
        cycle,
        "CORPUS-RATE-RF".to_string(),
        multiplier,
        spread,
        next_reset,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn pinned_corpus() -> Vec<CorpusCase> {
        generate(CORPUS_SEED, DIFFERENTIAL_CASES)
    }

    fn provider_for(case: &CorpusCase) -> actus_engine::StateProvider {
        let mut provider = actus_engine::StateProvider::new();
        if let Some(code) = case.terms.market_object_code_of_rate_reset.as_deref() {
            for (time, value) in &case.risk_factors {
                provider = provider.with_rate(code, *time, *value);
            }
        }
        provider
    }

    #[test]
    fn corpus_is_byte_stable_for_the_pinned_seed() {
        let cases = pinned_corpus();
        assert_eq!(cases.len(), DIFFERENTIAL_CASES);
        assert_eq!(
            corpus_hash(&cases),
            0xA641_CA64_A549_7978,
            "corpus drift detected: regenerate the pinned hash only after reviewing the change"
        );
    }

    #[test]
    fn corpus_mixes_the_four_contract_types_and_unique_ids() {
        let cases = pinned_corpus();
        let mut counts: BTreeMap<ContractType, usize> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for case in &cases {
            *counts.entry(case.terms.contract_type).or_insert(0) += 1;
            assert!(ids.insert(case.id.clone()), "duplicate id {}", case.id);
        }
        assert_eq!(counts[&ContractType::Pam], 512);
        assert_eq!(counts[&ContractType::Lam], 512);
        assert_eq!(counts[&ContractType::Nam], 512);
        assert_eq!(counts[&ContractType::Ann], 512);
    }

    #[test]
    fn ann_slice_mixes_constant_and_derived_payments() {
        let with_attribute = pinned_corpus()
            .into_iter()
            .filter(|c| c.terms.contract_type == ContractType::Ann)
            .filter(|c| c.terms.next_principal_redemption_payment.is_some())
            .count();
        assert!(
            (150..=400).contains(&with_attribute),
            "ANN PRNXT attribute coverage: {with_attribute}"
        );
    }

    #[test]
    fn corpus_dates_are_sensible_and_terms_carry_the_documented_mix() {
        let cases = pinned_corpus();
        let mut floating = 0;
        let mut capitalizing = 0;
        let mut fixed_reset = 0;
        let mut premium_discount = 0;
        let mut eom = 0;
        let mut shifted_calendar = 0;
        let mut day_counts = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for case in &cases {
            let terms = &case.terms;
            let ied = terms.initial_exchange_date.expect("ied set");
            let maturity = terms.maturity_date.expect("maturity set");
            assert!(maturity > ied, "case {} maturity after ied", case.id);
            assert!(ied > terms.status_date.expect("status date set"));
            if terms.market_object_code_of_rate_reset.is_some() {
                floating += 1;
                assert!(
                    !case.risk_factors.is_empty(),
                    "floating case needs observations"
                );
                assert!(
                    case.risk_factors.windows(2).all(|w| w[0].0 < w[1].0),
                    "observation series ascending"
                );
            } else {
                assert!(case.risk_factors.is_empty());
            }
            if capitalization_is_active(terms) {
                capitalizing += 1;
            }
            if terms.next_reset_rate.is_some() {
                fixed_reset += 1;
            }
            if terms.premium_discount_at_ied.is_some() {
                premium_discount += 1;
            }
            if terms.end_of_month_convention == Some(EndOfMonthConvention::Eom) {
                eom += 1;
            }
            if terms.business_day_convention != Some(BusinessDayConvention::Nos) {
                assert_eq!(terms.calendar, Some(Calendar::Mf));
                shifted_calendar += 1;
            }
            day_counts.insert(terms.day_count_convention.expect("dcc set"));
            roles.insert(terms.contract_role.expect("role set"));
        }
        assert_eq!(day_counts.len(), 3);
        assert_eq!(roles.len(), 2);
        assert!(floating > 700, "floating coverage: {floating}");
        assert!(
            capitalizing > 150,
            "capitalization coverage: {capitalizing}"
        );
        assert!(fixed_reset > 100, "RRF coverage: {fixed_reset}");
        assert!(premium_discount > 100, "PDIED coverage: {premium_discount}");
        assert!(eom > 300, "EOM coverage: {eom}");
        assert!(
            shifted_calendar > 300,
            "business day shift coverage: {shifted_calendar}"
        );
    }

    #[test]
    fn pam_corpus_evaluates_through_the_engine_without_error() {
        let registry = crate::engine_registry();
        for case in pinned_corpus()
            .into_iter()
            .filter(|c| c.terms.contract_type == ContractType::Pam)
        {
            let provider = provider_for(&case);
            registry
                .evaluate(&case.terms, &provider)
                .unwrap_or_else(|e| panic!("case {} failed to evaluate: {e}", case.id));
        }
    }

    #[test]
    fn amortizing_corpus_evaluates_through_the_engine_without_error() {
        let registry = crate::engine_registry();
        for case in pinned_corpus()
            .into_iter()
            .filter(|c| c.terms.contract_type != ContractType::Pam)
        {
            let provider = provider_for(&case);
            registry
                .evaluate(&case.terms, &provider)
                .unwrap_or_else(|e| panic!("case {} failed to evaluate: {e}", case.id));
        }
    }
}
