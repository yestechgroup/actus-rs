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

    /// A registry with every contract type engine of this crate (18 of the
    /// 21 dictionary types; BCS, CDSWP and MAR have no engine yet).
    ///
    /// This is the registry [`evaluate`] uses, and the one the conformance
    /// harness and the WASM bindings use.
    #[must_use]
    pub fn with_all_engines() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(crate::pam::PamEngine));
        registry.register(Box::new(crate::lam::LamEngine));
        registry.register(Box::new(crate::lax::LaxEngine));
        registry.register(Box::new(crate::nam::NamEngine));
        registry.register(Box::new(crate::ann::AnnEngine));
        registry.register(Box::new(crate::csh::CshEngine));
        registry.register(Box::new(crate::clm::ClmEngine));
        registry.register(Box::new(crate::ump::UmpEngine));
        registry.register(Box::new(crate::stk::StkEngine));
        registry.register(Box::new(crate::com::ComEngine));
        registry.register(Box::new(crate::fxout::FxoutEngine));
        registry.register(Box::new(crate::swppv::SwppvEngine));
        registry.register(Box::new(crate::swaps::SwapsEngine));
        registry.register(Box::new(crate::capfl::CapflEngine));
        registry.register(Box::new(crate::optns::OptnsEngine));
        registry.register(Box::new(crate::futur::FuturEngine));
        registry.register(Box::new(crate::ceg::CegEngine));
        registry.register(Box::new(crate::cec::CecEngine));
        registry
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

/// Evaluates a contract with every engine of this crate
/// ([`EngineRegistry::with_all_engines`]).
///
/// # Errors
/// [`EngineError::UnsupportedContractType`] for a type without an engine
/// (BCS, CDSWP, MAR), otherwise whatever the type's engine reports (for
/// example [`EngineError::MissingAttribute`]).
pub fn evaluate(
    terms: &ContractTerms,
    risk: &dyn RiskFactorProvider,
) -> Result<Vec<ContractEvent>, EngineError> {
    EngineRegistry::with_all_engines().evaluate(terms, risk)
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
    fn types_without_an_engine_are_reported_as_unsupported() {
        for contract_type in [ContractType::Bcs, ContractType::Cdswp, ContractType::Mar] {
            let err = evaluate(&terms_of(contract_type), &StateProviderStub).unwrap_err();
            assert!(
                matches!(err, EngineError::UnsupportedContractType(t) if t == contract_type),
                "{contract_type:?}: {err}"
            );
        }
    }

    /// The public `evaluate` used to build an empty registry and reject
    /// every contract (#20). Every type with an engine must now reach it:
    /// with empty terms the engine itself reports what is missing.
    #[test]
    fn evaluate_reaches_an_engine_for_every_supported_type() {
        let registry = EngineRegistry::with_all_engines();
        let mut reached = 0;
        for &contract_type in ContractType::ALL {
            let unsupported = matches!(
                evaluate(&terms_of(contract_type), &StateProviderStub),
                Err(EngineError::UnsupportedContractType(_))
            );
            let registered = registry
                .engines
                .iter()
                .any(|e| e.contract_type() == contract_type);
            assert_eq!(unsupported, !registered, "{contract_type:?}");
            reached += usize::from(registered);
        }
        assert_eq!(reached, 18, "one engine per supported type");
    }

    #[test]
    fn evaluate_runs_a_real_contract() {
        let terms: ContractTerms = serde_json::from_value(serde_json::json!({
            "contractType": "PAM",
            "contractRole": "RPA",
            "statusDate": "2012-12-30T00:00:00",
            "initialExchangeDate": "2013-01-01T00:00:00",
            "maturityDate": "2014-01-01T00:00:00",
            "notionalPrincipal": "1000",
            "nominalInterestRate": "0.05",
            "cycleAnchorDateOfInterestPayment": "2013-07-01T00:00:00",
            "cycleOfInterestPayment": "P6ML0",
            "dayCountConvention": "A365"
        }))
        .unwrap();
        let events = evaluate(&terms, &StateProviderStub).expect("PAM evaluates");
        let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
        assert_eq!(
            types,
            vec![
                EventType::InitialExchange,
                EventType::InterestPayment,
                EventType::InterestPayment,
                EventType::Maturity
            ]
        );
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
