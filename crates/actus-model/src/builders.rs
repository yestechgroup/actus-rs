//! Applicability-enforcing builders for the fixed-income family.
//!
//! Each builder pins [`ContractTerms`] to one contract type and enforces, at
//! `build()` time, the GENERATED applicability tables: every base-required
//! attribute must be present and every set attribute must be applicable to
//! that contract type. The base-required tier is the dictionary `NN` set
//! minus administrative identifiers (`contractType`, `contractID`,
//! `creatorID`, `counterpartyID`), minus `dayCountConvention` (upstream
//! tooling defaults it to `A365`; the engine applies its own fallback), minus
//! attributes only mandatory in scenario columns (purchase, termination,
//! scaling, rate-reset legs).

use crate::cycle::Cycle;
use crate::enums::{
    BusinessDayConvention, Calendar, ContractRole, DayCountConvention, DeliverySettlement,
    EndOfMonthConvention, InterestCalculationBase, ScalingEffect,
};
use crate::error::ModelError;
use crate::generated::applicability;
use crate::generated::contract_type::ContractType;
use crate::terms::{ContractReference, ContractTerms};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;

fn validate(terms: &ContractTerms) -> Result<(), ModelError> {
    let tables = applicability::tables(terms.contract_type);
    let presence = terms.presence();
    for (identifier, present) in presence {
        if present && !tables.applicable.contains(&identifier) {
            return Err(ModelError::AttributeNotApplicable {
                attribute: identifier,
                contract_type: terms.contract_type.as_acronym(),
            });
        }
    }
    for identifier in tables.base_required {
        if !presence.iter().any(|(id, set)| *id == *identifier && *set) {
            return Err(ModelError::MissingRequiredAttribute {
                attribute: identifier,
                contract_type: terms.contract_type.as_acronym(),
            });
        }
    }
    Ok(())
}

