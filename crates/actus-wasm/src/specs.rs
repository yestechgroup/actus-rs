//! Contract parameter form specifications: which dictionary attributes to
//! show per contract type, with labels, input kinds, defaults and select
//! options, plus the mapping from form values to contract terms JSON.
//!
//! Shared by the SSR state builder (server render) and the browser client
//! (via the WASM bindings) so both build identical terms from the same
//! form. Values are strings: decimal strings for numbers, `YYYY-MM-DD` for
//! dates, dictionary tokens for selects; an empty value means "unset" and
//! the attribute is omitted from the terms.

use std::collections::BTreeMap;

use actus_model::ContractType;
use serde::Serialize;

/// Input kind of one parameter field.
pub const KIND_NUMBER: &str = "number";
pub const KIND_DATE: &str = "date";
pub const KIND_SELECT: &str = "select";
pub const KIND_TEXT: &str = "text";

/// One parameter field of a contract type's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamSpec {
    /// Dictionary identifier of the attribute (the terms JSON key), or a
    /// virtual key such as `leg1.notionalPrincipal` for composed contracts.
    pub key: String,
    /// Display label, e.g. `Notional (NT)`.
    pub label: String,
    /// Input kind: `number` | `date` | `select` | `text`.
    pub kind: String,
    /// Default value as a string.
    pub value: String,
    /// `step` attribute of number inputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Options of select inputs, each with a friendly English label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<OptionChoice>>,
}

/// One selectable option: the raw terms value plus its display label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionChoice {
    /// The value written into the terms JSON.
    pub value: String,
    /// The human-facing label, e.g. `Quarterly` for `P3ML0`.
    pub label: String,
}

const CYCLE_OPTIONS: [&str; 5] = ["P1ML0", "P3ML0", "P6ML0", "P1YL0", "P2YL0"];
const DAY_COUNT_OPTIONS: [&str; 6] = ["30E360", "A365", "AA", "A360", "30E360ISDA", "28E336"];
const ROLE_OPTIONS: [&str; 7] = ["RPA", "RPL", "RFL", "PFL", "RF", "PF", "BUY"];
const CURRENCY_OPTIONS: [&str; 5] = ["USD", "EUR", "GBP", "JPY", "CHF"];

/// English label of a coded option value (cycles, day counts, roles,
/// currencies, fee bases, option types). Unknown values pass through.
#[must_use]
pub fn friendly_option_label(value: &str) -> String {
    match value {
        // Day count conventions.
        "30E360" => "30E/360".into(),
        "A365" => "Act/365".into(),
        "AA" => "Act/Act".into(),
        "A360" => "Act/360".into(),
        "30E360ISDA" => "30E/360 ISDA".into(),
        "28E336" => "28E/336".into(),
        // Contract roles: the sign convention matters to the user.
        "RPA" => "Lender (+)".into(),
        "RPL" => "Borrower (\u{2212})".into(),
        "RFL" => "Receive leg (+)".into(),
        "PFL" => "Pay leg (\u{2212})".into(),
        "RF" => "Receive (+)".into(),
        "PF" => "Pay (\u{2212})".into(),
        "BUY" => "Buy protection (+)".into(),
        "SEL" => "Sell protection (\u{2212})".into(),
        "LG" => "Long (+)".into(),
        "ST" => "Short (\u{2212})".into(),
        // Fee basis.
        "N" => "On notional".into(),
        "A" => "Absolute amount".into(),
        // Option types.
        "C" => "Call".into(),
        "P" => "Put".into(),
        "CP" => "Call or put".into(),
        // Currencies.
        "USD" => "USD \u{b7} US dollar".into(),
        "EUR" => "EUR \u{b7} Euro".into(),
        "GBP" => "GBP \u{b7} Pound sterling".into(),
        "JPY" => "JPY \u{b7} Japanese yen".into(),
        "CHF" => "CHF \u{b7} Swiss franc".into(),
        other => cycle_label(other).unwrap_or_else(|| other.to_string()),
    }
}

