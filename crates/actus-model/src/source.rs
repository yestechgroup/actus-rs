//! Access to the vendored ACTUS dictionary files.
//!
//! The dictionary lives at `<repo>/vendor/actus/dictionary` and is a read-only
//! normative input. Upstream quirk: `actus-dictionary-terms.json` is not valid
//! strict JSON because description strings contain typographic curly quotes
//! (U+201C/U+201D); [`normalize_quotes`] repairs that before parsing.

use crate::error::ModelError;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Resolve the vendored dictionary directory relative to this crate.
///
/// Uses `CARGO_MANIFEST_DIR` at compile time, so the path is valid both for the
/// generator binary and for tests run from any working directory.
#[must_use]
pub fn dictionary_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/actus/dictionary")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/actus/dictionary")
        })
}

/// Replace typographic curly quotes (U+201C/U+201D) with plain ASCII quotes.
///
/// Upstream ships `actus-dictionary-terms.json` with curly quotes inside
/// description strings, which strict JSON parsers reject.
#[must_use]
pub fn normalize_quotes(input: &str) -> String {
    input.replace(['\u{201C}', '\u{201D}'], "\"")
}

/// Read and parse one dictionary file, normalising curly quotes first.
///
/// `file_name` is a bare name such as `actus-dictionary-terms.json`.
pub fn read_dictionary(file_name: &str) -> Result<Value, ModelError> {
    let path = dictionary_dir().join(file_name);
    let raw = fs::read_to_string(&path)
        .map_err(|e| ModelError::Source(format!("cannot read {}: {e}", path.display())))?;
    let normalized = normalize_quotes(&raw);
    serde_json::from_str(&normalized)
        .map_err(|e| ModelError::Source(format!("cannot parse {}: {e}", path.display())))
}
