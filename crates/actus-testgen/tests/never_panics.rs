//! Stable-toolchain twin of the cargo-fuzz targets in `fuzz/`.
//!
//! The property: for any input, the JSON entry points of `actus-wasm`
//! (`validate_json`, `evaluate_json_with_risk`) return `Ok` or `Err`. They
//! never panic, which in the WASM build would abort and crash the ACTUS
//! Explorer, and they finish in bounded time.
//!
//! The inputs are the structure-aware cases of [`actus_testgen::generate`]
//! (extreme but well-formed terms for all 18 engine types) plus raw strings.
//! The seed is fixed, so plain CI is deterministic; the nightly fuzz job
//! explores beyond it.

use std::time::{Duration, Instant};

use actus_wasm::dto::{evaluate_json_with_risk, validate_json};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

/// Generous wall-clock bound per evaluation in an unoptimised test build. A
/// hang (an unbounded loop) blows through it; slow-but-finite schedules
/// near the engine's roll limit stay well inside.
const TIME_BOUND: Duration = Duration::from_secs(10);

fn config(cases: u32) -> Config {
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x00AC_7005),
        failure_persistence: None,
        ..Config::default()
    }
}

/// Runs both entry points on one input and checks the time bound. A panic
/// fails the property through proptest's own panic handling.
fn check(terms: &str, scenario: &str) -> Result<(), TestCaseError> {
    let started = Instant::now();
    let _ = validate_json(terms);
    let _ = evaluate_json_with_risk(terms, scenario);
    let elapsed = started.elapsed();
    prop_assert!(
        elapsed < TIME_BOUND,
        "evaluation took {elapsed:?}\nterms: {terms}\nscenario: {scenario}"
    );
    Ok(())
}

proptest! {
    #![proptest_config(config(2048))]

    #[test]
    fn generated_terms_never_panic_and_finish(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let case = actus_testgen::generate(&data);
        check(&case.terms, &case.scenario)?;
    }
}

proptest! {
    #![proptest_config(config(512))]

    #[test]
    fn arbitrary_strings_never_panic(terms in ".{0,200}", scenario in ".{0,50}") {
        check(&terms, &scenario)?;
    }
}

/// Deterministic sweep: every seed (all 18 engine types), with each of its
/// decimal attributes in turn at the edges of the `Decimal` range. Before
/// checked arithmetic this panicked (overflow, division by zero) in most
/// engines.
#[test]
fn every_seed_survives_extreme_decimals() {
    let extremes = [
        "79228162514264337593543950335",
        "-79228162514264337593543950335",
        "0.0000000000000000000000000001",
        "0",
    ];
    let mut evaluated = 0;
    for seed in actus_testgen::seed_cases() {
        for value in extremes {
            for terms in actus_testgen::with_each_decimal_set_to(&seed.terms, value) {
                check(&terms, &seed.scenario).expect("bounded time");
                evaluated += 1;
            }
        }
    }
    assert!(evaluated > 1000, "only {evaluated} variants");
}
