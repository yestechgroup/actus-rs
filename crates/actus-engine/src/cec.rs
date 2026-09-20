//! CEC: Credit Enhancement Collateral (docs/actus.md §7.18; techspec section
//! "CEC: Credit Enhancement Collateral").
//!
//! A CEC contract is collateral backing a guarantee over one or more covered
//! contracts (`CTST^CoveredContract`, reference role `COVE`); the collateral
//! itself is one or more covering objects (`CTST^CoveringContract`, reference
//! role `COVI`, e.g. a `COM` gold position with a market object code). The
//! contract carries no exchange or interest schedule of its own: it holds a
//! notional state until a credit event on a covered contract triggers the
//! protection, then settles.
//!
//! The guaranteed notional per the states table aggregates the covered
//! exposure `n_i` (techspec "CEC" states and functions, attribute `CEGE`):
//! the covered contract's notional `Nt` for `NO`, `Nt + IPAC` for `NI`, and
//! the covered contract's own market value observation for `MV`, scaled by
//! the coverage ratio `CECV` and capped by the covering collateral value
//! `v_j = obs(rf, MOC_j, t)`. The credit event arrives through the
//! externally observed event stream: an observed `CE` event triggers the CEC
//! when its contract identifier matches a covered contract, its contract
//! performance state (`PRF`) equals `creditEventTypeCovered` (`CETC`,
//! default `DF`), and it occurs at or before the CEC maturity (the parent
//! `MD` term, else the latest covered contract maturity per the states
//! table).
//!
//! Resolved against the official testbed
//! (vendor/actus/tests/actus-tests-cec.json):
//!
//! - The exercise `XD` fires at the credit event time with zero payoff and
//!   sets `Nt = sgn x CECV x sum(n_i)` — the guaranteed exposure is *not*
//!   capped by the collateral value here (collateral15: `Nt = 3.5M` against
//!   a 100k collateral observation).
//! - The settlement `STD` pays `sgn x min(sum(v_j), Nt)` — the settlement is
//!   capped by the collateral value (collateral15 pays the 100k observation)
//!   — and zeroes the notional. The settlement date is the credit event time
//!   plus `settlementPeriod` (`STP`, default `P0D`), shifted per the
//!   business day convention (collateral13: Sunday 2020-09-20 under `MF` /
//!   `CSF` settles Monday 2020-09-21).
//! - Without a qualifying credit event the CEC emits a single zero-payoff
//!   `MD` at the maturity (collateral01, collateral03/04/05/07/12: the
//!   credit event is missing, references an uncovered contract, arrives
//!   after the covered maturity, or carries a performance state other than
//!   the covered one).
//! - The covered children are evaluated under their *own* contract roles and
//!   the parent role sign is applied after aggregation (collateral08/09: a
//!   `SEL` parent reports the negative of the positive covered exposure).

use chrono::{Duration, NaiveDateTime};
use rust_decimal::Decimal;

use actus_model::{
    BusinessDayConvention, Calendar, ContractReference, ContractReferenceRole, ContractRole,
    ContractTerms, ContractType, CreditEventType, Cycle, CyclePeriod, CycleStub,
    DayCountConvention, EndOfMonthConvention, EventType, GuaranteedExposure,
};

use crate::ann::AnnEngine;
use crate::clm::ClmEngine;
use crate::daycount::{day_count_fraction, normalize_timestamp};
use crate::engine::{ContractEngine, EngineRegistry};
use crate::event::ContractEvent;
use crate::lam::LamEngine;
use crate::nam::NamEngine;
use crate::pam::PamEngine;
use crate::risk::{ObservedCreditEvent, RiskFactorProvider};
use crate::schedule::{cycle_step, shift_business_day};
use crate::state::ContractState;
use crate::EngineError;

/// Implementation of the CEC contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CecEngine;

