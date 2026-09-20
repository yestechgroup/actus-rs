//! Configurable tolerance comparator between evaluated contract events and
//! the expected testbed results.
//!
//! The upstream reference implementation prints payoffs and states as f64, so
//! exact decimal equality is not meaningful; the comparison uses an absolute
//! one-cent tolerance widened by a relative term
//! `max(tol.absolute, tol.relative * max(|actual|, |expected|))`. Event times
//! and event types are compared exactly.

use rust_decimal::Decimal;

use actus_engine::ContractEvent;

use crate::cases::ExpectedEvent;

/// Tolerance configuration for numeric comparisons.
#[derive(Debug, Clone, Copy)]
pub struct Tolerance {
    /// Absolute tolerance applied to every numeric comparison.
    pub absolute: Decimal,
    /// Relative tolerance term, multiplied by the larger absolute value of
    /// the compared numbers.
    pub relative: Decimal,
}

impl Default for Tolerance {
    fn default() -> Tolerance {
        Tolerance {
            absolute: Decimal::new(1, 2),
            relative: Decimal::new(1, 9),
        }
    }
}

/// Result of comparing one case's event stream against the expected results.
#[derive(Debug, Clone)]
pub struct CaseReport {
    /// Case identifier the report refers to.
    pub case_id: String,
    /// Whether every check passed.
    pub passed: bool,
    /// Precise failure messages (index, expected vs actual), in event order.
    pub failures: Vec<String>,
}

impl CaseReport {
    /// An empty passing report for `case_id`.
    pub fn passing(case_id: impl Into<String>) -> CaseReport {
        CaseReport {
            case_id: case_id.into(),
            passed: true,
            failures: Vec::new(),
        }
    }
}

/// Compares evaluated events against the expected testbed results.
///
/// Checks, in order per event: event count equality, event type equality,
/// event time equality (exact `NaiveDateTime`), payoff within tolerance and
/// post-event state (`NT`, `IPNR`, `IPAC`) within tolerance. The returned
/// report carries an empty case id; use [`compare_case`] to fill it.
pub fn compare(
    actual: &[ContractEvent],
    expected: &[ExpectedEvent],
    tol: &Tolerance,
) -> CaseReport {
    let mut report = CaseReport::passing("");
    if actual.len() != expected.len() {
        report.failures.push(format!(
            "event count mismatch: expected {}, actual {}",
            expected.len(),
            actual.len()
        ));
    }
    let common = actual.len().min(expected.len());
    for index in 0..common {
        let (a, e) = (&actual[index], &expected[index]);
        let label = format!("event[{index}]");
        if a.event_type != e.event_type {
            report.failures.push(format!(
                "{label}: type expected {}, actual {}",
                e.event_type, a.event_type
            ));
            continue;
        }
        let label = format!("{label} {} at {}", a.event_type, a.time);
        if a.time != e.event_date {
            report.failures.push(format!(
                "{label}: time expected {}, actual {}",
                e.event_date, a.time
            ));
            continue;
        }
        if !within(a.payoff, e.payoff, tol) {
            report.failures.push(format!(
                "{label}: payoff expected {}, actual {} (diff {} exceeds tolerance)",
                e.payoff,
                a.payoff,
                (a.payoff - e.payoff).abs()
            ));
        }
        if !within(a.state.notional_principal, e.notional_principal, tol) {
            report.failures.push(format!(
                "{label}: notionalPrincipal expected {}, actual {}",
                e.notional_principal, a.state.notional_principal
            ));
        }
        if let Some(expected) = e.nominal_interest_rate {
            if !within(a.state.nominal_interest_rate, expected, tol) {
                report.failures.push(format!(
                    "{label}: nominalInterestRate expected {}, actual {}",
                    expected, a.state.nominal_interest_rate
                ));
            }
        }
        if let Some(expected) = e.accrued_interest {
            if !within(a.state.accrued_interest, expected, tol) {
                report.failures.push(format!(
                    "{label}: accruedInterest expected {}, actual {}",
                    expected, a.state.accrued_interest
                ));
            }
        }
        if a.currency.as_deref() != Some(e.currency.as_str()) {
            report.failures.push(format!(
                "{label}: currency expected {:?}, actual {:?}",
                e.currency, a.currency
            ));
        }
    }
    report.passed = report.failures.is_empty();
    report
}

