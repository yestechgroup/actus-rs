//! ACTUS contract terms: the attribute bag of the fixed-income family
//! (PAM/LAM/NAM/ANN testbed vocabulary).
//!
//! Every field is optional except [`ContractTerms::contract_type`]; unknown
//! wire keys are ignored, and missing/null keys deserialize to `None`. The
//! bag deserializes directly from a testbed `terms` object (camelCase keys).

use crate::cycle::Cycle;
use crate::enums::{
    BusinessDayConvention, Calendar, ContractReferenceRole, ContractReferenceType, ContractRole,
    CreditEventType, DayCountConvention, DeliverySettlement, EndOfMonthConvention,
    GuaranteedExposure, InterestCalculationBase, ScalingEffect,
};
use crate::generated::contract_type::ContractType;
use crate::serde_helpers::{decimal_option, timestamp_option};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// ACTUS contract terms for the fixed-income family.
///
/// Field docs map each attribute to its dictionary acronym; `fixingDays` is
/// carried by the testbeds but missing from dictionary v1.4 (see the crate
/// documentation for the full gap list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractTerms {
    /// ACTUS attribute `CT` — Contract Type (required).
    #[serde(rename = "contractType")]
    pub contract_type: ContractType,
    /// ACTUS attribute `CID` — Contract Identifier.
    #[serde(
        rename = "contractID",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_id: Option<String>,
    /// ACTUS attribute `CNTRL` — Contract Role.
    #[serde(
        rename = "contractRole",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_role: Option<ContractRole>,
    /// ACTUS attribute `SD` — Status Date.
    #[serde(
        rename = "statusDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub status_date: Option<NaiveDateTime>,
    /// ACTUS attribute `CDD` — Contract Deal Date.
    #[serde(
        rename = "contractDealDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_deal_date: Option<NaiveDateTime>,
    /// ACTUS attribute `IED` — Initial Exchange Date.
    #[serde(
        rename = "initialExchangeDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_exchange_date: Option<NaiveDateTime>,
    /// ACTUS attribute `MD` — Maturity Date.
    #[serde(
        rename = "maturityDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub maturity_date: Option<NaiveDateTime>,
    /// ACTUS attribute `NT` — Notional Principal.
    #[serde(
        rename = "notionalPrincipal",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub notional_principal: Option<Decimal>,
    /// ACTUS attribute `IPNR` — Nominal Interest Rate.
    #[serde(
        rename = "nominalInterestRate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub nominal_interest_rate: Option<Decimal>,
    /// ACTUS attribute `IPAC` — Accrued Interest.
    #[serde(
        rename = "accruedInterest",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub accrued_interest: Option<Decimal>,
    /// ACTUS attribute `IPANX` — Cycle Anchor Date Of Interest Payment.
    #[serde(
        rename = "cycleAnchorDateOfInterestPayment",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_interest_payment: Option<NaiveDateTime>,
    /// ACTUS attribute `IPCL` — Cycle Of Interest Payment.
    #[serde(
        rename = "cycleOfInterestPayment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_interest_payment: Option<Cycle>,
    /// ACTUS attribute `PRANX` — Cycle Anchor Date Of Principal Redemption.
    #[serde(
        rename = "cycleAnchorDateOfPrincipalRedemption",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_principal_redemption: Option<NaiveDateTime>,
    /// ACTUS attribute `PRCL` — Cycle Of Principal Redemption.
    #[serde(
        rename = "cycleOfPrincipalRedemption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_principal_redemption: Option<Cycle>,
    /// ACTUS attribute `RRANX` — Cycle Anchor Date Of Rate Reset.
    #[serde(
        rename = "cycleAnchorDateOfRateReset",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_rate_reset: Option<NaiveDateTime>,
    /// ACTUS attribute `RRCL` — Cycle Of Rate Reset.
    #[serde(
        rename = "cycleOfRateReset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_rate_reset: Option<Cycle>,
    /// ACTUS attribute `IPCBANX` — Cycle Anchor Date Of Interest Calculation Base.
    #[serde(
        rename = "cycleAnchorDateOfInterestCalculationBase",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_interest_calculation_base: Option<NaiveDateTime>,
    /// ACTUS attribute `IPCBCL` — Cycle Of Interest Calculation Base.
    #[serde(
        rename = "cycleOfInterestCalculationBase",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_interest_calculation_base: Option<Cycle>,
    /// ACTUS attribute `SCANX` — Cycle Anchor Date Of Scaling Index.
    #[serde(
        rename = "cycleAnchorDateOfScalingIndex",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_scaling_index: Option<NaiveDateTime>,
    /// ACTUS attribute `SCCL` — Cycle Of Scaling Index.
    #[serde(
        rename = "cycleOfScalingIndex",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_scaling_index: Option<Cycle>,
    /// ACTUS attribute `IPDC` — Day Count Convention.
    #[serde(
        rename = "dayCountConvention",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub day_count_convention: Option<DayCountConvention>,
    /// ACTUS attribute `EOMC` — End Of Month Convention.
    #[serde(
        rename = "endOfMonthConvention",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end_of_month_convention: Option<EndOfMonthConvention>,
    /// ACTUS attribute `BDC` — Business Day Convention.
    #[serde(
        rename = "businessDayConvention",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub business_day_convention: Option<BusinessDayConvention>,
    /// ACTUS attribute `CLDR` — Calendar.
    #[serde(rename = "calendar", default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    /// ACTUS attribute `RRMLT` — Rate Multiplier.
    #[serde(
        rename = "rateMultiplier",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub rate_multiplier: Option<Decimal>,
    /// ACTUS attribute `RRSP` — Rate Spread.
    #[serde(
        rename = "rateSpread",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub rate_spread: Option<Decimal>,
    /// ACTUS attribute `RRNXT` — Next Reset Rate.
    #[serde(
        rename = "nextResetRate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_reset_rate: Option<Decimal>,
    /// Rate reset fixing offset (testbed values `P0D`/`P2D`). Carried by the
    /// testbeds but absent from dictionary v1.4, so it is kept as the raw
    /// ISO 8601 period string.
    #[serde(
        rename = "fixingDays",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub fixing_days: Option<String>,
    /// ACTUS attribute `RRMO` — Market Object Code Of Rate Reset.
    #[serde(
        rename = "marketObjectCodeOfRateReset",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub market_object_code_of_rate_reset: Option<String>,
    /// ACTUS attribute `PDIED` — Premium Discount At IED.
    #[serde(
        rename = "premiumDiscountAtIED",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub premium_discount_at_ied: Option<Decimal>,
    /// ACTUS attribute `IPCED` — Capitalization End Date.
    #[serde(
        rename = "capitalizationEndDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub capitalization_end_date: Option<NaiveDateTime>,
    /// ACTUS attribute `AMD` — Amortization Date.
    #[serde(
        rename = "amortizationDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub amortization_date: Option<NaiveDateTime>,
    /// ACTUS attribute `PRNXT` — Next Principal Redemption Payment.
    #[serde(
        rename = "nextPrincipalRedemptionPayment",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_principal_redemption_payment: Option<Decimal>,
    /// ACTUS attribute `IPCB` — Interest Calculation Base.
    #[serde(
        rename = "interestCalculationBase",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub interest_calculation_base: Option<InterestCalculationBase>,
    /// ACTUS attribute `IPCBA` — Interest Calculation Base Amount.
    #[serde(
        rename = "interestCalculationBaseAmount",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub interest_calculation_base_amount: Option<Decimal>,
    /// ACTUS attribute `SCEF` — Scaling Effect.
    #[serde(
        rename = "scalingEffect",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub scaling_effect: Option<ScalingEffect>,
    /// ACTUS attribute `SCMO` — Market Object Code Of Scaling Index.
    #[serde(
        rename = "marketObjectCodeOfScalingIndex",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub market_object_code_of_scaling_index: Option<String>,
    /// ACTUS attribute `SCCDD` — Scaling Index At Contract Deal Date.
    #[serde(
        rename = "scalingIndexAtContractDealDate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub scaling_index_at_contract_deal_date: Option<Decimal>,
    /// ACTUS attribute `SCNT` — Notional Scaling Multiplier.
    #[serde(
        rename = "notionalScalingMultiplier",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub notional_scaling_multiplier: Option<Decimal>,
    /// ACTUS attribute `SCIP` — Interest Scaling Multiplier.
    #[serde(
        rename = "interestScalingMultiplier",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub interest_scaling_multiplier: Option<Decimal>,
    /// ACTUS attribute `PRD` — Purchase Date.
    #[serde(
        rename = "purchaseDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub purchase_date: Option<NaiveDateTime>,
    /// ACTUS attribute `PPRD` — Price At Purchase Date.
    #[serde(
        rename = "priceAtPurchaseDate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub price_at_purchase_date: Option<Decimal>,
    /// ACTUS attribute `TD` — Termination Date.
    #[serde(
        rename = "terminationDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub termination_date: Option<NaiveDateTime>,
    /// ACTUS attribute `PTD` — Price At Termination Date.
    #[serde(
        rename = "priceAtTerminationDate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub price_at_termination_date: Option<Decimal>,
    /// ACTUS attribute `CUR` — Currency (ISO 4217).
    #[serde(
        rename = "currency",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub currency: Option<String>,
    /// ACTUS attribute `XDN` — X Day Notice (ISO 8601 period string).
    #[serde(
        rename = "xDayNotice",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub x_day_notice: Option<String>,
    /// ACTUS attribute `DS` — Delivery Settlement.
    #[serde(
        rename = "deliverySettlement",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub delivery_settlement: Option<DeliverySettlement>,
    /// ACTUS attribute `CETC` — Credit Event Type Covered (dictionary
    /// default `DF`).
    #[serde(
        rename = "creditEventTypeCovered",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub credit_event_type_covered: Option<CreditEventType>,
    /// ACTUS attribute `CECV` — Coverage Of Credit Enhancement (dictionary
    /// default `1`).
    #[serde(
        rename = "coverageOfCreditEnhancement",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub coverage_of_credit_enhancement: Option<Decimal>,
    /// ACTUS attribute `CEGE` — Guaranteed Exposure.
    #[serde(
        rename = "guaranteedExposure",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub guaranteed_exposure: Option<GuaranteedExposure>,
    /// ACTUS attribute `STP` — Settlement Period (ISO 8601 period string,
    /// dictionary default `P0D`).
    #[serde(
        rename = "settlementPeriod",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub settlement_period: Option<String>,
    /// ACTUS attribute `MOC` — Market Object Code of the contract's own
    /// market value observation (e.g. the commodity series of a `COM`
    /// covering object).
    #[serde(
        rename = "marketObjectCode",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub market_object_code: Option<String>,
    /// ACTUS attribute `CTS` — Contract Structure (contract references).
    #[serde(
        rename = "contractStructure",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_structure: Option<Vec<ContractReference>>,
}