/// `P<n><D|W|M|Y>` (with optional stub/index suffix, e.g. `P1ML0`) as English.
fn cycle_label(value: &str) -> Option<String> {
    let no_index = value
        .strip_suffix(|c: char| c.is_ascii_digit())
        .unwrap_or(value);
    let core = no_index.strip_suffix(['L', 'R', 'U']).unwrap_or(no_index);
    let body = core.strip_prefix('P')?;
    let split = body
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(body.len());
    let (digits, rest) = body.split_at(split);
    if digits.is_empty() || rest.len() != 1 {
        return None;
    }
    let unit = rest.chars().next()?;
    let n: u32 = digits.parse().ok()?;
    let unit_name = match unit {
        'D' => "day",
        'W' => "week",
        'M' => "month",
        'Y' => "year",
        _ => return None,
    };
    let special = match (unit, n) {
        ('D', 1) => Some("Daily"),
        ('W', 1) => Some("Weekly"),
        ('M', 1) => Some("Monthly"),
        ('M', 3) => Some("Quarterly"),
        ('M', 6) => Some("Semiannual"),
        ('Y', 1) => Some("Annual"),
        ('Y', 2) => Some("Every 2 years"),
        ('Y', 3) => Some("Every 3 years"),
        ('Y', 5) => Some("Every 5 years"),
        ('Y', 10) => Some("Every 10 years"),
        _ => None,
    };
    Some(
        special
            .map(String::from)
            .unwrap_or_else(|| format!("Every {n} {unit_name}s")),
    )
}

fn choice(value: &str) -> OptionChoice {
    OptionChoice {
        value: value.to_string(),
        label: friendly_option_label(value),
    }
}

fn spec(key: &str, label: &str, kind: &str, value: &str) -> ParamSpec {
    ParamSpec {
        key: key.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        value: value.to_string(),
        step: None,
        options: None,
    }
}

fn number(key: &str, label: &str, value: &str, step: &str) -> ParamSpec {
    let mut s = spec(key, label, KIND_NUMBER, value);
    s.step = Some(step.to_string());
    s
}

fn date(key: &str, label: &str, value: &str) -> ParamSpec {
    spec(key, label, KIND_DATE, value)
}

fn select(key: &str, label: &str, value: &str, options: &[&str]) -> ParamSpec {
    let mut s = spec(key, label, KIND_SELECT, value);
    s.options = Some(options.iter().map(|o| choice(o)).collect());
    s
}

fn cycle(key: &str, label: &str, value: &str) -> ParamSpec {
    select(key, label, value, &CYCLE_OPTIONS)
}

