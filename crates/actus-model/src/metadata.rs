//! serde-serializable metadata views over the generated vocabulary, for
//! UI clients (the WASM bindings, testbed explorers): contract type
//! taxonomy, per-type applicability tables and the labelled attribute
//! dictionary. Pure data — no I/O, no engine logic.

use crate::generated::applicability;
use crate::generated::attribute::{self, AttributeType};
use crate::generated::contract_type::ContractType;
use serde::Serialize;

/// One contract type of the ACTUS taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractTypeInfo {
    /// Taxonomy acronym, e.g. `PAM`.
    pub acronym: String,
    /// Taxonomy identifier, e.g. `principalAtMaturity`.
    pub identifier: String,
    /// Taxonomy display name with release status, e.g.
    /// `Principal at Maturity (Released)`.
    pub name: String,
    /// Taxonomy family (`Basic`, `Combined`, `Credit Enhancement`), when
    /// the dictionary declares one.
    pub category: Option<String>,
}

/// Applicability tables of one contract type (attribute identifiers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicabilityInfo {
    /// Attributes the dictionary marks mandatory in the base matrix.
    pub required: Vec<String>,
    /// Attributes the applicability-enforcing builders require.
    pub base_required: Vec<String>,
    /// Attributes that may be set on this contract type.
    pub applicable: Vec<String>,
}

/// One ACTUS dictionary attribute with its documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeInfo {
    /// Dictionary acronym, e.g. `NT`.
    pub acronym: String,
    /// Dictionary display name (label), e.g. `Notional Principal`.
    pub name: String,
    /// Dictionary description.
    pub description: String,
    /// Dictionary declared data type name, e.g. `Real`.
    pub data_type: Option<String>,
}

/// Metadata for every released/implemented contract type, in
/// [`ContractType::ALL`] order.
#[must_use]
pub fn contract_types() -> Vec<ContractTypeInfo> {
    ContractType::ALL
        .iter()
        .map(|t| ContractTypeInfo {
            acronym: t.as_acronym().to_string(),
            identifier: t.identifier().to_string(),
            name: t.name().to_string(),
            category: taxonomy_category(*t),
        })
        .collect()
}

/// Applicability tables for one contract type.
#[must_use]
pub fn applicability_info(t: ContractType) -> ApplicabilityInfo {
    let tables = applicability::tables(t);
    ApplicabilityInfo {
        required: tables.required.iter().map(|s| (*s).to_string()).collect(),
        base_required: tables
            .base_required
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        applicable: tables.applicable.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Documentation metadata for every dictionary attribute, in
/// [`attribute::ALL`] order.
#[must_use]
pub fn attribute_meta() -> Vec<AttributeInfo> {
    attribute::ALL
        .iter()
        .map(|a| AttributeInfo {
            acronym: a.acronym.to_string(),
            name: a.name.to_string(),
            description: a.description.to_string(),
            data_type: Some(attribute_type_name(a.attribute_type).to_string()),
        })
        .collect()
}

fn taxonomy_category(t: ContractType) -> Option<String> {
    let family = t.family();
    if family.is_empty() {
        None
    } else {
        Some(family.to_string())
    }
}

fn attribute_type_name(t: AttributeType) -> &'static str {
    match t {
        AttributeType::Boolean => "Boolean",
        AttributeType::ContractReferenceArray => "ContractReferenceArray",
        AttributeType::Cycle => "Cycle",
        AttributeType::CycleArray => "CycleArray",
        AttributeType::Enum => "Enum",
        AttributeType::EnumArray => "EnumArray",
        AttributeType::Period => "Period",
        AttributeType::Real => "Real",
        AttributeType::RealArray => "RealArray",
        AttributeType::Text => "Text",
        AttributeType::Timestamp => "Timestamp",
        AttributeType::TimestampArray => "TimestampArray",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_types_cover_the_taxonomy_with_categories() {
        let all = contract_types();
        assert_eq!(all.len(), ContractType::ALL.len());
        let pam = all.iter().find(|t| t.acronym == "PAM").unwrap();
        assert_eq!(pam.identifier, "principalAtMaturity");
        assert_eq!(pam.category.as_deref(), Some("Basic"));
        let swaps = all.iter().find(|t| t.acronym == "SWAPS").unwrap();
        assert_eq!(swaps.category.as_deref(), Some("Combined"));
    }

    #[test]
    fn applicability_info_delegates_to_the_tables() {
        let info = applicability_info(ContractType::Pam);
        assert!(info.required.contains(&"notionalPrincipal".to_string()));
        assert!(info.base_required.contains(&"maturityDate".to_string()));
        assert!(info
            .applicable
            .contains(&"premiumDiscountAtIED".to_string()));
        let empty = applicability_info(ContractType::Cdswp);
        assert!(empty.required.is_empty());
        assert!(empty.applicable.is_empty());
    }

    #[test]
    fn attribute_meta_covers_the_dictionary() {
        let all = attribute_meta();
        assert_eq!(all.len(), attribute::ALL.len());
        let nt = all.iter().find(|a| a.acronym == "NT").unwrap();
        assert_eq!(nt.name, "Notional Principal");
        assert!(!nt.description.is_empty());
        assert_eq!(nt.data_type.as_deref(), Some("Real"));
        let ipcl = all.iter().find(|a| a.acronym == "IPCL").unwrap();
        assert_eq!(ipcl.data_type.as_deref(), Some("Cycle"));
    }
}
