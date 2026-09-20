//! Native integration tests for the WASM binding contract: the golden PAM
//! schedule comes from the vendored testbed case `pam01`.

use actus_model::generated::contract_type::ContractType;
use serde_json::Value;

const PAM01_TERMS: &str = r#"{
    "contractType": "PAM",
    "contractID": "pam01",
    "statusDate": "2012-12-30T00:00:00",
    "contractDealDate": "2012-12-28T00:00:00",
    "currency": "USD",
    "notionalPrincipal": "3000",
    "initialExchangeDate": "2013-01-01T00:00:00",
    "maturityDate": "2014-01-01T00:00:00",
    "nominalInterestRate": "0.1",
    "cycleAnchorDateOfInterestPayment": "2013-01-01T00:00:00",
    "cycleOfInterestPayment": "P1ML0",
    "dayCountConvention": "A365",
    "endOfMonthConvention": "SD",
    "premiumDiscountAtIED": "0",
    "rateMultiplier": "1.0",
    "contractRole": "RPA"
}"#;

#[test]
fn evaluate_runs_the_pam01_testbed_schedule() {
    let json = actus_wasm::evaluate(PAM01_TERMS.to_string()).expect("evaluation succeeds");
    let v: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(v["contractStatus"], "matured");
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 15, "IED + 13 IP + MD");
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["eventType"].as_str().unwrap())
        .collect();
    assert_eq!(types[0], "IED");
    assert!(types[1..14].iter().all(|t| *t == "IP"));
    assert_eq!(types[14], "MD");

    assert_eq!(events[0]["eventDate"], "2013-01-01T00:00:00");
    assert_eq!(events[0]["payoff"], "-3000");
    assert_eq!(events[0]["currency"], "USD");
    assert_eq!(events[0]["notionalPrincipal"], "3000");
    assert_eq!(events[14]["eventDate"], "2014-01-01T00:00:00");
    assert_eq!(events[14]["payoff"], "3000");
    assert_eq!(events[14]["notionalPrincipal"], "0");

    let february_ip = &events[2];
    assert_eq!(february_ip["eventDate"], "2013-02-01T00:00:00");
    let payoff: f64 = february_ip["payoff"].as_str().unwrap().parse().unwrap();
    assert!(
        (payoff - 25.4794520547945).abs() < 1e-6,
        "testbed pam01 February IP payoff, got {payoff}"
    );
}

#[test]
fn evaluate_rejects_broken_input() {
    assert!(actus_wasm::dto::evaluate_json("not json").is_err());
    assert!(actus_wasm::dto::evaluate_json(r#"{"contractType": "PAM"}"#).is_err());
    assert!(actus_wasm::dto::evaluate_json(r#"{"contractType": "XXX"}"#).is_err());
}

#[test]
fn validate_rejects_attribute_not_applicable_to_pam() {
    let terms = r#"{
        "contractType": "PAM",
        "cycleOfPrincipalRedemption": "P1ML0"
    }"#;
    let json = actus_wasm::dto::validate_json(terms).expect("validation runs");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["valid"], false);
    let errors = v["errors"].as_array().unwrap();
    let not_applicable: Vec<&Value> = errors
        .iter()
        .filter(|e| e["code"] == "AttributeNotApplicable")
        .collect();
    assert_eq!(not_applicable.len(), 1);
    assert_eq!(not_applicable[0]["attribute"], "cycleOfPrincipalRedemption");
    assert_eq!(v["termStatus"].get("cycleOfPrincipalRedemption"), None);
}

#[test]
fn validate_flags_missing_base_required_attributes() {
    let terms = r#"{
        "contractType": "PAM",
        "contractRole": "RPA",
        "statusDate": "2012-12-30T00:00:00",
        "currency": "USD",
        "initialExchangeDate": "2013-01-01T00:00:00",
        "maturityDate": "2014-01-01T00:00:00",
        "nominalInterestRate": "0.1",
        "notionalPrincipal": "3000"
    }"#;
    let json = actus_wasm::dto::validate_json(terms).expect("validation runs");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["valid"], false);
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "only contractDealDate missing: {json}");
    assert_eq!(errors[0]["code"], "MissingAttribute");
    assert_eq!(errors[0]["attribute"], "contractDealDate");
    assert_eq!(v["termStatus"]["notionalPrincipal"], "required-set");
    assert_eq!(v["termStatus"]["contractDealDate"], "required-missing");
    assert_eq!(v["termStatus"]["accruedInterest"], "optional-unset");
}

