//! Schedule generation primitives (ACTUS techspec sections "Schedule",
//! "End Of Month Shift Convention", "Business Day Shift Convention" and
//! "Business Day Calendar").

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, Weekday};

use actus_model::{BusinessDayConvention, Calendar, Cycle, CyclePeriod, EndOfMonthConvention};

const MAX_SCHEDULE_ROLLS: u64 = 100_000;
const MAX_SHIFT_HOPS: u32 = 366;

/// Last-stub scheme of a schedule (techspec section "Schedule", stub `S`).
///
/// The techspec defines the cycle stub as `+` (long last stub) or `-` (short
/// last stub). In the dictionary encoding (`[ISO8601 Duration]L[s={0,1}]`,
/// e.g. `P1ML0`, `P1ML1`) the trailing indicator carries this information:
/// `0` is a long last stub, `1` a short last stub. This is the encoding the
/// vendored testbeds use (`P1ML0` schedules drop the overshooting roll, e.g.
/// lam09; `P1ML1` schedules keep it and append the termination, e.g. lam19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastStub {
    /// Final period is extended to the termination date (dictionary `0`,
    /// techspec `+`).
    Long,
    /// Termination date is appended after the last roll (dictionary `1`,
    /// techspec `-`).
    Short,
}

/// Resolves the last-stub scheme of a cycle from its stub indicator digit.
fn last_stub(cycle: &Cycle) -> LastStub {
    match cycle.index() {
        0 => LastStub::Long,
        1 => LastStub::Short,
        other => panic!("unknown cycle stub indicator {other} in cycle {cycle}"),
    }
}

/// ACTUS Business Day Function (techspec section "Business Day Calendar").
///
/// Returns whether `t` is a business day under calendar `cal`. `NC`
/// (NoHoliday) counts every calendar day as a business day; `MF`
/// (MondayToFriday) counts Monday through Friday. These are the two standard
/// calendars named by the techspec and the only tokens occurring in the
/// vendored testbeds.
pub fn is_business_day(t: NaiveDateTime, cal: Calendar) -> bool {
    match cal {
        Calendar::Nc => true,
        Calendar::Mf => !matches!(t.weekday(), Weekday::Sat | Weekday::Sun),
    }
}

/// ACTUS Business Day Shift (techspec section "Business Day Shift Convention").
///
/// Shifts `t` to a business day per convention `bdc` and calendar `cal`; a
/// business day is returned unchanged. The `SC*` and `CS*` variants produce
/// the same shifted event date: per the techspec they differ only in whether
/// payoff calculation uses the pre-shift or post-shift time, which is a
/// payoff-function concern of the contract implementations, not of the
/// schedule itself. `NOS` never shifts.
///
/// The dictionary v1.4 lists `SCMP` twice and never defines `CSMP`, so eight
/// distinct conventions exist; `actus_model::BusinessDayConvention` mirrors
/// that upstream defect and this function covers all of them.
pub fn shift_business_day(
    t: NaiveDateTime,
    bdc: BusinessDayConvention,
    cal: Calendar,
) -> NaiveDateTime {
    #[derive(Clone, Copy)]
    enum Direction {
        Following,
        Preceding,
    }
    let (direction, modified) = match bdc {
        BusinessDayConvention::Nos => return t,
        BusinessDayConvention::Scf | BusinessDayConvention::Csf => (Direction::Following, false),
        BusinessDayConvention::Scmf | BusinessDayConvention::Csmf => (Direction::Following, true),
        BusinessDayConvention::Scp | BusinessDayConvention::Csp => (Direction::Preceding, false),
        BusinessDayConvention::Scmp => (Direction::Preceding, true),
    };
    if is_business_day(t, cal) {
        return t;
    }
    let original_month = t.month();
    type ShiftFn = fn(NaiveDateTime) -> NaiveDateTime;
    let (forward, backward): (ShiftFn, ShiftFn) = match direction {
        Direction::Following => (next_day, previous_day),
        Direction::Preceding => (previous_day, next_day),
    };
    let shifted = walk_to_business_day(t, cal, forward);
    if modified && shifted.month() != original_month {
        walk_to_business_day(t, cal, backward)
    } else {
        shifted
    }
}

fn walk_to_business_day(
    t: NaiveDateTime,
    cal: Calendar,
    step: impl Fn(NaiveDateTime) -> NaiveDateTime,
) -> NaiveDateTime {
    let mut candidate = t;
    let mut hops = 0;
    while !is_business_day(candidate, cal) {
        candidate = step(candidate);
        hops += 1;
        if hops > MAX_SHIFT_HOPS {
            panic!("business day shift exceeded {MAX_SHIFT_HOPS} hops; calendar yields no business days");
        }
    }
    candidate
}