/// One entry of the `contractStructure` attribute `CTS` (dictionary
/// `contractReference`): a referenced child object, its reference type and
/// the role it plays in the referencing (parent) contract. Composed contract
/// types address their child contracts through these entries
/// (techspec sections 4 "Contract Composition" and 6 "Child Contract
/// Observer", e.g. the `FirstLeg`/`SecondLeg` references of SWAPS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractReference {
    /// The referenced object; a full contract terms bag for `CNT`
    /// (contract) references.
    #[serde(default)]
    pub object: Option<ContractTerms>,
    /// Reference type `RTP` (e.g. `CNT` for an embedded contract object).
    #[serde(
        rename = "referenceType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reference_type: Option<ContractReferenceType>,
    /// Reference role `RRL` (e.g. `FIL` first leg, `SEL` second leg).
    #[serde(
        rename = "referenceRole",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reference_role: Option<ContractReferenceRole>,
}

impl ContractTerms {
    /// An all-`None` bag for the given contract type.
    #[must_use]
    pub fn new(contract_type: ContractType) -> ContractTerms {
        ContractTerms {
            contract_type,
            contract_id: None,
            contract_role: None,
            status_date: None,
            contract_deal_date: None,
            initial_exchange_date: None,
            maturity_date: None,
            notional_principal: None,
            nominal_interest_rate: None,
            accrued_interest: None,
            cycle_anchor_date_of_interest_payment: None,
            cycle_of_interest_payment: None,
            cycle_anchor_date_of_principal_redemption: None,
            cycle_of_principal_redemption: None,
            cycle_anchor_date_of_rate_reset: None,
            cycle_of_rate_reset: None,
            cycle_anchor_date_of_interest_calculation_base: None,
            cycle_of_interest_calculation_base: None,
            cycle_anchor_date_of_scaling_index: None,
            cycle_of_scaling_index: None,
            day_count_convention: None,
            end_of_month_convention: None,
            business_day_convention: None,
            calendar: None,
            rate_multiplier: None,
            rate_spread: None,
            next_reset_rate: None,
            fixing_days: None,
            market_object_code_of_rate_reset: None,
            premium_discount_at_ied: None,
            capitalization_end_date: None,
            amortization_date: None,
            next_principal_redemption_payment: None,
            interest_calculation_base: None,
            interest_calculation_base_amount: None,
            scaling_effect: None,
            market_object_code_of_scaling_index: None,
            scaling_index_at_contract_deal_date: None,
            notional_scaling_multiplier: None,
            interest_scaling_multiplier: None,
            purchase_date: None,
            price_at_purchase_date: None,
            termination_date: None,
            price_at_termination_date: None,
            currency: None,
            x_day_notice: None,
            delivery_settlement: None,
            credit_event_type_covered: None,
            coverage_of_credit_enhancement: None,
            guaranteed_exposure: None,
            settlement_period: None,
            market_object_code: None,
            contract_structure: None,
        }
    }

