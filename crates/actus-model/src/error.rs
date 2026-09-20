//! Error type for the ACTUS model crate.

use thiserror::Error;

/// Errors raised when parsing ACTUS vocabulary or building [`crate::terms::ContractTerms`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    /// A cycle term (e.g. `P1ML0`) failed to parse.
    #[error("invalid ACTUS cycle term `{0}`")]
    InvalidCycle(String),
    /// A timestamp failed to parse. Accepted shapes are `YYYY-MM-DD`,
    /// `YYYY-MM-DDTHH:MM` and `YYYY-MM-DDTHH:MM:SS`.
    #[error("invalid ACTUS date `{0}`")]
    InvalidDate(String),
    /// A numeric attribute value failed to parse as a decimal.
    #[error("invalid ACTUS decimal value `{0}`")]
    InvalidDecimal(String),
    /// A contract type acronym is not part of the dictionary taxonomy.
    #[error("unknown ACTUS contract type `{0}`")]
    UnknownContractType(String),
    /// An enum value is not part of the referenced vocabulary.
    #[error("unknown {vocabulary} value `{value}`")]
    UnknownEnumValue {
        /// Name of the enum vocabulary, e.g. `DayCountConvention`.
        vocabulary: &'static str,
        /// The offending raw token.
        value: String,
    },
    /// A required attribute (per the generated applicability tables) is absent.
    #[error("missing required attribute `{attribute}` for contract type {contract_type}")]
    MissingRequiredAttribute {
        /// Dictionary identifier of the missing attribute.
        attribute: &'static str,
        /// Acronym of the contract type being built.
        contract_type: &'static str,
    },
    /// An attribute was set that the dictionary does not make applicable to the
    /// contract type being built.
    #[error("attribute `{attribute}` is not applicable to contract type {contract_type}")]
    AttributeNotApplicable {
        /// Dictionary identifier of the offending attribute.
        attribute: &'static str,
        /// Acronym of the contract type being built.
        contract_type: &'static str,
    },
    /// The terms bag is internally inconsistent.
    #[error("invalid contract terms: {0}")]
    InvalidTerms(String),
    /// A vendored dictionary file could not be read or parsed.
    #[error("ACTUS dictionary source error: {0}")]
    Source(String),
}