fn next_day(t: NaiveDateTime) -> NaiveDateTime {
    t.date()
        .succ_opt()
        .expect("date successor is representable")
        .and_time(t.time())
}

fn previous_day(t: NaiveDateTime) -> NaiveDateTime {
    t.date()
        .pred_opt()
        .expect("date predecessor is representable")
        .and_time(t.time())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next_month =
        NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid first of month");
    first_of_next_month
        .pred_opt()
        .map(|last| last.day())
        .unwrap_or(30)
}

fn is_last_day_of_month(t: NaiveDateTime) -> bool {
    t.day() == days_in_month(t.year(), t.month())
}

fn month_has_less_than_31_days(t: NaiveDateTime) -> bool {
    days_in_month(t.year(), t.month()) < 31
}

fn last_day_of_month(t: NaiveDateTime) -> NaiveDateTime {
    let date = NaiveDate::from_ymd_opt(t.year(), t.month(), days_in_month(t.year(), t.month()))
        .expect("last day of month is representable");
    date.and_time(t.time())
}

/// The i-th unrolled cycle increment from the anchor `t` (i starts at 1).
///
/// Month-based increments are always computed from the anchor so that a
/// clamped intermediate date does not become the new anchor: a monthly
/// schedule from Jan 31 unrolls Jan 31, Feb 28, Mar 31, Apr 30, ... (testbed
/// pam09), not Jan 31, Feb 28, Mar 28, ...
fn add_cycle(
    anchor: NaiveDateTime,
    i: u64,
    cycle: &Cycle,
    snap_to_month_end: bool,
) -> NaiveDateTime {
    let length = u64::from(cycle.length());
    let steps = length * i;
    match cycle.period() {
        CyclePeriod::Day => anchor + Duration::days(steps as i64),
        CyclePeriod::Week => anchor + Duration::days((steps * 7) as i64),
        CyclePeriod::Month => add_months(
            anchor,
            u32::try_from(steps).unwrap_or(u32::MAX),
            snap_to_month_end,
        ),
        CyclePeriod::Year => add_months(
            anchor,
            u32::try_from(steps * 12).unwrap_or(u32::MAX),
            snap_to_month_end,
        ),
    }
}

fn add_months(t: NaiveDateTime, months: u32, snap_to_month_end: bool) -> NaiveDateTime {
    let date = t
        .date()
        .checked_add_months(Months::new(months))
        .unwrap_or_else(|| panic!("month arithmetic overflows for {t} + {months} months"));
    let shifted = date.and_time(t.time());
    if snap_to_month_end {
        last_day_of_month(shifted)
    } else {
        shifted
    }
}

/// Whether the End Of Month Shift Convention snaps rolled dates to month ends.
///
/// Per the techspec, EOM applies to a schedule only if the schedule start `s`
/// is the last day of a month with fewer than 31 days (Feb, April, ...) and
/// the cycle is a multiple of one month (M or Y cycles; Q and H are spelled
/// as `3M`/`6M`). Otherwise rolled dates keep the same day of month, clamped
/// to shorter months.
fn snaps_to_month_end(anchor: NaiveDateTime, cycle: &Cycle, eomc: EndOfMonthConvention) -> bool {
    let month_based = matches!(cycle.period(), CyclePeriod::Month | CyclePeriod::Year);
    match eomc {
        EndOfMonthConvention::Eom => {
            month_based && is_last_day_of_month(anchor) && month_has_less_than_31_days(anchor)
        }
        EndOfMonthConvention::Sd => false,
    }
}

/// The i-th unrolled cycle increment from the anchor `t` (i starts at 1)
/// with the End Of Month Shift Convention applied (techspec sections
/// "Schedule" and "End Of Month Shift Convention").
///
/// Contract implementations use this to position one-off schedule elements
/// relative to an anchor, e.g. the `IPCI` series termination at the
/// capitalization end date, where the rolled dates must obey the same EOM
/// snapping rule as the unrolled series itself.
///
/// Panics when the increment overflows the calendar; such inputs cannot
/// describe a valid contract schedule.
pub fn cycle_step(
    anchor: NaiveDateTime,
    i: u64,
    cycle: &Cycle,
    eomc: EndOfMonthConvention,
) -> NaiveDateTime {
    let snap_to_month_end = snaps_to_month_end(anchor, cycle, eomc);
    add_cycle(anchor, i, cycle, snap_to_month_end)
}

