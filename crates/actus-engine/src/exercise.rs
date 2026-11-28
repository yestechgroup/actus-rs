//! The exercise and settlement stream shared by OPTNS and FUTUR (techspec
//! "OPTNS: Option" and "FUTUR: Future", whose schedule and states are "Same
//! as OPTNS"; SI-008).
//!
//! Events, in the dictionary sequence `PRD, TD, MD, XD, STD`:
//!
//! - `PRD` at `purchaseDate`: the premium, `-R(CNTRL) x PPRD`.
//! - `TD` at a `terminationDate` before `MD`: the sale, `R(CNTRL) x PTD`;
//!   nothing follows it.
//! - `MD` at `maturityDate`: zero payoff.
//! - `XD` at the exercise date `Xd`: zero payoff; it fixes the exercise
//!   amount `Xa = x(S_t)`, the contract's payoff function of the underlying
//!   price `S_t` observed at `Xd`. `Xd` is the `exerciseDate` attribute
//!   when set (a contract exercised before `t0`, whose `Xa` is then the
//!   `exerciseAmount` attribute), else the maturity date: European exercise.
//!   The official testbeds report `XD` (and `STD`) even when `Xa = 0`
//!   (option01, option12), where the techspec leaves `Xd` undefined.
//! - `STD` at `Xd + settlementPeriod`, shifted per `BDC`/`CLDR`: the
//!   settlement `R(CNTRL) x Xa`.
//!
//! The underlying price series is the market object of the `UDL` reference
//! in `contractStructure` (`{"marketObjectCode": ...}`, or an embedded
//! contract's `marketObjectCode`), else the contract's own
//! `marketObjectCode`; it is observed through the provider's `index` view,
//! then its `rate` view. Without an observation, the `exerciseAmount`
//! attribute stands in for `Xa`; with neither the evaluation reports
//! [`EngineError::RiskFactorMissing`].
//!
//! There is no notional state (techspec states: `Xd`, `Xa`, `Prf`, `Sd`;
//! the testbeds report `notionalPrincipal` 0). American and Bermudan
//! exercise (`OPXT` `A`/`B`), which the techspec drives by observed `XD`
//! events, are out of scope unless the contract carries its exercise date.
//! Events at or before the status date `t0` are not observed.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use actus_model::enums::OptionExerciseType;
use actus_model::{ContractReferenceRole, ContractTerms, EventType};

use crate::common::role_sign;
use crate::credit_enhancement::settlement_date;
use crate::daycount::normalize_timestamp;
use crate::event::ContractEvent;
use crate::num::ck;
use crate::risk::RiskFactorProvider;
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Evaluates the exercise stream; `exercise_amount` is the contract's payoff
/// function `x(S_t)` of the observed underlying price.
pub(crate) fn evaluate_exercise(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
    exercise_amount: &dyn Fn(Decimal) -> Result<Decimal, EngineError>,
) -> Result<Vec<ContractEvent>, EngineError> {
    let sgn = role_sign(terms);
    let t0 = ContractState::initial(terms).status_date;
    let maturity = terms
        .maturity_date
        .or(terms.option_exercise_end_date)
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute("maturityDate"))?;
    let exercise_date = terms.exercise_date.map(normalize_timestamp);
    if exercise_date.is_none()
        && terms
            .option_exercise_type
            .is_some_and(|style| style != OptionExerciseType::European)
    {
        return Err(EngineError::InvalidTransition(
            "American and Bermudan exercise (observed XD events) is not implemented".to_string(),
        ));
    }

    let mut events: Vec<(EventType, NaiveDateTime, Decimal, ContractStatus)> = Vec::new();
    if let Some(prd) = terms.purchase_date.map(normalize_timestamp) {
        if prd > t0 {
            let price = terms
                .price_at_purchase_date
                .ok_or(EngineError::MissingAttribute("priceAtPurchaseDate"))?;
            let payoff = (-ck(sgn) * price).get()?;
            events.push((EventType::Purchase, prd, payoff, ContractStatus::Active));
        }
    }
    if let Some(td) = terms.termination_date.map(normalize_timestamp) {
        if td > t0 && td < maturity && exercise_date.is_none_or(|xd| td < xd) {
            let price = terms
                .price_at_termination_date
                .ok_or(EngineError::MissingAttribute("priceAtTerminationDate"))?;
            let payoff = (ck(sgn) * price).get()?;
            events.push((
                EventType::Termination,
                td,
                payoff,
                ContractStatus::Terminated,
            ));
            return Ok(finish(terms, events));
        }
    }
    if maturity > t0 {
        events.push((
            EventType::Maturity,
            maturity,
            Decimal::ZERO,
            ContractStatus::Matured,
        ));
    }
    let xd = exercise_date.unwrap_or(maturity);
    let xa = match (exercise_date, terms.exercise_amount) {
        // Exercised before t0: the states carry `Xa(t0) = XA`.
        (Some(xd), Some(xa)) if xd <= t0 => xa,
        _ => {
            let observed = underlying_code(terms).map(|code| {
                let price = risk.index(&code, xd).or_else(|| risk.rate(&code, xd));
                (code, price)
            });
            match (observed, terms.exercise_amount) {
                (Ok((_, Some(price))), _) => exercise_amount(price)?,
                // No observation (or no underlying): the agreed `XA`.
                (_, Some(agreed)) => agreed,
                (Ok((code, None)), None) => {
                    return Err(EngineError::RiskFactorMissing {
                        code,
                        at: xd.to_string(),
                    })
                }
                (Err(missing), None) => return Err(missing),
            }
        }
    };
    if xd > t0 {
        events.push((
            EventType::Exercise,
            xd,
            Decimal::ZERO,
            ContractStatus::Matured,
        ));
    }
    let std = settlement_date(terms, xd)?;
    if std > t0 {
        let payoff = (ck(sgn) * xa).get()?;
        events.push((EventType::Settlement, std, payoff, ContractStatus::Matured));
    }
    Ok(finish(terms, events))
}

/// Orders the events by time and dictionary sequence (`MD, XD, STD` on a
/// shared date, as the testbeds report them) and attaches the states.
fn finish(
    terms: &ContractTerms,
    mut events: Vec<(EventType, NaiveDateTime, Decimal, ContractStatus)>,
) -> Vec<ContractEvent> {
    events.sort_by_key(|(event_type, time, _, _)| (*time, event_type.priority()));
    events
        .into_iter()
        .map(
            |(event_type, time, payoff, contract_status)| ContractEvent {
                event_type,
                time,
                payoff,
                currency: terms.currency.clone(),
                state: ContractState {
                    status_date: time,
                    contract_status,
                    ..ContractState::default()
                },
            },
        )
        .collect()
}

/// The market object code of the underlying price series: the `UDL`
/// reference of `contractStructure` (a market object, or an embedded
/// contract's `marketObjectCode`), else the contract's `marketObjectCode`.
fn underlying_code(terms: &ContractTerms) -> Result<String, EngineError> {
    let referenced = terms.contract_structure.as_deref().and_then(|structure| {
        structure
            .iter()
            .find(|r| r.reference_role == Some(ContractReferenceRole::Underlying))
            .and_then(|r| {
                r.market_object_code().map(str::to_string).or_else(|| {
                    r.embedded_contract()
                        .and_then(|child| child.market_object_code.clone())
                })
            })
    });
    referenced
        .or_else(|| terms.market_object_code.clone())
        .ok_or(EngineError::MissingAttribute("contractStructure"))
}
