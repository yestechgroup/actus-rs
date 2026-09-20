//! CAPFL: Cap-Floor (paper §7.14, section 6 "Child Contract Observer").
//!
//! A cap-floor is optionality on interest rates: the underlying rate-bearing
//! contract is evaluated twice — once as-is and once with the lifetime rate
//! caps/floors applied — and the absolute difference of the interest
//! payments is paid at each underlying `IP` date.
//!
//! **Underlying** (child contract observer, paper §6): the `CTST` reference
//! carrying the `UDL` role (else the first embedded contract object), or —
//! when no `contractStructure` is present — an embedded PAM-like schedule
//! built from the CAPFL terms themselves (`IED`/`MD`/`NT`/`IPNR`/`IPANX`/
//! `IPCL` plus the reset schedule `RRANX`/`RRCL`, `RRMO`, `RRMLT`, `RRSP`,
//! `RRNXT`).
//!
//! **Capped run**: the underlying is re-evaluated through its own engine
//! with a clamping risk factor observer. Per the PAM `STF_RR` transition
//! `Ipnr+ = min(max(Ipnr + Δr, RRLF), RRLC)` the reset observation is
//! clamped into `[RRLF, RRLC]` (the *life cap* `RRLC` / *life floor* `RRLF`
//! strike rates; the parent's values, falling back to the underlying's own
//! when the parent carries none). Because the child engine applies
//! `observed x RRMLT + RRSP` after the observation, the clamp bounds are
//! inverted through the affine transform; a non-positive `RRMLT` (which has
//! no inverse) is reported as an invalid transition. The initial `IPNR` and
//! `RRNXT` of the capped run are clamped directly. Both runs share the
//! schedule (it depends only on attribute *presence*, not values), so the
//! event streams pair positionally.
//!
//! **Payoff convention**: at each underlying `IP` date the CAPFL holder
//! receives `R(CNTRL) x |IP_uncapped - IP_capped|` — the absolute difference
//! of the two runs' interest payments (paper §7.14 "takes the absolute
//! difference"). Non-`IP` flows (principal exchanges, resets) are identical
//! in both runs and contribute nothing. Unbreached periods pay zero; a
//! zero-payoff terminal `MD` closes the stream at the underlying maturity.
//! Post-event states report the *uncapped* run's snapshots.

use rust_decimal::Decimal;

use actus_model::{
    ContractReferenceRole, ContractTerms, ContractType, DayCountConvention, EventType,
};

use crate::common::role_sign;
use crate::daycount::normalize_timestamp;
use crate::engine::{ContractEngine, EngineRegistry};
use crate::event::ContractEvent;
use crate::risk::{ObservedCreditEvent, ObservedEvent, RiskFactorProvider};
use crate::state::{ContractState, ContractStatus};
use crate::EngineError;

/// Implementation of the CAPFL contract type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapflEngine;

impl ContractEngine for CapflEngine {
    fn contract_type(&self) -> ContractType {
        ContractType::Capfl
    }

    /// Evaluates the underlying twice (uncapped and clamped) and returns the
    /// difference payoffs on the underlying `IP` schedule plus the terminal
    /// `MD`.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        let child = underlying_terms(terms)?;
        let cap = terms.life_cap.or(child.life_cap);
        let floor = terms.life_floor.or(child.life_floor);
        if cap.is_none() && floor.is_none() {
            return Err(EngineError::InvalidTransition(
                "CAPFL needs a lifeCap (RRLC) or lifeFloor (RRLF) strike".to_string(),
            ));
        }
        let multiplier = child.rate_multiplier.unwrap_or(Decimal::ONE);
        if multiplier <= Decimal::ZERO {
            return Err(EngineError::InvalidTransition(
                "CAPFL clamping requires a positive rateMultiplier (RRMLT)".to_string(),
            ));
        }
        let spread = child.rate_spread.unwrap_or(Decimal::ZERO);

        let uncapped = child_registry().evaluate(&child, risk)?;
        let mut clamped_terms = child;
        if let Some(rate) = clamped_terms.nominal_interest_rate {
            clamped_terms.nominal_interest_rate = Some(clamp_rate(rate, cap, floor));
        }
        if let Some(rate) = clamped_terms.next_reset_rate {
            clamped_terms.next_reset_rate = Some(clamp_rate(rate, cap, floor));
        }
        let capped = child_registry().evaluate(
            &clamped_terms,
            &ClampedRiskProvider {
                inner: risk,
                cap,
                floor,
                multiplier,
                spread,
            },
        )?;
        if capped.len() != uncapped.len() {
            return Err(EngineError::InvalidTransition(
                "capped and uncapped underlying evaluations diverged".to_string(),
            ));
        }