#[test]
fn validate_flags_unknown_attributes() {
    let terms = r#"{
        "contractType": "PAM",
        "statusDate": "2012-12-30T00:00:00",
        "contractRole": "RPA",
        "contractDealDate": "2012-12-28T00:00:00",
        "currency": "USD",
        "initialExchangeDate": "2013-01-01T00:00:00",
        "maturityDate": "2014-01-01T00:00:00",
        "nominalInterestRate": "0.1",
        "notionalPrincipal": "3000",
        "shrinkageDate": "2013-06-01T00:00:00"
    }"#;
    let json = actus_wasm::dto::validate_json(terms).expect("validation runs");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["valid"], false);
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "UnknownAttribute");
    assert_eq!(errors[0]["attribute"], "shrinkageDate");
}

#[test]
fn validate_rejects_unparseable_terms() {
    assert!(actus_wasm::dto::validate_json("[]").is_err());
    assert!(actus_wasm::dto::validate_json("not json").is_err());
    assert!(actus_wasm::dto::validate_json(r#"{"contractType": "XXX"}"#).is_err());
}

#[test]
fn validate_accepts_a_complete_pam() {
    let json = actus_wasm::validate(PAM01_TERMS.to_string()).expect("validation runs");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["valid"], true);
    assert_eq!(v["errors"].as_array().unwrap().len(), 0);
}

#[test]
fn contract_types_covers_the_taxonomy() {
    let json = actus_wasm::contract_types();
    let v: Value = serde_json::from_str(&json).unwrap();
    let array = v.as_array().unwrap();
    assert_eq!(array.len(), ContractType::ALL.len());
    let pam = array.iter().find(|t| t["acronym"] == "PAM").unwrap();
    assert_eq!(pam["identifier"], "principalAtMaturity");
    assert_eq!(pam["category"], "Basic");
    let swaps = array.iter().find(|t| t["acronym"] == "SWAPS").unwrap();
    assert_eq!(swaps["category"], "Combined");
}

#[test]
fn applicability_parses_case_insensitively_and_rejects_unknown() {
    let json = actus_wasm::dto::applicability_json("pam").expect("PAM parses");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v["baseRequired"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "notionalPrincipal"));
    assert!(actus_wasm::dto::applicability_json("XXX").is_err());
}

#[test]
fn applicability_matrix_covers_all_contract_types() {
    let json = actus_wasm::applicability_matrix();
    let v: Value = serde_json::from_str(&json).unwrap();
    let array = v.as_array().unwrap();
    assert_eq!(array.len(), ContractType::ALL.len());
    for entry in array {
        assert!(entry["acronym"].is_string());
        assert!(entry["required"].is_array());
        assert!(entry["baseRequired"].is_array());
        assert!(entry["applicable"].is_array());
    }
    let pam = array.iter().find(|e| e["acronym"] == "PAM").unwrap();
    assert!(!pam["baseRequired"].as_array().unwrap().is_empty());
    let cdswp = array.iter().find(|e| e["acronym"] == "CDSWP").unwrap();
    assert!(cdswp["applicable"].as_array().unwrap().is_empty());
}

#[test]
fn attribute_meta_carries_labels_and_descriptions() {
    let json = actus_wasm::attribute_meta();
    let v: Value = serde_json::from_str(&json).unwrap();
    let array = v.as_array().unwrap();
    assert!(!array.is_empty());
    let nt = array
        .iter()
        .find(|a| a["acronym"] == "NT")
        .expect("NT in dictionary");
    assert_eq!(nt["name"], "Notional Principal");
    assert!(!nt["description"].as_str().unwrap().is_empty());
    assert_eq!(nt["dataType"], "Real");
    assert!(array
        .iter()
        .all(|a| a["acronym"].is_string() && a["dataType"].is_string()));
}
