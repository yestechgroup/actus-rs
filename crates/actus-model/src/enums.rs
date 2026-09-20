//! ACTUS enum attribute vocabularies (tokens from the vendored dictionary
//! `allowedValues`, cross-checked against the vendored testbeds).

use crate::error::ModelError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

macro_rules! actus_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $token:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $(
                $(#[$vmeta])*
                #[serde(rename = $token)]
                $variant,
            )*
        }

        impl $name {
            /// All values of this vocabulary in dictionary order.
            #[must_use]
            pub fn all() -> &'static [$name] {
                &[$($name::$variant),*]
            }

            /// The dictionary token for this value.
            #[must_use]
            pub fn as_token(&self) -> &'static str {
                match self {
                    $($name::$variant => $token),*
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_token())
            }
        }

        impl FromStr for $name {
            type Err = ModelError;

            fn from_str(s: &str) -> Result<$name, ModelError> {
                match s {
                    $($token => Ok($name::$variant),)*
                    _ => Err(ModelError::UnknownEnumValue {
                        vocabulary: stringify!($name),
                        value: s.to_string(),
                    }),
                }
            }
        }
    };
}

actus_enum! {
    /// ACTUS attribute `CNTRL` — Contract Role. Perspective of the contract
    /// parties; determines payoff orientation.
    pub enum ContractRole {
        /// `RPA` — real position long, performer pays the counterparty.
        Rpa => "RPA",
        /// `RPL` — real position long, performer receives.
        Rpl => "RPL",
        /// `RFL` — real position long, both parties pay/receive.
        Rfl => "RFL",
        /// `PFL` — secondary position long.
        Pfl => "PFL",
        /// `RF` — real position short.
        Rf => "RF",
        /// `PF` — secondary position short.
        Pf => "PF",
        /// `BUY` — buyer.
        Buy => "BUY",
        /// `SEL` — seller.
        Sel => "SEL",
        /// `COL` — collateral taker/provider perspective.
        Col => "COL",
        /// `CNO` — credit event only observer.
        Cno => "CNO",
        /// `UDL` — underlier leg long.
        Udl => "UDL",
        /// `UDLP` — underlier leg long, paying fixed.
        Udlp => "UDLP",
        /// `UDLM` — underlier leg short.
        Udlm => "UDLM",
    }
}

actus_enum! {
    /// ACTUS attribute `IPDC` — Day Count Convention. Tokens are the exact
    /// dictionary spellings (v1.4 defines no `30U360` or `BF252`).
    pub enum DayCountConvention {
        /// `AA` — actual/actual.
        Aa => "AA",
        /// `A360` — actual/360.
        A360 => "A360",
        /// `A365` — actual/365.
        A365 => "A365",
        /// `30E360ISDA` — 30E/360 ISDA.
        ThirtyE360Isda => "30E360ISDA",
        /// `30E360` — 30E/360.
        ThirtyE360 => "30E360",
        /// `28E336` — 28E/336.
        TwentyEightE336 => "28E336",
    }
}

actus_enum! {
    /// ACTUS attribute `EOMC` — End Of Month Convention.
    pub enum EndOfMonthConvention {
        /// `SD` — same day.
        Sd => "SD",
        /// `EOM` — end of month.
        Eom => "EOM",
    }
}

actus_enum! {
    /// ACTUS attribute `BDC` — Business Day Convention. The dictionary lists
    /// nine `allowedValues` entries but the acronym `SCMP` (options 6 and 8)
    /// is duplicated upstream, so eight distinct tokens exist.
    pub enum BusinessDayConvention {
        /// `NOS` — no shift.
        Nos => "NOS",
        /// `SCF` — shift-calculate following.
        Scf => "SCF",
        /// `SCMF` — shift-calculate modified following.
        Scmf => "SCMF",
        /// `CSF` — calculate-shift following.
        Csf => "CSF",
        /// `CSMF` — calculate-shift modified following.
        Csmf => "CSMF",
        /// `SCP` — shift-calculate preceding.
        Scp => "SCP",
        /// `SCMP` — shift-calculate modified preceding.
        Scmp => "SCMP",
        /// `CSP` — calculate-shift preceding.
        Csp => "CSP",
    }
}

