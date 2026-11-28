//! The verification tier table (`verification.toml`, yestechgroup/actus-rs#19):
//! how thoroughly each contract type is checked, in a form CI can hold
//! against reality ([`check`]) and the report and the README can render
//! ([`markdown_table`]).
//!
//! The file is a flat TOML subset: one `[TYPE]` table per contract type with
//! `key = "string"`, `key = number` and `key = true` lines and `#` comments.
//! It is parsed here without a TOML dependency, like the report binary's
//! manifest metadata.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use actus_engine::{EngineError, EngineRegistry, StateProvider};
use actus_model::{ContractTerms, ContractType};

use crate::{cases, comparator};

/// A verification tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Official testbed plus the independent oracle and its corpus.
    A,
    /// Official testbed, passing except documented testbed defects.
    B,
    /// No complete official testbed.
    C,
    /// No engine.
    None,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tier::A => "A",
            Tier::B => "B",
            Tier::C => "C",
            Tier::None => "none",
        })
    }
}

/// One testbed claim of a type: the testbed name and its pass count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestbedClaim {
    /// Testbed name, e.g. `fxout` or `ad0`.
    pub name: String,
    /// Cases claimed to pass.
    pub passing: usize,
    /// Cases in the testbed.
    pub cases: usize,
}

/// One row of the tier table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeTier {
    /// The contract type.
    pub contract_type: ContractType,
    /// Its tier.
    pub tier: Tier,
    /// The official testbeds it claims, with pass counts.
    pub testbeds: Vec<TestbedClaim>,
    /// Whether the independent oracle and its differential corpus cover it.
    pub oracle: bool,
    /// Failing cases that are documented testbed defects, as
    /// `testbed/case`.
    pub known_defects: Vec<String>,
    /// The spec-interpretation entry documenting the defects.
    pub defect_note: Option<String>,
    /// The issue that tracks the work a tier-C type needs.
    pub issue: Option<u32>,
    /// A free-text note.
    pub note: Option<String>,
}

/// Loads and parses `verification.toml`.
pub fn load() -> Result<Vec<TypeTier>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("verification.toml");
    let raw = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&raw)
}

/// Parses the tier table text.
pub fn parse(raw: &str) -> Result<Vec<TypeTier>, String> {
    let mut tables: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    for (number, line) in raw.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            tables.push((name.trim().to_string(), BTreeMap::new()));
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `key = value`", number + 1))?;
        let table = tables
            .last_mut()
            .ok_or_else(|| format!("line {}: key outside a [TYPE] table", number + 1))?;
        let value = value.trim().trim_matches('"').to_string();
        table.1.insert(key.trim().to_string(), value);
    }
    tables
        .into_iter()
        .map(|(name, keys)| row(&name, &keys))
        .collect()
}

