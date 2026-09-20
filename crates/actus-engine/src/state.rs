//! Contract state variables (ACTUS dictionary "States" vocabulary and
//! techspec section "State Variables"; initialisation follows the PAM
//! states-at-t0 table in techspec section "PAM: Principal At Maturity").

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::{ContractTerms, ScalingEffect};
/// Lifetime status of a contract under evaluation.
///
/// Derived from the Contract Performance state (`PRF`) of the states
/// dictionary: the performing, delayed, delinquent and default stages map to
/// [`ContractStatus::Active`], `MA` (matured) to
/// [`ContractStatus::Matured`] and `TE` (terminated) to
/// [`ContractStatus::Terminated`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContractStatus {
    /// Contract is within its lifetime and performing its schedule.
    #[default]
    Active,
    /// Contract reached maturity (`MD`): principal redeemed, states zeroed.
    Matured,
    /// Contract was terminated early (`TD`).
    Terminated,
}

/// The state variables carried through a contract evaluation.
///
/// Field names follow the states dictionary identifiers; the techspec
/// acronyms are given in the doc comments. Decimal-valued states use
/// `Decimal`; time-valued states use `NaiveDateTime`.
///
/// The accrual anchor for interest is the status date `SD`
/// (`status_date`): every state transition function updates
/// `status_date` to the event time, and accrued interest is accumulated as
/// `IPAC(t+) = IPAC(t-) + YF(SD(t-), t) * IPNR(t-) * NT(t-)` (techspec
/// section "State Transition Functions").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContractState {
    /// Notional principal `NT`.
    pub notional_principal: Decimal,
    /// Nominal interest rate `IPNR`.
    pub nominal_interest_rate: Decimal,
    /// Accrued interest `IPAC`.
    pub accrued_interest: Decimal,
    /// Accrued fee `FEAC`.
    pub fee_accrued: Decimal,
    /// Interest calculation base amount `IPCB`.
    pub interest_calculation_base_amount: Decimal,
    /// Next principal redemption payment `PRNXT`.
    pub next_principal_redemption_payment: Decimal,
    /// Notional scaling multiplier `NSC`.
    pub notional_scaling_multiplier: Decimal,
    /// Interest scaling multiplier `ISC`.
    pub interest_scaling_multiplier: Decimal,
    /// Time of the most recent interest payment event, when one has applied.
    pub last_interest_payment_date: Option<NaiveDateTime>,
    /// Status date `SD`: the accrual anchor, equal to the last event time.
    pub status_date: NaiveDateTime,
    /// Lifetime status of the contract.
    pub contract_status: ContractStatus,
}

impl ContractState {
    /// Builds the pre-initial-exchange-date state from contract terms.
    ///
    /// Mirrors the states-at-t0 table for the fixed income family at a
    /// status date before `IED`: notional principal, nominal interest rate,
    /// accrued interest, accrued fee and interest calculation base are zero;
    /// the scaling multipliers are initialised from the scaling effect
    /// (`SCEF`) and scaling index at status date (`SCIXSD`) terms; the next
    /// principal redemption payment is taken from the terms when present.
    /// Contract-type specific initialisation (e.g. interest accrued between
    /// `IPANX` and `IED`) is applied by the evaluation implementations of
    /// the contract types themselves.
    pub fn initial(terms: &ContractTerms) -> ContractState {
        let (interest_scaling_multiplier, notional_scaling_multiplier) = scaling_multipliers(terms);
        ContractState {
            notional_principal: Decimal::ZERO,
            nominal_interest_rate: Decimal::ZERO,
            accrued_interest: Decimal::ZERO,
            fee_accrued: Decimal::ZERO,
            interest_calculation_base_amount: Decimal::ZERO,
            next_principal_redemption_payment: terms
                .next_principal_redemption_payment
                .unwrap_or(Decimal::ZERO),
            notional_scaling_multiplier,
            interest_scaling_multiplier,
            last_interest_payment_date: None,
            status_date: initial_status_date(terms),
            contract_status: ContractStatus::Active,
        }
    }
}

/// Maps the scaling effect (`SCEF`) onto the initial `ISC`/`NSC` pair.
///
/// Per the PAM states-at-t0 table: `NSC` takes the scaling index at status
/// date (`SCIXSD`) when the scaling effect affects notional (`[x]N[x]`),
/// `ISC` takes it when the scaling effect affects interest (`I[x][x]`);
/// both default to 1.0 otherwise.
fn scaling_multipliers(terms: &ContractTerms) -> (Decimal, Decimal) {
    let scaling_index_at_status_date = terms
        .scaling_index_at_contract_deal_date
        .unwrap_or(Decimal::ONE);
    let none = (Decimal::ONE, Decimal::ONE);
    match terms.scaling_effect {
        None => none,
        Some(effect) => match effect {
            ScalingEffect::NoScaling => none,
            ScalingEffect::Interest => (scaling_index_at_status_date, Decimal::ONE),
            ScalingEffect::Notional => (Decimal::ONE, scaling_index_at_status_date),
            ScalingEffect::InterestAndNotional => {
                (scaling_index_at_status_date, scaling_index_at_status_date)
            }
        },
    }
}