actus_enum! {
    /// ACTUS attribute `CLDR` — Calendar. v1.4 defines only `NC` and `MF`
    /// (there is no `NO` or `SCF` calendar token).
    pub enum Calendar {
        /// `NC` — no calendar.
        Nc => "NC",
        /// `MF` — Monday to Friday.
        Mf => "MF",
    }
}

actus_enum! {
    /// ACTUS attribute `IPCB` — Interest Calculation Base.
    pub enum InterestCalculationBase {
        /// `NT` — notional principal.
        Nt => "NT",
        /// `NTIED` — notional at initial exchange date.
        Ntied => "NTIED",
        /// `NTL` — notional minus redeemed principal.
        Ntl => "NTL",
    }
}

actus_enum! {
    /// ACTUS attribute `SCEF` — Scaling Effect. The dictionary tokens use
    /// digit `0` (`000`, `I00`, `0N0`, `IN0`) while the vendored testbeds
    /// spell them with letter `O` (`OOO`, `IOO`, `ONO`, `INO`); both spellings
    /// deserialize and the canonical dictionary token is displayed.
    pub enum ScalingEffect {
        /// `000` — no scaling.
        #[serde(alias = "OOO")]
        NoScaling => "000",
        /// `I00` — scale interest only.
        #[serde(alias = "IOO")]
        Interest => "I00",
        /// `0N0` — scale notional only.
        #[serde(alias = "ONO")]
        Notional => "0N0",
        /// `IN0` — scale interest and notional.
        #[serde(alias = "INO")]
        InterestAndNotional => "IN0",
    }
}

actus_enum! {
    /// ACTUS attribute `DS` — Delivery Settlement.
    pub enum DeliverySettlement {
        /// `S` — physical settlement.
        S => "S",
        /// `D` — cash settlement.
        D => "D",
    }
}

actus_enum! {
    /// ACTUS contract-reference attribute `RTP` — Reference Type (dictionary
    /// `contractReference.type` codelist): what kind of object a
    /// [`crate::terms::ContractReference`] points at.
    pub enum ContractReferenceType {
        /// `CNT` — an actual contract object.
        Contract => "CNT",
        /// `CID` — the identifier of an actual contract.
        ContractIdentifier => "CID",
        /// `MOC` — the identifier of a market object.
        MarketObjectIdentifier => "MOC",
        /// `EID` — the identifier of a legal entity.
        LegalEntityIdentifier => "EID",
        /// `CST` — a nested ContractStructure.
        ContractStructure => "CST",
    }
}

actus_enum! {
    /// ACTUS contract-reference attribute `RRL` — Reference Role (dictionary
    /// `contractReference.role` codelist): the part a referenced object plays
    /// in the parent contract.
    pub enum ContractReferenceRole {
        /// `UDL` — a simple underlyer contract.
        Underlying => "UDL",
        /// `FIL` — the first leg contract of a swap.
        FirstLeg => "FIL",
        /// `SEL` — the second leg contract of a swap.
        SecondLeg => "SEL",
        /// `COVE` — a contract covered under the parent contract.
        CoveredContract => "COVE",
        /// `COVI` — a contract covering contracts under the parent.
        CoveringContract => "COVI",
    }
}

actus_enum! {
    /// ACTUS attribute `ARINCDEC` — Array Increase Decrease (dictionary
    /// `arrayIncreaseDecrease`, type `Enum[]`). Element `i` states whether
    /// the `i`-th principal-redemption segment of an array-scheduled
    /// maturity contract (`ANX`, `NAX`, `LAX`) increases or decreases the
    /// notional (paper §7.3).
    pub enum ArrayIncDec {
        /// `INC` — the notional is increased in this period.
        Inc => "INC",
        /// `DEC` — the notional is decreased in this period.
        Dec => "DEC",
    }
}

actus_enum! {
    /// ACTUS attribute `ARFIXVAR` — Array Fixed Variable (dictionary
    /// `arrayFixedVariable`). Defines the meaning of the `ARRATE`
    /// (`arrayRate`) elements of an array-type rate reset schedule. The
    /// allowedValues acronyms are `F`/`V`; the dictionary description spells
    /// them `FIX`/`VAR`, so both spellings deserialize and the acronym is
    /// displayed.
    pub enum ArrayFixVar {
        /// `F` — `arrayRate` carries the fixed nominal interest rate
        /// (corresponding to `IPNR`).
        #[serde(alias = "FIX")]
        Fixed => "F",
        /// `V` — `arrayRate` carries the spread on top of the reference
        /// rate (corresponding to `RRSP`).
        #[serde(alias = "VAR")]
        Variable => "V",
    }
}

