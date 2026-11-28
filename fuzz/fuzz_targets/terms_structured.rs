//! Structure-aware terms for all 18 engine types (`actus_testgen::generate`):
//! realistic seed contracts mutated towards extreme cycles, dates, amounts,
//! rates and arrays, with a matching risk scenario.
//!
//! Property: validation and evaluation return `Ok` or `Err`, never panic,
//! and finish within libFuzzer's `-timeout`.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let case = actus_testgen::generate(data);
    let _ = actus_wasm::dto::validate_json(&case.terms);
    let _ = actus_wasm::dto::evaluate_json_with_risk(&case.terms, &case.scenario);
});
