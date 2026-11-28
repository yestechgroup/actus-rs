//! Raw bytes into the JSON entry points of the WASM bindings.
//!
//! Property: `validate_json`, `evaluate_json` and `evaluate_json_with_risk`
//! return `Ok` or `Err` for every input; a panic (an abort in the WASM
//! build) or a hang (libFuzzer's `-timeout`) is a finding. The input is
//! split at the first NUL byte into terms and scenario.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (terms, scenario) = match data.iter().position(|b| *b == 0) {
        Some(split) => (&data[..split], &data[split + 1..]),
        None => (data, &b"{}"[..]),
    };
    let terms = String::from_utf8_lossy(terms);
    let scenario = String::from_utf8_lossy(scenario);
    let _ = actus_wasm::dto::validate_json(&terms);
    let _ = actus_wasm::dto::evaluate_json(&terms);
    let _ = actus_wasm::dto::evaluate_json_with_risk(&terms, &scenario);
});
