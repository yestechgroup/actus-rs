//! The `#[wasm_bindgen]` exports run inside a real wasm runtime (Node via
//! `wasm-bindgen-test-runner`), including the error paths: those build a
//! `JsValue`, which only exists on wasm targets, so `tests/wasm_api.rs`
//! cannot reach them natively.
//!
//! ```sh
//! cargo test -p actus-wasm --target wasm32-unknown-unknown
//! ```
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

const PAM_TERMS: &str = r#"{
    "contractType": "PAM",
    "contractID": "pam01",
    "statusDate": "2012-12-30T00:00:00",
    "currency": "USD",
    "notionalPrincipal": "3000",
    "initialExchangeDate": "2013-01-01T00:00:00",
    "maturityDate": "2014-01-01T00:00:00",
    "nominalInterestRate": "0.1",
    "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
    "cycleOfInterestPayment": "P1ML0",
    "dayCountConvention": "A365",
    "contractRole": "RPA"
}"#;

/// The error message carried by a rejected export, as a JS string.
fn message(err: JsValue) -> String {
    err.as_string().expect("errors are JS strings")
}

fn parse(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("exports return JSON")
}

#[wasm_bindgen_test]
fn metadata_exports_return_json_arrays() {
    for json in [
        actus_wasm::contract_types(),
        actus_wasm::applicability_matrix(),
        actus_wasm::attribute_meta(),
    ] {
        assert!(parse(&json).as_array().is_some_and(|a| !a.is_empty()));
    }
}

#[wasm_bindgen_test]
fn evaluate_runs_in_the_wasm_runtime() {
    let result = parse(&actus_wasm::evaluate(PAM_TERMS.to_string()).unwrap());
    let events = result["events"].as_array().expect("events");
    assert_eq!(events.first().unwrap()["eventType"], "IED");
    assert_eq!(events.last().unwrap()["eventType"], "MD");
    let with_risk =
        parse(&actus_wasm::evaluate_with_risk(PAM_TERMS.to_string(), "{}".to_string()).unwrap());
    assert_eq!(result, with_risk);
}

#[wasm_bindgen_test]
fn evaluate_errors_reach_js_as_strings() {
    assert!(!message(actus_wasm::evaluate("not json".to_string()).unwrap_err()).is_empty());
    assert!(!message(
        actus_wasm::evaluate_with_risk(PAM_TERMS.to_string(), "not json".to_string()).unwrap_err()
    )
    .is_empty());
}

#[wasm_bindgen_test]
fn validate_and_applicability_errors_reach_js_as_strings() {
    assert!(!message(actus_wasm::validate("{".to_string()).unwrap_err()).is_empty());
    assert!(parse(&actus_wasm::applicability("PAM".to_string()).unwrap()).is_object());
    assert!(message(actus_wasm::applicability("XXX".to_string()).unwrap_err()).contains("XXX"));
}

#[wasm_bindgen_test]
fn spec_and_terms_builders_report_bad_input() {
    assert!(parse(&actus_wasm::contract_param_specs("ann".to_string()).unwrap()).is_array());
    assert!(
        message(actus_wasm::contract_param_specs("XXX".to_string()).unwrap_err()).contains("XXX")
    );
    let built = parse(
        &actus_wasm::build_terms_json(
            "PAM".to_string(),
            r#"{"notionalPrincipal": "1000"}"#.to_string(),
        )
        .unwrap(),
    );
    assert_eq!(built["notionalPrincipal"], "1000");
    assert!(message(
        actus_wasm::build_terms_json("PAM".to_string(), "[]".to_string()).unwrap_err()
    )
    .starts_with("invalid values JSON"));
    assert!(actus_wasm::build_terms_json("XXX".to_string(), "{}".to_string()).is_err());
}