actus_enum! {
    /// ACTUS attribute `FEB` — Fee Basis (dictionary `feeBasis`): how the
    /// fee rate `FER` is interpreted when fee events (`FP`) are paid.
    pub enum FeeBasis {
        /// `A` — the fee rate represents an absolute value.
        AbsoluteValue => "A",
        /// `N` — the fee rate applies to the nominal value.
        NominalValue => "N",
    }
}

actus_enum! {
    /// ACTUS attribute `CETC` — Credit Event Type Covered (dictionary
    /// `creditEventTypeCovered`, type `Enum[]`; the testbeds carry a single
    /// token). Which contract performance state of the covered contracts
    /// triggers the protection of a credit enhancement contract.
    pub enum CreditEventType {
        /// `DL` — delay of the underlying is a credit event.
        Delayed => "DL",
        /// `DQ` — delinquency of the underlying is a credit event.
        Delinquent => "DQ",
        /// `DF` — default of the underlying is a credit event.
        Default => "DF",
    }
}

actus_enum! {
    /// ACTUS attribute `CEGE` — Guaranteed Exposure (dictionary
    /// `guaranteedExposure`): which value of the covered exposure a credit
    /// enhancement contract guarantees.
    pub enum GuaranteedExposure {
        /// `NO` — nominal value of the exposure is covered.
        NominalValue => "NO",
        /// `NI` — nominal value plus accrued interest is covered.
        NominalValuePlusInterest => "NI",
        /// `MV` — market value of the exposure is covered.
        MarketValue => "MV",
    }
}

actus_enum! {
    /// ACTUS attribute `OPTP` — Option Type (dictionary `optionType`): the
    /// direction of the option right. Combined with `CNTRL`, which defines
    /// whether the creator is the buyer or the seller of the right.
    pub enum OptionType {
        /// `C` — call option.
        Call => "C",
        /// `P` — put option.
        Put => "P",
        /// `CP` — combination of call and put option.
        CallPut => "CP",
    }
}

actus_enum! {
    /// ACTUS attribute `OPXT` — Option Exercise Type (dictionary
    /// `optionExerciseType`): the exercise style of an option.
    pub enum OptionExerciseType {
        /// `E` — European-type exercise (at a specific date).
        European => "E",
        /// `B` — Bermudan-type exercise (at certain points during a span
        /// of time).
        Bermudan => "B",
        /// `A` — American-type exercise (during a span of time).
        American => "A",
    }
}

actus_enum! {
    /// ACTUS attribute `PPEF` — Prepayment Effect (dictionary
    /// `prepaymentEffect`): whether the prepayment right exists and how a
    /// prepayment affects the remaining principal redemption schedule.
    pub enum PrepaymentEffect {
        /// `N` — prepayment is not allowed under the agreement.
        NoPrepayment => "N",
        /// `A` — prepayment reduces the redemption amount for the remaining
        /// period up to maturity.
        ReducesRedemptionAmount => "A",
        /// `M` — prepayment reduces the maturity.
        ReducesMaturity => "M",
    }
}

actus_enum! {
    /// ACTUS attribute `PYTP` — Penalty Type (dictionary `penaltyType`):
    /// which penalty applies to a prepayment. The dictionary `defaultValue`
    /// column carries the stray token `O` that its own `allowedValues` do
    /// not define; the four defined acronyms are modelled here.
    pub enum PenaltyType {
        /// `N` — no penalty applies.
        NoPenalty => "N",
        /// `A` — a fixed amount applies as penalty.
        FixedPenalty => "A",
        /// `R` — a penalty relative to the notional outstanding applies.
        RelativePenalty => "R",
        /// `I` — a penalty based on the current interest rate differential
        /// relative to the notional outstanding applies.
        InterestRateDifferential => "I",
    }
}

actus_enum! {
    /// ACTUS attributes `IPPNT` — Cycle Point Of Interest Payment and
    /// `RRPNT` — Cycle Point Of Rate Reset (dictionary `cyclePoint`
    /// codelist): whether the cyclic payment respectively rate applies at
    /// the beginning or the end of its cycle.
    pub enum CyclePoint {
        /// `B` — the value applies at the beginning of the cycle.
        Beginning => "B",
        /// `E` — the value applies at the end of the cycle.
        End => "E",
    }
}

