//! LAX scalar-path gate: LAX without array attributes is a LAM, so every
//! official LAM testbed case, re-typed as LAX, must reproduce the LAM
//! reference results. LAX has no official testbed of its own; this gives
//! its non-array paths (IPCI, IPCB, RR/RRF, SC, PRD/TD, progressed
//! contracts) reference-level evidence.

use actus_conformance::{cases, comparator, engine_registry};
use actus_model::ContractType;

#[test]
fn every_lam_testbed_case_passes_when_evaluated_as_lax() {
    let registry = engine_registry();
    let tolerance = comparator::Tolerance::default();
    let loaded = cases::load_testbed(ContractType::Lam).expect("LAM testbed loads");
    assert_eq!(loaded.len(), 31);
    let mut failures = Vec::new();
    for mut case in loaded {
        case.terms.contract_type = ContractType::Lax;
        let report = match cases::evaluate_case(&registry, &case) {
            Ok(events) => comparator::compare_case(
                case.identifier.clone(),
                &events,
                &case.results,
                &tolerance,
            ),
            Err(e) => {
                failures.push(format!("{}: evaluation error: {e}", case.identifier));
                continue;
            }
        };
        if !report.passed {
            failures.push(format!(
                "{}: {}",
                report.case_id,
                report.failures.first().map(String::as_str).unwrap_or("")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "LAM testbed cases failing as LAX:\n  {}",
        failures.join("\n  ")
    );
}
