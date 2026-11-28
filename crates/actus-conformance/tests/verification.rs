//! The verification tier table (`verification.toml`) against reality, and
//! its README rendering (yestechgroup/actus-rs#19).

use actus_conformance::{engine_registry, verification};

#[test]
fn the_tier_table_matches_the_testbeds() {
    let table = verification::load().expect("verification.toml parses");
    let problems = verification::check(&table, &engine_registry());
    assert!(
        problems.is_empty(),
        "verification.toml is wrong:\n{}",
        problems.join("\n")
    );
}

/// The README carries the rendered table between its markers, so the
/// published claims cannot drift from the checked ones.
#[test]
fn the_readme_renders_the_tier_table() {
    let table = verification::load().expect("verification.toml parses");
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
        .expect("README.md");
    let start = "<!-- verification-tiers:start -->\n";
    let end = "<!-- verification-tiers:end -->";
    let rendered = readme
        .split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map(|(table, _)| table)
        .expect("README.md has the verification-tiers markers");
    assert_eq!(
        rendered,
        verification::markdown_table(&table),
        "regenerate the README table: cargo run -p actus-conformance --bin report -- --tiers"
    );
}
