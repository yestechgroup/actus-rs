//! Official-testbed conformance gates for the ACTUS engine.

use actus_conformance::{cases, comparator, engine_registry};
use actus_model::ContractType;

/// Tolerance used across the conformance runs (one cent absolute, relative
/// widening for large numbers).
fn tolerance() -> comparator::Tolerance {
    comparator::Tolerance::default()
}

/// Evaluates every loaded case through the conformance registry and compares
/// against the expected results.
fn evaluate_all(
    contract_type: ContractType,
) -> (
    Vec<comparator::CaseReport>,
    usize,
    usize,
    Vec<cases::ActusTestCase>,
) {
    let registry = engine_registry();
    let tol = tolerance();
    let loaded = cases::load_testbed(contract_type).expect("testbed loads");
    let mut reports = Vec::new();
    let mut actual_events = 0;
    let mut expected_events = 0;
    for case in &loaded {
        let events = match cases::evaluate_case(&registry, case) {
            Ok(events) => events,
            Err(e) => {
                reports.push(comparator::CaseReport {
                    case_id: case.identifier.clone(),
                    passed: false,
                    failures: vec![format!("evaluation error: {e}")],
                });
                continue;
            }
        };
        actual_events += events.len();
        expected_events += case.results.len();
        reports.push(comparator::compare_case(
            case.identifier.clone(),
            &events,
            &case.results,
            &tol,
        ));
    }
    (reports, actual_events, expected_events, loaded)
}

#[test]
fn official_pam_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Pam);
    assert_eq!(loaded.len(), 25, "25 PAM cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("PAM conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn pam_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Pam);
    assert_eq!(expected, 347, "the official PAM testbed carries 347 events");
    assert_eq!(actual, 347, "the engine must evaluate exactly 347 events");
}

#[test]
fn official_lam_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Lam);
    assert_eq!(loaded.len(), 31, "31 LAM cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("LAM conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn lam_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Lam);
    assert_eq!(expected, 820, "the official LAM testbed carries 820 events");
    assert_eq!(actual, 820, "the engine must evaluate exactly 820 events");
}

#[test]
fn official_nam_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Nam);
    assert_eq!(loaded.len(), 22, "22 NAM cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("NAM conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn nam_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Nam);
    assert_eq!(expected, 672, "the official NAM testbed carries 672 events");
    assert_eq!(actual, 672, "the engine must evaluate exactly 672 events");
}

#[test]
fn official_ann_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Ann);
    assert_eq!(loaded.len(), 31, "31 ANN cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("ANN conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn ann_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Ann);
    assert_eq!(
        expected, 1060,
        "the official ANN testbed carries 1060 events"
    );
    assert_eq!(actual, 1060, "the engine must evaluate exactly 1060 events");
}

#[test]
fn official_csh_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Csh);
    assert_eq!(loaded.len(), 4, "4 CSH cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("CSH conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn csh_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Csh);
    assert_eq!(expected, 4, "the official CSH testbed carries 4 events");
    assert_eq!(actual, 4, "the engine must evaluate exactly 4 events");
}

#[test]
fn official_clm_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Clm);
    assert_eq!(loaded.len(), 15, "15 CLM cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("CLM conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn clm_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Clm);
    assert_eq!(expected, 123, "the official CLM testbed carries 123 events");
    assert_eq!(actual, 123, "the engine must evaluate exactly 123 events");
}

#[test]
fn official_swaps_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Swaps);
    assert_eq!(loaded.len(), 11, "11 SWAPS cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("SWAPS conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn swaps_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Swaps);
    assert_eq!(
        expected, 368,
        "the official SWAPS testbed carries 368 events"
    );
    assert_eq!(actual, 368, "the engine must evaluate exactly 368 events");
}

