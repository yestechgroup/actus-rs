//! Guard against copy-pasted logic between engine modules (#17).
//!
//! Scans every `src/*.rs` file (outside its `#[cfg(test)]` module), takes
//! the body of each function, and fails when a body of more than
//! [`MAX_SHARED_LINES`] meaningful lines appears in two or more files.
//! Shared logic belongs in one place (`fixed_income`, `credit_enhancement`,
//! `common`, ...) so a fix lands once and cannot drift between engines.
//!
//! "Meaningful lines" ignores blank lines, comments and indentation, so
//! reformatting a copy does not hide it.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Identical bodies up to this many lines are allowed: short glue such as
/// trait method one-liners and slot constructors.
const MAX_SHARED_LINES: usize = 10;

/// Functions whose identical bodies are wiring, not logic. Each fixed-income
/// engine's `ContractEngine::evaluate` only hands its own `Context` methods
/// to `fixed_income::evaluate_schedule`; sharing it would need a macro.
const WIRING: &[&str] = &["evaluate"];

struct Function {
    file: String,
    name: String,
    body: Vec<String>,
}

fn source_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The name of the function declared on `line`, if it starts a function
/// with a body (not a trait method declaration).
fn function_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = ["pub(crate) fn ", "pub fn ", "fn "]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))?;
    if line.trim_end().ends_with(';') {
        return None;
    }
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(&rest[..end])
}

fn functions() -> Vec<Function> {
    let mut found = Vec::new();
    let mut paths: Vec<PathBuf> = fs::read_dir(source_dir())
        .expect("src directory")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    for path in paths {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).expect("source file");
        let lines: Vec<&str> = text
            .lines()
            .take_while(|line| !line.starts_with("#[cfg(test)]"))
            .collect();
        for (start, line) in lines.iter().enumerate() {
            let Some(name) = function_name(line) else {
                continue;
            };
            let indent = &line[..line.len() - line.trim_start().len()];
            let closing = format!("{indent}}}");
            let Some(end) = (start + 1..lines.len()).find(|&i| lines[i] == closing) else {
                continue;
            };
            let body = lines[start + 1..end]
                .iter()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty() && !line.starts_with("//"))
                .map(str::to_string)
                .collect();
            found.push(Function {
                file: file.clone(),
                name: name.to_string(),
                body,
            });
        }
    }
    found
}

#[test]
fn the_scanner_sees_the_engine_modules() {
    let functions = functions();
    for (file, name) in [
        ("fixed_income.rs", "evaluate_schedule"),
        ("lam.rs", "apply"),
        ("credit_enhancement.rs", "settlement_date"),
    ] {
        assert!(
            functions.iter().any(|f| f.file == file && f.name == name),
            "scanner missed {file}::{name}"
        );
    }
}

#[test]
fn no_logic_is_duplicated_between_source_files() {
    let mut by_body: BTreeMap<Vec<String>, Vec<(String, String)>> = BTreeMap::new();
    for function in functions() {
        if function.body.len() > MAX_SHARED_LINES && !WIRING.contains(&function.name.as_str()) {
            by_body
                .entry(function.body)
                .or_default()
                .push((function.file, function.name));
        }
    }
    let duplicated: Vec<String> = by_body
        .into_iter()
        .filter(|(_, sites)| {
            let first = &sites[0].0;
            sites.iter().any(|(file, _)| file != first)
        })
        .map(|(body, sites)| {
            let sites: Vec<String> = sites
                .iter()
                .map(|(file, name)| format!("{file}::{name}"))
                .collect();
            format!("{} lines in {}", body.len(), sites.join(", "))
        })
        .collect();
    assert!(
        duplicated.is_empty(),
        "identical function bodies in several files; move them to a shared \
         module:\n  {}",
        duplicated.join("\n  ")
    );
}
