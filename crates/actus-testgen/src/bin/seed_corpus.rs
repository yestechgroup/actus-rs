//! Writes the initial cargo-fuzz corpora (`fuzz/seed-corpus.sh` runs this).
//!
//! `seed_corpus <json_raw dir> <terms_structured dir>`:
//! - `json_raw` gets every seed's terms JSON (testbeds, Explorer defaults,
//!   `seeds/*.json`) as a file;
//! - `terms_structured` gets the generator input that replays each seed.

use std::fs;
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(raw), Some(structured)) = (args.next(), args.next()) else {
        eprintln!("usage: seed_corpus <json_raw dir> <terms_structured dir>");
        std::process::exit(2);
    };
    fs::create_dir_all(&raw)?;
    fs::create_dir_all(&structured)?;
    for (i, terms) in actus_testgen::seed_terms().iter().enumerate() {
        fs::write(raw.join(format!("seed-{i:04}.json")), terms)?;
    }
    for (i, input) in actus_testgen::seed_inputs().iter().enumerate() {
        fs::write(structured.join(format!("seed-{i:04}")), input)?;
    }
    Ok(())
}
