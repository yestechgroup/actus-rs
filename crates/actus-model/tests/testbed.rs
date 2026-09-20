//! Testbed-driven tests: every terms object in the four vendored fixed-income
//! testbeds must deserialize into `ContractTerms`, all cycle strings must
//! parse, and every case must satisfy the generated applicability tables.

use actus_model::builders::{AnnBuilder, LamBuilder, NamBuilder, PamBuilder};
use actus_model::cycle::Cycle;
use actus_model::generated::applicability;
use actus_model::generated::contract_type::ContractType;
use actus_model::serde_helpers::{decimal_from_value, parse_decimal, timestamp_from_value};
use actus_model::terms::ContractTerms;
use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn fixed_income_files() -> Vec<std::path::PathBuf> {
    let files: Vec<_> = testbed_files()
        .into_iter()
        .filter(|p| {
            let name = p.to_string_lossy();
            ["pam", "lam", "nam", "ann"]
                .iter()
                .any(|t| name.contains(t))
        })
        .collect();
    assert_eq!(files.len(), 4, "four fixed-income testbeds expected");
    files
}

fn deserializable_files() -> Vec<std::path::PathBuf> {
    testbed_files()
}

fn testbed_files() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/actus/tests");
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("testbed directory")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    files.sort();
    assert_eq!(files.len(), 8, "eight vendored testbeds expected");
    files
}

fn load(file: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(file).expect("testbed readable"))
        .expect("testbed json")
}

#[test]
fn every_testbed_terms_object_deserializes_into_contract_terms() {
    let mut total_cases = 0;
    for file in deserializable_files() {
        let doc = load(&file);
        for (case_id, case) in doc.as_object().expect("testbed object") {
            let terms: ContractTerms = serde_json::from_value(case["terms"].clone())
                .unwrap_or_else(|e| panic!("{case_id}: {e}"));
            assert_eq!(
                case["terms"]["contractType"].as_str().unwrap(),
                terms.contract_type.as_acronym(),
                "{case_id}"
            );
            total_cases += 1;
        }
    }
    assert_eq!(
        total_cases, 154,
        "25 PAM + 31 LAM + 22 NAM + 31 ANN + 15 CLM + 4 CSH + 11 SWAPS + 15 CEC"
    );
}

#[test]
fn cec_credit_enhancement_terms_deserialize() {
    let file = testbed_files()
        .into_iter()
        .find(|p| p.to_string_lossy().contains("cec"))
        .expect("cec testbed");
    let doc = load(&file);

    let collateral06: ContractTerms =
        serde_json::from_value(doc["collateral06"]["terms"].clone()).expect("collateral06 terms");
    assert_eq!(
        collateral06.credit_event_type_covered,
        Some(actus_model::CreditEventType::Default)
    );
    assert_eq!(
        collateral06.coverage_of_credit_enhancement,
        Some(Decimal::from_str_exact("0.7").unwrap())
    );
    assert_eq!(collateral06.guaranteed_exposure, None);
    assert_eq!(collateral06.settlement_period.as_deref(), Some("P0D"));
    assert_eq!(collateral06.maturity_date, None);

    let collateral09: ContractTerms =
        serde_json::from_value(doc["collateral09"]["terms"].clone()).expect("collateral09 terms");
    assert_eq!(
        collateral09.guaranteed_exposure,
        Some(actus_model::GuaranteedExposure::NominalValue)
    );
    assert_eq!(
        collateral09.coverage_of_credit_enhancement,
        Some(Decimal::ONE)
    );

    let collateral10: ContractTerms =
        serde_json::from_value(doc["collateral10"]["terms"].clone()).expect("collateral10 terms");
    assert_eq!(
        collateral10.guaranteed_exposure,
        Some(actus_model::GuaranteedExposure::NominalValuePlusInterest)
    );

    let structure = collateral09.contract_structure.expect("structure");
    let covered: Vec<_> = structure
        .iter()
        .filter(|r| r.reference_role == Some(actus_model::ContractReferenceRole::CoveredContract))
        .collect();
    let covering: Vec<_> = structure
        .iter()
        .filter(|r| r.reference_role == Some(actus_model::ContractReferenceRole::CoveringContract))
        .collect();
    assert_eq!(covered.len(), 2);
    assert_eq!(covering.len(), 1);
    assert_eq!(
        covered[0].object.as_ref().expect("object").contract_id,
        Some("loan01".to_string())
    );
    assert_eq!(
        covering[0]
            .object
            .as_ref()
            .expect("object")
            .market_object_code,
        Some("GOLD".to_string())
    );
}