/// The default field of one dictionary attribute, if it has a sensible
/// web-form rendering. Drives the generic forms of contract types without
/// an authored parameter list.
#[must_use]
pub fn field_default(identifier: &str) -> Option<ParamSpec> {
    let field = match identifier {
        "contractRole" => select(identifier, "Contract Role (CNTRL)", "RPA", &ROLE_OPTIONS),
        "marketObjectCodeOfRateReset" => spec(
            identifier,
            "Rate Reset Market Object (RRMO)",
            KIND_TEXT,
            "ADR-MKT",
        ),
        "marketObjectCodeOfScalingIndex" => spec(
            identifier,
            "Scaling Index Market Object (SCMO)",
            KIND_TEXT,
            "",
        ),
        "marketObjectCode" => spec(identifier, "Market Object Code (MOC)", KIND_TEXT, "GOLD"),
        "currency" => select(identifier, "Currency (CUR)", "USD", &CURRENCY_OPTIONS),
        "currency2" => select(identifier, "Currency 2 (CUR2)", "EUR", &CURRENCY_OPTIONS),
        "dayCountConvention" => select(identifier, "Day Count (DCC)", "30E360", &DAY_COUNT_OPTIONS),
        "notionalPrincipal" => number(identifier, "Notional (NT)", "1000000", "1000"),
        "nominalInterestRate" => number(identifier, "Interest Rate (IPNR)", "0.05", "0.001"),
        "nominalInterestRate2" => number(identifier, "Interest Rate 2 (IPNR2)", "0.025", "0.001"),
        "nextPrincipalRedemptionPayment" => {
            number(identifier, "Principal Payment (PRNXT)", "100000", "1000")
        }
        "premiumDiscountAtIED" => number(identifier, "Premium / Discount (PDIED)", "0", "1"),
        "feeRate" => number(identifier, "Fee Rate (FER)", "0.005", "0.001"),
        "penaltyRate" => number(identifier, "Penalty Rate (PTR)", "0.01", "0.001"),
        "rateMultiplier" => number(identifier, "Rate Multiplier (RRMLT)", "1", "0.1"),
        "rateSpread" => number(identifier, "Rate Spread (RRSP)", "0", "0.001"),
        "periodCap" => number(identifier, "Period Cap (RRPC)", "0", "0.001"),
        "periodFloor" => number(identifier, "Period Floor (RRPF)", "0", "0.001"),
        "lifeCap" => number(identifier, "Life Cap (RRLC)", "0", "0.001"),
        "lifeFloor" => number(identifier, "Life Floor (RRLF)", "0", "0.001"),
        "coverageOfCreditEnhancement" => number(identifier, "Coverage Ratio (CECVR)", "1.2", "0.1"),
        "creditEventTypeCovered" => select(
            identifier,
            "Credit Event Covered (CETC)",
            "DF",
            &["DL", "DQ", "DF"],
        ),
        "guaranteedExposure" => select(
            identifier,
            "Guaranteed Exposure (CEGE)",
            "NO",
            &["NO", "NI", "MV"],
        ),
        "optionType" => select(identifier, "Option Type (OPTP)", "C", &["C", "P"]),
        "optionStrike1" => number(identifier, "Strike Price (OPS1)", "100", "0.1"),
        "futuresPrice" => number(identifier, "Forward Price (PFUT)", "100", "0.1"),
        "priceAtPurchaseDate" => number(identifier, "Purchase Price (PPRD)", "75.5", "0.1"),
        "priceAtTerminationDate" => number(identifier, "Termination Price (PPTD)", "80", "0.1"),
        "quantity" => number(identifier, "Quantity (QNTR)", "1000", "1"),
        "unit" => spec(identifier, "Unit (UNT)", KIND_TEXT, "ONC"),
        "deliverySettlement" => select(identifier, "Delivery / Settlement (DS)", "D", &["D", "S"]),
        "contractDealDate" => date(identifier, "Deal Date (CD)", "2025-12-30"),
        "statusDate" => date(identifier, "Status Date (STD)", "2026-01-01"),
        "initialExchangeDate" => date(identifier, "Initial Exchange (IED)", "2026-01-01"),
        "maturityDate" => date(identifier, "Maturity (MD)", "2031-01-01"),
        "exerciseDate" => date(identifier, "Exercise Date (XD)", "2026-07-01"),
        "settlementDate" => date(identifier, "Settlement Date (STD)", "2026-07-01"),
        "purchaseDate" => date(identifier, "Purchase Date (PRD)", "2026-01-01"),
        "terminationDate" => date(identifier, "Termination Date (TD)", "2026-07-01"),
        "capitalizationEndDate" => date(identifier, "Capitalization End (IPCED)", "2026-07-01"),
        "cycleAnchorDateOfInterestPayment" => {
            date(identifier, "Interest Anchor (IPANX)", "2027-01-01")
        }
        "cycleAnchorDateOfPrincipalRedemption" => {
            date(identifier, "Principal Anchor (PRANX)", "2027-01-01")
        }
        "cycleAnchorDateOfRateReset" => date(identifier, "Rate Reset Anchor (RRANX)", ""),
        "cycleAnchorDateOfScalingIndex" => date(identifier, "Scaling Anchor (SCANX)", "2027-01-01"),
        "cycleAnchorDateOfFee" => date(identifier, "Fee Anchor (FEANX)", "2027-01-01"),
        "cycleAnchorDateOfOptionality" => {
            date(identifier, "Optionality Anchor (OPANX)", "2027-01-01")
        }
        "cycleOfInterestPayment" => cycle(identifier, "Interest Cycle (IPCL)", "P1YL0"),
        "cycleOfPrincipalRedemption" => cycle(identifier, "Principal Cycle (PRCL)", "P1YL0"),
        "cycleOfRateReset" => cycle(identifier, "Rate Reset Cycle (RRCL)", ""),
        "cycleOfScalingIndex" => cycle(identifier, "Scaling Cycle (SCCL)", "P1YL0"),
        "cycleOfFee" => cycle(identifier, "Fee Cycle (FECL)", "P3ML0"),
        "cycleOfOptionality" => cycle(identifier, "Optionality Cycle (OPCL)", "P1YL0"),
        "xDayNotice" => date(identifier, "Notice Day (XDN)", "2026-04-01"),
        "prepaymentEffect" => select(identifier, "Prepayment Effect (PYE)", "N", &["N", "A", "M"]),
        _ => return None,
    };
    Some(field)
}

