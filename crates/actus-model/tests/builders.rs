//! Builder tests: applicability enforcement per contract type.

use actus_model::builders::{AnnBuilder, LamBuilder, NamBuilder, PamBuilder};
use actus_model::error::ModelError;
use actus_model::{
    BusinessDayConvention, ContractRole, Cycle, DayCountConvention, EndOfMonthConvention,
    ScalingEffect,
};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
}

fn d(value: i64) -> Decimal {
    Decimal::from(value)
}

fn pam_base() -> PamBuilder {
    PamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(3000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_maturity_date(ts("2014-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
}

#[test]
fn pam_happy_path_builds() {
    let terms = pam_base().build().expect("pam base is complete");
    assert_eq!(terms.contract_type.as_acronym(), "PAM");
    assert_eq!(terms.notional_principal, Some(d(3000)));
}

#[test]
fn pam_missing_required_attribute_lists_the_name() {
    let err = PamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(3000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_maturity_date(ts("2014-01-01T00:00:00"))
        .build()
        .expect_err("nominalInterestRate missing");
    match err {
        ModelError::MissingRequiredAttribute {
            attribute,
            contract_type,
        } => {
            assert_eq!(attribute, "nominalInterestRate");
            assert_eq!(contract_type, "PAM");
            assert!(err.to_string().contains("nominalInterestRate"));
        }
        other => panic!("wrong error: {other}"),
    }
}

#[test]
fn pam_rejects_non_applicable_attribute() {
    let err = pam_base()
        .set_cycle_of_principal_redemption(Cycle::parse("P1ML0").unwrap())
        .build()
        .expect_err("PRCL is not applicable to PAM");
    match err {
        ModelError::AttributeNotApplicable {
            attribute,
            contract_type,
        } => {
            assert_eq!(attribute, "cycleOfPrincipalRedemption");
            assert_eq!(contract_type, "PAM");
        }
        other => panic!("wrong error: {other}"),
    }
}

#[test]
fn lam_required_set_is_enforced() {
    let complete = LamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
        .build();
    assert!(complete.is_ok(), "lam base builds: {:?}", complete.err());

    let err = LamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .build()
        .expect_err("nominalInterestRate missing");
    assert!(matches!(
        err,
        ModelError::MissingRequiredAttribute {
            attribute: "nominalInterestRate",
            ..
        }
    ));
}

#[test]
fn nam_required_set_is_enforced() {
    let complete = NamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
        .set_next_principal_redemption_payment(d(500))
        .set_rate_spread(d(0))
        .set_market_object_code_of_rate_reset("USD.BSA-SA")
        .build();
    assert!(complete.is_ok(), "nam base builds: {:?}", complete.err());

    let err = NamBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
        .set_rate_spread(d(0))
        .set_market_object_code_of_rate_reset("USD.BSA-SA")
        .build()
        .expect_err("nextPrincipalRedemptionPayment missing");
    assert!(matches!(
        err,
        ModelError::MissingRequiredAttribute {
            attribute: "nextPrincipalRedemptionPayment",
            ..
        }
    ));
}

#[test]
fn ann_required_set_is_enforced() {
    let complete = AnnBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
        .build();
    assert!(complete.is_ok(), "ann base builds: {:?}", complete.err());

    let err = AnnBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .build()
        .expect_err("nominalInterestRate missing");
    assert!(matches!(
        err,
        ModelError::MissingRequiredAttribute {
            attribute: "nominalInterestRate",
            ..
        }
    ));
}

#[test]
fn optional_attributes_flow_through_builders() {
    let terms = pam_base()
        .set_contract_id("pam-test")
        .set_day_count_convention(DayCountConvention::A365)
        .set_end_of_month_convention(EndOfMonthConvention::Eom)
        .set_business_day_convention(BusinessDayConvention::Scmf)
        .set_cycle_anchor_date_of_interest_payment(ts("2013-01-01T00:00:00"))
        .set_cycle_of_interest_payment(Cycle::parse("P1ML0").unwrap())
        .set_premium_discount_at_ied(d(0))
        .set_scaling_effect(ScalingEffect::InterestAndNotional)
        .build()
        .expect("pam with optionals");
    assert_eq!(terms.contract_id.as_deref(), Some("pam-test"));
    assert_eq!(terms.day_count_convention, Some(DayCountConvention::A365));
    assert_eq!(
        terms.cycle_of_interest_payment,
        Some(Cycle::parse("P1ML0").unwrap())
    );
    assert_eq!(
        terms.scaling_effect,
        Some(ScalingEffect::InterestAndNotional)
    );
}

#[test]
fn builder_contract_type_is_pinned() {
    let err = pam_base()
        .set_amortization_date(ts("2013-06-01T00:00:00"))
        .build()
        .expect_err("AMD is not applicable to PAM");
    assert!(matches!(
        err,
        ModelError::AttributeNotApplicable {
            attribute: "amortizationDate",
            ..
        }
    ));

    let ok = AnnBuilder::new()
        .set_contract_role(ContractRole::Rpa)
        .set_status_date(ts("2012-12-30T00:00:00"))
        .set_contract_deal_date(ts("2012-12-28T00:00:00"))
        .set_currency("USD")
        .set_notional_principal(d(5000))
        .set_initial_exchange_date(ts("2013-01-01T00:00:00"))
        .set_nominal_interest_rate(Decimal::from_str_exact("0.1").unwrap())
        .set_amortization_date(ts("2013-06-01T00:00:00"))
        .build();
    assert!(ok.is_ok(), "AMD is applicable to ANN: {:?}", ok.err());
}