/// The date one full cycle before `anchor`, i.e. the hypothetical schedule
/// element that would roll into the anchor (techspec schedule roll inverted
/// by one step).
///
/// Month and year cycles subtract whole months while keeping the anchor's
/// day of month (clamped to shorter months, mirroring [`add_cycle`]); day
/// and week cycles subtract the corresponding number of days. With the End
/// Of Month Shift Convention active for the anchor the result snaps to the
/// previous month end, so an EOM schedule rolls `Jan 31 -> Feb 28 -> Jan 31`
/// in both directions.
///
/// The ANN annuity recalculation uses this to start the discount chain one
/// full cycle before the first remaining redemption payment.
///
/// Panics when the subtraction overflows the calendar; such inputs cannot
/// describe a valid contract schedule.
pub fn cycle_back(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    eomc: EndOfMonthConvention,
) -> NaiveDateTime {
    let snap_to_month_end = snaps_to_month_end(anchor, cycle, eomc);
    let length = u64::from(cycle.length());
    let date = match cycle.period() {
        CyclePeriod::Day => anchor - Duration::days(length as i64),
        CyclePeriod::Week => anchor - Duration::days((length * 7) as i64),
        CyclePeriod::Month => anchor
            .date()
            .checked_sub_months(Months::new(u32::try_from(length).unwrap_or(u32::MAX)))
            .unwrap_or_else(|| panic!("month arithmetic overflows for {anchor} - {length} months"))
            .and_time(anchor.time()),
        CyclePeriod::Year => anchor
            .date()
            .checked_sub_months(Months::new(u32::try_from(length * 12).unwrap_or(u32::MAX)))
            .unwrap_or_else(|| panic!("month arithmetic overflows for {anchor} - {length} years"))
            .and_time(anchor.time()),
    };
    if snap_to_month_end {
        last_day_of_month(date)
    } else {
        date
    }
}