impl ContractEngine for CecEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Cec
    }

    /// Evaluates the covered exposure against the observed credit events and
    /// returns the settlement stream: `XD` + `STD` on a qualifying credit
    /// event, a single zero-payoff `MD` otherwise.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let structure = terms
            .contract_structure
            .as_deref()
            .ok_or(EngineError::MissingAttribute("contractStructure"))?;
        let covered: Vec<&ContractReference> = structure
            .iter()
            .filter(|r| r.reference_role == Some(ContractReferenceRole::CoveredContract))
            .collect();
        let covering: Vec<&ContractReference> = structure
            .iter()
            .filter(|r| r.reference_role == Some(ContractReferenceRole::CoveringContract))
            .collect();
        if covered.is_empty() {
            return Err(EngineError::InvalidTransition(
                "contract structure needs a covered contract reference".to_string(),
            ));
        }

        let sign = cec_role_sign(terms.contract_role);
        let maturity = maturity_date(terms, &covered)?;
        let covered_type = terms
            .credit_event_type_covered
            .unwrap_or(CreditEventType::Default);

        let trigger = match credit_event_trigger(risk, &covered, covered_type, maturity) {
            Some(trigger) => trigger,
            None => return Ok(vec![maturity_event(terms, maturity)]),
        };

        let coverage = terms.coverage_of_credit_enhancement.unwrap_or(Decimal::ONE);
        let exposure =
            coverage * covered_exposure(&covered, trigger.time, terms.guaranteed_exposure, risk)?;
        let collateral = covering_value(&covering, trigger.time, risk)?;
        let settlement_time = settlement_date(terms, trigger.time)?;

        Ok(vec![
            ContractEvent {
                event_type: EventType::Exercise,
                time: trigger.time,
                payoff: Decimal::ZERO,
                currency: terms.currency.clone(),
                state: event_state(sign * exposure, trigger.time),
            },
            ContractEvent {
                event_type: EventType::Settlement,
                time: settlement_time,
                payoff: sign * collateral.min(exposure),
                currency: terms.currency.clone(),
                state: event_state(Decimal::ZERO, settlement_time),
            },
        ])
    }
}

/// The engines a covered child may route through.
///
/// The CEC testbed covers PAM and LAM contracts; the remaining
/// schedule-driven engines are registered so any fixed-income child
/// evaluates through its own implementation.
fn child_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(PamEngine));
    registry.register(Box::new(LamEngine));
    registry.register(Box::new(NamEngine));
    registry.register(Box::new(AnnEngine));
    registry.register(Box::new(ClmEngine));
    registry
}

/// The contract role sign of the CEC parent (dictionary Table 1): the
/// protection buyer (`BUY`) carries `+1`, the protection seller (`SEL`)
/// carries `-1`; other roles default to the buying orientation.
fn cec_role_sign(role: Option<ContractRole>) -> Decimal {
    match role {
        Some(ContractRole::Sel) => -Decimal::ONE,
        _ => Decimal::ONE,
    }
}

/// The CEC maturity: the parent `MD` term when set, else the latest covered
/// contract maturity (techspec "CEC" states table, `Md` initialisation).
fn maturity_date(
    terms: &ContractTerms,
    covered: &[&ContractReference],
) -> Result<NaiveDateTime, EngineError> {
    if let Some(maturity) = terms.maturity_date {
        return Ok(normalize_timestamp(maturity));
    }
    covered
        .iter()
        .filter_map(|reference| {
            reference
                .object
                .as_ref()
                .and_then(|object| object.maturity_date)
        })
        .map(normalize_timestamp)
        .max()
        .ok_or(EngineError::MissingAttribute("maturityDate"))
}

/// The first observed credit event that qualifies as the trigger.
///
/// A credit event qualifies when it names a covered contract, carries the
/// covered contract performance state (`CETC`) and occurs at or before the
/// CEC maturity. The earliest qualifying observation triggers.
fn credit_event_trigger(
    risk: &dyn RiskFactorProvider,
    covered: &[&ContractReference],
    covered_type: CreditEventType,
    maturity: NaiveDateTime,
) -> Option<ObservedCreditEvent> {
    let covered_ids: Vec<Option<&String>> = covered
        .iter()
        .map(|reference| {
            reference
                .object
                .as_ref()
                .and_then(|object| object.contract_id.as_ref())
        })
        .collect();
    risk.observed_credit_events()
        .into_iter()
        .filter(|event| event.time <= maturity)
        .filter(|event| covered_ids.contains(&event.contract_id.as_ref()))
        .filter(|event| event.performance.map(|p| p.as_token()) == Some(covered_type.as_token()))
        .min_by_key(|event| event.time)
}

