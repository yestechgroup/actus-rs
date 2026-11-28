//! Spec-derived invariants over the seed contracts of all 18 engine types
//! (`actus_testgen::seed_cases`: the testbeds, the Explorer defaults and the
//! hand-written seeds of `crates/actus-testgen/seeds/`).

use actus_wasm::dto::evaluate_json_with_risk;
use serde_json::Value;

/// The contract role sign function `R(CNTRL)` (techspec section "Contract
/// Role Sign Convention", table `tbl:cntrl`), over the v1.4 dictionary role
/// vocabulary. `RF`/`PF` (receive/pay fix) are not in the techspec table;
/// they sign like `RFL`/`PFL`, as in actus-core `ContractRoleConvention`.
const CLAIMS: [&str; 8] = ["RPA", "RFL", "RF", "BUY", "COL", "CNO", "UDL", "UDLP"];
const OBLIGATIONS: [&str; 5] = ["RPL", "PFL", "PF", "SEL", "UDLM"];

fn events(terms: &Value, scenario: &str) -> Option<Vec<Value>> {
    let result = evaluate_json_with_risk(&terms.to_string(), scenario).ok()?;
    let parsed: Value = serde_json::from_str(&result).ok()?;
    parsed["events"].as_array().cloned()
}

fn decimal(value: &Value) -> rust_decimal::Decimal {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

/// Every role of a sign group yields the same events, and the two groups
/// mirror each other: payoffs and the role-signed states (notional, accrued
/// interest) negate. Exceptions, both by the spec's own formulas:
/// - `PRD`/`TD` pay `sgn x (price + Ipac)` with an already role-signed
///   `Ipac`, which is not a mirror (SI-001);
/// - CAPFL reports the states of its underlying, which is evaluated under
///   its own role (techspec CAPFL: `CNTRL = RPA` for the child), so only
///   the payoffs mirror.
///
/// SWAPS accepts only the leg roles `RFL`/`PFL` and SWPPV only `RF`/`PF` (and
/// their `RFL`/`PFL` synonyms), so both are excluded; the SWPPV unit tests
/// check that `RF` mirrors `PF`.
#[test]
fn role_sign_groups_agree_and_mirror() {
    let mut compared = 0;
    for seed in actus_testgen::seed_cases() {
        let terms: Value = serde_json::from_str(&seed.terms).expect("seed terms");
        let contract_type = terms["contractType"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if contract_type == "SWAPS" || contract_type == "SWPPV" {
            continue;
        }
        let with_role = |role: &str| {
            let mut t = terms.clone();
            t["contractRole"] = Value::String(role.to_string());
            events(&t, &seed.scenario)
        };
        let Some(claim) = with_role("RPA") else {
            continue;
        };
        for role in CLAIMS {
            assert_eq!(
                with_role(role).as_ref(),
                Some(&claim),
                "{contract_type} {role} must evaluate like RPA\n{}",
                seed.terms
            );
        }
        let obligation = with_role("RPL").unwrap_or_else(|| {
            panic!(
                "{contract_type} RPL must evaluate like RPA does\n{}",
                seed.terms
            )
        });
        for role in OBLIGATIONS {
            assert_eq!(
                with_role(role).as_ref(),
                Some(&obligation),
                "{contract_type} {role} must evaluate like RPL\n{}",
                seed.terms
            );
        }
        assert_eq!(
            claim.len(),
            obligation.len(),
            "{contract_type}\n{}",
            seed.terms
        );
        for (a, b) in claim.iter().zip(&obligation) {
            let event_type = a["eventType"].as_str().unwrap_or_default();
            assert_eq!(event_type, b["eventType"].as_str().unwrap_or_default());
            if event_type != "PRD" && event_type != "TD" {
                assert_eq!(
                    decimal(&b["payoff"]),
                    -decimal(&a["payoff"]),
                    "{contract_type} {event_type} payoff\n{}",
                    seed.terms
                );
            }
            if contract_type != "CAPFL" {
                for field in ["notionalPrincipal", "accruedInterest"] {
                    assert_eq!(
                        decimal(&b[field]),
                        -decimal(&a[field]),
                        "{contract_type} {event_type} {field}\n{}",
                        seed.terms
                    );
                }
            }
        }
        compared += 1;
    }
    assert!(compared > 100, "only {compared} seeds compared");
}