/// Ordered dictionary identifiers of the authored parameter lists, mirroring
/// the issue #1 prototype's per-type forms (extended with the attributes the
/// engines and validator actually need).
fn authored_order(acronym: ContractType) -> Option<Vec<&'static str>> {
    use ContractType::*;
    let fields: Vec<&'static str> = match acronym {
        Pam => [
            "contractRole",
            "notionalPrincipal",
            "nominalInterestRate",
            "dayCountConvention",
            "cycleOfInterestPayment",
            "cycleAnchorDateOfInterestPayment",
            "initialExchangeDate",
            "maturityDate",
            "marketObjectCodeOfRateReset",
            "cycleOfRateReset",
            "cycleAnchorDateOfRateReset",
            "premiumDiscountAtIED",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Lam => [
            "contractRole",
            "notionalPrincipal",
            "nominalInterestRate",
            "dayCountConvention",
            "nextPrincipalRedemptionPayment",
            "cycleOfPrincipalRedemption",
            "cycleAnchorDateOfPrincipalRedemption",
            "cycleOfInterestPayment",
            "cycleAnchorDateOfInterestPayment",
            "initialExchangeDate",
            "maturityDate",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Nam => [
            "contractRole",
            "notionalPrincipal",
            "nominalInterestRate",
            "dayCountConvention",
            "rateSpread",
            "marketObjectCodeOfRateReset",
            "nextPrincipalRedemptionPayment",
            "cycleOfPrincipalRedemption",
            "cycleAnchorDateOfPrincipalRedemption",
            "cycleOfInterestPayment",
            "cycleAnchorDateOfInterestPayment",
            "initialExchangeDate",
            "maturityDate",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Ann => [
            "contractRole",
            "notionalPrincipal",
            "nominalInterestRate",
            "dayCountConvention",
            "nextPrincipalRedemptionPayment",
            "cycleOfPrincipalRedemption",
            "cycleAnchorDateOfPrincipalRedemption",
            "cycleOfInterestPayment",
            "cycleAnchorDateOfInterestPayment",
            "initialExchangeDate",
            "maturityDate",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Csh => [
            "contractRole",
            "notionalPrincipal",
            "currency",
            "statusDate",
        ]
        .to_vec(),
        Clm => [
            "contractRole",
            "notionalPrincipal",
            "nominalInterestRate",
            "dayCountConvention",
            "cycleOfInterestPayment",
            "cycleAnchorDateOfInterestPayment",
            "initialExchangeDate",
            "maturityDate",
            "xDayNotice",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Swaps => [
            "contractRole",
            "leg1.notionalPrincipal",
            "leg1.nominalInterestRate",
            "leg2.notionalPrincipal",
            "leg2.nominalInterestRate",
            "deliverySettlement",
            "currency",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        Cec => [
            "contractRole",
            "covered.notionalPrincipal",
            "covered.nominalInterestRate",
            "coverageOfCreditEnhancement",
            "creditEventTypeCovered",
            "guaranteedExposure",
            "collateral.quantity",
            "collateral.marketObjectCode",
            "contractDealDate",
            "statusDate",
        ]
        .to_vec(),
        _ => return None,
    };
    Some(fields)
}

/// Identifiers the generic form offers in addition to the required
/// attributes, when applicable for the type.
const GENERIC_EXTRAS: [&str; 8] = [
    "nominalInterestRate",
    "cycleOfInterestPayment",
    "cycleAnchorDateOfInterestPayment",
    "maturityDate",
    "dayCountConvention",
    "currency",
    "feeRate",
    "cycleOfFee",
];

/// The parameter form of one contract type: the authored list for the
/// engine-backed types (mirroring the issue #1 prototype), otherwise a
/// generic form derived from the applicability tables.
#[must_use]
pub fn param_specs(contract_type: ContractType) -> Vec<ParamSpec> {
    let tables = actus_model::generated::applicability::tables(contract_type);
    if let Some(order) = authored_order(contract_type) {
        let mut specs: Vec<ParamSpec> = Vec::new();
        for key in order {
            if let Some(spec) = param_spec_for_key(key) {
                specs.push(spec);
            }
        }
        // Ensure every validator-required attribute is on the form, even
        // dictionary gaps without a curated default (rendered as text).
        // Composed contracts provide `contractStructure` virtually.
        for required in tables.base_required {
            if (contract_type == ContractType::Swaps || contract_type == ContractType::Cec)
                && *required == "contractStructure"
            {
                continue;
            }
            if !specs.iter().any(|s| s.key == *required) {
                let synthesized = spec(required, required, KIND_TEXT, "");
                specs.push(field_default(required).unwrap_or(synthesized));
            }
        }
        if contract_type == ContractType::Swaps {
            // The parent swap role: RFL receives the first leg.
            for s in &mut specs {
                if s.key == "contractRole" {
                    s.value = "RFL".to_string();
                }
            }
        }
        return specs;
    }

    // Generic form: required attributes first, then common extras.
    let mut keys: Vec<String> = Vec::new();
    let push = |key: &str, keys: &mut Vec<String>| {
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    };
    for required in tables.base_required {
        push(required, &mut keys);
    }
    for required in tables.required {
        push(required, &mut keys);
    }
    for extra in GENERIC_EXTRAS {
        if tables.applicable.contains(&extra) {
            push(extra, &mut keys);
        }
    }
    keys.iter()
        .map(|key| {
            let synthesized = spec(key, key, KIND_TEXT, "");
            field_default(key).unwrap_or(synthesized)
        })
        .collect()
}

/// The form field of one (possibly virtual, `group.attribute`) key.
fn param_spec_for_key(key: &str) -> Option<ParamSpec> {
    match key.split_once('.') {
        Some((_, tail)) => {
            let mut spec = field_default(tail)?;
            spec.key = key.to_string();
            spec.label = virtual_leg_label(key).unwrap_or(spec.label);
            Some(spec)
        }
        None => field_default(key),
    }
}

fn virtual_leg_label(key: &str) -> Option<String> {
    Some(match key {
        "leg1.notionalPrincipal" => "Leg 1 Notional".to_string(),
        "leg1.nominalInterestRate" => "Leg 1 Fixed Rate".to_string(),
        "leg2.notionalPrincipal" => "Leg 2 Notional".to_string(),
        "leg2.nominalInterestRate" => "Leg 2 Floating Rate".to_string(),
        "covered.notionalPrincipal" => "Covered Notional".to_string(),
        "covered.nominalInterestRate" => "Covered Interest Rate".to_string(),
        "collateral.quantity" => "Collateral Quantity".to_string(),
        "collateral.marketObjectCode" => "Collateral Market Object".to_string(),
        _ => return None,
    })
}

/// Builds the contract terms JSON from form values: `{identifier: value}`
/// with empty strings meaning "unset". Virtual composed-contract keys
/// (`leg1.*`/`leg2.*` for `SWAPS`, `covered.*`/`collateral.*` for `CEC`)
/// are expanded into the `contractStructure` references.
///
/// # Errors
/// The contract type acronym is unknown.
pub fn terms_json(
    contract_type: ContractType,
    values: &BTreeMap<String, String>,
) -> serde_json::Value {
    let mut terms = serde_json::Map::new();
    terms.insert(
        "contractType".to_string(),
        serde_json::Value::String(contract_type.as_acronym().to_string()),
    );
    for (key, value) in values {
        let value = value.trim();
        if value.is_empty() || key == "contractType" || key.contains('.') {
            continue;
        }
        terms.insert(key.clone(), serde_json::Value::String(term_value(value)));
    }
    match contract_type {
        ContractType::Swaps => apply_swaps_structure(&mut terms, values),
        ContractType::Cec => apply_cec_structure(&mut terms, values),
        _ => {}
    }
    serde_json::Value::Object(terms)
}

/// Normalizes one form value into the terms wire format: bare dates
/// (`YYYY-MM-DD`) become full `YYYY-MM-DDTHH:MM:SS` timestamps.
fn term_value(value: &str) -> String {
    if value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
    {
        format!("{value}T00:00:00")
    } else {
        value.to_string()
    }
}

/// One embedded contract object of a `contractStructure` reference.
fn structure_reference(
    object: serde_json::Map<String, serde_json::Value>,
    role: &str,
) -> serde_json::Value {
    serde_json::json!({
        "object": serde_json::Value::Object(object),
        "referenceType": "CNT",
        "referenceRole": role,
    })
}

/// Shared context fields the parent hands down to every embedded leg.
fn leg_shared(
    terms: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    let mut object = serde_json::Map::new();
    for key in keys {
        if let Some(value) = terms.get(*key) {
            if !value.is_null() && value != &serde_json::Value::String(String::new()) {
                object.insert((*key).to_string(), value.clone());
            }
        }
    }
    object
}

fn leg_insert(
    object: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<serde_json::Value>,
) {
    if let Some(value) = value {
        if value != serde_json::Value::String(String::new()) && !value.is_null() {
            object.insert(key.to_string(), value);
        }
    }
}

fn virtual_value(values: &BTreeMap<String, String>, key: &str) -> Option<serde_json::Value> {
    values
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(serde_json::Value::String)
}

fn apply_swaps_structure(
    terms: &mut serde_json::Map<String, serde_json::Value>,
    values: &BTreeMap<String, String>,
) {
    // The swap legs are internally fixed PAM schedules (annual interest,
    // IED at max(deal date, status date), 5y maturity); the form drives the
    // two notionals and rates plus the parent-level settings.
    let parse = |key: &str| {
        terms
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
    };
    let deal_date = parse("contractDealDate");
    let status_date = parse("statusDate");
    let ied = deal_date
        .into_iter()
        .chain(status_date)
        .max()
        .unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .expect("valid date")
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
        });
    let maturity = ied
        .date()
        .checked_add_months(chrono::Months::new(60))
        .unwrap_or(ied.date())
        .and_hms_opt(0, 0, 0)
        .expect("midnight");
    let ied_text = ied.format("%Y-%m-%dT%H:%M:%S").to_string();
    let maturity_text = maturity.format("%Y-%m-%dT%H:%M:%S").to_string();

    let mut legs = Vec::new();
    for (prefix, role) in [("leg1", "FIL"), ("leg2", "SEL")] {
        let mut object = leg_shared(terms, &["contractDealDate", "statusDate", "currency"]);
        object.insert(
            "contractType".to_string(),
            serde_json::Value::String("PAM".to_string()),
        );
        object.insert(
            "dayCountConvention".to_string(),
            serde_json::Value::String("30E360".to_string()),
        );
        object.insert(
            "initialExchangeDate".to_string(),
            serde_json::Value::String(ied_text.clone()),
        );
        object.insert(
            "maturityDate".to_string(),
            serde_json::Value::String(maturity_text.clone()),
        );
        object.insert(
            "cycleAnchorDateOfInterestPayment".to_string(),
            serde_json::Value::String(ied_text.clone()),
        );
        object.insert(
            "cycleOfInterestPayment".to_string(),
            serde_json::Value::String("P1YL0".to_string()),
        );
        object.insert(
            "premiumDiscountAtIED".to_string(),
            serde_json::Value::String("0".to_string()),
        );
        leg_insert(
            &mut object,
            "notionalPrincipal",
            virtual_value(values, &format!("{prefix}.notionalPrincipal")),
        );
        leg_insert(
            &mut object,
            "nominalInterestRate",
            virtual_value(values, &format!("{prefix}.nominalInterestRate")),
        );
        legs.push(structure_reference(object, role));
    }
    terms.insert(
        "contractStructure".to_string(),
        serde_json::Value::Array(legs),
    );
    terms
        .entry("deliverySettlement".to_string())
        .or_insert(serde_json::Value::String("D".to_string()));
}

fn apply_cec_structure(
    terms: &mut serde_json::Map<String, serde_json::Value>,
    values: &BTreeMap<String, String>,
) {
    let parse = |key: &str| {
        terms
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
    };
    let ied = parse("contractDealDate")
        .into_iter()
        .chain(parse("statusDate"))
        .max()
        .unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                .expect("valid date")
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
        });
    let maturity = ied
        .date()
        .checked_add_months(chrono::Months::new(12))
        .unwrap_or(ied.date())
        .and_hms_opt(0, 0, 0)
        .expect("midnight");
    let ied_text = ied.format("%Y-%m-%dT%H:%M:%S").to_string();
    let maturity_text = maturity.format("%Y-%m-%dT%H:%M:%S").to_string();

    let mut covered = leg_shared(terms, &["contractDealDate", "statusDate"]);
    covered.insert(
        "contractType".to_string(),
        serde_json::Value::String("PAM".to_string()),
    );
    covered.insert(
        "contractRole".to_string(),
        serde_json::Value::String("RPA".to_string()),
    );
    covered.insert(
        "dayCountConvention".to_string(),
        serde_json::Value::String("30E360".to_string()),
    );
    covered.insert(
        "initialExchangeDate".to_string(),
        serde_json::Value::String(ied_text.clone()),
    );
    covered.insert(
        "maturityDate".to_string(),
        serde_json::Value::String(maturity_text.clone()),
    );
    covered.insert(
        "cycleAnchorDateOfInterestPayment".to_string(),
        serde_json::Value::String(ied_text),
    );
    covered.insert(
        "cycleOfInterestPayment".to_string(),
        serde_json::Value::String("P1ML0".to_string()),
    );
    leg_insert(
        &mut covered,
        "notionalPrincipal",
        virtual_value(values, "covered.notionalPrincipal"),
    );
    leg_insert(
        &mut covered,
        "nominalInterestRate",
        virtual_value(values, "covered.nominalInterestRate"),
    );

    let mut collateral = leg_shared(terms, &["contractDealDate", "statusDate"]);
    collateral.insert(
        "contractType".to_string(),
        serde_json::Value::String("COM".to_string()),
    );
    collateral.insert(
        "contractRole".to_string(),
        serde_json::Value::String("RPA".to_string()),
    );
    collateral.insert(
        "quantity".to_string(),
        virtual_value(values, "collateral.quantity")
            .unwrap_or_else(|| serde_json::Value::String("1".to_string())),
    );
    leg_insert(
        &mut collateral,
        "marketObjectCode",
        virtual_value(values, "collateral.marketObjectCode"),
    );

    terms.insert(
        "contractStructure".to_string(),
        serde_json::Value::Array(vec![
            structure_reference(covered, "COVE"),
            structure_reference(collateral, "COVI"),
        ]),
    );
}