#[test]
fn every_testbed_case_satisfies_the_applicability_tables() {
    for file in fixed_income_files() {
        let doc = load(&file);
        for (case_id, case) in doc.as_object().expect("testbed object") {
            let terms: ContractTerms = serde_json::from_value(case["terms"].clone())
                .unwrap_or_else(|e| panic!("{case_id}: {e}"));
            let tables = applicability::tables(terms.contract_type);
            let presence = terms.presence();
            for (identifier, set) in presence {
                if set {
                    assert!(
                        tables.applicable.contains(&identifier),
                        "{case_id}: {identifier} not applicable to {}",
                        terms.contract_type
                    );
                }
            }
            for identifier in tables.base_required {
                let set = presence.iter().any(|(id, s)| *id == *identifier && *s);
                assert!(set, "{case_id}: missing base-required {identifier}");
            }
        }
    }
}

#[test]
fn every_testbed_cycle_string_parses_and_round_trips() {
    let mut cycles = std::collections::BTreeSet::new();
    for file in testbed_files() {
        let doc = load(&file);
        for case in doc.as_object().expect("testbed object").values() {
            for (key, value) in case["terms"].as_object().expect("terms") {
                if key.starts_with("cycleOf") {
                    let raw = value
                        .as_str()
                        .unwrap_or_else(|| panic!("{key} not a string"));
                    cycles.insert(raw.to_string());
                }
            }
        }
    }
    assert!(!cycles.is_empty());
    for raw in cycles {
        let cycle = Cycle::parse(&raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(cycle.to_string(), raw, "display round-trip");
        assert_eq!(Cycle::parse(&cycle.to_string()).unwrap(), cycle);
        assert_eq!(
            serde_json::to_value(cycle).unwrap(),
            serde_json::json!(raw),
            "serde round-trip"
        );
    }
}

#[test]
fn testbed_value_shapes_coerce() {
    let file = testbed_files()
        .into_iter()
        .find(|p| p.to_string_lossy().contains("pam"))
        .expect("pam testbed");
    let doc = load(&file);
    let terms: ContractTerms = serde_json::from_value(doc["pam01"]["terms"].clone()).unwrap();
    assert_eq!(terms.notional_principal, Some(Decimal::from(3000)));
    assert_eq!(
        terms.nominal_interest_rate,
        Some(Decimal::from_str_exact("0.1").unwrap())
    );
    assert_eq!(
        terms.contract_role.map(|r| r.to_string()),
        Some("RPA".to_string())
    );
    assert_eq!(terms.currency.as_deref(), Some("USD"));
}

fn as_str_value(v: &Value) -> String {
    v.as_str().expect("string value").to_string()
}

fn as_naive(v: &Value) -> NaiveDateTime {
    timestamp_from_value(v)
        .expect("timestamp")
        .expect("non-null")
}

fn as_decimal(v: &Value) -> Decimal {
    decimal_from_value(v).expect("decimal").expect("non-null")
}

fn as_cycle(v: &Value) -> Cycle {
    Cycle::parse(v.as_str().expect("cycle string")).expect("cycle")
}

fn as_enum<T: serde::de::DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v.clone()).expect("enum token")
}

