//! Differential gate: the pinned randomized fixed-income corpus (PAM, LAM,
//! NAM, ANN — 512 cases each, 2048 in total), evaluated by the engine and by
//! the independent in-Rust oracle, compared event by event.
//!
//! Event types and times must match exactly; payoffs and the `NT`/`IPNR`/
//! `IPAC`/`PRNXT` states must agree within the tolerance derived from the
//! oracle's fixed point regime (see `actus_conformance::oracle` module docs:
//! `|engine - oracle| <= 1e-5 + 1e-8 x max(|engine|, |oracle|)`). Any
//! mismatch panics with the case id, the event index and both values.
//! Nothing is tolerated away: the gate carries no divergence list. Three
//! discrepancies the corpus exposed have been resolved rather than
//! tolerated: intermediate `IPCI` transitions calculated on the unshifted
//! schedule date under `SC*` conventions (against `docs/actus.md` section
//! 3.4), which also broke the `IP`/`IPCI` schedule deduplication, and a
//! shifted `IPCI` double-count of the accrued interest — both fixed as
//! engine defects — and the emission of schedule elements whose preceding
//! business day shift moved them before the initial exchange date, which
//! the ANN implementation suppressed per the ann09 reading while PAM, LAM
//! and NAM reported them; the engine now suppresses them uniformly, with
//! the official testbeds still green. The one remaining shape the corpus
//! exercises that no official fixture pins (NAM capitalization dates
//! landing on redemption dates) is a documented spec ambiguity both sides
//! resolve through the fixture-pinned replacement machinery; see the
//! oracle module docs.

use std::str::FromStr;

use chrono::NaiveDateTime;

use rust_decimal::Decimal;

use actus_conformance::{corpus, engine_registry, oracle};
use actus_model::{ContractType, EventType};

const ABSOLUTE_TOLERANCE: &str = "0.00001";
const RELATIVE_TOLERANCE: &str = "0.00000001";

#[test]
fn pam_corpus_agrees_with_the_independent_oracle() {
    compare_slice(ContractType::Pam, oracle::evaluate_pam);
}

#[test]
fn lam_corpus_agrees_with_the_independent_oracle() {
    compare_slice(ContractType::Lam, oracle::evaluate_lam);
}

#[test]
fn nam_corpus_agrees_with_the_independent_oracle() {
    compare_slice(ContractType::Nam, oracle::evaluate_nam);
}

#[test]
fn ann_corpus_agrees_with_the_independent_oracle() {
    compare_slice(ContractType::Ann, oracle::evaluate_ann);
}

/// The oracle entry-point shape shared by the per-contract-type evaluators.
type OracleEvaluate = fn(
    &actus_model::ContractTerms,
    &[(NaiveDateTime, Decimal)],
) -> Result<Vec<oracle::OracleEvent>, String>;

/// Compares one contract-type slice of the pinned corpus: engine events
/// against oracle events, strictly, for every case. The four slices jointly
/// gate all 2048 corpus cases.
fn compare_slice(contract_type: ContractType, oracle_evaluate: OracleEvaluate) {
    let cases = corpus::generate(corpus::CORPUS_SEED, corpus::DIFFERENTIAL_CASES);
    let slice: Vec<&corpus::CorpusCase> = cases
        .iter()
        .filter(|case| case.terms.contract_type == contract_type)
        .collect();
    assert_eq!(
        slice.len(),
        512,
        "the pinned corpus carries 512 {contract_type} cases"
    );

    let registry = engine_registry();
    let mut compared_events = 0;
    for case in &slice {
        let mut provider = actus_engine::StateProvider::new();
        if let Some(code) = case.terms.market_object_code_of_rate_reset.as_deref() {
            for (time, value) in &case.risk_factors {
                provider = provider.with_rate(code, *time, *value);
            }
        }
        let engine_events = registry
            .evaluate(&case.terms, &provider)
            .unwrap_or_else(|e| panic!("{}: engine evaluation failed: {e}", case.id));
        let oracle_events = oracle_evaluate(&case.terms, &case.risk_factors)
            .unwrap_or_else(|e| panic!("{}: oracle evaluation failed: {e}", case.id));
        if engine_events.len() != oracle_events.len() {
            let engine_types: Vec<(EventType, chrono::NaiveDateTime)> = engine_events
                .iter()
                .map(|event| (event.event_type, event.time))
                .collect();
            let oracle_types: Vec<(EventType, chrono::NaiveDateTime)> = oracle_events
                .iter()
                .map(|event| (event.event_type, event.time))
                .collect();
            let first_split = engine_types
                .iter()
                .zip(oracle_types.iter())
                .position(|(engine, oracle)| engine != oracle);
            panic!(
                "{}: engine and oracle disagree: event count: engine {}, oracle {}; \
                 first stream split at {first_split:?}: engine {engine_types:?} vs oracle {oracle_types:?}",
                case.id,
                engine_events.len(),
                oracle_events.len()
            );
        }

        for (index, engine) in engine_events.iter().enumerate() {
            let expected = &oracle_events[index];
            let mut failures: Vec<String> = Vec::new();
            if engine.event_type != expected.event_type {
                failures.push(format!(
                    "type: engine {}, oracle {}",
                    engine.event_type, expected.event_type
                ));
            }
            if engine.time != expected.time {
                failures.push(format!(
                    "time: engine {}, oracle {}",
                    engine.time, expected.time
                ));
            }
            compare_field(
                &mut failures,
                "payoff",
                engine.payoff,
                oracle::money_to_decimal(expected.payoff),
            );
            compare_field(
                &mut failures,
                "notional",
                engine.state.notional_principal,
                oracle::money_to_decimal(expected.notional),
            );
            compare_field(
                &mut failures,
                "rate",
                engine.state.nominal_interest_rate,
                oracle::rate_to_decimal(expected.rate),
            );
            compare_field(
                &mut failures,
                "accrued",
                engine.state.accrued_interest,
                oracle::money_to_decimal(expected.accrued),
            );
            compare_field(
                &mut failures,
                "nextPrincipalRedemption",
                engine.state.next_principal_redemption_payment,
                oracle::money_to_decimal(expected.next_principal_redemption),
            );
            if !failures.is_empty() {
                panic!(
                    "{}: engine and oracle disagree at event[{index}]: {}",
                    case.id,
                    failures.join("; ")
                );
            }
        }
        compared_events += engine_events.len();
    }
    assert!(
        compared_events > 5_000,
        "{contract_type}: compared {compared_events} events"
    );
}

/// Compares one numeric field against the derived tolerance.
fn compare_field(failures: &mut Vec<String>, name: &str, engine: Decimal, oracle: Decimal) {
    let difference = (engine - oracle).abs();
    let magnitude = engine.abs().max(oracle.abs());
    let bound = Decimal::from_str(ABSOLUTE_TOLERANCE).expect("absolute tolerance")
        + Decimal::from_str(RELATIVE_TOLERANCE).expect("relative tolerance") * magnitude;
    if difference > bound {
        failures.push(format!(
            "{name}: engine {engine}, oracle {oracle} (diff {difference} > bound {bound})"
        ));
    }
}