    /// Dictionary identifier + set flag for every attribute, in field order.
    ///
    /// Builders use this against the generated applicability tables to enforce
    /// required and applicable sets.
    #[must_use]
    pub fn presence(&self) -> [(&'static str, bool); 53] {
        [
            ("contractType", true),
            ("contractID", self.contract_id.is_some()),
            ("contractRole", self.contract_role.is_some()),
            ("statusDate", self.status_date.is_some()),
            ("contractDealDate", self.contract_deal_date.is_some()),
            ("initialExchangeDate", self.initial_exchange_date.is_some()),
            ("maturityDate", self.maturity_date.is_some()),
            ("notionalPrincipal", self.notional_principal.is_some()),
            ("nominalInterestRate", self.nominal_interest_rate.is_some()),
            ("accruedInterest", self.accrued_interest.is_some()),
            (
                "cycleAnchorDateOfInterestPayment",
                self.cycle_anchor_date_of_interest_payment.is_some(),
            ),
            (
                "cycleOfInterestPayment",
                self.cycle_of_interest_payment.is_some(),
            ),
            (
                "cycleAnchorDateOfPrincipalRedemption",
                self.cycle_anchor_date_of_principal_redemption.is_some(),
            ),
            (
                "cycleOfPrincipalRedemption",
                self.cycle_of_principal_redemption.is_some(),
            ),
            (
                "cycleAnchorDateOfRateReset",
                self.cycle_anchor_date_of_rate_reset.is_some(),
            ),
            ("cycleOfRateReset", self.cycle_of_rate_reset.is_some()),
            (
                "cycleAnchorDateOfInterestCalculationBase",
                self.cycle_anchor_date_of_interest_calculation_base
                    .is_some(),
            ),
            (
                "cycleOfInterestCalculationBase",
                self.cycle_of_interest_calculation_base.is_some(),
            ),
            (
                "cycleAnchorDateOfScalingIndex",
                self.cycle_anchor_date_of_scaling_index.is_some(),
            ),
            ("cycleOfScalingIndex", self.cycle_of_scaling_index.is_some()),
            ("dayCountConvention", self.day_count_convention.is_some()),
            (
                "endOfMonthConvention",
                self.end_of_month_convention.is_some(),
            ),
            (
                "businessDayConvention",
                self.business_day_convention.is_some(),
            ),
            ("calendar", self.calendar.is_some()),
            ("rateMultiplier", self.rate_multiplier.is_some()),
            ("rateSpread", self.rate_spread.is_some()),
            ("nextResetRate", self.next_reset_rate.is_some()),
            ("fixingDays", self.fixing_days.is_some()),
            (
                "marketObjectCodeOfRateReset",
                self.market_object_code_of_rate_reset.is_some(),
            ),
            (
                "premiumDiscountAtIED",
                self.premium_discount_at_ied.is_some(),
            ),
            (
                "capitalizationEndDate",
                self.capitalization_end_date.is_some(),
            ),
            ("amortizationDate", self.amortization_date.is_some()),
            (
                "nextPrincipalRedemptionPayment",
                self.next_principal_redemption_payment.is_some(),
            ),
            (
                "interestCalculationBase",
                self.interest_calculation_base.is_some(),
            ),
            (
                "interestCalculationBaseAmount",
                self.interest_calculation_base_amount.is_some(),
            ),
            ("scalingEffect", self.scaling_effect.is_some()),
            (
                "marketObjectCodeOfScalingIndex",
                self.market_object_code_of_scaling_index.is_some(),
            ),
            (
                "scalingIndexAtContractDealDate",
                self.scaling_index_at_contract_deal_date.is_some(),
            ),
            (
                "notionalScalingMultiplier",
                self.notional_scaling_multiplier.is_some(),
            ),
            (
                "interestScalingMultiplier",
                self.interest_scaling_multiplier.is_some(),
            ),
            ("purchaseDate", self.purchase_date.is_some()),
            ("priceAtPurchaseDate", self.price_at_purchase_date.is_some()),
            ("terminationDate", self.termination_date.is_some()),
            (
                "priceAtTerminationDate",
                self.price_at_termination_date.is_some(),
            ),
            ("currency", self.currency.is_some()),
            ("xDayNotice", self.x_day_notice.is_some()),
            ("deliverySettlement", self.delivery_settlement.is_some()),
            (
                "creditEventTypeCovered",
                self.credit_event_type_covered.is_some(),
            ),
            (
                "coverageOfCreditEnhancement",
                self.coverage_of_credit_enhancement.is_some(),
            ),
            ("guaranteedExposure", self.guaranteed_exposure.is_some()),
            ("settlementPeriod", self.settlement_period.is_some()),
            ("marketObjectCode", self.market_object_code.is_some()),
            ("contractStructure", self.contract_structure.is_some()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_from_minimal_camel_case_json() {
        let json = serde_json::json!({
            "contractType": "PAM",
            "statusDate": "2012-12-30",
            "notionalPrincipal": 3000,
            "nominalInterestRate": 0.1,
            "unknownFutureKey": "ignored",
            "currency": null
        });
        let terms: ContractTerms = serde_json::from_value(json).expect("terms");
        assert_eq!(terms.contract_type, ContractType::Pam);
        assert_eq!(
            terms.status_date,
            Some(
                NaiveDateTime::parse_from_str("2012-12-30T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap()
            )
        );
        assert_eq!(terms.notional_principal, Some(Decimal::from(3000)));
        assert_eq!(
            terms.nominal_interest_rate,
            Some(Decimal::from_str_exact("0.1").unwrap())
        );
        assert_eq!(terms.currency, None);
        assert!(terms.contract_id.is_none());
    }

    #[test]
    fn date_shapes_all_parse() {
        for raw in ["2013-01-01", "2013-01-01T00:00", "2013-01-01T00:00:00"] {
            let json = serde_json::json!({
                "contractType": "PAM",
                "statusDate": raw
            });
            let terms: ContractTerms =
                serde_json::from_value(json).unwrap_or_else(|e| panic!("{raw}: {e}"));
            assert_eq!(
                terms.status_date,
                Some(
                    NaiveDateTime::parse_from_str("2013-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                        .unwrap()
                ),
                "{raw}"
            );
        }
        let bad: Result<ContractTerms, _> = serde_json::from_value(
            serde_json::json!({"contractType": "PAM", "statusDate": "01/02/2013"}),
        );
        assert!(bad.is_err());
    }

    #[test]
    fn contract_type_is_required_on_deserialization() {
        let missing: Result<ContractTerms, _> = serde_json::from_value(serde_json::json!({}));
        assert!(missing.is_err());
    }

    #[test]
    fn contract_structure_deserializes_reference_objects() {
        let json = serde_json::json!({
            "contractType": "SWAPS",
            "contractRole": "RFL",
            "deliverySettlement": "S",
            "contractStructure": [
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "leg1",
                        "notionalPrincipal": "1000"
                    },
                    "referenceType": "CNT",
                    "referenceRole": "FIL"
                },
                {
                    "object": {
                        "contractType": "PAM",
                        "contractID": "leg2",
                        "contractRole": "RPA",
                        "notionalPrincipal": 1200
                    },
                    "referenceType": "CNT",
                    "referenceRole": "SEL"
                }
            ]
        });
        let terms: ContractTerms = serde_json::from_value(json).expect("terms");
        let structure = terms.contract_structure.expect("structure");
        assert_eq!(structure.len(), 2);
        assert_eq!(
            structure[0].reference_role,
            Some(crate::enums::ContractReferenceRole::FirstLeg)
        );
        assert_eq!(
            structure[0].reference_type,
            Some(crate::enums::ContractReferenceType::Contract)
        );
        let leg1 = structure[0].object.as_ref().expect("leg1 object");
        assert_eq!(leg1.contract_type, ContractType::Pam);
        assert_eq!(leg1.notional_principal, Some(Decimal::from(1000)));
        let leg2 = structure[1].object.as_ref().expect("leg2 object");
        assert_eq!(leg2.contract_role, Some(crate::enums::ContractRole::Rpa));
        assert_eq!(
            structure[1].reference_role,
            Some(crate::enums::ContractReferenceRole::SecondLeg)
        );
        let without = serde_json::from_value::<ContractTerms>(serde_json::json!({
            "contractType": "PAM"
        }))
        .expect("terms without structure");
        assert!(without.contract_structure.is_none());
    }
}