/// Compares one case, producing a report carrying the case identifier.
pub fn compare_case(
    case_id: impl Into<String>,
    actual: &[ContractEvent],
    expected: &[ExpectedEvent],
    tol: &Tolerance,
) -> CaseReport {
    let mut report = compare(actual, expected, tol);
    report.case_id = case_id.into();
    report
}

/// Whether `actual` is within the tolerance around `expected`.
fn within(actual: Decimal, expected: Decimal, tol: &Tolerance) -> bool {
    let diff = (actual - expected).abs();
    let bound = tol
        .absolute
        .max(tol.relative * actual.abs().max(expected.abs()));
    diff <= bound
}

#[cfg(test)]
mod tests {
    use super::*;
    use actus_engine::ContractState;
    use actus_model::EventType;
    use chrono::NaiveDateTime;
    use std::str::FromStr;

    fn d(raw: &str) -> Decimal {
        Decimal::from_str(raw).expect("test decimal")
    }

    fn event(payoff: Decimal, notional: Decimal, rate: Decimal, accrued: Decimal) -> ContractEvent {
        ContractEvent {
            event_type: EventType::InterestPayment,
            time: NaiveDateTime::parse_from_str("2013-02-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
            payoff,
            currency: Some("USD".to_string()),
            state: ContractState {
                notional_principal: notional,
                nominal_interest_rate: rate,
                accrued_interest: accrued,
                ..ContractState::default()
            },
        }
    }

    fn expected(payoff: Decimal) -> ExpectedEvent {
        serde_json::from_value(serde_json::json!({
            "eventDate": "2013-02-01T00:00",
            "eventType": "IP",
            "payoff": payoff.to_string(),
            "currency": "USD",
            "notionalPrincipal": "3000",
            "nominalInterestRate": "0.1",
            "accruedInterest": "0"
        }))
        .expect("expected event")
    }

    #[test]
    fn matching_stream_passes() {
        let actual = vec![event(d("25"), d("3000"), d("0.1"), Decimal::ZERO)];
        let expected = vec![expected(d("25"))];
        let report = compare(&actual, &expected, &Tolerance::default());
        assert!(report.passed, "{:?}", report.failures);
    }

    #[test]
    fn sub_cent_drift_passes_and_cent_drift_fails() {
        let actual = vec![event(
            d("25.0049999999"),
            d("3000"),
            d("0.1"),
            Decimal::ZERO,
        )];
        let expected = vec![expected(d("25"))];
        let report = compare(&actual, &expected, &Tolerance::default());
        assert!(report.passed, "{:?}", report.failures);

        let drifting = vec![event(d("25.02"), d("3000"), d("0.1"), Decimal::ZERO)];
        let report = compare(&drifting, &expected, &Tolerance::default());
        assert!(!report.passed);
        assert!(report.failures[0].contains("payoff"));
    }

    #[test]
    fn count_mismatch_and_type_mismatch_are_reported() {
        let actual: Vec<ContractEvent> = Vec::new();
        let expected = vec![expected(d("25"))];
        let report = compare(&actual, &expected, &Tolerance::default());
        assert!(!report.passed);
        assert!(report.failures[0].starts_with("event count mismatch"));

        let swapped = vec![event(Decimal::ZERO, d("3000"), d("0.1"), Decimal::ZERO)];
        let mut mismatch = expected.clone();
        mismatch[0].event_type = EventType::Maturity;
        let report = compare(&swapped, &mismatch, &Tolerance::default());
        assert!(!report.passed);
        assert!(report.failures[0].contains("type expected MD"));
    }

    #[test]
    fn state_mismatch_reports_field_name() {
        let actual = vec![event(d("25"), d("2999"), d("0.1"), Decimal::ZERO)];
        let expected = vec![expected(d("25"))];
        let report = compare(&actual, &expected, &Tolerance::default());
        assert!(!report.passed);
        assert!(report
            .failures
            .iter()
            .any(|f| f.contains("notionalPrincipal")));
    }

    #[test]
    fn compare_case_carries_the_case_id() {
        let report = compare_case("pam01", &[], &[], &Tolerance::default());
        assert_eq!(report.case_id, "pam01");
        assert!(report.passed);
    }
}