        let sign = role_sign(terms);
        let mut events = Vec::new();
        let mut last_time = None;
        for (uncapped_event, capped_event) in uncapped.iter().zip(capped.iter()) {
            last_time = Some(uncapped_event.time);
            if uncapped_event.event_type != EventType::InterestPayment {
                continue;
            }
            events.push(ContractEvent {
                event_type: EventType::InterestPayment,
                time: uncapped_event.time,
                payoff: sign * (uncapped_event.payoff - capped_event.payoff).abs(),
                currency: terms.currency.clone(),
                state: uncapped_event.state.clone(),
            });
        }
        let terminal = last_time.ok_or(EngineError::InvalidTransition(
            "the CAPFL underlying produced no events".to_string(),
        ))?;
        events.push(ContractEvent {
            event_type: EventType::Maturity,
            time: terminal,
            payoff: Decimal::ZERO,
            currency: terms.currency.clone(),
            state: ContractState {
                status_date: terminal,
                contract_status: ContractStatus::Matured,
                ..ContractState::default()
            },
        });
        Ok(events)
    }
}

/// The engines the underlying may route through (the CAPFL underlying is a
/// rate-bearing fixed income contract; the same registry as SWAPS legs).
fn child_registry() -> EngineRegistry {
    let mut registry = EngineRegistry::new();
    registry.register(Box::new(crate::pam::PamEngine));
    registry.register(Box::new(crate::lam::LamEngine));
    registry.register(Box::new(crate::nam::NamEngine));
    registry.register(Box::new(crate::ann::AnnEngine));
    registry.register(Box::new(crate::clm::ClmEngine));
    registry
}

/// Resolves the underlying contract terms: the `UDL` reference object (else
/// the first embedded object of the structure), or the embedded PAM-like
/// construction from the CAPFL terms themselves.
fn underlying_terms(terms: &ContractTerms) -> Result<ContractTerms, EngineError> {
    match terms.contract_structure.as_deref() {
        Some(structure) => structure
            .iter()
            .find(|r| r.reference_role == Some(ContractReferenceRole::Underlying))
            .or_else(|| structure.first())
            .and_then(|r| r.object.clone())
            .ok_or(EngineError::InvalidTransition(
                "contract structure needs an underlying reference with an embedded contract \
                 object"
                    .to_string(),
            )),
        None => embedded_underlying(terms),
    }
}

/// Builds the embedded PAM-like underlying from the CAPFL terms.
fn embedded_underlying(terms: &ContractTerms) -> Result<ContractTerms, EngineError> {
    let ied = terms
        .initial_exchange_date
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute("initialExchangeDate"))?;
    let maturity = terms
        .maturity_date
        .map(normalize_timestamp)
        .ok_or(EngineError::MissingAttribute("maturityDate"))?;
    let mut child = ContractTerms::new(ContractType::Pam);
    child.contract_id = terms.contract_id.clone();
    child.status_date = terms.status_date;
    child.contract_deal_date = terms.contract_deal_date;
    child.initial_exchange_date = Some(ied);
    child.maturity_date = Some(maturity);
    child.notional_principal = terms.notional_principal;
    child.nominal_interest_rate = terms.nominal_interest_rate;
    child.currency = terms.currency.clone();
    child.cycle_anchor_date_of_interest_payment = terms.cycle_anchor_date_of_interest_payment;
    child.cycle_of_interest_payment = terms.cycle_of_interest_payment;
    child.cycle_anchor_date_of_rate_reset = terms.cycle_anchor_date_of_rate_reset;
    child.cycle_of_rate_reset = terms.cycle_of_rate_reset;
    child.market_object_code_of_rate_reset = terms.market_object_code_of_rate_reset.clone();
    child.rate_multiplier = terms.rate_multiplier;
    child.rate_spread = terms.rate_spread;
    child.next_reset_rate = terms.next_reset_rate;
    child.day_count_convention = Some(
        terms
            .day_count_convention
            .unwrap_or(DayCountConvention::A365),
    );
    child.end_of_month_convention = terms.end_of_month_convention;
    child.business_day_convention = terms.business_day_convention;
    child.calendar = terms.calendar;
    Ok(child)
}

/// Clamps a rate into the strike band `[RRLF, RRLC]` (only the set strikes
/// apply).
fn clamp_rate(rate: Decimal, cap: Option<Decimal>, floor: Option<Decimal>) -> Decimal {
    let mut clamped = rate;
    if let Some(cap) = cap {
        if clamped > cap {
            clamped = cap;
        }
    }
    if let Some(floor) = floor {
        if clamped < floor {
            clamped = floor;
        }
    }
    clamped
}

