//! Refactoring guard: prints every engine event (type, time, payoff and the
//! full post-event state) for all official testbed cases, the LAM shapes
//! replayed as LAX, and the pinned differential corpus.
//!
//! The eleven testbeds vendored with the hand-tested types (STK, COM,
//! FXOUT, SWPPV, CAPFL, OPTNS, FUTUR, CEG, UMP, LAX and the ANN case `ad0`)
//! print last, under `## <testbed>/<case>` headings: case identifiers are
//! only unique within a file (`ad0` has an `ann01` too).
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
use actus_engine::EngineRegistry;
use actus_model::ContractType;

/// The testbeds printed at the top under their bare case identifiers.
const ORIGINAL_TESTBEDS: [&str; 8] = ["pam", "lam", "nam", "ann", "csh", "clm", "swaps", "cec"];

fn print_case(registry: &EngineRegistry, heading: &str, case: &cases::ActusTestCase) {
    println!("## {heading}");
    match cases::evaluate_case(registry, case) {
        Ok(evs) => {
            for e in evs {
                println!("{e:?}");
            }
        }
        Err(e) => println!("ERR {e}"),
    }
}

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
    for testbed in cases::TESTBEDS {
        if ORIGINAL_TESTBEDS.contains(&testbed.name) {
            continue;
        }
        for case in testbed.load().unwrap() {
            print_case(
                &registry,
                &format!("{}/{}", testbed.name, case.identifier),
                &case,
            );
        }
    }
}
