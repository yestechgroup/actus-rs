//! Evaluation scaffolding: the dispatch seam between the pure engine and the
//! per-contract-type implementations (ACTUS techspec section "Contract Types").

use actus_model::{ContractTerms, ContractType};

use crate::event::{sort_events, ContractEvent};
use crate::risk::RiskFactorProvider;
use crate::EngineError;

/// Implementation of the schedule/state/payoff machinery of one contract type.
///
/// Each contract type (PAM, LAM, NAM, ANN, ...) registers an implementation;
/// [`EngineRegistry::evaluate`] dispatches on the contract type of the terms,
/// then enforces the deterministic event order via
/// [`sort_events`](crate::event::sort_events). Implementations must be pure
/// functions of `(terms, risk)`: no I/O, no clocks, no randomness.
pub trait ContractEngine {
    /// The contract type this engine implements.
    fn contract_type(&self) -> ContractType;

    /// Evaluates the full event sequence of the contract.
    fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError>;
}

/// Registry of contract type implementations.
///
/// Registration-based dispatch: engines are looked up by
/// [`ContractEngine::contract_type`], evaluated, and their events sorted.
/// Evaluation is deterministic because registration order is fixed and the
/// final event order comes from [`sort_events`], never from iteration order
/// of hashed containers.
#[derive(Default)]
pub struct EngineRegistry {
    engines: Vec<Box<dyn ContractEngine>>,
}

impl EngineRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an implementation for its contract type.
    ///
    /// A later registration for the same contract type takes precedence.
    pub fn register(&mut self, engine: Box<dyn ContractEngine>) {
        self.engines.push(engine);
    }

    /// Dispatches evaluation to the registered implementation for the terms'
    /// contract type and returns the events in deterministic order.
    pub fn evaluate(
        &self,
        terms: &ContractTerms,
        risk: &dyn RiskFactorProvider,
    ) -> Result<Vec<ContractEvent>, EngineError> {
        for engine in &self.engines {
            if engine.contract_type() == terms.contract_type {
                let mut events = engine.evaluate(terms, risk)?;
                sort_events(&mut events);
                return Ok(events);
            }
        }
        Err(EngineError::UnsupportedContractType(terms.contract_type))
    }
}

/// Evaluates a contract with the default engine registry.
///
/// In this wave the default registry is empty, so every contract type yields
/// [`EngineError::UnsupportedContractType`]; the PAM/LAM/NAM/ANN
/// implementations of the contract wave register themselves here.
pub fn evaluate(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
) -> Result<Vec<ContractEvent>, EngineError> {
    EngineRegistry::new().evaluate(terms, risk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::RiskFactorProvider;
    use crate::state::ContractState;
    use actus_model::EventType;
    use chrono::NaiveDateTime;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    struct StubEngine {
        contract_type: ContractType,
        events: Vec<ContractEvent>,
    }

    impl ContractEngine for StubEngine {
        fn contract_type(&self) -> ContractType {
            self.contract_type
        }

        fn evaluate(
            &self,
            _terms: &ContractTerms,
            _risk: &dyn RiskFactorProvider,
        ) -> Result<Vec<ContractEvent>, EngineError> {
            Ok(self.events.clone())
        }
    }

    fn terms_of(contract_type: ContractType) -> ContractTerms {
        ContractTerms::new(contract_type)
    }

    fn stub_event(event_type: EventType, time: &str, payoff: Decimal) -> ContractEvent {
        ContractEvent {
            event_type,
            time: NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S").unwrap(),
            payoff,
            currency: Some("USD".to_string()),
            state: ContractState::default(),
        }
    }

    #[test]
    fn unsupported_contract_type_is_reported() {
        let terms = terms_of(ContractType::Pam);
        let err = evaluate(&terms, &StateProviderStub).unwrap_err();
        assert!(matches!(
            err,
            EngineError::UnsupportedContractType(ContractType::Pam)
        ));
    }

    #[test]
    fn registry_dispatches_to_registered_engine_and_sorts_events() {
        let mut registry = EngineRegistry::new();
        registry.register(Box::new(StubEngine {
            contract_type: ContractType::Lam,
            events: vec![
                stub_event(EventType::Maturity, "2014-06-01T00:00:00", dec!(5000)),
                stub_event(
                    EventType::PrincipalRedemption,
                    "2014-01-01T00:00:00",
                    dec!(450),
                ),
                stub_event(
                    EventType::InitialExchange,
                    "2013-01-01T00:00:00",
                    dec!(-5000),
                ),
            ],
        }));

        let events = registry
            .evaluate(&terms_of(ContractType::Lam), &StateProviderStub)
            .unwrap();
        let times: Vec<NaiveDateTime> = events.iter().map(|e| e.time).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted);
        assert_eq!(events[0].event_type, EventType::InitialExchange);
        assert_eq!(events[2].event_type, EventType::Maturity);
    }

    #[test]
    fn registry_reports_unregistered_contract_type() {
        let mut registry = EngineRegistry::new();
        registry.register(Box::new(StubEngine {
            contract_type: ContractType::Pam,
            events: Vec::new(),
        }));
        let err = registry
            .evaluate(&terms_of(ContractType::Ann), &StateProviderStub)
            .unwrap_err();
        assert!(matches!(
            err,
            EngineError::UnsupportedContractType(ContractType::Ann)
        ));
    }

    struct StateProviderStub;

    impl RiskFactorProvider for StateProviderStub {}
}