/// The covered exposure `sum(n_i)` at the credit event time (techspec "CEC"
/// states table, `Nt` initialisation).
///
/// Each covered contract is evaluated under its own contract role through
/// its own engine; the parent role sign is applied by the caller after
/// aggregation.
fn covered_exposure(
    covered: &[&ContractReference],
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    covered
        .iter()
        .map(|reference| covered_contract_value(reference, at, basis, risk))
        .sum()
}

/// The guaranteed value `n_i` of one covered contract at `at`.
///
/// `NO` guarantees the notional state the child's last event at or before
/// `at` reports; `NI` adds the interest accrued from that event's status
/// date to `at`; `MV` reads the child's own market value observation.
fn covered_contract_value(
    reference: &ContractReference,
    at: NaiveDateTime,
    basis: Option<GuaranteedExposure>,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    let child_terms = reference
        .object
        .clone()
        .ok_or(EngineError::InvalidTransition(
            "covered contract reference without an embedded contract object".to_string(),
        ))?;
    if basis == Some(GuaranteedExposure::MarketValue) {
        let code = child_terms
            .market_object_code
            .clone()
            .ok_or(EngineError::MissingAttribute("marketObjectCode"))?;
        return risk.index(&code, at).ok_or(EngineError::RiskFactorMissing {
            code,
            at: at.to_string(),
        });
    }
    let events = child_registry().evaluate(&child_terms, risk)?;
    let state = events
        .iter()
        .rev()
        .find(|event| event.time <= at)
        .map(|event| event.state.clone())
        .unwrap_or_else(|| ContractState::initial(&child_terms));
    match basis {
        Some(GuaranteedExposure::NominalValuePlusInterest) => {
            Ok(state.notional_principal + projected_accrued(&state, at, &child_terms)?)
        }
        _ => Ok(state.notional_principal),
    }
}

/// The interest accrued from the state's status date to `at`.
///
/// Mirrors the accrual transition `IPAC(t+) = IPAC(t-) + YF(SD(t-), t) x
/// IPNR(t-) x IP base(t-)`: the child's last event freezes the states at its
/// status date, and the accrual to the credit event time is projected with
/// the child's day count convention (dictionary default `A365`) over the
/// interest calculation base (the notional when the base state is unset).
fn projected_accrued(
    state: &ContractState,
    at: NaiveDateTime,
    child_terms: &ContractTerms,
) -> Result<Decimal, EngineError> {
    let convention = child_terms
        .day_count_convention
        .unwrap_or(DayCountConvention::A365);
    let year_fraction = day_count_fraction(state.status_date, at, convention)?;
    let base = if state.interest_calculation_base_amount.is_zero() {
        state.notional_principal
    } else {
        state.interest_calculation_base_amount
    };
    Ok(state.accrued_interest + year_fraction * state.nominal_interest_rate * base)
}

/// The covering collateral value `sum(v_j)` at the credit event time, read
/// from the market value observation of each covering object (`v_j =
/// obs(rf, MOC_j, t)`).
fn covering_value(
    covering: &[&ContractReference],
    at: NaiveDateTime,
    risk: &dyn RiskFactorProvider,
) -> Result<Decimal, EngineError> {
    covering
        .iter()
        .map(|reference| {
            let object = reference
                .object
                .as_ref()
                .ok_or(EngineError::InvalidTransition(
                    "covering contract reference without an embedded contract object".to_string(),
                ))?;
            let code = object
                .market_object_code
                .clone()
                .ok_or(EngineError::MissingAttribute("marketObjectCode"))?;
            risk.index(&code, at).ok_or(EngineError::RiskFactorMissing {
                code,
                at: at.to_string(),
            })
        })
        .sum()
}