/// A risk factor observer whose rate observations are pre-clamped into the
/// strike band (the capped run of the underlying evaluation).
///
/// The child engine computes `observed x RRMLT + RRSP` from the observation,
/// so the strike band is inverted through the affine transform:
/// `clamp(obs, (floor - spread) / mlt, (cap - spread) / mlt)`. With a
/// positive multiplier (validated by the engine) this lands the reset rate
/// inside `[floor, cap]` exactly.
struct ClampedRiskProvider<'a> {
    inner: &'a dyn RiskFactorProvider,
    cap: Option<Decimal>,
    floor: Option<Decimal>,
    multiplier: Decimal,
    spread: Decimal,
}

impl RiskFactorProvider for ClampedRiskProvider<'_> {
    fn rate(&self, market_object_code: &str, at: chrono::NaiveDateTime) -> Option<Decimal> {
        let observed = self.inner.rate(market_object_code, at)?;
        let mut clamped = observed;
        if let Some(cap) = self.cap {
            let bound = (cap - self.spread) / self.multiplier;
            if clamped > bound {
                clamped = bound;
            }
        }
        if let Some(floor) = self.floor {
            let bound = (floor - self.spread) / self.multiplier;
            if clamped < bound {
                clamped = bound;
            }
        }
        Some(clamped)
    }

    fn index(&self, market_object_code: &str, at: chrono::NaiveDateTime) -> Option<Decimal> {
        self.inner.index(market_object_code, at)
    }

    fn observed_events(&self) -> Vec<ObservedEvent> {
        self.inner.observed_events()
    }

    fn observed_credit_events(&self) -> Vec<ObservedCreditEvent> {
        self.inner.observed_credit_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::StateProvider;
    use actus_model::ContractReference;
    use chrono::NaiveDateTime;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn terms(raw: serde_json::Value) -> ContractTerms {
        serde_json::from_value(raw).expect("terms")
    }

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").expect("timestamp")
    }

    /// The golden cap-floor: embedded PAM underlying of 1M, quarterly 30E360
    /// schedule, resets at IED and each quarter, observations crossing both
    /// strikes (cap 5%, floor 1.5%; the 2% baseline sits inside the band).
    fn golden() -> ContractTerms {
        terms(json!({
            "contractType": "CAPFL",
            "contractID": "capfl01",
            "contractRole": "RPA",
            "currency": "USD",
            "statusDate": "2025-01-01T00:00:00",
            "initialExchangeDate": "2025-01-01T00:00:00",
            "maturityDate": "2026-01-01T00:00:00",
            "notionalPrincipal": "1000000",
            "nominalInterestRate": "0.02",
            "cycleAnchorDateOfInterestPayment": "2025-01-01T00:00:00",
            "cycleOfInterestPayment": "P3ML0",
            "cycleAnchorDateOfRateReset": "2025-01-01T00:00:00",
            "cycleOfRateReset": "P3ML0",
            "marketObjectCodeOfRateReset": "LIBOR6M",
            "dayCountConvention": "30E360",
            "lifeCap": "0.05",
            "lifeFloor": "0.015"
        }))
    }

    fn crossing_risk() -> StateProvider {
        StateProvider::new()
            .with_rate("LIBOR6M", t("2025-01-01T00:00:00"), dec!(0.02))
            .with_rate("LIBOR6M", t("2025-04-01T00:00:00"), dec!(0.06))
            .with_rate("LIBOR6M", t("2025-07-01T00:00:00"), dec!(0.01))
            .with_rate("LIBOR6M", t("2025-10-01T00:00:00"), dec!(0.02))
    }

    #[test]
    fn difference_payoffs_only_on_breached_periods() {
        let events = CapflEngine
            .evaluate(&golden(), &crossing_risk())
            .expect("events");
        let ips: Vec<(NaiveDateTime, Decimal)> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| (e.time, e.payoff))
            .collect();
        // Uncapped IP: 0 / 5,000 / 15,000 / 2,500 / 5,000.
        // Capped IP:   0 / 5,000 / 12,500 / 3,750 / 5,000.
        assert_eq!(
            ips,
            vec![
                (t("2025-01-01T00:00:00"), Decimal::ZERO),
                (t("2025-04-01T00:00:00"), Decimal::ZERO),
                (t("2025-07-01T00:00:00"), dec!(2500)),
                (t("2025-10-01T00:00:00"), dec!(1250)),
                (t("2026-01-01T00:00:00"), Decimal::ZERO),
            ]
        );
        let md = events.last().expect("MD");
        assert_eq!(md.event_type, EventType::Maturity);
        assert_eq!(md.payoff, Decimal::ZERO);
        assert_eq!(md.state.contract_status, ContractStatus::Matured);
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn composed_underlying_reference_produces_the_same_stream() {
        let mut capfl = golden();
        capfl.initial_exchange_date = None;
        capfl.maturity_date = None;
        capfl.notional_principal = None;
        capfl.nominal_interest_rate = None;
        capfl.cycle_anchor_date_of_interest_payment = None;
        capfl.cycle_of_interest_payment = None;
        capfl.cycle_anchor_date_of_rate_reset = None;
        capfl.cycle_of_rate_reset = None;
        capfl.market_object_code_of_rate_reset = None;
        capfl.contract_structure = Some(vec![ContractReference {
            object: Some(embedded_underlying(&golden()).expect("child")),
            reference_type: Some(actus_model::ContractReferenceType::Contract),
            reference_role: Some(ContractReferenceRole::Underlying),
        }]);
        let composed = CapflEngine
            .evaluate(&capfl, &crossing_risk())
            .expect("events");
        let embedded = CapflEngine
            .evaluate(&golden(), &crossing_risk())
            .expect("events");
        assert_eq!(composed, embedded);
    }

    #[test]
    fn cap_only_and_floor_only_strikes_clamp_one_sided() {
        let mut capfl = golden();
        capfl.life_floor = None;
        let events = CapflEngine
            .evaluate(&capfl, &crossing_risk())
            .expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        // Only the 6% observation above the 5% cap is clamped.
        assert_eq!(
            ips,
            vec![
                Decimal::ZERO,
                Decimal::ZERO,
                dec!(2500),
                Decimal::ZERO,
                Decimal::ZERO
            ]
        );

        let mut capfl = golden();
        capfl.life_cap = None;
        let events = CapflEngine
            .evaluate(&capfl, &crossing_risk())
            .expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        // Only the 1% observation below the 1.5% floor is clamped: the
        // floored period pays |2,500 - 3,750| = 1,250.
        assert_eq!(
            ips,
            vec![
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
                dec!(1250),
                Decimal::ZERO
            ]
        );
    }

    #[test]
    fn multiplier_and_spread_transform_the_clamp_band() {
        // Reset rate = 2 x obs + 0.01: the 3% observation resets to 7%
        // uncapped but clamps to the 5% cap (observation bound
        // (5% - 1%) / 2 = 2%), paying the 0.02 x 250k = 5,000 difference in
        // the period it applies to.
        let mut capfl = golden();
        capfl.rate_multiplier = Some(dec!(2));
        capfl.rate_spread = Some(dec!(0.01));
        let risk = StateProvider::new()
            .with_rate("LIBOR6M", t("2025-01-01T00:00:00"), dec!(0.02))
            .with_rate("LIBOR6M", t("2025-04-01T00:00:00"), dec!(0.03))
            .with_rate("LIBOR6M", t("2025-07-01T00:00:00"), dec!(0.01))
            .with_rate("LIBOR6M", t("2025-10-01T00:00:00"), dec!(0.02));
        let events = CapflEngine.evaluate(&capfl, &risk).expect("events");
        let ips: Vec<Decimal> = events
            .iter()
            .filter(|e| e.event_type == EventType::InterestPayment)
            .map(|e| e.payoff)
            .collect();
        assert_eq!(
            ips,
            vec![
                Decimal::ZERO,
                Decimal::ZERO,
                dec!(5000),
                Decimal::ZERO,
                Decimal::ZERO
            ]
        );
    }

    #[test]
    fn missing_strikes_and_non_positive_multiplier_are_reported() {
        let mut capfl = golden();
        capfl.life_cap = None;
        capfl.life_floor = None;
        let error = CapflEngine.evaluate(&capfl, &crossing_risk()).unwrap_err();
        assert!(matches!(error, EngineError::InvalidTransition(_)));

        let mut capfl = golden();
        capfl.rate_multiplier = Some(Decimal::ZERO);
        let error = CapflEngine.evaluate(&capfl, &crossing_risk()).unwrap_err();
        assert!(matches!(error, EngineError::InvalidTransition(_)));
    }

    #[test]
    fn missing_embedded_attributes_are_reported() {
        let error = CapflEngine
            .evaluate(
                &terms(json!({"contractType": "CAPFL", "lifeCap": "0.05"})),
                &crossing_risk(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::MissingAttribute("initialExchangeDate")
        ));
    }
}