actus_enum! {
    /// ACTUS state variable `PRF` — Contract Performance (states dictionary
    /// `contractPerformance`). Lifetime credit state of a contract, carried
    /// by externally observed credit events (`CE`) that trigger credit
    /// enhancement contracts (CEC/CEG).
    pub enum ContractPerformance {
        /// `PF` — contract performs according to its terms.
        Performant => "PF",
        /// `DL` — payment obligations delayed per the grace period.
        Delayed => "DL",
        /// `DQ` — payment obligations delinquent per the delinquency period.
        Delinquent => "DQ",
        /// `DF` — contract defaulted on payment obligations.
        Default => "DF",
        /// `MA` — contract matured.
        Matured => "MA",
        /// `TE` — contract terminated.
        Terminated => "TE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_role_round_trips() {
        for role in ContractRole::all() {
            assert_eq!(role.to_string().parse::<ContractRole>().as_ref(), Ok(role));
        }
    }

    #[test]
    fn day_count_dictionary_tokens() {
        assert_eq!(
            DayCountConvention::from_str("A365").unwrap(),
            DayCountConvention::A365
        );
        assert_eq!(
            DayCountConvention::from_str("30E360").unwrap(),
            DayCountConvention::ThirtyE360
        );
        assert!(DayCountConvention::from_str("BF252").is_err());
    }

    #[test]
    fn scaling_effect_accepts_dictionary_and_testbed_spellings() {
        assert_eq!(
            ScalingEffect::from_str("IN0").unwrap(),
            ScalingEffect::InterestAndNotional
        );
        assert_eq!(
            serde_json::from_value::<ScalingEffect>(serde_json::json!("INO")).unwrap(),
            ScalingEffect::InterestAndNotional
        );
        assert_eq!(
            serde_json::from_value::<ScalingEffect>(serde_json::json!("IOO")).unwrap(),
            ScalingEffect::Interest
        );
        assert_eq!(
            serde_json::to_value(ScalingEffect::Interest).unwrap(),
            serde_json::json!("I00")
        );
    }

    #[test]
    fn business_day_convention_tokens() {
        for token in ["NOS", "SCF", "SCMF", "CSF", "CSMF", "SCP", "SCMP", "CSP"] {
            assert!(BusinessDayConvention::from_str(token).is_ok(), "{token}");
        }
        assert!(BusinessDayConvention::from_str("SF").is_err());
    }

    #[test]
    fn calendar_tokens() {
        assert_eq!(Calendar::from_str("NC").unwrap(), Calendar::Nc);
        assert_eq!(Calendar::from_str("MF").unwrap(), Calendar::Mf);
    }

    #[test]
    fn interest_calculation_base_tokens() {
        assert_eq!(
            InterestCalculationBase::from_str("NTIED").unwrap(),
            InterestCalculationBase::Ntied
        );
    }

    #[test]
    fn array_increase_decrease_tokens() {
        assert_eq!(ArrayIncDec::from_str("INC").unwrap(), ArrayIncDec::Inc);
        assert_eq!(ArrayIncDec::from_str("DEC").unwrap(), ArrayIncDec::Dec);
        assert_eq!(
            serde_json::from_value::<ArrayIncDec>(serde_json::json!("DEC")).unwrap(),
            ArrayIncDec::Dec
        );
        assert_eq!(
            serde_json::to_value(ArrayIncDec::Inc).unwrap(),
            serde_json::json!("INC")
        );
        assert!(ArrayIncDec::from_str("INCDEC").is_err());
    }

    #[test]
    fn array_fixed_variable_accepts_acronym_and_description_spellings() {
        assert_eq!(ArrayFixVar::from_str("F").unwrap(), ArrayFixVar::Fixed);
        assert_eq!(ArrayFixVar::from_str("V").unwrap(), ArrayFixVar::Variable);
        assert_eq!(
            serde_json::from_value::<ArrayFixVar>(serde_json::json!("FIX")).unwrap(),
            ArrayFixVar::Fixed
        );
        assert_eq!(
            serde_json::from_value::<ArrayFixVar>(serde_json::json!("VAR")).unwrap(),
            ArrayFixVar::Variable
        );
        assert_eq!(
            serde_json::to_value(ArrayFixVar::Variable).unwrap(),
            serde_json::json!("V")
        );
        assert!(ArrayFixVar::from_str("X").is_err());
    }
}
