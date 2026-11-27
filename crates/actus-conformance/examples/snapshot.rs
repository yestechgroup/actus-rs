//! Refactoring guard: prints every engine event (type, time, payoff and the
//! full post-event state) for all official testbed cases, the LAM shapes
//! replayed as LAX, and the pinned differential corpus.
//!
//! Use it to prove a refactor changes no behaviour:
//!
//! ```sh
//! cargo run -q -p actus-conformance --example snapshot > before.txt
//! # ... refactor ...
//! cargo run -q -p actus-conformance --example snapshot > after.txt
//! cmp before.txt after.txt
//! ```
//!
//! Unlike the conformance tests, which compare within a tolerance and only
//! the testbed-reported states, this compares everything byte for byte.

use actus_conformance::{cases, corpus, engine_registry};
use actus_model::ContractType;

fn main() {
    let registry = engine_registry();
    for ct in [
        ContractType::Pam,
        ContractType::Lam,
        ContractType::Nam,
        ContractType::Ann,
        ContractType::Csh,
        ContractType::Clm,
        ContractType::Swaps,
        ContractType::Cec,
    ] {
        for case in cases::load_testbed(ct).unwrap() {
            println!("## {}", case.identifier);
            match cases::evaluate_case(&registry, &case) {
                Ok(evs) => {
                    for e in evs {
                        println!("{e:?}");
                    }
                }
                Err(e) => println!("ERR {e}"),
            }
        }
    }
    // LAX has no testbed or corpus: replay the LAM shapes as LAX.
    for mut case in cases::load_testbed(ContractType::Lam).unwrap() {
        case.terms.contract_type = ContractType::Lax;
        println!("## lax-{}", case.identifier);
        match cases::evaluate_case(&registry, &case) {
            Ok(evs) => {
                for e in evs {
                    println!("{e:?}");
                }
            }
            Err(e) => println!("ERR {e}"),
        }
    }
    for mut case in corpus::generate(corpus::CORPUS_SEED, corpus::DIFFERENTIAL_CASES) {
        if case.terms.contract_type != ContractType::Lam {
            continue;
        }
        case.terms.contract_type = ContractType::Lax;
        println!("## lax-{}", case.id);
        let mut p = actus_engine::StateProvider::new();
        if let Some(code) = case.terms.market_object_code_of_rate_reset.as_deref() {
            for (t, v) in &case.risk_factors {
                p = p.with_rate(code, *t, *v);
            }
        }
        match registry.evaluate(&case.terms, &p) {
            Ok(evs) => {
                for e in evs {
                    println!("{e:?}");
                }
            }
            Err(e) => println!("ERR {e}"),
        }
    }
    for case in corpus::generate(corpus::CORPUS_SEED, corpus::DIFFERENTIAL_CASES) {
        println!("## {}", case.id);
        let mut p = actus_engine::StateProvider::new();
        if let Some(code) = case.terms.market_object_code_of_rate_reset.as_deref() {
            for (t, v) in &case.risk_factors {
                p = p.with_rate(code, *t, *v);
            }
        }
        match registry.evaluate(&case.terms, &p) {
            Ok(evs) => {
                for e in evs {
                    println!("{e:?}");
                }
            }
            Err(e) => println!("ERR {e}"),
        }
    }
}