#[test]
fn builders_accept_every_testbed_case() {
    let mut built = 0;
    for file in fixed_income_files() {
        let doc = load(&file);
        for (case_id, case) in doc.as_object().expect("testbed object") {
            let raw = case["terms"].clone();
            let parsed: ContractTerms =
                serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{case_id}: {e}"));
            let rebuilt = match parsed.contract_type {
                ContractType::Pam => replay_pam(&raw).build().unwrap(),
                ContractType::Lam => replay_lam(&raw).build().unwrap(),
                ContractType::Nam => replay_nam(&raw).build().unwrap(),
                ContractType::Ann => replay_ann(&raw).build().unwrap(),
                other => panic!("{case_id}: unexpected contract type {other}"),
            };
            assert_eq!(rebuilt, parsed, "{case_id}: builder round-trip");
            built += 1;
        }
    }
    assert_eq!(built, 109);
}

macro_rules! replay_fns {
    ($($fn_name:ident => $builder_ty:ty),* $(,)?) => {
        $(
            fn $fn_name(raw: &Value) -> $builder_ty {
                let mut builder = <$builder_ty>::new();
                macro_rules! set_if_present {
                    ($key:literal, $method:ident, $map:expr) => {
                        if let Some(value) = raw.get($key) {
                            if !value.is_null() {
                                builder = builder.$method($map(value));
                            }
                        }
                    };
                }
                set_if_present!("contractID", set_contract_id, as_str_value);
                set_if_present!("contractRole", set_contract_role, as_enum::<actus_model::ContractRole>);
                set_if_present!("statusDate", set_status_date, as_naive);
                set_if_present!("contractDealDate", set_contract_deal_date, as_naive);
                set_if_present!("initialExchangeDate", set_initial_exchange_date, as_naive);
                set_if_present!("maturityDate", set_maturity_date, as_naive);
                set_if_present!("notionalPrincipal", set_notional_principal, as_decimal);
                set_if_present!("nominalInterestRate", set_nominal_interest_rate, as_decimal);
                set_if_present!("accruedInterest", set_accrued_interest, as_decimal);
                set_if_present!("cycleAnchorDateOfInterestPayment", set_cycle_anchor_date_of_interest_payment, as_naive);
                set_if_present!("cycleOfInterestPayment", set_cycle_of_interest_payment, as_cycle);
                set_if_present!("cycleAnchorDateOfPrincipalRedemption", set_cycle_anchor_date_of_principal_redemption, as_naive);
                set_if_present!("cycleOfPrincipalRedemption", set_cycle_of_principal_redemption, as_cycle);
                set_if_present!("cycleAnchorDateOfRateReset", set_cycle_anchor_date_of_rate_reset, as_naive);
                set_if_present!("cycleOfRateReset", set_cycle_of_rate_reset, as_cycle);
                set_if_present!("cycleAnchorDateOfInterestCalculationBase", set_cycle_anchor_date_of_interest_calculation_base, as_naive);
                set_if_present!("cycleOfInterestCalculationBase", set_cycle_of_interest_calculation_base, as_cycle);
                set_if_present!("cycleAnchorDateOfScalingIndex", set_cycle_anchor_date_of_scaling_index, as_naive);
                set_if_present!("cycleOfScalingIndex", set_cycle_of_scaling_index, as_cycle);
                set_if_present!("dayCountConvention", set_day_count_convention, as_enum::<actus_model::DayCountConvention>);
                set_if_present!("endOfMonthConvention", set_end_of_month_convention, as_enum::<actus_model::EndOfMonthConvention>);
                set_if_present!("businessDayConvention", set_business_day_convention, as_enum::<actus_model::BusinessDayConvention>);
                set_if_present!("calendar", set_calendar, as_enum::<actus_model::Calendar>);
                set_if_present!("rateMultiplier", set_rate_multiplier, as_decimal);
                set_if_present!("rateSpread", set_rate_spread, as_decimal);
                set_if_present!("nextResetRate", set_next_reset_rate, as_decimal);
                set_if_present!("fixingDays", set_fixing_days, as_str_value);
                set_if_present!("marketObjectCodeOfRateReset", set_market_object_code_of_rate_reset, as_str_value);
                set_if_present!("premiumDiscountAtIED", set_premium_discount_at_ied, as_decimal);
                set_if_present!("capitalizationEndDate", set_capitalization_end_date, as_naive);
                set_if_present!("amortizationDate", set_amortization_date, as_naive);
                set_if_present!("nextPrincipalRedemptionPayment", set_next_principal_redemption_payment, as_decimal);
                set_if_present!("interestCalculationBase", set_interest_calculation_base, as_enum::<actus_model::InterestCalculationBase>);
                set_if_present!("interestCalculationBaseAmount", set_interest_calculation_base_amount, as_decimal);
                set_if_present!("scalingEffect", set_scaling_effect, as_enum::<actus_model::ScalingEffect>);
                set_if_present!("marketObjectCodeOfScalingIndex", set_market_object_code_of_scaling_index, as_str_value);
                set_if_present!("scalingIndexAtContractDealDate", set_scaling_index_at_contract_deal_date, as_decimal);
                set_if_present!("notionalScalingMultiplier", set_notional_scaling_multiplier, as_decimal);
                set_if_present!("interestScalingMultiplier", set_interest_scaling_multiplier, as_decimal);
                set_if_present!("purchaseDate", set_purchase_date, as_naive);
                set_if_present!("priceAtPurchaseDate", set_price_at_purchase_date, as_decimal);
                set_if_present!("terminationDate", set_termination_date, as_naive);
                set_if_present!("priceAtTerminationDate", set_price_at_termination_date, as_decimal);
                set_if_present!("currency", set_currency, as_str_value);
                set_if_present!("xDayNotice", set_x_day_notice, as_str_value);
                set_if_present!("deliverySettlement", set_delivery_settlement, as_enum::<actus_model::DeliverySettlement>);
                builder
            }
        )*
    };
}