fn row(name: &str, keys: &BTreeMap<String, String>) -> Result<TypeTier, String> {
    let contract_type: ContractType = name
        .parse()
        .map_err(|_| format!("[{name}]: unknown contract type"))?;
    let tier = match keys.get("tier").map(String::as_str) {
        Some("A") => Tier::A,
        Some("B") => Tier::B,
        Some("C") => Tier::C,
        Some("none") => Tier::None,
        other => {
            return Err(format!(
                "[{name}]: tier must be A, B, C or none, got {other:?}"
            ))
        }
    };
    let list = |key: &str| -> Vec<String> {
        keys.get(key)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let testbeds = list("testbeds")
        .iter()
        .map(|claim| {
            let (testbed, counts) = claim.split_once(':').ok_or_else(|| {
                format!("[{name}]: testbed claim {claim:?} needs name:passing/cases")
            })?;
            let (passing, cases) = counts
                .split_once('/')
                .ok_or_else(|| format!("[{name}]: testbed claim {claim:?} needs passing/cases"))?;
            let count = |s: &str| {
                s.trim()
                    .parse::<usize>()
                    .map_err(|_| format!("[{name}]: bad count in {claim:?}"))
            };
            Ok(TestbedClaim {
                name: testbed.trim().to_string(),
                passing: count(passing)?,
                cases: count(cases)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let issue = keys
        .get("issue")
        .map(|v| {
            v.parse::<u32>()
                .map_err(|_| format!("[{name}]: bad issue {v:?}"))
        })
        .transpose()?;
    Ok(TypeTier {
        contract_type,
        tier,
        testbeds,
        oracle: keys.get("oracle").map(String::as_str) == Some("true"),
        known_defects: list("known_defects"),
        defect_note: keys.get("defect_note").cloned(),
        issue,
        note: keys.get("note").cloned(),
    })
}

/// The failing cases of a testbed, as `testbed/case`, and its case count.
fn failing_cases(
    registry: &EngineRegistry,
    testbed: &cases::Testbed,
) -> Result<(Vec<String>, usize), String> {
    let tol = comparator::Tolerance::default();
    let loaded = testbed
        .load()
        .map_err(|e| format!("{}: {e}", testbed.name))?;
    let failing = loaded
        .iter()
        .filter(|case| {
            !cases::evaluate_case(registry, case)
                .map(|events| {
                    comparator::compare_case(&case.identifier, &events, &case.results, &tol).passed
                })
                .unwrap_or(false)
        })
        .map(|case| format!("{}/{}", testbed.name, case.identifier))
        .collect();
    Ok((failing, loaded.len()))
}

/// Whether the registry has an engine for the type (an empty bag reports
/// any error but [`EngineError::UnsupportedContractType`]).
fn has_engine(registry: &EngineRegistry, contract_type: ContractType) -> bool {
    !matches!(
        registry.evaluate(&ContractTerms::new(contract_type), &StateProvider::new()),
        Err(EngineError::UnsupportedContractType(_))
    )
}

/// Holds the table against reality; returns every violation.
pub fn check(table: &[TypeTier], registry: &EngineRegistry) -> Vec<String> {
    let mut problems = Vec::new();
    for contract_type in ContractType::ALL.iter().copied() {
        let rows = table
            .iter()
            .filter(|r| r.contract_type == contract_type)
            .count();
        let engine = has_engine(registry, contract_type);
        if rows != 1 && (engine || rows > 1) {
            problems.push(format!("{contract_type}: listed {rows} times"));
        }
    }
    for row in table {
        let ct = row.contract_type;
        let engine = has_engine(registry, ct);
        match (row.tier, engine) {
            (Tier::None, true) => problems.push(format!("{ct}: has an engine but tier none")),
            (Tier::None, false) => continue,
            (_, false) => problems.push(format!("{ct}: tier {} without an engine", row.tier)),
            _ => {}
        }
        if row.tier == Tier::A && !row.oracle {
            problems.push(format!("{ct}: tier A needs the oracle"));
        }
        if row.oracle && !crate::oracle::COVERED_TYPES.contains(&ct) {
            problems.push(format!("{ct}: claims the oracle, which does not cover it"));
        }
        if matches!(row.tier, Tier::A | Tier::B) && row.testbeds.is_empty() {
            problems.push(format!("{ct}: tier {} needs an official testbed", row.tier));
        }
        if row.tier == Tier::C && !row.testbeds.is_empty() && row.issue.is_none() {
            problems.push(format!("{ct}: a failing testbed in tier C needs its issue"));
        }
        if !row.known_defects.is_empty() && row.defect_note.is_none() {
            problems.push(format!("{ct}: known defects need their defect_note"));
        }
        let mut failing_all = Vec::new();
        for claim in &row.testbeds {
            let Some(testbed) = cases::testbed(&claim.name) else {
                problems.push(format!("{ct}: testbed {} is not vendored", claim.name));
                continue;
            };
            if testbed.contract_type != ct {
                problems.push(format!(
                    "{ct}: testbed {} is {}",
                    claim.name, testbed.contract_type
                ));
            }
            match failing_cases(registry, &testbed) {
                Ok((failing, total)) => {
                    let passing = total.saturating_sub(failing.len());
                    if (claim.passing, claim.cases) != (passing, total) {
                        problems.push(format!(
                            "{ct}: {} claims {}/{}, actual {passing}/{total}",
                            claim.name, claim.passing, claim.cases
                        ));
                    }
                    failing_all.extend(failing);
                }
                Err(e) => problems.push(e),
            }
        }
        match row.tier {
            Tier::A | Tier::B => {
                if failing_all != row.known_defects {
                    problems.push(format!(
                        "{ct}: failing cases {failing_all:?} are not exactly the known defects {:?}",
                        row.known_defects
                    ));
                }
            }
            Tier::C | Tier::None => {}
        }
    }
    problems
}

/// The tier table as Markdown, for the README and the report.
pub fn markdown_table(table: &[TypeTier]) -> String {
    let mut out = String::from(
        "| Type | Tier | Official testbed | Oracle | Notes |\n|---|---|---|---|---|\n",
    );
    for row in table {
        let testbeds = if row.testbeds.is_empty() {
            "—".to_string()
        } else {
            row.testbeds
                .iter()
                .map(|c| format!("`{}` {}/{}", c.name, c.passing, c.cases))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut notes = Vec::new();
        if !row.known_defects.is_empty() {
            notes.push(format!(
                "testbed defect: {} ({})",
                row.known_defects.join(", "),
                row.defect_note.as_deref().unwrap_or("")
            ));
        }
        if let Some(issue) = row.issue {
            notes.push(format!("#{issue}"));
        }
        if let Some(note) = &row.note {
            notes.push(note.clone());
        }
        if row.tier == Tier::None {
            notes.push("no engine".to_string());
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            row.contract_type,
            row.tier,
            testbeds,
            if row.oracle { "yes" } else { "—" },
            notes.join("; ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_and_rejects_malformed_ones() {
        let table = parse(
            "# comment\n[FXOUT]\ntier = \"B\"\ntestbeds = \"fxout:11/12\"\n\
             known_defects = \"fxout/fxout10\"\ndefect_note = \"SI-007\"\n\n[BCS]\ntier = \"none\"\n",
        )
        .expect("parses");
        assert_eq!(table.len(), 2);
        assert_eq!(table[0].tier, Tier::B);
        assert_eq!(
            table[0].testbeds,
            vec![TestbedClaim {
                name: "fxout".into(),
                passing: 11,
                cases: 12
            }]
        );
        assert_eq!(table[0].known_defects, vec!["fxout/fxout10".to_string()]);
        assert_eq!(table[1].tier, Tier::None);
        for bad in [
            "tier = \"A\"\n",
            "[XYZ]\ntier = \"A\"\n",
            "[PAM]\ntier = \"Z\"\n",
            "[PAM]\ntier = \"A\"\ntestbeds = \"pam\"\n",
            "[PAM]\ntier = \"A\"\ntestbeds = \"pam:x/25\"\n",
            "[PAM]\nnonsense\n",
            "[CEG]\ntier = \"C\"\nissue = \"x\"\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn renders_a_markdown_row_per_type() {
        let table = parse(
            "[CEG]\ntier = \"C\"\ntestbeds = \"ceg:0/14\"\nissue = 48\n[MAR]\ntier = \"none\"\n",
        )
        .expect("parses");
        let md = markdown_table(&table);
        assert!(md.contains("| CEG | C | `ceg` 0/14 | — | #48 |"), "{md}");
        assert!(md.contains("| MAR | none | — | — | no engine |"), "{md}");
    }
}