/// Resolves the status date the initial state refers to.
///
/// Falls back from `statusDate` to `contractDealDate` and then to the initial
/// exchange date, mirroring how the testbeds anchor pre-evaluation states.
fn initial_status_date(terms: &ContractTerms) -> NaiveDateTime {
    terms
        .status_date
        .or(terms.contract_deal_date)
        .or(terms.initial_exchange_date)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use actus_model::ContractType;
    use rust_decimal_macros::dec;

    fn base_terms() -> ContractTerms {
        let mut terms = ContractTerms::new(ContractType::Pam);
        terms.status_date = Some(t("2012-12-30T00:00:00"));
        terms.contract_deal_date = Some(t("2012-12-28T00:00:00"));
        terms.initial_exchange_date = Some(t("2013-01-01T00:00:00"));
        terms
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    #[test]
    fn initial_state_is_zeroed_before_initial_exchange() {
        let state = ContractState::initial(&base_terms());
        assert_eq!(state.notional_principal, Decimal::ZERO);
        assert_eq!(state.nominal_interest_rate, Decimal::ZERO);
        assert_eq!(state.accrued_interest, Decimal::ZERO);
        assert_eq!(state.fee_accrued, Decimal::ZERO);
        assert_eq!(state.interest_calculation_base_amount, Decimal::ZERO);
        assert_eq!(state.contract_status, ContractStatus::Active);
        assert_eq!(state.last_interest_payment_date, None);
    }

    #[test]
    fn initial_state_prefers_status_date_as_accrual_anchor() {
        let state = ContractState::initial(&base_terms());
        assert_eq!(state.status_date, t("2012-12-30T00:00:00"));

        let mut terms = base_terms();
        terms.status_date = None;
        let state = ContractState::initial(&terms);
        assert_eq!(state.status_date, t("2012-12-28T00:00:00"));

        terms.contract_deal_date = None;
        let state = ContractState::initial(&terms);
        assert_eq!(state.status_date, t("2013-01-01T00:00:00"));
    }

    #[test]
    fn initial_state_carries_next_principal_redemption_from_terms() {
        let mut terms = base_terms();
        terms.next_principal_redemption_payment = Some(dec!(450));
        let state = ContractState::initial(&terms);
        assert_eq!(state.next_principal_redemption_payment, dec!(450));

        let state = ContractState::initial(&base_terms());
        assert_eq!(state.next_principal_redemption_payment, Decimal::ZERO);
    }

    #[test]
    fn scaling_effect_maps_to_initial_multipliers() {
        let mut terms = base_terms();
        terms.scaling_effect = Some(ScalingEffect::Notional);
        terms.scaling_index_at_contract_deal_date = Some(dec!(1.25));
        let state = ContractState::initial(&terms);
        assert_eq!(state.notional_scaling_multiplier, dec!(1.25));
        assert_eq!(state.interest_scaling_multiplier, Decimal::ONE);

        terms.scaling_effect = Some(ScalingEffect::Interest);
        let state = ContractState::initial(&terms);
        assert_eq!(state.notional_scaling_multiplier, Decimal::ONE);
        assert_eq!(state.interest_scaling_multiplier, dec!(1.25));

        terms.scaling_effect = Some(ScalingEffect::InterestAndNotional);
        let state = ContractState::initial(&terms);
        assert_eq!(state.notional_scaling_multiplier, dec!(1.25));
        assert_eq!(state.interest_scaling_multiplier, dec!(1.25));

        terms.scaling_effect = Some(ScalingEffect::NoScaling);
        let state = ContractState::initial(&terms);
        assert_eq!(state.notional_scaling_multiplier, Decimal::ONE);
        assert_eq!(state.interest_scaling_multiplier, Decimal::ONE);
    }

    #[test]
    fn state_exposes_conformance_result_variables() {
        let mut state = ContractState::initial(&base_terms());
        state.notional_principal = dec!(3000);
        state.nominal_interest_rate = dec!(0.1);
        state.accrued_interest = dec!(25.4794520547945);
        assert_eq!(state.notional_principal, dec!(3000));
        assert_eq!(state.nominal_interest_rate, dec!(0.1));
        assert_eq!(state.accrued_interest, dec!(25.4794520547945));
    }
}