macro_rules! setter_methods {
    () => {
        /// ACTUS attribute `CID` — Contract Identifier.
        #[must_use]
        pub fn set_contract_id(mut self, value: impl Into<String>) -> Self {
            self.terms.contract_id = Some(value.into());
            self
        }

        /// ACTUS attribute `CNTRL` — Contract Role.
        #[must_use]
        pub fn set_contract_role(mut self, value: ContractRole) -> Self {
            self.terms.contract_role = Some(value);
            self
        }

        /// ACTUS attribute `SD` — Status Date.
        #[must_use]
        pub fn set_status_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.status_date = Some(value);
            self
        }

        /// ACTUS attribute `CDD` — Contract Deal Date.
        #[must_use]
        pub fn set_contract_deal_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.contract_deal_date = Some(value);
            self
        }

        /// ACTUS attribute `IED` — Initial Exchange Date.
        #[must_use]
        pub fn set_initial_exchange_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.initial_exchange_date = Some(value);
            self
        }

        /// ACTUS attribute `MD` — Maturity Date.
        #[must_use]
        pub fn set_maturity_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.maturity_date = Some(value);
            self
        }

        /// ACTUS attribute `NT` — Notional Principal.
        #[must_use]
        pub fn set_notional_principal(mut self, value: Decimal) -> Self {
            self.terms.notional_principal = Some(value);
            self
        }

        /// ACTUS attribute `IPNR` — Nominal Interest Rate.
        #[must_use]
        pub fn set_nominal_interest_rate(mut self, value: Decimal) -> Self {
            self.terms.nominal_interest_rate = Some(value);
            self
        }

        /// ACTUS attribute `IPAC` — Accrued Interest.
        #[must_use]
        pub fn set_accrued_interest(mut self, value: Decimal) -> Self {
            self.terms.accrued_interest = Some(value);
            self
        }

        /// ACTUS attribute `IPANX` — Cycle Anchor Date Of Interest Payment.
        #[must_use]
        pub fn set_cycle_anchor_date_of_interest_payment(mut self, value: NaiveDateTime) -> Self {
            self.terms.cycle_anchor_date_of_interest_payment = Some(value);
            self
        }

        /// ACTUS attribute `IPCL` — Cycle Of Interest Payment.
        #[must_use]
        pub fn set_cycle_of_interest_payment(mut self, value: Cycle) -> Self {
            self.terms.cycle_of_interest_payment = Some(value);
            self
        }

        /// ACTUS attribute `PRANX` — Cycle Anchor Date Of Principal Redemption.
        #[must_use]
        pub fn set_cycle_anchor_date_of_principal_redemption(
            mut self,
            value: NaiveDateTime,
        ) -> Self {
            self.terms.cycle_anchor_date_of_principal_redemption = Some(value);
            self
        }

        /// ACTUS attribute `PRCL` — Cycle Of Principal Redemption.
        #[must_use]
        pub fn set_cycle_of_principal_redemption(mut self, value: Cycle) -> Self {
            self.terms.cycle_of_principal_redemption = Some(value);
            self
        }

        /// ACTUS attribute `RRANX` — Cycle Anchor Date Of Rate Reset.
        #[must_use]
        pub fn set_cycle_anchor_date_of_rate_reset(mut self, value: NaiveDateTime) -> Self {
            self.terms.cycle_anchor_date_of_rate_reset = Some(value);
            self
        }

        /// ACTUS attribute `RRCL` — Cycle Of Rate Reset.
        #[must_use]
        pub fn set_cycle_of_rate_reset(mut self, value: Cycle) -> Self {
            self.terms.cycle_of_rate_reset = Some(value);
            self
        }

        /// ACTUS attribute `IPCBANX` — Cycle Anchor Date Of Interest Calculation Base.
        #[must_use]
        pub fn set_cycle_anchor_date_of_interest_calculation_base(
            mut self,
            value: NaiveDateTime,
        ) -> Self {
            self.terms.cycle_anchor_date_of_interest_calculation_base = Some(value);
            self
        }

        /// ACTUS attribute `IPCBCL` — Cycle Of Interest Calculation Base.
        #[must_use]
        pub fn set_cycle_of_interest_calculation_base(mut self, value: Cycle) -> Self {
            self.terms.cycle_of_interest_calculation_base = Some(value);
            self
        }

        /// ACTUS attribute `SCANX` — Cycle Anchor Date Of Scaling Index.
        #[must_use]
        pub fn set_cycle_anchor_date_of_scaling_index(mut self, value: NaiveDateTime) -> Self {
            self.terms.cycle_anchor_date_of_scaling_index = Some(value);
            self
        }

        /// ACTUS attribute `SCCL` — Cycle Of Scaling Index.
        #[must_use]
        pub fn set_cycle_of_scaling_index(mut self, value: Cycle) -> Self {
            self.terms.cycle_of_scaling_index = Some(value);
            self
        }

        /// ACTUS attribute `IPDC` — Day Count Convention.
        #[must_use]
        pub fn set_day_count_convention(mut self, value: DayCountConvention) -> Self {
            self.terms.day_count_convention = Some(value);
            self
        }

        /// ACTUS attribute `EOMC` — End Of Month Convention.
        #[must_use]
        pub fn set_end_of_month_convention(mut self, value: EndOfMonthConvention) -> Self {
            self.terms.end_of_month_convention = Some(value);
            self
        }

        /// ACTUS attribute `BDC` — Business Day Convention.
        #[must_use]
        pub fn set_business_day_convention(mut self, value: BusinessDayConvention) -> Self {
            self.terms.business_day_convention = Some(value);
            self
        }

        /// ACTUS attribute `CLDR` — Calendar.
        #[must_use]
        pub fn set_calendar(mut self, value: Calendar) -> Self {
            self.terms.calendar = Some(value);
            self
        }

        /// ACTUS attribute `RRMLT` — Rate Multiplier.
        #[must_use]
        pub fn set_rate_multiplier(mut self, value: Decimal) -> Self {
            self.terms.rate_multiplier = Some(value);
            self
        }

        /// ACTUS attribute `RRSP` — Rate Spread.
        #[must_use]
        pub fn set_rate_spread(mut self, value: Decimal) -> Self {
            self.terms.rate_spread = Some(value);
            self
        }

        /// ACTUS attribute `RRNXT` — Next Reset Rate.
        #[must_use]
        pub fn set_next_reset_rate(mut self, value: Decimal) -> Self {
            self.terms.next_reset_rate = Some(value);
            self
        }

        /// Rate reset fixing offset (raw ISO 8601 period, e.g. `P2D`); not in
        /// dictionary v1.4.
        #[must_use]
        pub fn set_fixing_days(mut self, value: impl Into<String>) -> Self {
            self.terms.fixing_days = Some(value.into());
            self
        }

        /// ACTUS attribute `RRMO` — Market Object Code Of Rate Reset.
        #[must_use]
        pub fn set_market_object_code_of_rate_reset(mut self, value: impl Into<String>) -> Self {
            self.terms.market_object_code_of_rate_reset = Some(value.into());
            self
        }

        /// ACTUS attribute `PDIED` — Premium Discount At IED.
        #[must_use]
        pub fn set_premium_discount_at_ied(mut self, value: Decimal) -> Self {
            self.terms.premium_discount_at_ied = Some(value);
            self
        }

        /// ACTUS attribute `IPCED` — Capitalization End Date.
        #[must_use]
        pub fn set_capitalization_end_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.capitalization_end_date = Some(value);
            self
        }

        /// ACTUS attribute `AMD` — Amortization Date.
        #[must_use]
        pub fn set_amortization_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.amortization_date = Some(value);
            self
        }

        /// ACTUS attribute `PRNXT` — Next Principal Redemption Payment.
        #[must_use]
        pub fn set_next_principal_redemption_payment(mut self, value: Decimal) -> Self {
            self.terms.next_principal_redemption_payment = Some(value);
            self
        }

        /// ACTUS attribute `IPCB` — Interest Calculation Base.
        #[must_use]
        pub fn set_interest_calculation_base(mut self, value: InterestCalculationBase) -> Self {
            self.terms.interest_calculation_base = Some(value);
            self
        }

        /// ACTUS attribute `IPCBA` — Interest Calculation Base Amount.
        #[must_use]
        pub fn set_interest_calculation_base_amount(mut self, value: Decimal) -> Self {
            self.terms.interest_calculation_base_amount = Some(value);
            self
        }

        /// ACTUS attribute `SCEF` — Scaling Effect.
        #[must_use]
        pub fn set_scaling_effect(mut self, value: ScalingEffect) -> Self {
            self.terms.scaling_effect = Some(value);
            self
        }

        /// ACTUS attribute `SCMO` — Market Object Code Of Scaling Index.
        #[must_use]
        pub fn set_market_object_code_of_scaling_index(mut self, value: impl Into<String>) -> Self {
            self.terms.market_object_code_of_scaling_index = Some(value.into());
            self
        }

        /// ACTUS attribute `SCCDD` — Scaling Index At Contract Deal Date.
        #[must_use]
        pub fn set_scaling_index_at_contract_deal_date(mut self, value: Decimal) -> Self {
            self.terms.scaling_index_at_contract_deal_date = Some(value);
            self
        }

        /// ACTUS attribute `SCNT` — Notional Scaling Multiplier.
        #[must_use]
        pub fn set_notional_scaling_multiplier(mut self, value: Decimal) -> Self {
            self.terms.notional_scaling_multiplier = Some(value);
            self
        }

        /// ACTUS attribute `SCIP` — Interest Scaling Multiplier.
        #[must_use]
        pub fn set_interest_scaling_multiplier(mut self, value: Decimal) -> Self {
            self.terms.interest_scaling_multiplier = Some(value);
            self
        }

        /// ACTUS attribute `PRD` — Purchase Date.
        #[must_use]
        pub fn set_purchase_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.purchase_date = Some(value);
            self
        }

        /// ACTUS attribute `PPRD` — Price At Purchase Date.
        #[must_use]
        pub fn set_price_at_purchase_date(mut self, value: Decimal) -> Self {
            self.terms.price_at_purchase_date = Some(value);
            self
        }

        /// ACTUS attribute `TD` — Termination Date.
        #[must_use]
        pub fn set_termination_date(mut self, value: NaiveDateTime) -> Self {
            self.terms.termination_date = Some(value);
            self
        }

        /// ACTUS attribute `PTD` — Price At Termination Date.
        #[must_use]
        pub fn set_price_at_termination_date(mut self, value: Decimal) -> Self {
            self.terms.price_at_termination_date = Some(value);
            self
        }

        /// ACTUS attribute `CUR` — Currency (ISO 4217).
        #[must_use]
        pub fn set_currency(mut self, value: impl Into<String>) -> Self {
            self.terms.currency = Some(value.into());
            self
        }

        /// ACTUS attribute `XDN` — X Day Notice (ISO 8601 period string).
        #[must_use]
        pub fn set_x_day_notice(mut self, value: impl Into<String>) -> Self {
            self.terms.x_day_notice = Some(value.into());
            self
        }

        /// ACTUS attribute `DS` — Delivery Settlement.
        #[must_use]
        pub fn set_delivery_settlement(mut self, value: DeliverySettlement) -> Self {
            self.terms.delivery_settlement = Some(value);
            self
        }

        /// ACTUS attribute `CTS` — Contract Structure (contract references).
        #[must_use]
        pub fn set_contract_structure(mut self, value: Vec<ContractReference>) -> Self {
            self.terms.contract_structure = Some(value);
            self
        }
    };
}

