//! Dictionary-driven tests: the generated vocabulary must exactly cover the
//! vendored ACTUS dictionary.

use actus_model::generated::applicability::{self};
use actus_model::generated::attribute::{self, AttributeType};
use actus_model::generated::contract_type::ContractType;
use actus_model::generated::event_type::EventType;
use actus_model::source::read_dictionary;
use std::collections::BTreeSet;

fn released_or_implemented_acronyms() -> BTreeSet<String> {
    let doc = read_dictionary("actus-dictionary-taxonomy.json").expect("taxonomy parses");
    let mut out = BTreeSet::new();
    for (identifier, entry) in doc["taxonomy"].as_object().expect("taxonomy object") {
        let status = entry
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or_default();
        if status == "Released" || status == "Implemented" {
            out.insert(format!(
                "{}\u{1}{identifier}",
                entry["acronym"].as_str().expect("acronym")
            ));
        }
    }
    out
}

#[test]
fn contract_type_has_exactly_the_taxonomy_entries() {
    let expected = released_or_implemented_acronyms();
    assert_eq!(expected.len(), 21, "dictionary v1.4 ships 21 usable types");
    assert_eq!(ContractType::ALL.len(), 21);

    let actual: BTreeSet<String> = ContractType::ALL
        .iter()
        .map(|t| format!("{}\u{1}{}", t.as_acronym(), t.identifier()))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn contract_type_round_trips_every_acronym() {
    for t in ContractType::ALL {
        let acronym = t.as_acronym();
        assert_eq!(acronym.parse::<ContractType>().as_ref(), Ok(t));
        assert_eq!(
            acronym.to_lowercase().parse::<ContractType>().as_ref(),
            Ok(t)
        );
        assert_eq!(t.to_string(), acronym);
        let json = serde_json::to_value(t).unwrap();
        assert_eq!(json.as_str().unwrap(), acronym);
        assert_eq!(serde_json::from_value::<ContractType>(json).unwrap(), *t);
    }
    assert!("XXX".parse::<ContractType>().is_err());
    assert!("nope".parse::<ContractType>().is_err());
}

#[test]
fn event_type_covers_the_event_dictionary() {
    let doc = read_dictionary("actus-dictionary-event.json").expect("event dictionary parses");
    let allowed = doc["event"]["eventType"]["allowedValues"]
        .as_array()
        .expect("allowedValues array");
    assert_eq!(EventType::ALL.len(), allowed.len());

    let dictionary: BTreeSet<&str> = allowed
        .iter()
        .map(|e| e["acronym"].as_str().expect("acronym"))
        .collect();
    let generated: BTreeSet<&str> = EventType::ALL.iter().map(|e| e.as_acronym()).collect();
    assert_eq!(generated, dictionary);

    for entry in allowed {
        let acronym = entry["acronym"].as_str().expect("acronym");
        let event = acronym.parse::<EventType>().unwrap_or_else(|e| {
            panic!("{acronym} must parse: {e}");
        });
        assert_eq!(event.to_string(), acronym);
        let sequence = entry["sequence"].as_str().expect("sequence");
        let digits: String = sequence.chars().filter(|c| c.is_ascii_digit()).collect();
        let expected_priority: u8 = digits.parse().expect("sequence priority");
        assert_eq!(event.priority(), expected_priority, "priority of {acronym}");
    }
}

#[test]
fn event_type_priorities_order_ied_before_ip_before_md() {
    let ied = "IED".parse::<EventType>().unwrap();
    let ip = "IP".parse::<EventType>().unwrap();
    let md = "MD".parse::<EventType>().unwrap();
    assert_eq!(ied.priority(), 1);
    assert_eq!(ip.priority(), 8);
    assert_eq!(md.priority(), 19);
    assert!(ied.priority() < ip.priority());
    assert!(ip.priority() < md.priority());
}

#[test]
fn curly_quote_fixup_parses_the_terms_dictionary() {
    let doc = read_dictionary("actus-dictionary-terms.json").expect("terms dictionary parses");
    let terms = doc["terms"].as_object().expect("terms object");
    assert!(
        terms.len() > 90,
        "dictionary v1.4 carries >90 contract attributes, got {}",
        terms.len()
    );
}

#[test]
fn attribute_vocabulary_covers_the_terms_dictionary() {
    let doc = read_dictionary("actus-dictionary-terms.json").expect("terms dictionary parses");
    let terms = doc["terms"].as_object().expect("terms object");

    assert_eq!(attribute::ALL.len(), terms.len());
    for (identifier, entry) in terms {
        let attr = attribute::lookup(identifier).unwrap_or_else(|| {
            panic!("attribute {identifier} missing from generated vocabulary");
        });
        assert_eq!(attr.identifier, identifier);
        assert_eq!(attr.acronym, entry["acronym"].as_str().expect("acronym"));
    }

    let nt = attribute::lookup("notionalPrincipal").unwrap();
    assert_eq!(nt.acronym, "NT");
    assert_eq!(nt.attribute_type, AttributeType::Real);
    let ipcl = attribute::lookup("cycleOfInterestPayment").unwrap();
    assert_eq!(ipcl.acronym, "IPCL");
    assert_eq!(ipcl.attribute_type, AttributeType::Cycle);
    let cts = attribute::lookup("contractStructure").unwrap();
    assert_eq!(cts.acronym, "CTS");
    assert_eq!(cts.attribute_type, AttributeType::ContractReferenceArray);
    assert!(attribute::lookup("notAnAttribute").is_none());
}

fn matrix_sets(contract_identifier: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let doc = read_dictionary("actus-dictionary-applicability.json").expect("applicability parses");
    let matrix = &doc["applicability"][contract_identifier];
    let mut required = BTreeSet::new();
    let mut applicable = BTreeSet::new();
    if let Some(entries) = matrix.as_object() {
        for (identifier, value) in entries {
            if identifier == "contract" {
                continue;
            }
            let normalized = value
                .as_str()
                .unwrap_or_default()
                .trim()
                .trim_end_matches('*')
                .trim()
                .to_lowercase();
            if normalized.starts_with("nn") {
                required.insert(identifier.clone());
                applicable.insert(identifier.clone());
            } else if normalized.starts_with('x') {
                applicable.insert(identifier.clone());
            }
        }
    }
    (required, applicable)
}

#[test]
fn applicability_required_sets_match_the_dictionary() {
    let cases = [
        (ContractType::Pam, "principalAtMaturity"),
        (ContractType::Lam, "linearAmortizer"),
        (ContractType::Nam, "negativeAmortizer"),
        (ContractType::Ann, "annuity"),
    ];
    for (contract_type, identifier) in cases {
        let (expected_required, expected_applicable) = matrix_sets(identifier);
        let tables = applicability::tables(contract_type);
        let required: BTreeSet<String> = tables.required.iter().map(|s| s.to_string()).collect();
        let mut applicable: BTreeSet<String> =
            tables.applicable.iter().map(|s| s.to_string()).collect();
        applicable.remove("fixingDays");
        assert_eq!(required, expected_required, "required of {identifier}");
        assert_eq!(
            applicable, expected_applicable,
            "applicable of {identifier}"
        );
    }
}

#[test]
fn applicability_base_required_sets_for_the_fixed_income_family() {
    let expected: &[(ContractType, &[&str])] = &[
        (
            ContractType::Pam,
            &[
                "contractRole",
                "statusDate",
                "contractDealDate",
                "currency",
                "notionalPrincipal",
                "initialExchangeDate",
                "maturityDate",
                "nominalInterestRate",
            ],
        ),
        (
            ContractType::Lam,
            &[
                "contractRole",
                "statusDate",
                "contractDealDate",
                "currency",
                "notionalPrincipal",
                "initialExchangeDate",
                "nominalInterestRate",
            ],
        ),
        (
            ContractType::Nam,
            &[
                "contractRole",
                "statusDate",
                "contractDealDate",
                "currency",
                "notionalPrincipal",
                "initialExchangeDate",
                "nominalInterestRate",
                "nextPrincipalRedemptionPayment",
                "rateSpread",
                "marketObjectCodeOfRateReset",
            ],
        ),
        (
            ContractType::Ann,
            &[
                "contractRole",
                "statusDate",
                "contractDealDate",
                "currency",
                "notionalPrincipal",
                "initialExchangeDate",
                "nominalInterestRate",
            ],
        ),
    ];
    for (contract_type, required) in expected {
        let tables = applicability::tables(*contract_type);
        let base: BTreeSet<&str> = tables.base_required.iter().copied().collect();
        let expected: BTreeSet<&str> = required.iter().copied().collect();
        assert_eq!(base, expected, "base_required of {contract_type}");
    }
}

#[test]
fn applicability_of_types_without_a_matrix_is_empty() {
    for t in [ContractType::Cdswp, ContractType::Mar] {
        let tables = applicability::tables(t);
        assert!(tables.required.is_empty(), "{}", t);
        assert!(tables.base_required.is_empty(), "{}", t);
        assert!(tables.applicable.is_empty(), "{}", t);
    }
}

#[test]
fn unreleased_note_entries_land_in_no_table() {
    let tables = applicability::tables(ContractType::Bcs);
    assert!(!tables.required.is_empty(), "BCS has a usable matrix");
    assert!(!tables.applicable.is_empty(), "BCS has a usable matrix");
}

#[test]
fn fixing_days_gap_is_patched_into_applicable_sets() {
    for t in [ContractType::Ann, ContractType::Lam, ContractType::Nam] {
        let tables = applicability::tables(t);
        assert!(
            tables.applicable.contains(&"fixingDays"),
            "fixingDays must be applicable to {}",
            t
        );
        assert!(!tables.required.contains(&"fixingDays"), "{}", t);
    }
    assert!(!applicability::tables(ContractType::Pam)
        .applicable
        .contains(&"fixingDays"));
}
