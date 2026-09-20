//! Day count fractions (ACTUS techspec section "Year Fraction Convention").

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use rust_decimal::Decimal;

use actus_model::DayCountConvention;

use crate::EngineError;

/// ACTUS timestamp normalisation (techspec section "Date/Time").
///
/// ACTUS interprets the timestamp `23:59:59` as midnight (i.e. as the
/// beginning of the following day, `00:00:00`), because many ISO 8601
/// implementations lack the `24:00:00` format. All engine date arithmetic
/// normalises timestamps through this function first.
pub fn normalize_timestamp(t: NaiveDateTime) -> NaiveDateTime {
    if t.hour() == 23 && t.minute() == 59 && t.second() == 59 {
        t.date()
            .succ_opt()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .unwrap_or(t)
    } else {
        t
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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
    t.date().day() == days_in_month(t.year(), t.month())
}

/// ACTUS day count fraction (DCF) (techspec section "Year Fraction Convention").
///
/// Returns the fraction of a year between `start` and `end` under the day
/// count convention `dcc`. Implemented conventions (dictionary v1.4
/// vocabulary):
///
/// - `A365`: actual days / 365 (fixed).
/// - `A360`: actual days / 360 (fixed).
/// - `Aa`: actual/actual, summing actual days of each calendar year divided
///   by 365 (non-leap years) or 366 (leap years).
/// - `ThirtyE360` (Eurobond): `((y2-y1)*360 + (m2-m1)*30 + (d2-d1)) / 360`
///   with both day-of-month values capped at 30.
/// - `ThirtyE360Isda`: as `ThirtyE360`, with additional ISDA end-of-February
///   adjustments.
/// - `TwentyEightE336`: `((y2-y1)*336 + (m2-m1)*28 +
///   (min(d2,28)-min(d1,28))) / 336`.
///
/// The 30U/360 (US) and BUS/252 conventions are outside the v1.4 dictionary
/// vocabulary and are exposed as [`day_count_fraction_30u360`] and
/// [`day_count_fraction_bus252`].
///
/// Fractions are computed in exact `Decimal` arithmetic; no floating point is
/// involved. Both inputs are normalised via [`normalize_timestamp`]. The
/// function is undefined for `end < start` and returns
/// [`EngineError::InvalidDayCountRange`].
pub fn day_count_fraction(
    start: NaiveDateTime,
    end: NaiveDateTime,
    dcc: DayCountConvention,
) -> Result<Decimal, EngineError> {
    let start = normalize_timestamp(start);
    let end = normalize_timestamp(end);
    if end < start {
        return Err(EngineError::InvalidDayCountRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }
    let fraction = match dcc {
        DayCountConvention::A365 => actual_over_fixed(start, end, 365),
        DayCountConvention::A360 => actual_over_fixed(start, end, 360),
        DayCountConvention::Aa => actual_actual(start, end),
        DayCountConvention::ThirtyE360 => thirty_e_360(start, end, false),
        DayCountConvention::ThirtyE360Isda => thirty_e_360(start, end, true),
        DayCountConvention::TwentyEightE336 => twenty_eight_e_336(start, end),
    };
    Ok(fraction)
}

/// 30U/360 (US, NASD) day count fraction.
///
/// Not part of the v1.4 dictionary vocabulary, provided for completeness:
/// `d1 = 31 -> 30`, `d2 = 31 and d1 in (30, 31) -> 30`, then
/// `((y2-y1)*360 + (m2-m1)*30 + (d2-d1)) / 360`.
pub fn day_count_fraction_30u360(
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Result<Decimal, EngineError> {
    let start = normalize_timestamp(start);
    let end = normalize_timestamp(end);
    if end < start {
        return Err(EngineError::InvalidDayCountRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }
    Ok(thirty_u_360(start, end))
}

/// BUS/252 (business days / 252) day count fraction.
///
/// Not part of the v1.4 dictionary vocabulary, provided for completeness:
/// counts Monday-to-Friday days in `[start, end)` and divides by 252.
pub fn day_count_fraction_bus252(
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Result<Decimal, EngineError> {
    let start = normalize_timestamp(start);
    let end = normalize_timestamp(end);
    if end < start {
        return Err(EngineError::InvalidDayCountRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }
    Ok(business_days_over_fixed(start, end, 252))
}

fn actual_over_fixed(start: NaiveDateTime, end: NaiveDateTime, basis: i64) -> Decimal {
    let days = whole_days(start, end);
    Decimal::from(days) / Decimal::from(basis)
}

fn whole_days(start: NaiveDateTime, end: NaiveDateTime) -> i64 {
    end.signed_duration_since(start).num_days()
}

fn actual_actual(start: NaiveDateTime, end: NaiveDateTime) -> Decimal {
    let mut cursor = start;
    let mut total = Decimal::ZERO;
    while cursor < end {
        let year_end = NaiveDate::from_ymd_opt(cursor.year() + 1, 1, 1)
            .expect("january first of the next year is representable")
            .and_hms_opt(0, 0, 0)
            .expect("midnight is representable");
        let segment_end = if year_end < end { year_end } else { end };
        let days = whole_days(cursor, segment_end);
        let basis = if is_leap_year(cursor.year()) {
            366
        } else {
            365
        };
        total += Decimal::from(days) / Decimal::from(basis);
        cursor = segment_end;
    }
    total
}

fn thirty_e_360(start: NaiveDateTime, end: NaiveDateTime, isda: bool) -> Decimal {
    let (d1, d2) = if isda {
        let d1 = if is_last_day_of_feb(start) {
            30
        } else {
            start.day()
        };
        let d2 = if is_last_day_of_feb(end) {
            30
        } else {
            end.day().min(30)
        };
        (d1, d2)
    } else {
        (start.day().min(30), end.day().min(30))
    };
    thirty_basis_fraction(start, end, d1, d2, 30, 360)
}

fn thirty_u_360(start: NaiveDateTime, end: NaiveDateTime) -> Decimal {
    let mut d1 = start.day();
    let mut d2 = end.day();
    if d1 == 31 {
        d1 = 30;
    }
    if d2 == 31 && d1 == 30 {
        d2 = 30;
    }
    thirty_basis_fraction(start, end, d1, d2, 30, 360)
}

fn thirty_basis_fraction(
    start: NaiveDateTime,
    end: NaiveDateTime,
    d1: u32,
    d2: u32,
    days_per_month: i64,
    basis: i64,
) -> Decimal {
    let month_span = i64::from(end.year() - start.year()) * 12 + i64::from(end.month())
        - i64::from(start.month());
    let days = month_span * days_per_month + i64::from(d2) - i64::from(d1);
    Decimal::from(days) / Decimal::from(basis)
}

fn twenty_eight_e_336(start: NaiveDateTime, end: NaiveDateTime) -> Decimal {
    let d1 = start.day().min(28);
    let d2 = end.day().min(28);
    thirty_basis_fraction(start, end, d1, d2, 28, 336)
}

fn business_days_over_fixed(start: NaiveDateTime, end: NaiveDateTime, basis: i64) -> Decimal {
    let mut count: i64 = 0;
    let mut cursor = start.date();
    let last = end.date();
    while cursor < last {
        let weekday = cursor.weekday();
        if !matches!(weekday, chrono::Weekday::Sat | chrono::Weekday::Sun) {
            count += 1;
        }
        cursor = match cursor.succ_opt() {
            Some(next) => next,
            None => break,
        };
    }
    Decimal::from(count) / Decimal::from(basis)
}

fn is_last_day_of_feb(t: NaiveDateTime) -> bool {
    t.month() == 2 && is_last_day_of_month(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn assert_close(actual: Decimal, expected: Decimal) {
        let tolerance = dec!(0.0000000001);
        assert!(
            (actual - expected).abs() < tolerance,
            "actual {actual} vs expected {expected}"
        );
    }

    #[test]
    fn same_day_is_zero_for_every_convention() {
        let cases = [
            DayCountConvention::A365,
            DayCountConvention::A360,
            DayCountConvention::Aa,
            DayCountConvention::ThirtyE360,
            DayCountConvention::ThirtyE360Isda,
            DayCountConvention::TwentyEightE336,
        ];
        for dcc in cases {
            let fraction =
                day_count_fraction(t("2013-01-01T00:00:00"), t("2013-01-01T00:00:00"), dcc)
                    .unwrap();
            assert_eq!(fraction, Decimal::ZERO, "convention {dcc:?}");
        }
        let extra = [
            day_count_fraction_30u360(t("2013-01-01T00:00:00"), t("2013-01-01T00:00:00")).unwrap(),
            day_count_fraction_bus252(t("2013-01-01T00:00:00"), t("2013-01-01T00:00:00")).unwrap(),
        ];
        for fraction in extra {
            assert_eq!(fraction, Decimal::ZERO);
        }
    }

    #[test]
    fn pam01_jan_to_feb_is_31_over_365() {
        let fraction = day_count_fraction(
            t("2013-01-01T00:00:00"),
            t("2013-02-01T00:00:00"),
            DayCountConvention::A365,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(31, 0) / Decimal::new(365, 0));
        assert_close(fraction, dec!(0.0849315068493151));
        assert_close(fraction * dec!(3000) * dec!(0.1), dec!(25.4794520547945));
    }

    #[test]
    fn a360_jan_to_feb_is_31_over_360() {
        let fraction = day_count_fraction(
            t("2013-01-01T00:00:00"),
            t("2013-02-01T00:00:00"),
            DayCountConvention::A360,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(31, 0) / Decimal::new(360, 0));
    }

    #[test]
    fn a365_spans_leap_year_with_actual_days() {
        let fraction = day_count_fraction(
            t("2020-01-01T00:00:00"),
            t("2020-03-01T00:00:00"),
            DayCountConvention::A365,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(60, 0) / Decimal::new(365, 0));
    }

    #[test]
    fn aa_same_non_leap_year_is_days_over_365() {
        let fraction = day_count_fraction(
            t("2013-01-01T00:00:00"),
            t("2013-01-31T00:00:00"),
            DayCountConvention::Aa,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(30, 0) / Decimal::new(365, 0));
        assert_close(fraction * dec!(5000) * dec!(0.08), dec!(32.8767123287671));
    }

    #[test]
    fn aa_splits_years_across_leap_boundary() {
        let fraction = day_count_fraction(
            t("2015-12-01T00:00:00"),
            t("2016-01-15T00:00:00"),
            DayCountConvention::Aa,
        )
        .unwrap();
        let expected =
            Decimal::new(31, 0) / Decimal::new(365, 0) + Decimal::new(14, 0) / Decimal::new(366, 0);
        assert_close(fraction, expected);
    }

    #[test]
    fn aa_whole_leap_year_is_one() {
        let fraction = day_count_fraction(
            t("2020-01-01T00:00:00"),
            t("2021-01-01T00:00:00"),
            DayCountConvention::Aa,
        )
        .unwrap();
        assert_close(fraction, Decimal::ONE);
    }

    #[test]
    fn thirty_e_360_caps_both_day_counts() {
        let fraction = day_count_fraction(
            t("2013-02-28T00:00:00"),
            t("2013-03-31T00:00:00"),
            DayCountConvention::ThirtyE360,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(32, 0) / Decimal::new(360, 0));
        assert_close(fraction * dec!(3000) * dec!(0.1), dec!(26.6666666666667));
    }

    #[test]
    fn thirty_e_360_caps_start_day_at_30() {
        let fraction = day_count_fraction(
            t("2013-01-31T00:00:00"),
            t("2013-02-28T00:00:00"),
            DayCountConvention::ThirtyE360,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(28, 0) / Decimal::new(360, 0));
        assert_close(fraction * dec!(3000) * dec!(0.1), dec!(23.3333333333333));
    }

    #[test]
    fn thirty_e_360_full_year_is_one() {
        let fraction = day_count_fraction(
            t("2013-01-01T00:00:00"),
            t("2014-01-01T00:00:00"),
            DayCountConvention::ThirtyE360,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::ONE);
    }

    #[test]
    fn thirty_u_360_only_caps_end_when_start_was_capped() {
        let jan_to_mar =
            day_count_fraction_30u360(t("2013-01-31T00:00:00"), t("2013-03-31T00:00:00")).unwrap();
        assert_eq!(jan_to_mar, Decimal::new(60, 0) / Decimal::new(360, 0));

        let feb_to_mar =
            day_count_fraction_30u360(t("2013-02-28T00:00:00"), t("2013-03-31T00:00:00")).unwrap();
        assert_eq!(feb_to_mar, Decimal::new(33, 0) / Decimal::new(360, 0));
    }

    #[test]
    fn twenty_eight_e_336_caps_days_and_uses_336_basis() {
        let fraction = day_count_fraction(
            t("2013-01-15T00:00:00"),
            t("2013-02-20T00:00:00"),
            DayCountConvention::TwentyEightE336,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::new(33, 0) / Decimal::new(336, 0));
    }

    #[test]
    fn bus252_counts_weekdays_only() {
        let fraction =
            day_count_fraction_bus252(t("2013-01-07T00:00:00"), t("2013-01-14T00:00:00")).unwrap();
        assert_eq!(fraction, Decimal::new(5, 0) / Decimal::new(252, 0));
    }

    #[test]
    fn negative_direction_is_an_error() {
        let err = day_count_fraction(
            t("2013-02-01T00:00:00"),
            t("2013-01-01T00:00:00"),
            DayCountConvention::A365,
        )
        .unwrap_err();
        assert!(matches!(err, EngineError::InvalidDayCountRange { .. }));
    }

    #[test]
    fn late_night_timestamp_is_normalised_to_midnight() {
        let fraction = day_count_fraction(
            t("2013-01-31T00:00:00"),
            t("2013-01-31T23:59:59"),
            DayCountConvention::A365,
        )
        .unwrap();
        assert_eq!(fraction, Decimal::ONE / Decimal::new(365, 0));
    }

    #[test]
    fn normalize_timestamp_shifts_end_of_day_only() {
        assert_eq!(
            normalize_timestamp(t("2013-12-31T23:59:59")),
            t("2014-01-01T00:00:00")
        );
        assert_eq!(
            normalize_timestamp(t("2013-12-31T23:58:59")),
            t("2013-12-31T23:58:59")
        );
        assert_eq!(
            normalize_timestamp(t("2013-12-31T00:00:00")),
            t("2013-12-31T00:00:00")
        );
    }
}