/// The default form values of one contract type, keyed by parameter key.
#[must_use]
pub fn default_values(contract_type: ContractType) -> BTreeMap<String, String> {
    param_specs(contract_type)
        .into_iter()
        .map(|s| (s.key, s.value))
        .collect()
}

/// The default terms JSON of one contract type (all form defaults set).
#[must_use]
pub fn default_terms_json(contract_type: ContractType) -> serde_json::Value {
    terms_json(contract_type, &default_values(contract_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto;

    #[test]
    fn friendly_labels_map_cycles_and_codes() {
        assert_eq!(friendly_option_label("P1ML0"), "Monthly");
        assert_eq!(friendly_option_label("P3ML0"), "Quarterly");
        assert_eq!(friendly_option_label("P6ML0"), "Semiannual");
        assert_eq!(friendly_option_label("P1YL0"), "Annual");
        assert_eq!(friendly_option_label("P2YL0"), "Every 2 years");
        assert_eq!(friendly_option_label("P1M"), "Monthly");
        assert_eq!(friendly_option_label("P3ML1"), "Quarterly");
        assert_eq!(friendly_option_label("P10YL0"), "Every 10 years");
        assert_eq!(friendly_option_label("RPA"), "Lender (+)");
        assert_eq!(friendly_option_label("A365"), "Act/365");
        assert_eq!(friendly_option_label("ZZZ"), "ZZZ");
    }

    fn terms_for(acronym: &str) -> serde_json::Value {
        let t: ContractType = acronym.parse().expect("known type");
        default_terms_json(t)
    }

    #[test]
    fn every_engine_type_default_terms_validate_and_evaluate() {
        for acronym in ["PAM", "LAM", "NAM", "ANN", "CSH", "CLM", "SWAPS", "CEC"] {
            let terms = terms_for(acronym);
            let raw = serde_json::to_string(&terms).unwrap();
            let report = dto::validate_json(&raw)
                .unwrap_or_else(|e| panic!("{acronym} default terms must validate: {e}"));
            let report: dto::ValidationReport =
                serde_json::from_str(&report).expect("report parses");
            assert!(
                report.valid,
                "{acronym} default terms must validate: {:?}",
                report.errors
            );
            let result = dto::evaluate_json(&raw)
                .unwrap_or_else(|e| panic!("{acronym} default terms must evaluate: {e}"));
            let value: serde_json::Value = serde_json::from_str(&result).unwrap();
            // CSH is a static cash position: without observed analysis
            // dates its event stream is legitimately empty.
            if acronym != "CSH" {
                assert!(
                    !value["events"].as_array().expect("events").is_empty(),
                    "{acronym} must produce events"
                );
            }
        }
    }

    #[test]
    fn every_type_has_a_form_with_required_attributes() {
        for t in ContractType::ALL {
            let specs = param_specs(*t);
            let tables = actus_model::generated::applicability::tables(*t);
            if tables.base_required.is_empty() && tables.applicable.is_empty() {
                // Dictionary gaps (CDSWP, MAR): nothing is applicable, so
                // the honest form is empty.
                assert!(
                    specs.is_empty(),
                    "{} should not offer fields for an empty applicability table",
                    t.as_acronym()
                );
                continue;
            }
            assert!(!specs.is_empty(), "{} has no form fields", t.as_acronym());
            for required in tables.base_required {
                // Composed contracts provide `contractStructure` virtually.
                if (*t == ContractType::Swaps || *t == ContractType::Cec)
                    && *required == "contractStructure"
                {
                    continue;
                }
                assert!(
                    specs.iter().any(|s| s.key == *required),
                    "{} form misses base-required attribute {}",
                    t.as_acronym(),
                    required
                );
            }
        }
    }

    #[test]
    fn swaps_terms_build_leg_structure() {
        let terms = terms_for("SWAPS");
        let structure = terms["contractStructure"].as_array().expect("legs");
        assert_eq!(structure.len(), 2);
        assert_eq!(structure[0]["object"]["notionalPrincipal"], "1000000");
        assert_eq!(structure[0]["referenceRole"], "FIL");
        assert_eq!(structure[1]["object"]["notionalPrincipal"], "1000000");
        assert_eq!(structure[1]["referenceRole"], "SEL");
        assert_eq!(structure[1]["object"]["dayCountConvention"], "30E360");
    }

    #[test]
    fn cec_terms_build_covered_and_collateral() {
        let terms = terms_for("CEC");
        let structure = terms["contractStructure"].as_array().expect("legs");
        assert_eq!(structure.len(), 2);
        assert_eq!(structure[0]["referenceRole"], "COVE");
        assert_eq!(structure[1]["object"]["contractType"], "COM");
        assert_eq!(structure[1]["object"]["quantity"], "1000");
        assert_eq!(terms["coverageOfCreditEnhancement"], "1.2");
    }

    #[test]
    fn empty_values_are_omitted() {
        let t: ContractType = "PAM".parse().unwrap();
        let mut values = default_values(t);
        values.insert("nominalInterestRate".to_string(), String::new());
        let terms = terms_json(t, &values);
        assert!(terms.get("nominalInterestRate").is_none());
        assert_eq!(terms["contractType"], "PAM");
    }
}
