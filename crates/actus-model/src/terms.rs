//! ACTUS contract terms: the attribute bag of the fixed-income family
//! (PAM/LAM/NAM/ANN testbed vocabulary).
//!
//! Every field is optional except [`ContractTerms::contract_type`]; unknown
//! wire keys are ignored, and missing/null keys deserialize to `None`. The
//! bag deserializes directly from a testbed `terms` object (camelCase keys).

use crate::cycle::Cycle;
use crate::enums::{
    ArrayFixVar, ArrayIncDec, BusinessDayConvention, Calendar, ContractPerformance,
    ContractReferenceRole, ContractReferenceType, ContractRole, CreditEventType, CyclePoint,
    DayCountConvention, DeliverySettlement, EndOfMonthConvention, FeeBasis, GuaranteedExposure,
    InterestCalculationBase, OptionExerciseType, OptionType, PenaltyType, PrepaymentEffect,
    ScalingEffect,
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
    /// ACTUS attribute `ARIPANX` — Array Cycle Anchor Date Of Interest
    /// Payment (dictionary type `Timestamp[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleAnchorDateOfInterestPayment",
        default,
        deserialize_with = "crate::serde_helpers::timestamp_vec_option::deserialize",
        serialize_with = "crate::serde_helpers::timestamp_vec_option::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_anchor_date_of_interest_payment: Option<Vec<NaiveDateTime>>,
    /// ACTUS attribute `ARIPCL` — Array Cycle Of Interest Payment
    /// (dictionary type `Cycle[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleOfInterestPayment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_of_interest_payment: Option<Vec<Cycle>>,
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
    /// ACTUS attribute `ARPRANX` — Array Cycle Anchor Date Of Principal
    /// Redemption (dictionary type `Timestamp[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleAnchorDateOfPrincipalRedemption",
        default,
        deserialize_with = "crate::serde_helpers::timestamp_vec_option::deserialize",
        serialize_with = "crate::serde_helpers::timestamp_vec_option::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_anchor_date_of_principal_redemption: Option<Vec<NaiveDateTime>>,
    /// ACTUS attribute `ARPRCL` — Array Cycle Of Principal Redemption
    /// (dictionary type `Cycle[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleOfPrincipalRedemption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_of_principal_redemption: Option<Vec<Cycle>>,
    /// ACTUS attribute `ARPRNXT` — Array Next Principal Redemption Payment
    /// (dictionary type `Real[]`; paper §7.3). Element `i` is the next
    /// principal redemption payment of schedule segment `i`.
    #[serde(
        rename = "arrayNextPrincipalRedemptionPayment",
        default,
        deserialize_with = "crate::serde_helpers::decimal_vec_option::deserialize",
        serialize_with = "crate::serde_helpers::decimal_vec_option::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub array_next_principal_redemption_payment: Option<Vec<Decimal>>,
    /// ACTUS attribute `ARINCDEC` — Array Increase Decrease (dictionary
    /// type `Enum[]`; paper §7.3). Element `i` states whether segment `i`
    /// increases (`INC`) or decreases (`DEC`) the notional.
    #[serde(
        rename = "arrayIncreaseDecrease",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub array_increase_decrease: Option<Vec<ArrayIncDec>>,
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
    /// ACTUS attribute `ARRRANX` — Array Cycle Anchor Date Of Rate Reset
    /// (dictionary type `Timestamp[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleAnchorDateOfRateReset",
        default,
        deserialize_with = "crate::serde_helpers::timestamp_vec_option::deserialize",
        serialize_with = "crate::serde_helpers::timestamp_vec_option::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_anchor_date_of_rate_reset: Option<Vec<NaiveDateTime>>,
    /// ACTUS attribute `ARRRCL` — Array Cycle Of Rate Reset (dictionary
    /// type `Cycle[]`; paper §7.3).
    #[serde(
        rename = "arrayCycleOfRateReset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub array_cycle_of_rate_reset: Option<Vec<Cycle>>,
    /// ACTUS attribute `ARRATE` — Array Rate (dictionary type `Real[]`).
    /// Element `i` is the rate (`ARFIXVAR = F`) or the spread over the
    /// reference rate (`ARFIXVAR = V`) of rate schedule segment `i`.
    #[serde(
        rename = "arrayRate",
        default,
        deserialize_with = "crate::serde_helpers::decimal_vec_option::deserialize",
        serialize_with = "crate::serde_helpers::decimal_vec_option::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub array_rate: Option<Vec<Decimal>>,
    /// ACTUS attribute `ARFIXVAR` — Array Fixed Variable (dictionary
    /// `arrayFixedVariable`). Defines the meaning of [`ContractTerms::array_rate`].
    #[serde(
        rename = "arrayFixedVariable",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub array_fixed_variable: Option<ArrayFixVar>,
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
    /// ACTUS attribute `PRF` — Contract Performance (dictionary default
    /// `PF`; carried as a term for contract types whose performance is set
    /// at deal time and by observed credit events).
    #[serde(
        rename = "contractPerformance",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub contract_performance: Option<ContractPerformance>,
    /// ACTUS attribute `SEN` — Seniority.
    #[serde(
        rename = "seniority",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub seniority: Option<String>,
    /// ACTUS attribute `GRP` — Grace Period (ISO 8601 period string).
    #[serde(
        rename = "gracePeriod",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub grace_period: Option<String>,
    /// ACTUS attribute `DQP` — Delinquency Period (ISO 8601 period string).
    #[serde(
        rename = "delinquencyPeriod",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub delinquency_period: Option<String>,
    /// ACTUS attribute `DQR` — Delinquency Rate.
    #[serde(
        rename = "delinquencyRate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub delinquency_rate: Option<Decimal>,
    /// ACTUS attribute `MPFD` — Maximum Penalty Free Disbursement.
    #[serde(
        rename = "maximumPenaltyFreeDisbursement",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub maximum_penalty_free_disbursement: Option<Decimal>,
    /// ACTUS attribute `NPD` — Non Performing Date.
    #[serde(
        rename = "nonPerformingDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub non_performing_date: Option<NaiveDateTime>,
    /// ACTUS attribute `FER` — Fee Rate.
    #[serde(
        rename = "feeRate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub fee_rate: Option<Decimal>,
    /// ACTUS attribute `FEB` — Fee Basis (dictionary default `N`).
    #[serde(rename = "feeBasis", default, skip_serializing_if = "Option::is_none")]
    pub fee_basis: Option<FeeBasis>,
    /// ACTUS attribute `FEAC` — Fee Accrued.
    #[serde(
        rename = "feeAccrued",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub fee_accrued: Option<Decimal>,
    /// ACTUS attribute `FEANX` — Cycle Anchor Date Of Fee.
    #[serde(
        rename = "cycleAnchorDateOfFee",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_fee: Option<NaiveDateTime>,
    /// ACTUS attribute `FECL` — Cycle Of Fee.
    #[serde(
        rename = "cycleOfFee",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_fee: Option<Cycle>,
    /// ACTUS attribute `DVANX` — Cycle Anchor Date Of Dividend.
    #[serde(
        rename = "cycleAnchorDateOfDividend",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_dividend: Option<NaiveDateTime>,
    /// ACTUS attribute `DVCL` — Cycle Of Dividend.
    #[serde(
        rename = "cycleOfDividend",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_dividend: Option<Cycle>,
    /// ACTUS attribute `DVEX` — Ex Dividend Date.
    #[serde(
        rename = "exDividendDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub ex_dividend_date: Option<NaiveDateTime>,
    /// ACTUS attribute `DVNP` — Next Dividend Payment Amount.
    #[serde(
        rename = "nextDividendPaymentAmount",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_dividend_payment_amount: Option<Decimal>,
    /// ACTUS attribute `QT` — Quantity.
    #[serde(
        rename = "quantity",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub quantity: Option<Decimal>,
    /// ACTUS attribute `UT` — Unit.
    #[serde(
        rename = "unit",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub unit: Option<String>,
    /// ACTUS attribute `MVO` — Market Value Observed.
    #[serde(
        rename = "marketValueObserved",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub market_value_observed: Option<Decimal>,
    /// ACTUS attribute `CUR2` — Currency 2 (ISO 4217).
    #[serde(
        rename = "currency2",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub currency2: Option<String>,
    /// ACTUS attribute `NT2` — Notional Principal 2.
    #[serde(
        rename = "notionalPrincipal2",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub notional_principal2: Option<Decimal>,
    /// ACTUS attribute `XD` — Exercise Date.
    #[serde(
        rename = "exerciseDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub exercise_date: Option<NaiveDateTime>,
    /// ACTUS attribute `XA` — Exercise Amount.
    #[serde(
        rename = "exerciseAmount",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub exercise_amount: Option<Decimal>,
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
    /// ACTUS attribute `OPTP` — Option Type.
    #[serde(
        rename = "optionType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub option_type: Option<OptionType>,
    /// ACTUS attribute `OPS1` — Option Strike 1 (strike price of the
    /// option).
    #[serde(
        rename = "optionStrike1",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub option_strike1: Option<Decimal>,
    /// ACTUS attribute `OPS2` — Option Strike 2 (put price in case of a
    /// call/put combination).
    #[serde(
        rename = "optionStrike2",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub option_strike2: Option<Decimal>,
    /// ACTUS attribute `OPXT` — Option Exercise Type.
    #[serde(
        rename = "optionExerciseType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub option_exercise_type: Option<OptionExerciseType>,
    /// ACTUS attribute `OPXED` — Option Exercise End Date (final exercise
    /// date for American/Bermudan options, expiry for European ones).
    #[serde(
        rename = "optionExerciseEndDate",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub option_exercise_end_date: Option<NaiveDateTime>,
    /// ACTUS attribute `PFUT` — Futures Price (agreed exchange/settlement
    /// price of the underlying of a `FUTUR`).
    #[serde(
        rename = "futuresPrice",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub futures_price: Option<Decimal>,
    /// ACTUS attribute `RRFIX` — Fixing Period (ISO 8601 period string):
    /// period between the fixing of a rate reset and its application.
    #[serde(
        rename = "fixingPeriod",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub fixing_period: Option<String>,
    /// ACTUS attribute `RRLC` — Life Cap (lifetime interest rate cap; cap
    /// strike rate of a `CAPFL`).
    #[serde(
        rename = "lifeCap",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub life_cap: Option<Decimal>,
    /// ACTUS attribute `RRLF` — Life Floor (lifetime interest rate floor;
    /// floor strike rate of a `CAPFL`).
    #[serde(
        rename = "lifeFloor",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub life_floor: Option<Decimal>,
    /// ACTUS attribute `RRPC` — Period Cap (maximum positive rate change
    /// per rate reset cycle).
    #[serde(
        rename = "periodCap",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub period_cap: Option<Decimal>,
    /// ACTUS attribute `RRPF` — Period Floor (maximum negative rate change
    /// per rate reset cycle).
    #[serde(
        rename = "periodFloor",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub period_floor: Option<Decimal>,
    /// ACTUS attribute `CURS` — Settlement Currency (ISO 4217).
    #[serde(
        rename = "settlementCurrency",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub settlement_currency: Option<String>,
    /// ACTUS attribute `PPEF` — Prepayment Effect.
    #[serde(
        rename = "prepaymentEffect",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prepayment_effect: Option<PrepaymentEffect>,
    /// ACTUS attribute `PPP` — Prepayment Period (ISO 8601 period string):
    /// a payment earlier than the scheduled date minus `PPP` counts as a
    /// prepayment.
    #[serde(
        rename = "prepaymentPeriod",
        default,
        deserialize_with = "crate::serde_helpers::string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub prepayment_period: Option<String>,
    /// ACTUS attribute `PYRT` — Penalty Rate (rate or absolute amount of
    /// the prepayment penalty).
    #[serde(
        rename = "penaltyRate",
        default,
        deserialize_with = "decimal_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub penalty_rate: Option<Decimal>,
    /// ACTUS attribute `PYTP` — Penalty Type.
    #[serde(
        rename = "penaltyType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub penalty_type: Option<PenaltyType>,
    /// ACTUS attribute `OPANX` — Cycle Anchor Date Of Optionality (begin
    /// of the exercise period of American/Bermudan options).
    #[serde(
        rename = "cycleAnchorDateOfOptionality",
        default,
        deserialize_with = "timestamp_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_anchor_date_of_optionality: Option<NaiveDateTime>,
    /// ACTUS attribute `OPCL` — Cycle Of Optionality (cycle of the option
    /// exercise date schedule).
    #[serde(
        rename = "cycleOfOptionality",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_of_optionality: Option<Cycle>,
    /// ACTUS attribute `IPPNT` — Cycle Point Of Interest Payment
    /// (dictionary default `E`).
    #[serde(
        rename = "cyclePointOfInterestPayment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_point_of_interest_payment: Option<CyclePoint>,
    /// ACTUS attribute `RRPNT` — Cycle Point Of Rate Reset (dictionary
    /// default `B`).
    #[serde(
        rename = "cyclePointOfRateReset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cycle_point_of_rate_reset: Option<CyclePoint>,
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
            array_cycle_anchor_date_of_interest_payment: None,
            array_cycle_of_interest_payment: None,
            cycle_anchor_date_of_principal_redemption: None,
            cycle_of_principal_redemption: None,
            array_cycle_anchor_date_of_principal_redemption: None,
            array_cycle_of_principal_redemption: None,
            array_next_principal_redemption_payment: None,
            array_increase_decrease: None,
            cycle_anchor_date_of_rate_reset: None,
            cycle_of_rate_reset: None,
            array_cycle_anchor_date_of_rate_reset: None,
            array_cycle_of_rate_reset: None,
            array_rate: None,
            array_fixed_variable: None,
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
            contract_performance: None,
            seniority: None,
            grace_period: None,
            delinquency_period: None,
            delinquency_rate: None,
            maximum_penalty_free_disbursement: None,
            non_performing_date: None,
            fee_rate: None,
            fee_basis: None,
            fee_accrued: None,
            cycle_anchor_date_of_fee: None,
            cycle_of_fee: None,
            cycle_anchor_date_of_dividend: None,
            cycle_of_dividend: None,
            ex_dividend_date: None,
            next_dividend_payment_amount: None,
            quantity: None,
            unit: None,
            market_value_observed: None,
            currency2: None,
            notional_principal2: None,
            exercise_date: None,
            exercise_amount: None,
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
            option_type: None,
            option_strike1: None,
            option_strike2: None,
            option_exercise_type: None,
            option_exercise_end_date: None,
            futures_price: None,
            fixing_period: None,
            life_cap: None,
            life_floor: None,
            period_cap: None,
            period_floor: None,
            settlement_currency: None,
            prepayment_effect: None,
            prepayment_period: None,
            penalty_rate: None,
            penalty_type: None,
            cycle_anchor_date_of_optionality: None,
            cycle_of_optionality: None,
            cycle_point_of_interest_payment: None,
            cycle_point_of_rate_reset: None,
        }
    }

    /// Dictionary identifier + set flag for every attribute, in field order.
    ///
    /// Builders use this against the generated applicability tables to enforce
    /// required and applicable sets.
    #[must_use]
    pub fn presence(&self) -> [(&'static str, bool); 106] {
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
                "arrayCycleAnchorDateOfInterestPayment",
                self.array_cycle_anchor_date_of_interest_payment.is_some(),
            ),
            (
                "arrayCycleOfInterestPayment",
                self.array_cycle_of_interest_payment.is_some(),
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
                "arrayCycleAnchorDateOfPrincipalRedemption",
                self.array_cycle_anchor_date_of_principal_redemption
                    .is_some(),
            ),
            (
                "arrayCycleOfPrincipalRedemption",
                self.array_cycle_of_principal_redemption.is_some(),
            ),
            (
                "arrayNextPrincipalRedemptionPayment",
                self.array_next_principal_redemption_payment.is_some(),
            ),
            (
                "arrayIncreaseDecrease",
                self.array_increase_decrease.is_some(),
            ),
            (
                "cycleAnchorDateOfRateReset",
                self.cycle_anchor_date_of_rate_reset.is_some(),
            ),
            ("cycleOfRateReset", self.cycle_of_rate_reset.is_some()),
            (
                "arrayCycleAnchorDateOfRateReset",
                self.array_cycle_anchor_date_of_rate_reset.is_some(),
            ),
            (
                "arrayCycleOfRateReset",
                self.array_cycle_of_rate_reset.is_some(),
            ),
            ("arrayRate", self.array_rate.is_some()),
            ("arrayFixedVariable", self.array_fixed_variable.is_some()),
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
            ("contractPerformance", self.contract_performance.is_some()),
            ("seniority", self.seniority.is_some()),
            ("gracePeriod", self.grace_period.is_some()),
            ("delinquencyPeriod", self.delinquency_period.is_some()),
            ("delinquencyRate", self.delinquency_rate.is_some()),
            (
                "maximumPenaltyFreeDisbursement",
                self.maximum_penalty_free_disbursement.is_some(),
            ),
            ("nonPerformingDate", self.non_performing_date.is_some()),
            ("feeRate", self.fee_rate.is_some()),
            ("feeBasis", self.fee_basis.is_some()),
            ("feeAccrued", self.fee_accrued.is_some()),
            (
                "cycleAnchorDateOfFee",
                self.cycle_anchor_date_of_fee.is_some(),
            ),
            ("cycleOfFee", self.cycle_of_fee.is_some()),
            (
                "cycleAnchorDateOfDividend",
                self.cycle_anchor_date_of_dividend.is_some(),
            ),
            ("cycleOfDividend", self.cycle_of_dividend.is_some()),
            ("exDividendDate", self.ex_dividend_date.is_some()),
            (
                "nextDividendPaymentAmount",
                self.next_dividend_payment_amount.is_some(),
            ),
            ("quantity", self.quantity.is_some()),
            ("unit", self.unit.is_some()),
            ("marketValueObserved", self.market_value_observed.is_some()),
            ("currency2", self.currency2.is_some()),
            ("notionalPrincipal2", self.notional_principal2.is_some()),
            ("exerciseDate", self.exercise_date.is_some()),
            ("exerciseAmount", self.exercise_amount.is_some()),
            ("optionType", self.option_type.is_some()),
            ("optionStrike1", self.option_strike1.is_some()),
            ("optionStrike2", self.option_strike2.is_some()),
            ("optionExerciseType", self.option_exercise_type.is_some()),
            (
                "optionExerciseEndDate",
                self.option_exercise_end_date.is_some(),
            ),
            ("futuresPrice", self.futures_price.is_some()),
            ("fixingPeriod", self.fixing_period.is_some()),
            ("lifeCap", self.life_cap.is_some()),
            ("lifeFloor", self.life_floor.is_some()),
            ("periodCap", self.period_cap.is_some()),
            ("periodFloor", self.period_floor.is_some()),
            ("settlementCurrency", self.settlement_currency.is_some()),
            ("prepaymentEffect", self.prepayment_effect.is_some()),
            ("prepaymentPeriod", self.prepayment_period.is_some()),
            ("penaltyRate", self.penalty_rate.is_some()),
            ("penaltyType", self.penalty_type.is_some()),
            (
                "cycleAnchorDateOfOptionality",
                self.cycle_anchor_date_of_optionality.is_some(),
            ),
            ("cycleOfOptionality", self.cycle_of_optionality.is_some()),
            (
                "cyclePointOfInterestPayment",
                self.cycle_point_of_interest_payment.is_some(),
            ),
            (
                "cyclePointOfRateReset",
                self.cycle_point_of_rate_reset.is_some(),
            ),
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

    #[test]
    fn array_attributes_deserialize_from_testbed_style_json() {
        let json = serde_json::json!({
            "contractType": "LAX",
            "arrayCycleAnchorDateOfPrincipalRedemption": ["2026-01-01T00:00:00", "2027-01-01"],
            "arrayCycleOfPrincipalRedemption": ["P1ML0", "P3ML0"],
            "arrayNextPrincipalRedemptionPayment": ["100", 60],
            "arrayIncreaseDecrease": ["INC", "DEC"],
            "arrayCycleAnchorDateOfInterestPayment": ["2026-01-01T00:00:00"],
            "arrayCycleOfInterestPayment": ["P1ML1"],
            "arrayCycleAnchorDateOfRateReset": ["2026-07-01T00:00:00"],
            "arrayCycleOfRateReset": ["P6ML0"],
            "arrayRate": ["0.05", 0.02],
            "arrayFixedVariable": "F"
        });
        let terms: ContractTerms = serde_json::from_value(json).expect("terms");
        assert_eq!(
            terms.array_cycle_anchor_date_of_principal_redemption,
            Some(vec![
                NaiveDateTime::parse_from_str("2026-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap(),
                NaiveDateTime::parse_from_str("2027-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S").unwrap(),
            ])
        );
        assert_eq!(
            terms.array_cycle_of_principal_redemption,
            Some(vec![
                "P1ML0".parse::<Cycle>().unwrap(),
                "P3ML0".parse::<Cycle>().unwrap()
            ])
        );
        assert_eq!(
            terms.array_next_principal_redemption_payment,
            Some(vec![
                Decimal::from_str_exact("100").unwrap(),
                Decimal::from(60)
            ])
        );
        assert_eq!(
            terms.array_increase_decrease,
            Some(vec![
                crate::enums::ArrayIncDec::Inc,
                crate::enums::ArrayIncDec::Dec
            ])
        );
        assert_eq!(
            terms.array_rate,
            Some(vec![
                Decimal::from_str_exact("0.05").unwrap(),
                Decimal::try_from(0.02).unwrap()
            ])
        );
        assert_eq!(
            terms.array_fixed_variable,
            Some(crate::enums::ArrayFixVar::Fixed)
        );
        let presence: Vec<(&str, bool)> = terms
            .presence()
            .into_iter()
            .filter(|(_, set)| *set)
            .collect();
        assert_eq!(presence.len(), 11);
    }

    #[test]
    fn array_attributes_default_to_none_and_skip_serialization() {
        let terms: ContractTerms =
            serde_json::from_value(serde_json::json!({"contractType": "PAM"})).expect("terms");
        assert!(terms
            .array_cycle_anchor_date_of_principal_redemption
            .is_none());
        assert!(terms.array_next_principal_redemption_payment.is_none());
        assert!(terms.array_fixed_variable.is_none());
        let wire = serde_json::to_value(&terms).expect("wire");
        assert!(wire.get("arrayRate").is_none());
        assert!(wire.get("arrayIncreaseDecrease").is_none());
    }
}