/// ACTUS schedule function S(s, c, T, EOMC, BDC) (techspec section "Schedule").
///
/// Unrolls cyclic dates from `anchor` to `termination`:
///
/// 1. Roll forward from `anchor` by `cycle` while the rolled date is strictly
///    before `termination`, applying the EOM snapping rule.
/// 2. If a roll hits `termination` exactly, no stub correction applies.
/// 3. Otherwise `termination` is appended (the schedule end belongs to the
///    schedule). With a long last stub (stub indicator `0`) the last rolled
///    date is removed so the final period is long; with a short last stub
///    (`1`) it is kept so the final period is short.
/// 4. Business day adjustment (`bdc` + `cal`) is applied to the rolled dates,
///    i.e. t_1..t_(n-1) per the techspec; the final schedule element
///    (`termination`) is not shifted.
///
/// Degenerate input `termination <= anchor` yields `[anchor]`. A cycle that
/// rolls more than [`MAX_SCHEDULE_ROLLS`] times, or a business day shift that
/// exceeds [`MAX_SHIFT_HOPS`] hops, panics: such inputs cannot describe a
/// valid contract schedule and must fail loudly.
pub fn generate_schedule(
    anchor: NaiveDateTime,
    cycle: &Cycle,
    termination: NaiveDateTime,
    eomc: EndOfMonthConvention,
    bdc: BusinessDayConvention,
    cal: Calendar,
) -> Vec<NaiveDateTime> {
    if termination <= anchor {
        return vec![anchor];
    }
    let snap_to_month_end = snaps_to_month_end(anchor, cycle, eomc);
    let mut rolled = vec![anchor];
    let mut i: u64 = 0;
    let roll_overshot_termination;
    loop {
        i += 1;
        let next = add_cycle(anchor, i, cycle, snap_to_month_end);
        if next >= termination {
            roll_overshot_termination = next > termination;
            break;
        }
        rolled.push(next);
        if i > MAX_SCHEDULE_ROLLS {
            panic!("schedule generation exceeded {MAX_SCHEDULE_ROLLS} cycle rolls; cycle is too fine for the given termination");
        }
    }
    if roll_overshot_termination {
        if matches!(last_stub(cycle), LastStub::Long) && rolled.len() > 1 {
            rolled.pop();
        }
        rolled.push(termination);
    } else {
        rolled.push(termination);
    }
    let final_index = rolled.len() - 1;
    rolled
        .into_iter()
        .enumerate()
        .map(|(index, t)| {
            if index < final_index {
                shift_business_day(t, bdc, cal)
            } else {
                t
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use actus_model::CycleStub;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn d(v: &[&str]) -> Vec<NaiveDateTime> {
        v.iter().map(|s| t(s)).collect()
    }

    fn cycle(length: u32, period: CyclePeriod, stub_indicator: u8) -> Cycle {
        Cycle::new(length, period, CycleStub::Long, stub_indicator)
            .expect("test cycles are well formed")
    }

    #[test]
    fn pam01_exact_monthly_hit_needs_no_stub_correction() {
        let schedule = generate_schedule(
            t("2013-01-01T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2014-01-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2013-01-01T00:00:00",
                "2013-02-01T00:00:00",
                "2013-03-01T00:00:00",
                "2013-04-01T00:00:00",
                "2013-05-01T00:00:00",
                "2013-06-01T00:00:00",
                "2013-07-01T00:00:00",
                "2013-08-01T00:00:00",
                "2013-09-01T00:00:00",
                "2013-10-01T00:00:00",
                "2013-11-01T00:00:00",
                "2013-12-01T00:00:00",
                "2014-01-01T00:00:00",
            ])
        );
    }

    #[test]
    fn lam19_short_last_stub_appends_termination() {
        let schedule = generate_schedule(
            t("2013-01-31T00:00:00"),
            &cycle(2, CyclePeriod::Week, 1),
            t("2013-05-18T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2013-01-31T00:00:00",
                "2013-02-14T00:00:00",
                "2013-02-28T00:00:00",
                "2013-03-14T00:00:00",
                "2013-03-28T00:00:00",
                "2013-04-11T00:00:00",
                "2013-04-25T00:00:00",
                "2013-05-09T00:00:00",
                "2013-05-18T00:00:00",
            ])
        );
    }

    #[test]
    fn lam09_long_last_stub_removes_overshooting_roll() {
        let schedule = generate_schedule(
            t("2013-02-01T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2013-11-15T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2013-02-01T00:00:00",
                "2013-03-01T00:00:00",
                "2013-04-01T00:00:00",
                "2013-05-01T00:00:00",
                "2013-06-01T00:00:00",
                "2013-07-01T00:00:00",
                "2013-08-01T00:00:00",
                "2013-09-01T00:00:00",
                "2013-10-01T00:00:00",
                "2013-11-15T00:00:00",
            ])
        );
    }

    #[test]
    fn lam21_eom_snaps_short_month_anchor_to_month_ends() {
        let schedule = generate_schedule(
            t("2013-02-28T00:00:00"),
            &cycle(1, CyclePeriod::Month, 1),
            t("2016-01-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(schedule.first(), Some(&t("2013-02-28T00:00:00")));
        assert_eq!(schedule.get(1), Some(&t("2013-03-31T00:00:00")));
        assert_eq!(schedule.get(2), Some(&t("2013-04-30T00:00:00")));
        assert_eq!(schedule.get(3), Some(&t("2013-05-31T00:00:00")));
        assert_eq!(
            schedule.get(schedule.len() - 2),
            Some(&t("2015-12-31T00:00:00"))
        );
        assert_eq!(schedule.last(), Some(&t("2016-01-01T00:00:00")));
        let all_month_ends = schedule
            .iter()
            .take(schedule.len() - 1)
            .all(|date| is_last_day_of_month(*date));
        assert!(all_month_ends);
    }

    #[test]
    fn pam09_scf_shifts_weekend_rolls_after_stub_correction() {
        let schedule = generate_schedule(
            t("2013-01-31T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2014-01-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Scf,
            Calendar::Mf,
        );
        assert_eq!(
            schedule,
            d(&[
                "2013-01-31T00:00:00",
                "2013-02-28T00:00:00",
                "2013-04-01T00:00:00",
                "2013-04-30T00:00:00",
                "2013-05-31T00:00:00",
                "2013-07-01T00:00:00",
                "2013-07-31T00:00:00",
                "2013-09-02T00:00:00",
                "2013-09-30T00:00:00",
                "2013-10-31T00:00:00",
                "2013-12-02T00:00:00",
                "2014-01-01T00:00:00",
            ])
        );
    }

    #[test]
    fn pam07_pam10_modified_conventions_pull_back_into_month() {
        for bdc in [BusinessDayConvention::Scmf, BusinessDayConvention::Scmp] {
            let schedule = generate_schedule(
                t("2013-01-31T00:00:00"),
                &cycle(1, CyclePeriod::Month, 0),
                t("2014-01-01T00:00:00"),
                EndOfMonthConvention::Eom,
                bdc,
                Calendar::Mf,
            );
            assert_eq!(
                schedule,
                d(&[
                    "2013-01-31T00:00:00",
                    "2013-02-28T00:00:00",
                    "2013-03-29T00:00:00",
                    "2013-04-30T00:00:00",
                    "2013-05-31T00:00:00",
                    "2013-06-28T00:00:00",
                    "2013-07-31T00:00:00",
                    "2013-08-30T00:00:00",
                    "2013-09-30T00:00:00",
                    "2013-10-31T00:00:00",
                    "2013-11-29T00:00:00",
                    "2014-01-01T00:00:00",
                ]),
                "convention {bdc:?}"
            );
        }
    }

    #[test]
    fn pam08_csf_matches_pam09_scf_schedule_dates() {
        let schedule = generate_schedule(
            t("2013-01-31T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2014-01-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Csf,
            Calendar::Mf,
        );
        assert_eq!(schedule.get(2), Some(&t("2013-04-01T00:00:00")));
        assert_eq!(schedule.get(5), Some(&t("2013-07-01T00:00:00")));
        assert_eq!(schedule.get(7), Some(&t("2013-09-02T00:00:00")));
        assert_eq!(schedule.get(10), Some(&t("2013-12-02T00:00:00")));
    }

    #[test]
    fn pam05_anchor_on_30th_ignores_eom_and_keeps_day_of_month() {
        let schedule = generate_schedule(
            t("2013-01-30T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2014-01-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2013-01-30T00:00:00",
                "2013-02-28T00:00:00",
                "2013-03-30T00:00:00",
                "2013-04-30T00:00:00",
                "2013-05-30T00:00:00",
                "2013-06-30T00:00:00",
                "2013-07-30T00:00:00",
                "2013-08-30T00:00:00",
                "2013-09-30T00:00:00",
                "2013-10-30T00:00:00",
                "2013-11-30T00:00:00",
                "2014-01-01T00:00:00",
            ])
        );
    }

    #[test]
    fn jan_31_plus_one_month_in_leap_year_clamps_to_feb_29() {
        let schedule = generate_schedule(
            t("2020-01-31T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2021-01-31T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2020-01-31T00:00:00",
                "2020-02-29T00:00:00",
                "2020-03-31T00:00:00",
                "2020-04-30T00:00:00",
                "2020-05-31T00:00:00",
                "2020-06-30T00:00:00",
                "2020-07-31T00:00:00",
                "2020-08-31T00:00:00",
                "2020-09-30T00:00:00",
                "2020-10-31T00:00:00",
                "2020-11-30T00:00:00",
                "2020-12-31T00:00:00",
                "2021-01-31T00:00:00",
            ])
        );
    }

    #[test]
    fn april_30_anchor_splits_eom_and_sd() {
        let eom_schedule = generate_schedule(
            t("2020-04-30T00:00:00"),
            &cycle(1, CyclePeriod::Month, 1),
            t("2020-09-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            eom_schedule,
            d(&[
                "2020-04-30T00:00:00",
                "2020-05-31T00:00:00",
                "2020-06-30T00:00:00",
                "2020-07-31T00:00:00",
                "2020-08-31T00:00:00",
                "2020-09-01T00:00:00",
            ])
        );

        let sd_schedule = generate_schedule(
            t("2020-04-30T00:00:00"),
            &cycle(1, CyclePeriod::Month, 1),
            t("2020-09-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            sd_schedule,
            d(&[
                "2020-04-30T00:00:00",
                "2020-05-30T00:00:00",
                "2020-06-30T00:00:00",
                "2020-07-30T00:00:00",
                "2020-08-30T00:00:00",
                "2020-09-01T00:00:00",
            ])
        );
    }

    #[test]
    fn feb_29_anchor_eom_snaps_to_month_ends() {
        let schedule = generate_schedule(
            t("2020-02-29T00:00:00"),
            &cycle(1, CyclePeriod::Month, 1),
            t("2020-08-15T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2020-02-29T00:00:00",
                "2020-03-31T00:00:00",
                "2020-04-30T00:00:00",
                "2020-05-31T00:00:00",
                "2020-06-30T00:00:00",
                "2020-07-31T00:00:00",
                "2020-08-15T00:00:00",
            ])
        );
    }

    #[test]
    fn feb_28_anchor_eom_reaches_feb_29_in_next_leap_year() {
        let eom_schedule = generate_schedule(
            t("2021-02-28T00:00:00"),
            &cycle(1, CyclePeriod::Year, 1),
            t("2024-03-01T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            eom_schedule,
            d(&[
                "2021-02-28T00:00:00",
                "2022-02-28T00:00:00",
                "2023-02-28T00:00:00",
                "2024-02-29T00:00:00",
                "2024-03-01T00:00:00",
            ])
        );

        let sd_schedule = generate_schedule(
            t("2021-02-28T00:00:00"),
            &cycle(1, CyclePeriod::Year, 1),
            t("2024-03-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(sd_schedule.get(3), Some(&t("2024-02-28T00:00:00")));
    }

    #[test]
    fn dec_31_plus_one_year_is_stable() {
        let schedule = generate_schedule(
            t("2020-12-31T00:00:00"),
            &cycle(1, CyclePeriod::Year, 0),
            t("2022-12-31T00:00:00"),
            EndOfMonthConvention::Eom,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2020-12-31T00:00:00",
                "2021-12-31T00:00:00",
                "2022-12-31T00:00:00",
            ])
        );
    }

    #[test]
    fn quarterly_as_three_month_cycle_crosses_leap_february() {
        let schedule = generate_schedule(
            t("2023-11-01T00:00:00"),
            &cycle(3, CyclePeriod::Month, 1),
            t("2024-08-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(
            schedule,
            d(&[
                "2023-11-01T00:00:00",
                "2024-02-01T00:00:00",
                "2024-05-01T00:00:00",
                "2024-08-01T00:00:00",
            ])
        );
    }

    #[test]
    fn termination_on_weekend_is_never_shifted_but_rolls_are() {
        let schedule = generate_schedule(
            t("2023-01-02T00:00:00"),
            &cycle(1, CyclePeriod::Month, 1),
            t("2023-06-03T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Scf,
            Calendar::Mf,
        );
        assert_eq!(
            schedule,
            d(&[
                "2023-01-02T00:00:00",
                "2023-02-02T00:00:00",
                "2023-03-02T00:00:00",
                "2023-04-03T00:00:00",
                "2023-05-02T00:00:00",
                "2023-06-02T00:00:00",
                "2023-06-03T00:00:00",
            ])
        );
    }

    #[test]
    fn scmp_falls_back_to_following_when_preceding_crosses_month() {
        let shifted = shift_business_day(
            t("2023-04-02T00:00:00"),
            BusinessDayConvention::Scmp,
            Calendar::Mf,
        );
        assert_eq!(shifted, t("2023-04-03T00:00:00"));

        let unmodified = shift_business_day(
            t("2023-04-02T00:00:00"),
            BusinessDayConvention::Scp,
            Calendar::Mf,
        );
        assert_eq!(unmodified, t("2023-03-31T00:00:00"));
    }

    #[test]
    fn no_calendar_counts_every_day_as_business_day() {
        let saturday = t("2023-04-01T00:00:00");
        assert!(is_business_day(saturday, Calendar::Nc));
        assert!(!is_business_day(saturday, Calendar::Mf));
        assert_eq!(
            shift_business_day(saturday, BusinessDayConvention::Scf, Calendar::Nc),
            saturday
        );
        assert_eq!(
            shift_business_day(saturday, BusinessDayConvention::Nos, Calendar::Mf),
            saturday
        );
    }

    #[test]
    fn degenerate_termination_yields_anchor_only() {
        let equal = generate_schedule(
            t("2013-01-01T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2013-01-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(equal, d(&["2013-01-01T00:00:00"]));

        let inverted = generate_schedule(
            t("2013-01-01T00:00:00"),
            &cycle(1, CyclePeriod::Month, 0),
            t("2012-01-01T00:00:00"),
            EndOfMonthConvention::Sd,
            BusinessDayConvention::Nos,
            Calendar::Nc,
        );
        assert_eq!(inverted, d(&["2013-01-01T00:00:00"]));
    }

    #[test]
    fn unknown_stub_indicator_fails_loudly() {
        let result = std::panic::catch_unwind(|| {
            let cycle = cycle(1, CyclePeriod::Month, 2);
            generate_schedule(
                t("2013-01-01T00:00:00"),
                &cycle,
                t("2013-12-15T00:00:00"),
                EndOfMonthConvention::Sd,
                BusinessDayConvention::Nos,
                Calendar::Nc,
            )
        });
        assert!(result.is_err());
    }
}