#[test]
fn official_cec_testbed_conformance() {
    let (reports, actual, expected, loaded) = evaluate_all(ContractType::Cec);
    assert_eq!(loaded.len(), 15, "15 CEC cases expected in the testbed");
    let failed: Vec<&comparator::CaseReport> = reports.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        let mut message = String::from("CEC conformance failures:\n");
        for report in &failed {
            message.push_str(&format!(
                "  {}: {}\n",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
        panic!("{message}");
    }
    assert_eq!(failed.len(), 0);
    assert_eq!(
        actual, expected,
        "evaluated event total must match the testbed"
    );
}

#[test]
fn cec_event_count_matches_testbed() {
    let (_, actual, expected, _) = evaluate_all(ContractType::Cec);
    assert_eq!(expected, 24, "the official CEC testbed carries 24 events");
    assert_eq!(actual, 24, "the engine must evaluate exactly 24 events");
}

// ------------------------------------------------------------------------
// The testbeds vendored with the hand-tested types. Each gate asserts the
// exact set of passing cases and the testbed's case and event totals, so a
// regression and an improvement both fail the gate until it is updated.
// A type that passes fully lists every case.

/// Evaluates every case of the named testbed and returns the identifiers of
/// the passing cases, the case count and the expected event total.
fn passing_cases(testbed: &str) -> (Vec<String>, usize, usize) {
    let registry = engine_registry();
    let tol = tolerance();
    let loaded = cases::testbed(testbed)
        .expect("testbed is listed")
        .load()
        .expect("testbed loads");
    let expected_events = loaded.iter().map(|c| c.results.len()).sum();
    let passing = loaded
        .iter()
        .filter(|case| {
            cases::evaluate_case(&registry, case)
                .map(|events| {
                    comparator::compare_case(case.identifier.clone(), &events, &case.results, &tol)
                        .passed
                })
                .unwrap_or(false)
        })
        .map(|case| case.identifier.clone())
        .collect();
    (passing, loaded.len(), expected_events)
}

/// Asserts a testbed's totals and its exact set of passing cases.
fn gate(testbed: &str, cases: usize, events: usize, expected_passing: &[&str]) {
    let (passing, loaded, expected_events) = passing_cases(testbed);
    assert_eq!(loaded, cases, "{testbed}: case count");
    assert_eq!(expected_events, events, "{testbed}: expected event total");
    assert_eq!(
        passing,
        expected_passing,
        "{testbed}: the passing cases changed; update the gate ({}/{cases} pass)",
        passing.len()
    );
}

#[test]
fn stk_testbed_gate() {
    let all = [
        "stk01", "stk02", "stk03", "stk04", "stk05", "stk06", "stk07", "stk08", "stk09", "stk10",
    ];
    gate("stk", 10, 78, &all);
}

#[test]
fn com_testbed_gate() {
    gate("com", 4, 6, &["com01", "com02", "com03", "com04"]);
}

#[test]
fn fxout_testbed_gate() {
    gate(
        "fxout",
        12,
        20,
        &[
            "fxout01", "fxout02", "fxout03", "fxout04", "fxout05", "fxout06", "fxout07", "fxout08",
            "fxout09", "fxout11", "fxout12",
        ],
    );
}

#[test]
fn swppv_testbed_gate() {
    gate("swppv", 14, 197, &[]);
}

#[test]
fn capfl_testbed_gate() {
    gate("capfl", 4, 16, &[]);
}

#[test]
fn optns_testbed_gate() {
    gate("optns", 23, 86, &[]);
}

#[test]
fn futur_testbed_gate() {
    gate("futur", 14, 50, &[]);
}

#[test]
fn ceg_testbed_gate() {
    gate("ceg", 14, 38, &[]);
}

#[test]
fn ump_testbed_gate() {
    gate("ump", 9, 63, &["ump01"]);
}

#[test]
fn lax_testbed_gate() {
    gate("lax", 18, 269, &["lax01", "lax02", "lax03"]);
}

#[test]
fn ad0_testbed_gate() {
    gate("ad0", 1, 26, &[]);
}