/// The settlement date: the credit event time plus `settlementPeriod`
/// (`STP`, default `P0D`), shifted per the business day convention.
///
/// The `XD` event itself emits at the unshifted credit event time; only the
/// settlement obligation is shifted (collateral13 settles the Sunday notice
/// on the following Monday under `MF` / `CSF`).
fn settlement_date(
    terms: &ContractTerms,
    event_time: NaiveDateTime,
) -> Result<NaiveDateTime, EngineError> {
    let unshifted = match terms.settlement_period.as_deref() {
        None => event_time,
        Some(raw) => add_period(event_time, raw, terms)?,
    };
    let convention = terms
        .business_day_convention
        .unwrap_or(BusinessDayConvention::Nos);
    let calendar = terms.calendar.unwrap_or(Calendar::Nc);
    Ok(shift_business_day(unshifted, convention, calendar))
}

/// Adds an ISO 8601 period (`P<n>D|W|M|Y`) to a timestamp.
///
/// Days and weeks add calendar durations; months and years step through the
/// cycle machinery with the end of month convention applied.
fn add_period(
    at: NaiveDateTime,
    raw: &str,
    terms: &ContractTerms,
) -> Result<NaiveDateTime, EngineError> {
    let upper = raw.trim().to_ascii_uppercase();
    let invalid = || EngineError::InvalidTransition(format!("invalid settlementPeriod: {raw}"));
    let body = upper.strip_prefix('P').ok_or_else(invalid)?;
    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
    let period = &body[digits.len()..];
    let n: i64 = digits.parse().map_err(|_| invalid())?;
    match period {
        "D" => at
            .checked_add_signed(Duration::days(n))
            .ok_or_else(|| EngineError::InvalidTransition("settlementPeriod overflow".into())),
        "W" => at
            .checked_add_signed(Duration::weeks(n))
            .ok_or_else(|| EngineError::InvalidTransition("settlementPeriod overflow".into())),
        "M" | "Y" => {
            let cycle_period = if period == "M" {
                CyclePeriod::Month
            } else {
                CyclePeriod::Year
            };
            let cycle = Cycle::new(n.max(1) as u32, cycle_period, CycleStub::Short, 0)
                .map_err(|e| EngineError::InvalidTransition(e.to_string()))?;
            let eomc = terms
                .end_of_month_convention
                .unwrap_or(EndOfMonthConvention::Sd);
            Ok(cycle_step(at, 1, &cycle, eomc))
        }
        _ => Err(invalid()),
    }
}

/// Builds a post-event state carrying the given notional at the event time.
fn event_state(notional: Decimal, time: NaiveDateTime) -> ContractState {
    ContractState {
        notional_principal: notional,
        status_date: time,
        ..ContractState::default()
    }
}