macro_rules! contract_builder {
    (
        $(#[$meta:meta])*
        $name:ident, $variant:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name {
            terms: ContractTerms,
        }

        impl $name {
            /// A builder with [`ContractTerms::contract_type`] pinned and all
            /// attributes unset.
            #[must_use]
            pub fn new() -> $name {
                $name {
                    terms: ContractTerms::new($variant),
                }
            }

            setter_methods!();

            /// Enforce the generated applicability tables and return the terms.
            ///
            /// # Errors
            /// [`ModelError::MissingRequiredAttribute`] when a base-required
            /// attribute is absent; [`ModelError::AttributeNotApplicable`]
            /// when a set attribute is not applicable to the contract type.
            pub fn build(self) -> Result<ContractTerms, ModelError> {
                validate(&self.terms)?;
                Ok(self.terms)
            }
        }

        impl Default for $name {
            fn default() -> $name {
                $name::new()
            }
        }
    };
}

contract_builder!(
    /// Builder for ACTUS `PAM` — Principal at Maturity.
    PamBuilder,
    ContractType::Pam
);

contract_builder!(
    /// Builder for ACTUS `LAM` — Linear Amortizer.
    LamBuilder,
    ContractType::Lam
);

contract_builder!(
    /// Builder for ACTUS `NAM` — Negative Amortizer.
    NamBuilder,
    ContractType::Nam
);

contract_builder!(
    /// Builder for ACTUS `ANN` — Annuity.
    AnnBuilder,
    ContractType::Ann
);