replay_fns!(
    replay_pam => PamBuilder,
    replay_lam => LamBuilder,
    replay_nam => NamBuilder,
    replay_ann => AnnBuilder,
);

#[test]
fn serialization_round_trips_the_dictionary_keys() {
    let file = testbed_files()
        .into_iter()
        .find(|p| p.to_string_lossy().contains("pam"))
        .expect("pam testbed");
    let doc = load(&file);
    let terms: ContractTerms = serde_json::from_value(doc["pam01"]["terms"].clone()).unwrap();
    let serialized = serde_json::to_value(&terms).unwrap();
    let keys = serialized.as_object().unwrap();
    for key in [
        "contractType",
        "contractID",
        "contractRole",
        "statusDate",
        "contractDealDate",
        "initialExchangeDate",
        "maturityDate",
        "notionalPrincipal",
        "nominalInterestRate",
        "cycleAnchorDateOfInterestPayment",
        "cycleOfInterestPayment",
        "dayCountConvention",
        "premiumDiscountAtIED",
        "currency",
    ] {
        assert!(keys.contains_key(key), "serialized terms miss {key}");
    }
    let reparsed: ContractTerms = serde_json::from_value(serialized).unwrap();
    assert_eq!(reparsed, terms);
}

#[test]
fn whitespace_padded_and_scientific_decimals_parse() {
    assert_eq!(parse_decimal("  5000").unwrap(), Decimal::from(5000));
    assert_eq!(
        parse_decimal("0.1").unwrap(),
        Decimal::from_str_exact("0.1").unwrap()
    );
    assert_eq!(
        parse_decimal("1.0").unwrap(),
        Decimal::from_str_exact("1.0").unwrap()
    );
    assert_eq!(parse_decimal("1E+3").unwrap(), Decimal::from(1000));
    assert_eq!(parse_decimal("-350").unwrap(), Decimal::from(-350));
    assert!(parse_decimal("not-a-number").is_err());
}