/// Builds the zero-payoff maturity event (techspec "CEC" functions, `MD`:
/// payoff `0.0`, `Nt(t+) = 0.0`).
fn maturity_event(terms: &ContractTerms, maturity: NaiveDateTime) -> ContractEvent {
    ContractEvent {
        event_type: EventType::Maturity,
        time: maturity,
        payoff: Decimal::ZERO,
        currency: terms.currency.clone(),
        state: event_state(Decimal::ZERO, maturity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use actus_model::ContractPerformance;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    fn close(actual: Decimal, expected: Decimal) {
        let diff = (actual - expected).abs();
        assert!(
            diff < Decimal::new(1, 8),
            "expected {expected}, actual {actual}"
        );
    }

    /// The covering gold object shared by the official fixtures.
    fn gold_covering() -> serde_json::Value {
        json!({
            "object": {
                "contractType": "COM",
                "contractID": "GOLD01X12DF3VW",
                "statusDate": "2020-01-01T00:00:00",
                "contractDealDate": "2020-01-01T00:00:00",
                "currency": "USD",
                "contractRole": "RPA",
                "creatorId": "PartyXYZ",
                "marketObjectCode": "GOLD",
                "quantity": "1",
                "unit": "ONC"
            },
            "referenceType": "CNT",
            "referenceRole": "COVI"
        })
    }

    /// The covered US91282XYZ01 bond of collateral01 to collateral08.
    fn covered_bond(maturity: &str) -> serde_json::Value {
        json!({
            "object": {
                "contractType": "PAM",
                "contractID": "US91282XYZ01",
                "statusDate": "2020-01-01T00:00:00",
                "contractDealDate": "2020-01-01T00:00:00",
                "currency": "USD",
                "notionalPrincipal": "1000000",
                "initialExchangeDate": "2020-01-02T00:00:00",
                "maturityDate": maturity,
                "nominalInterestRate": "0.03",
                "cycleAnchorDateOfInterestPayment": "2020-02-01T00:00:00",
                "cycleOfInterestPayment": "P1ML0",
                "dayCountConvention": "A365",
                "endOfMonthConvention": "SD",
                "contractRole": "RPA"
            },
            "referenceType": "CNT",
            "referenceRole": "COVE"
        })
    }

    /// The covered loan pair of collateral09 to collateral15: a PAM bullet
    /// of 5M and an amortising LAM of 6M redeeming 500k monthly.
    fn covered_loans() -> serde_json::Value {
        json!([
            {
                "object": {
                    "contractType": "PAM",
                    "contractID": "loan01",
                    "statusDate": "2020-01-01T00:00:00",
                    "contractDealDate": "2020-01-01T00:00:00",
                    "currency": "USD",
                    "notionalPrincipal": "5000000",
                    "initialExchangeDate": "2020-01-02T00:00:00",
                    "maturityDate": "2021-01-01T00:00:00",
                    "nominalInterestRate": "0.03",
                    "cycleAnchorDateOfInterestPayment": "2020-02-01T00:00:00",
                    "cycleOfInterestPayment": "P1ML0",
                    "dayCountConvention": "A365",
                    "endOfMonthConvention": "SD",
                    "contractRole": "RPA"
                },
                "referenceType": "CNT",
                "referenceRole": "COVE"
            },
            {
                "object": {
                    "contractType": "LAM",
                    "contractID": "loan02",
                    "statusDate": "2020-01-01T00:00:00",
                    "contractDealDate": "2020-01-01T00:00:00",
                    "currency": "USD",
                    "notionalPrincipal": "6000000",
                    "initialExchangeDate": "2020-01-02T00:00:00",
                    "maturityDate": "2021-01-01T00:00:00",
                    "nominalInterestRate": "0.024",
                    "cycleAnchorDateOfInterestPayment": "2020-02-01T00:00:00",
                    "cycleOfInterestPayment": "P1ML0",
                    "dayCountConvention": "A365",
                    "endOfMonthConvention": "SD",
                    "contractRole": "RPA",
                    "cycleAnchorDateOfPrincipalRedemption": "2020-02-01T00:00:00",
                    "cycleOfPrincipalRedemption": "P1ML0",
                    "nextPrincipalRedemptionPayment": "500000"
                },
                "referenceType": "CNT",
                "referenceRole": "COVE"
            }
        ])
    }

    fn base_terms(contract_role: &str, coverage: Option<&str>) -> serde_json::Value {
        let mut raw = json!({
            "contractType": "CEC",
            "contractID": "collateral",
            "contractRole": contract_role,
            "currency": "USD",
            "calendar": "NC",
            "contractDealDate": "2020-01-01T00:00:00",
            "statusDate": "2020-01-01T00:00:00",
            "creditEventTypeCovered": "DF",
            "settlementPeriod": "P0D",
            "contractStructure": [covered_bond("2020-12-31T00:00:00"), gold_covering()]
        });
        if let Some(coverage) = coverage {
            raw["coverageOfCreditEnhancement"] = json!(coverage);
        }
        raw
    }

    fn credit_event(contract_id: &str, performance: ContractPerformance) -> ObservedCreditEvent {
        ObservedCreditEvent {
            time: t("2020-09-20T00:00:00"),
            contract_id: Some(contract_id.to_string()),
            performance: Some(performance),
        }
    }

    fn risk_with_gold(value: &str, events: &[ObservedCreditEvent]) -> StateProvider {
        let mut risk = StateProvider::new().with_index(
            "GOLD",
            t("2020-09-20T00:00:00"),
            value.parse().expect("decimal"),
        );
        for event in events {
            risk = risk.with_observed_credit_event(event.clone());
        }
        risk
    }

    /// collateral02: the DF credit event on the covered bond guarantees the
    /// full 1M notional against the 10M collateral.
    #[test]
    fn credit_event_triggers_exercise_and_settlement_of_the_guaranteed_exposure() {
        let terms = terms(base_terms("BUY", None));
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("US91282XYZ01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::Exercise);
        assert_eq!(events[0].time, t("2020-09-20T00:00:00"));
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, dec!(1000000));
        assert_eq!(events[1].event_type, EventType::Settlement);
        assert_eq!(events[1].time, t("2020-09-20T00:00:00"));
        assert_eq!(events[1].payoff, dec!(1000000));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
    }

    /// collateral06: coverage 0.7 scales the guaranteed exposure.
    #[test]
    fn coverage_ratio_scales_the_exposure() {
        let terms = terms(base_terms("BUY", Some("0.7")));
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("US91282XYZ01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        close(events[0].state.notional_principal, dec!(700000));
        close(events[1].payoff, dec!(700000));
    }

    /// collateral08: the protection seller carries the negative orientation.
    #[test]
    fn seller_role_flips_the_sign() {
        let terms = terms(base_terms("SEL", Some("0.7")));
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("US91282XYZ01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        close(events[0].state.notional_principal, dec!(-700000));
        close(events[1].payoff, dec!(-700000));
    }

    /// collateral09: two covered loans guarantee the outstanding notional
    /// sum (5M bullet + 2M after eight monthly redemptions) under `NO`.
    #[test]
    fn nominal_basis_sums_the_outstanding_child_notionals() {
        let mut raw = base_terms("SEL", Some("1.0"));
        raw["guaranteedExposure"] = json!("NO");
        raw["contractStructure"] = json!([covered_loans()[0], covered_loans()[1], gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("loan01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        close(events[0].state.notional_principal, dec!(-7000000));
        close(events[1].payoff, dec!(-7000000));
    }

    /// collateral10: under `NI` the interest accrued since the children's
    /// last cycle event joins the notional sum.
    #[test]
    fn nominal_plus_interest_basis_projects_the_child_accruals() {
        let mut raw = base_terms("SEL", Some("1.0"));
        raw["guaranteedExposure"] = json!("NI");
        raw["contractStructure"] = json!([covered_loans()[0], covered_loans()[1], gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("loan01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        close(events[0].state.notional_principal, dec!(-7010306.84931506));
        close(events[1].payoff, dec!(-7010306.84931506));
    }

    /// collateral15: the `XD` notional carries the guaranteed exposure while
    /// the settlement payoff caps at the 100k collateral observation.
    #[test]
    fn settlement_caps_at_the_collateral_value() {
        let mut raw = base_terms("BUY", Some("0.5"));
        raw["contractStructure"] = json!([covered_loans()[0], covered_loans()[1], gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "100000",
            &[credit_event("loan01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        close(events[0].state.notional_principal, dec!(3500000));
        close(events[1].payoff, dec!(100000));
        assert_eq!(events[1].state.notional_principal, Decimal::ZERO);
    }

    /// collateral13: the Sunday notice settles on the following Monday under
    /// `MF` / `CSF`.
    #[test]
    fn settlement_shifts_to_the_next_business_day() {
        let mut raw = base_terms("BUY", Some("0.5"));
        raw["calendar"] = json!("MF");
        raw["businessDayConvention"] = json!("CSF");
        raw["contractStructure"] = json!([covered_loans()[0], covered_loans()[1], gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("loan01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events[0].time, t("2020-09-20T00:00:00"));
        assert_eq!(events[1].time, t("2020-09-21T00:00:00"));
    }

    /// collateral14: the settlement period delays the settlement obligation.
    #[test]
    fn settlement_period_delays_the_settlement() {
        let mut raw = base_terms("BUY", Some("0.5"));
        raw["calendar"] = json!("MF");
        raw["businessDayConvention"] = json!("CSF");
        raw["settlementPeriod"] = json!("P5D");
        raw["contractStructure"] = json!([covered_loans()[0], covered_loans()[1], gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("loan01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events[0].time, t("2020-09-20T00:00:00"));
        assert_eq!(events[1].time, t("2020-09-25T00:00:00"));
    }

    /// collateral01: without any observed credit event the CEC matures with
    /// a zero payoff at the covered maturity.
    #[test]
    fn no_credit_event_matures_at_the_covered_maturity() {
        let terms = terms(base_terms("BUY", None));
        let risk = risk_with_gold("10000000", &[]);
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Maturity);
        assert_eq!(events[0].time, t("2020-12-31T00:00:00"));
        assert_eq!(events[0].payoff, Decimal::ZERO);
        assert_eq!(events[0].state.notional_principal, Decimal::ZERO);
    }

    /// collateral04: a credit event after the covered maturity never
    /// triggers; the CEC matures at the covered maturity.
    #[test]
    fn credit_event_after_the_covered_maturity_does_not_trigger() {
        let mut raw = base_terms("BUY", None);
        raw["contractStructure"] = json!([covered_bond("2020-06-30T00:00:00"), gold_covering()]);
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("US91282XYZ01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Maturity);
        assert_eq!(events[0].time, t("2020-06-30T00:00:00"));
    }

    /// collateral03/collateral12: a credit event naming an uncovered
    /// contract never triggers.
    #[test]
    fn credit_event_on_an_uncovered_contract_does_not_trigger() {
        let terms = terms(base_terms("BUY", None));
        let risk = risk_with_gold(
            "10000000",
            &[credit_event(
                "NON_MATCHING_CID",
                ContractPerformance::Default,
            )],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Maturity);
        assert_eq!(events[0].time, t("2020-12-31T00:00:00"));
    }

    /// collateral05: a delinquency observation does not trigger default
    /// protection.
    #[test]
    fn other_performance_state_does_not_trigger() {
        let terms = terms(base_terms("BUY", None));
        let risk = risk_with_gold(
            "10000000",
            &[credit_event(
                "US91282XYZ01",
                ContractPerformance::Delinquent,
            )],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Maturity);
    }

    /// collateral07: protection covering delinquency is not triggered by a
    /// default observation.
    #[test]
    fn default_observation_does_not_trigger_delinquency_protection() {
        let mut raw = base_terms("BUY", None);
        raw["creditEventTypeCovered"] = json!("DQ");
        let terms = terms(raw);
        let risk = risk_with_gold(
            "10000000",
            &[credit_event("US91282XYZ01", ContractPerformance::Default)],
        );
        let events = CecEngine.evaluate(&terms, &risk).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::Maturity);
    }

    #[test]
    fn missing_structure_reports_the_missing_attribute() {
        let terms = terms(json!({
            "contractType": "CEC",
            "contractRole": "BUY",
            "currency": "USD"
        }));
        let error = CecEngine
            .evaluate(&terms, &StateProvider::new())
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("contractStructure")
        ));
    }

    #[test]
    fn unobserved_collateral_market_object_is_reported() {
        let terms = terms(base_terms("BUY", None));
        let risk = StateProvider::new()
            .with_observed_credit_event(credit_event("US91282XYZ01", ContractPerformance::Default));
        let error = CecEngine.evaluate(&terms, &risk).unwrap_err();
        assert!(matches!(error, EngineError::RiskFactorMissing { .. }));
    }
}
