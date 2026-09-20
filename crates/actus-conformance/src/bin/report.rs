//! Conformance report printer for the ACTUS engine.
//!
//! Evaluates the official testbeds for every registered contract type (or the
//! single type named as the command line argument, e.g. `report PAM`) and
//! prints the metadata header from `[package.metadata.actus]`, one PASS/FAIL
//! line per case, per-type counts, the TOTAL aggregates and the overall
//! conformance verdict. Exits 0 on full conformance, 1 otherwise and 2 on
//! load or usage errors.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use actus_conformance::{cases, comparator, engine_registry};
use actus_model::ContractType;

/// Pinned-input metadata carried by `[package.metadata.actus]` in the crate
/// manifest (dictionary version, dictionary commit, testbed commit).
#[derive(Debug, Clone, Default)]
struct ActusMetadata {
    dictionary_version: String,
    dictionary_commit: String,
    tests_commit: String,
}

impl ActusMetadata {
    /// Parses the metadata section out of a manifest text.
    ///
    /// Small toml-free scan: track the current table header and read the
    /// three basic-string keys the report header needs. Returns `None` when
    /// the section is absent or carries none of the keys.
    fn parse(manifest: &str) -> Option<ActusMetadata> {
        let mut metadata = ActusMetadata::default();
        let mut found = false;
        let mut in_section = false;
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_section = trimmed == "[package.metadata.actus]";
                continue;
            }
            if !in_section {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                let value = value.trim().trim_matches('"');
                match key.trim() {
                    "dictionary_version" => {
                        metadata.dictionary_version = value.to_string();
                        found = true;
                    }
                    "dictionary_commit" => {
                        metadata.dictionary_commit = value.to_string();
                        found = true;
                    }
                    "tests_commit" => {
                        metadata.tests_commit = value.to_string();
                        found = true;
                    }
                    _ => {}
                }
            }
        }
        found.then_some(metadata)
    }

    /// Reads the metadata from the crate manifest next to this binary.
    fn load() -> Option<ActusMetadata> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let manifest = fs::read_to_string(path).ok()?;
        Self::parse(&manifest)
    }

    /// The short commit abbreviation used in the report header.
    fn abbrev(commit: &str) -> &str {
        commit.get(..7).unwrap_or(commit)
    }
}

/// Prints the report header with the pinned-input metadata.
fn print_header(metadata: &ActusMetadata) {
    println!("ACTUS CONFORMANCE");
    println!(
        "dictionary v{} (commit {})  \u{b7}  testbeds commit {}",
        metadata.dictionary_version,
        ActusMetadata::abbrev(&metadata.dictionary_commit),
        ActusMetadata::abbrev(&metadata.tests_commit),
    );
    println!();
}

fn main() -> ExitCode {
    let filter = std::env::args().nth(1);
    let registry = engine_registry();
    let tol = comparator::Tolerance::default();

    let mut types: Vec<ContractType> = Vec::new();
    match filter {
        Some(arg) => match arg.trim().to_ascii_uppercase().parse::<ContractType>() {
            Ok(contract_type) => types.push(contract_type),
            Err(_) => {
                eprintln!("unknown contract type filter: {arg}");
                return ExitCode::from(2);
            }
        },
        None => {
            types.push(ContractType::Pam);
            types.push(ContractType::Lam);
            types.push(ContractType::Nam);
            types.push(ContractType::Ann);
            types.push(ContractType::Csh);
            types.push(ContractType::Clm);
            types.push(ContractType::Swaps);
            types.push(ContractType::Cec);
        }
    }

    let metadata = ActusMetadata::load().unwrap_or_default();
    print_header(&metadata);

    let mut contracts_passed = 0;
    let mut contracts_total = 0;
    let mut events_actual = 0;
    let mut events_expected = 0;
    let mut all_passed = true;

    for contract_type in types {
        let loaded = match cases::load_testbed(contract_type) {
            Ok(loaded) => loaded,
            Err(e) => {
                eprintln!("{contract_type}: {e}");
                return ExitCode::from(2);
            }
        };
        println!("{contract_type}");
        let mut type_passed = 0;
        for case in &loaded {
            contracts_total += 1;
            let report = match cases::evaluate_case(&registry, case) {
                Ok(events) => {
                    events_actual += events.len();
                    events_expected += case.results.len();
                    comparator::compare_case(case.identifier.clone(), &events, &case.results, &tol)
                }
                Err(e) => comparator::CaseReport {
                    case_id: case.identifier.clone(),
                    passed: false,
                    failures: vec![format!("evaluation error: {e}")],
                },
            };
            let marker = if report.passed { "PASS" } else { "FAIL" };
            println!("  {:<6}  {marker}", case.identifier);
            if report.passed {
                type_passed += 1;
                contracts_passed += 1;
            } else {
                all_passed = false;
                if let Some(first) = report.failures.first() {
                    println!("        {first}");
                }
            }
        }
        println!("  {type_passed}/{}", loaded.len());
    }

    println!("TOTAL");
    println!("  contracts: {contracts_passed}/{contracts_total}");
    println!("  events:    {events_actual}/{events_expected}");
    if events_actual != events_expected {
        all_passed = false;
    }
    println!();
    println!("CONFORMANCE: {}", if all_passed { "PASS" } else { "FAIL" });
    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_actus_metadata_section() {
        let manifest = "[package]\nname = \"actus-conformance\"\n\n\
            [package.metadata.actus]\n\
            dictionary_version = \"1.4\"\n\
            dictionary_commit = \"356f7663f26091105cc4fef4ae3496942dcf0ebf\"\n\
            tests_commit = \"f7a8064872b69db1f0beabac771c99dc3ce0c397\"\n\
            techspecs_commit = \"94ef09e4992f79d573f84f41d8480f557365870e\"\n\n\
            [[bin]]\nname = \"report\"\n";
        let metadata = ActusMetadata::parse(manifest).expect("metadata section present");
        assert_eq!(metadata.dictionary_version, "1.4");
        assert_eq!(
            ActusMetadata::abbrev(&metadata.dictionary_commit),
            "356f766"
        );
        assert_eq!(ActusMetadata::abbrev(&metadata.tests_commit), "f7a8064");
    }

    #[test]
    fn stops_reading_at_the_next_table_header() {
        let manifest = "[package.metadata.actus]\ndictionary_version = \"1.4\"\n\n\
            [dependencies]\ndictionary_version = \"9.9\"\n";
        let metadata = ActusMetadata::parse(manifest).expect("metadata section present");
        assert_eq!(metadata.dictionary_version, "1.4");
        assert_eq!(metadata.dictionary_commit, "");
        assert_eq!(metadata.tests_commit, "");
    }

    #[test]
    fn missing_section_parses_to_none() {
        assert!(ActusMetadata::parse("[package]\nname = \"x\"\n").is_none());
    }
}
