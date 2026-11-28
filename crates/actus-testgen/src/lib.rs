//! Structure-aware generator of extreme but well-formed ACTUS contract terms.
//!
//! One generator, two drivers: the cargo-fuzz targets in `fuzz/` (nightly,
//! coverage-guided) and the stable property test in `tests/never_panics.rs`
//! both turn a byte string into a [`Case`] with [`generate`]. The bytes pick a
//! contract type, a seed contract and a sequence of mutations. Every choice is
//! a pure function of the bytes, so a failing input replays exactly.
//!
//! **Seeds.** Every official testbed case of `vendor/actus/tests/*.json`,
//! the ACTUS Explorer default terms of each of the 18 engine types
//! ([`actus_wasm::specs::default_terms_json`]), and the hand-written seeds in
//! `seeds/*.json`. The hand-written seeds come from each engine's unit tests
//! and come with a scenario. They give the ten types without a testbed
//! (whose Explorer defaults mostly fail to evaluate) realistic starting
//! shapes, and every one of them evaluates to a non-empty event stream (see
//! the `hand_written_seeds_evaluate` test).
//!
//! **Mutations.** A mutation sets, replaces or removes one attribute, either
//! at the top level or inside a `contractStructure` child. New values follow
//! the attribute's declared dictionary type, with extreme values favoured:
//!
//! - decimals near `Decimal::MAX`, `1e-28`, zero and negatives;
//! - dates from year 1 to 9999, leap days and month ends;
//! - cycles of length 1 up to `u32::MAX`, every period and stub;
//! - arrays from empty up to 64 elements.
//!
//! Enum values are mostly taken from the dictionary's `allowedValues`. A few
//! mutations deliberately produce the wrong JSON type, to exercise
//! rejection.
//!
//! **Scenario.** A risk-factor scenario is generated alongside the terms:
//! series for every market object code and currency pair the terms mention,
//! plus observed events and credit events.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use actus_model::ContractType;
use serde_json::{json, Map, Value};

/// One generated input: terms JSON and a risk scenario JSON for
/// `actus_wasm::dto::evaluate_json_with_risk`.
#[derive(Debug, Clone)]
pub struct Case {
    /// Contract terms JSON.
    pub terms: String,
    /// Scenario JSON (`actus_wasm::dto::Scenario` shape).
    pub scenario: String,
}

/// Reads choices from a byte string. Past the end it returns zeros, so every
/// byte string, including the empty one, is a valid input.
pub struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    /// A reader over `data`.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The next byte, or 0 once the input is exhausted.
    pub fn u8(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos = self.pos.saturating_add(1);
        b
    }

    /// The next four bytes as a little-endian `u32`.
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }

    /// The next eight bytes as a little-endian `u64`.
    pub fn u64(&mut self) -> u64 {
        (u64::from(self.u32()) << 32) | u64::from(self.u32())
    }

    /// A choice in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        let wide = u16::from_le_bytes([self.u8(), self.u8()]);
        usize::from(wide) % n.max(1)
    }

    /// One element of a non-empty slice.
    pub fn pick<'s, T>(&mut self, items: &'s [T]) -> &'s T {
        &items[self.below(items.len())]
    }
}

/// Declared dictionary type of an attribute, as far as the generator needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Real,
    Timestamp,
    Cycle,
    Period,
    Enum,
    Text,
    Boolean,
    RealArray,
    TimestampArray,
    CycleArray,
    EnumArray,
    ContractReferenceArray,
}

#[derive(Debug, Clone)]
struct AttributeDef {
    kind: Kind,
    /// `allowedValues[*].acronym` for enum attributes.
    allowed: Vec<String>,
}

fn dictionary() -> &'static BTreeMap<String, AttributeDef> {
    static DICTIONARY: OnceLock<BTreeMap<String, AttributeDef>> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        // The vendored dictionary contains typographic quotes inside
        // description strings (vendor/actus/README.md, quirk 1).
        let raw = include_str!("../../../vendor/actus/dictionary/actus-dictionary-terms.json")
            .replace(['\u{201c}', '\u{201d}'], "\"");
        let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let mut out = BTreeMap::new();
        if let Some(terms) = parsed.get("terms").and_then(Value::as_object) {
            for (id, term) in terms {
                let kind = match term.get("type").and_then(Value::as_str).unwrap_or("") {
                    "Real" => Kind::Real,
                    "Timestamp" => Kind::Timestamp,
                    "Cycle" => Kind::Cycle,
                    "Period" => Kind::Period,
                    "Enum" => Kind::Enum,
                    "Boolean" => Kind::Boolean,
                    "Real[]" => Kind::RealArray,
                    "Timestamp[]" => Kind::TimestampArray,
                    "Cycle[]" => Kind::CycleArray,
                    "Enum[]" => Kind::EnumArray,
                    "ContractReference[]" => Kind::ContractReferenceArray,
                    _ => Kind::Text,
                };
                let allowed = term
                    .get("allowedValues")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|v| v.get("acronym").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                out.insert(id.clone(), AttributeDef { kind, allowed });
            }
        }
        out
    })
}

/// The contract types with an engine, in a fixed order.
pub const ENGINE_TYPES: [ContractType; 18] = [
    ContractType::Pam,
    ContractType::Lam,
    ContractType::Lax,
    ContractType::Nam,
    ContractType::Ann,
    ContractType::Clm,
    ContractType::Ump,
    ContractType::Csh,
    ContractType::Stk,
    ContractType::Com,
    ContractType::Fxout,
    ContractType::Swppv,
    ContractType::Swaps,
    ContractType::Capfl,
    ContractType::Optns,
    ContractType::Futur,
    ContractType::Ceg,
    ContractType::Cec,
];

/// Every vendored official testbed (`vendor/actus/tests`), by contract type.
const TESTBEDS: [(&str, &str); 19] = [
    (
        "PAM",
        include_str!("../../../vendor/actus/tests/actus-tests-pam.json"),
    ),
    (
        "LAM",
        include_str!("../../../vendor/actus/tests/actus-tests-lam.json"),
    ),
    (
        "NAM",
        include_str!("../../../vendor/actus/tests/actus-tests-nam.json"),
    ),
    (
        "ANN",
        include_str!("../../../vendor/actus/tests/actus-tests-ann.json"),
    ),
    (
        "CLM",
        include_str!("../../../vendor/actus/tests/actus-tests-clm.json"),
    ),
    (
        "CSH",
        include_str!("../../../vendor/actus/tests/actus-tests-csh.json"),
    ),
    (
        "SWAPS",
        include_str!("../../../vendor/actus/tests/actus-tests-swaps.json"),
    ),
    (
        "CEC",
        include_str!("../../../vendor/actus/tests/actus-tests-cec.json"),
    ),
    (
        "STK",
        include_str!("../../../vendor/actus/tests/actus-tests-stk.json"),
    ),
    (
        "COM",
        include_str!("../../../vendor/actus/tests/actus-tests-com.json"),
    ),
    (
        "FXOUT",
        include_str!("../../../vendor/actus/tests/actus-tests-fxout.json"),
    ),
    (
        "SWPPV",
        include_str!("../../../vendor/actus/tests/actus-tests-swppv.json"),
    ),
    (
        "CAPFL",
        include_str!("../../../vendor/actus/tests/actus-tests-capfl.json"),
    ),
    (
        "OPTNS",
        include_str!("../../../vendor/actus/tests/actus-tests-optns.json"),
    ),
    (
        "FUTUR",
        include_str!("../../../vendor/actus/tests/actus-tests-futur.json"),
    ),
    (
        "CEG",
        include_str!("../../../vendor/actus/tests/actus-tests-ceg.json"),
    ),
    (
        "UMP",
        include_str!("../../../vendor/actus/tests/actus-tests-ump.json"),
    ),
    (
        "LAX",
        include_str!("../../../vendor/actus/tests/actus-tests-lax.json"),
    ),
    (
        "ANN",
        include_str!("../../../vendor/actus/tests/actus-tests-ad0.json"),
    ),
];

const HAND_WRITTEN: [&str; 11] = [
    include_str!("../seeds/capfl.json"),
    include_str!("../seeds/cec.json"),
    include_str!("../seeds/ceg.json"),
    include_str!("../seeds/com.json"),
    include_str!("../seeds/futur.json"),
    include_str!("../seeds/fxout.json"),
    include_str!("../seeds/lax.json"),
    include_str!("../seeds/optns.json"),
    include_str!("../seeds/stk.json"),
    include_str!("../seeds/swppv.json"),
    include_str!("../seeds/ump.json"),
];

/// One seed: terms, plus the scenario it was written with (if any).
#[derive(Debug, Clone)]
struct Seed {
    terms: Value,
    scenario: Option<Value>,
}

impl Seed {
    fn terms_only(terms: Value) -> Self {
        Seed {
            terms,
            scenario: None,
        }
    }
}

/// The hand-written seeds of `seeds/*.json` (`name`, `terms`, `scenario`).
fn hand_written() -> Vec<(String, Seed)> {
    let mut out = Vec::new();
    for raw in HAND_WRITTEN {
        let parsed: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        for entry in parsed.as_array().into_iter().flatten() {
            if let Some(terms) = entry.get("terms") {
                let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
                out.push((
                    name.to_string(),
                    Seed {
                        terms: terms.clone(),
                        scenario: entry.get("scenario").cloned(),
                    },
                ));
            }
        }
    }
    out
}

fn seed_type(terms: &Value) -> Option<ContractType> {
    terms.get("contractType")?.as_str()?.parse().ok()
}

/// Seeds per engine type: the testbed cases of that type, its Explorer
/// default terms and its hand-written seeds. Every engine type has at least
/// one seed.
fn seeds() -> &'static BTreeMap<ContractType, Vec<Seed>> {
    static SEEDS: OnceLock<BTreeMap<ContractType, Vec<Seed>>> = OnceLock::new();
    SEEDS.get_or_init(|| {
        let mut out: BTreeMap<ContractType, Vec<Seed>> = BTreeMap::new();
        for ct in ENGINE_TYPES {
            out.entry(ct)
                .or_default()
                .push(Seed::terms_only(actus_wasm::specs::default_terms_json(ct)));
        }
        for (_, seed) in hand_written() {
            if let Some(ct) = seed_type(&seed.terms) {
                out.entry(ct).or_default().push(seed);
            }
        }
        for (acronym, raw) in TESTBEDS {
            let Ok(ct) = acronym.parse::<ContractType>() else {
                continue;
            };
            let parsed: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
            if let Some(cases) = parsed.as_object() {
                for case in cases.values() {
                    if let Some(terms) = case.get("terms") {
                        out.entry(ct)
                            .or_default()
                            .push(Seed::terms_only(terms.clone()));
                    }
                }
            }
        }
        out
    })
}

/// Every seed with its scenario (`"{}"` when it has none), as JSON strings.
#[must_use]
pub fn seed_cases() -> Vec<Case> {
    ENGINE_TYPES
        .iter()
        .filter_map(|ct| seeds().get(ct))
        .flatten()
        .map(|seed| Case {
            terms: seed.terms.to_string(),
            scenario: seed
                .scenario
                .as_ref()
                .map_or_else(|| "{}".to_string(), Value::to_string),
        })
        .collect()
}

/// The variants of `terms` (a JSON object) that set one decimal attribute
/// (declared `Real` in the dictionary), at the top level or inside a
/// `contractStructure` child, to `value`.
#[must_use]
pub fn with_each_decimal_set_to(terms: &str, value: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(terms) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let real = |key: &str| dictionary().get(key).is_some_and(|d| d.kind == Kind::Real);
    if let Some(map) = parsed.as_object() {
        for key in map.keys().filter(|k| real(k)) {
            let mut variant = parsed.clone();
            variant[key.as_str()] = json!(value);
            out.push(variant.to_string());
        }
        if let Some(Value::Array(children)) = map.get("contractStructure") {
            for (i, child) in children.iter().enumerate() {
                let Some(object) = child.get("object").and_then(Value::as_object) else {
                    continue;
                };
                for key in object.keys().filter(|k| real(k)) {
                    let mut variant = parsed.clone();
                    variant["contractStructure"][i]["object"][key.as_str()] = json!(value);
                    out.push(variant.to_string());
                }
            }
        }
    }
    out
}

/// Generator inputs that replay every seed unmutated with its own scenario
/// ([`generate`] maps each back to that seed), for seeding the structured
/// fuzz corpus.
#[must_use]
pub fn seed_inputs() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (type_index, ct) in ENGINE_TYPES.iter().enumerate() {
        let count = seeds().get(ct).map_or(0, Vec::len);
        for seed_index in 0..count {
            // `below` reads two little-endian bytes per choice: the type,
            // the seed, zero mutations, then "keep the seed's scenario".
            let mut input = Vec::new();
            input.extend_from_slice(&u16::try_from(type_index).unwrap_or(0).to_le_bytes());
            input.extend_from_slice(&u16::try_from(seed_index).unwrap_or(0).to_le_bytes());
            input.extend_from_slice(&[0, 0, 0]);
            out.push(input);
        }
    }
    out
}

/// The raw seed terms, as JSON strings, for seeding fuzz corpora.
#[must_use]
pub fn seed_terms() -> Vec<String> {
    seeds()
        .values()
        .flatten()
        .map(|seed| seed.terms.to_string())
        .collect()
}

const DECIMALS: &[&str] = &[
    "0",
    "1",
    "-1",
    "0.5",
    "0.05",
    "-0.05",
    "100",
    "1000000",
    "1000000000000000",
    "1000000000000000000000",
    "79228162514264337593543950335",
    "-79228162514264337593543950335",
    "39614081257132168796771975167",
    "0.0000000000000000000000000001",
    "0.9999999999999999999999999999",
    "1000000000",
    "-1000000000",
    "1e10",
    "1E-7",
];

const DATES: &[&str] = &[
    "0001-01-01T00:00:00",
    "1900-02-28T00:00:00",
    "1970-01-01T00:00:00",
    "2000-02-29T00:00:00",
    "2012-12-31T23:59:59",
    "2013-01-31T00:00:00",
    "2013-02-28T00:00:00",
    "2016-02-29T12:00:00",
    "2020-06-30T00:00:00",
    "2099-12-31T00:00:00",
    "9999-12-31T23:59:59",
    "2013-01-01",
];

fn decimal(b: &mut Bytes<'_>) -> Value {
    match b.below(4) {
        0 | 1 => json!(*b.pick(DECIMALS)),
        2 => {
            // Random mantissa at a random scale (0..=28), random sign.
            let mantissa = b.u64();
            let scale = b.below(29);
            let digits = mantissa.to_string();
            let text = if scale == 0 {
                digits
            } else if digits.len() > scale {
                let (int, frac) = digits.split_at(digits.len() - scale);
                format!("{int}.{frac}")
            } else {
                format!("0.{}{digits}", "0".repeat(scale - digits.len()))
            };
            if b.u8() & 1 == 1 {
                json!(format!("-{text}"))
            } else {
                json!(text)
            }
        }
        _ => json!(f64::from(b.u32()) / 1000.0 - 1_000_000.0),
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn timestamp(b: &mut Bytes<'_>, near: Option<&str>) -> Value {
    match b.below(4) {
        0 => json!(*b.pick(DATES)),
        1 | 2 => {
            // A date near the seed's own dates: same century most of the time.
            let base_year = near
                .and_then(|s| s.get(0..4))
                .and_then(|y| y.parse::<u32>().ok())
                .unwrap_or(2013);
            let span = *b.pick(&[0u32, 1, 2, 5, 30, 300]);
            let offset = if span == 0 {
                0
            } else {
                b.u32() % (2 * span + 1)
            };
            let year = (base_year + offset).saturating_sub(span).clamp(1, 9999);
            let month = 1 + b.u32() % 12;
            let day = 1 + b.u32() % days_in_month(year, month);
            let hour = if b.below(8) == 0 { b.u32() % 24 } else { 0 };
            json!(format!("{year:04}-{month:02}-{day:02}T{hour:02}:00:00"))
        }
        _ => {
            let year = 1 + b.u32() % 9999;
            let month = 1 + b.u32() % 12;
            let day = 1 + b.u32() % days_in_month(year, month);
            json!(format!("{year:04}-{month:02}-{day:02}T00:00:00"))
        }
    }
}

fn cycle(b: &mut Bytes<'_>) -> Value {
    let length = match b.below(6) {
        0..=2 => *b.pick(&[1u32, 2, 3, 6, 12]),
        3 => *b.pick(&[0u32, 1000, 99_999, u32::MAX]),
        _ => b.u32() % 400,
    };
    let unit = *b.pick(&['D', 'W', 'M', 'Q', 'H', 'Y']);
    let stub = *b.pick(&["L0", "L1", "R0", "R1", "U0", "", "L9"]);
    json!(format!("P{length}{unit}{stub}"))
}

fn period(b: &mut Bytes<'_>) -> Value {
    let length = *b.pick(&[0u32, 1, 2, 5, 30, 365, 99_999]);
    let unit = *b.pick(&['D', 'W', 'M', 'Y']);
    json!(format!("P{length}{unit}"))
}

fn text(b: &mut Bytes<'_>) -> Value {
    json!(*b.pick(&["", "X", "USD", "EUR", "CHF", "RATE", "GOLD", "IDX", "USD/EUR"]))
}

fn enum_value(b: &mut Bytes<'_>, def: &AttributeDef) -> Value {
    if def.allowed.is_empty() || b.below(16) == 0 {
        text(b)
    } else {
        json!(b.pick(&def.allowed).clone())
    }
}

fn array_len(b: &mut Bytes<'_>) -> usize {
    match b.below(8) {
        0 => 0,
        7 => 64,
        n => n,
    }
}

fn value_for(b: &mut Bytes<'_>, def: &AttributeDef, near: Option<&str>) -> Value {
    // One mutation in 32 produces a value of the wrong JSON type.
    if b.below(32) == 0 {
        return match b.below(4) {
            0 => Value::Null,
            1 => json!(true),
            2 => json!([]),
            _ => json!({}),
        };
    }
    match def.kind {
        Kind::Real => decimal(b),
        Kind::Timestamp => timestamp(b, near),
        Kind::Cycle => cycle(b),
        Kind::Period => period(b),
        Kind::Enum => enum_value(b, def),
        Kind::Text => text(b),
        Kind::Boolean => json!(b.u8() & 1 == 1),
        Kind::RealArray => Value::Array((0..array_len(b)).map(|_| decimal(b)).collect()),
        Kind::TimestampArray => {
            Value::Array((0..array_len(b)).map(|_| timestamp(b, near)).collect())
        }
        Kind::CycleArray => Value::Array((0..array_len(b)).map(|_| cycle(b)).collect()),
        Kind::EnumArray => Value::Array((0..array_len(b)).map(|_| enum_value(b, def)).collect()),
        Kind::ContractReferenceArray => Value::Array(Vec::new()),
    }
}

/// Some date already in the terms, to generate realistic nearby dates.
fn anchor_date(terms: &Map<String, Value>) -> Option<String> {
    ["statusDate", "initialExchangeDate", "maturityDate"]
        .iter()
        .find_map(|k| terms.get(*k).and_then(Value::as_str).map(str::to_string))
}

fn mutate(b: &mut Bytes<'_>, terms: &mut Map<String, Value>) {
    let dict = dictionary();
    let near = anchor_date(terms);
    let op = b.below(10);
    if op == 0 && !terms.is_empty() {
        // Remove an existing attribute.
        let keys: Vec<String> = terms.keys().cloned().collect();
        let key = b.pick(&keys).clone();
        if key != "contractType" {
            terms.remove(&key);
        }
        return;
    }
    if op == 1 {
        if let Some(Value::Array(children)) = terms.get_mut("contractStructure") {
            // Mutate, drop or duplicate one child contract.
            if children.is_empty() {
                return;
            }
            let i = b.below(children.len());
            match b.below(4) {
                0 => {
                    children.remove(i);
                }
                1 => {
                    let copy = children[i].clone();
                    children.push(copy);
                }
                _ => {
                    if let Some(Value::Object(child)) = children[i].get_mut("object") {
                        mutate(b, child);
                    }
                }
            }
            return;
        }
    }
    // Set an attribute: mostly one already present, sometimes any
    // dictionary attribute.
    let key = if op < 7 && !terms.is_empty() {
        let keys: Vec<String> = terms.keys().cloned().collect();
        b.pick(&keys).clone()
    } else {
        let keys: Vec<&String> = dict.keys().collect();
        (*b.pick(&keys)).clone()
    };
    if key == "contractType" || key == "contractStructure" {
        return;
    }
    let def = dict.get(&key).cloned().unwrap_or(AttributeDef {
        kind: Kind::Text,
        allowed: Vec::new(),
    });
    let value = value_for(b, &def, near.as_deref());
    terms.insert(key, value);
}

/// Market object codes and currency pairs the terms (and children) mention.
fn risk_codes(terms: &Value, out: &mut Vec<String>) {
    let Some(map) = terms.as_object() else {
        return;
    };
    for (key, value) in map {
        if key.starts_with("marketObjectCode") || key == "settlementCurrency" {
            if let Some(code) = value.as_str() {
                out.push(code.to_string());
            }
        }
    }
    if let (Some(c1), Some(c2)) = (
        map.get("currency").and_then(Value::as_str),
        map.get("currency2").and_then(Value::as_str),
    ) {
        out.push(format!("{c2}/{c1}"));
        out.push(format!("{c1}/{c2}"));
    }
    if let Some(Value::Array(children)) = map.get("contractStructure") {
        for child in children {
            if let Some(object) = child.get("object") {
                risk_codes(object, out);
            }
        }
    }
}

fn series(b: &mut Bytes<'_>, near: Option<&str>) -> Value {
    let mut points = Map::new();
    for _ in 0..b.below(6) {
        if let Value::String(t) = timestamp(b, near) {
            points.insert(t, decimal(b));
        }
    }
    Value::Object(points)
}

fn scenario(b: &mut Bytes<'_>, terms: &Value) -> Value {
    let mut codes = Vec::new();
    risk_codes(terms, &mut codes);
    codes.sort();
    codes.dedup();
    let near = terms.as_object().and_then(anchor_date);
    let mut rates = Map::new();
    let mut fx = Map::new();
    let mut prices = Map::new();
    for code in codes {
        let target = match b.below(4) {
            0 => &mut rates,
            1 => &mut fx,
            2 => &mut prices,
            _ => continue,
        };
        target.insert(code, series(b, near.as_deref()));
    }
    let mut observed = Vec::new();
    for _ in 0..b.below(3) {
        observed.push(json!({
            "eventType": *b.pick(&["PP", "AD", "XD", "CE", "TD", "XX"]),
            "time": timestamp(b, near.as_deref()),
            "payoff": decimal(b),
        }));
    }
    // Credit events name a child contract (CEG/CEC trigger only on events
    // naming a covered contract) or nothing.
    let mut child_ids = vec![Value::Null];
    if let Some(Value::Array(children)) = terms.get("contractStructure") {
        for child in children {
            if let Some(id) = child.get("object").and_then(|o| o.get("contractID")) {
                child_ids.push(id.clone());
            }
        }
    }
    let mut credit = Vec::new();
    for _ in 0..b.below(3) {
        credit.push(json!({
            "time": timestamp(b, near.as_deref()),
            "performance": *b.pick(&["PF", "DL", "DQ", "DF", "MA", "TE", "XX"]),
            "contractId": b.pick(&child_ids).clone(),
        }));
    }
    json!({
        "rates": rates,
        "fxRates": fx,
        "unitPrices": prices,
        "observedEvents": observed,
        "creditEvents": credit,
    })
}

/// Generates one case from a byte string.
#[must_use]
pub fn generate(data: &[u8]) -> Case {
    let mut b = Bytes::new(data);
    let ct = *b.pick(&ENGINE_TYPES);
    let seed_list = seeds().get(&ct).cloned().unwrap_or_default();
    let seed = if seed_list.is_empty() {
        Seed::terms_only(json!({ "contractType": ct.as_acronym() }))
    } else {
        b.pick(&seed_list).clone()
    };
    let mut terms = seed.terms;
    if let Value::Object(map) = &mut terms {
        for _ in 0..b.below(9) {
            mutate(&mut b, map);
        }
    }
    // Half the time keep the seed's own scenario (realistic observations for
    // the mutated terms), otherwise generate one from the terms' codes.
    let scenario = match seed.scenario {
        Some(own) if b.u8() & 1 == 0 => own,
        _ => scenario(&mut b, &terms),
    };
    Case {
        terms: terms.to_string(),
        scenario: scenario.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every vendored testbed file seeds the generator.
    #[test]
    fn every_vendored_testbed_is_a_seed_source() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/actus/tests");
        let files = std::fs::read_dir(dir)
            .expect("vendored testbeds")
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "json"))
            })
            .count();
        assert_eq!(TESTBEDS.len(), files);
    }

    #[test]
    fn every_engine_type_has_a_seed() {
        for ct in ENGINE_TYPES {
            assert!(!seeds()[&ct].is_empty(), "{ct:?}");
        }
        assert!(seed_terms().len() > ENGINE_TYPES.len());
    }

    #[test]
    fn hand_written_seeds_evaluate() {
        let seeds = hand_written();
        assert!(seeds.len() >= 40, "{} seeds", seeds.len());
        for (name, seed) in seeds {
            let scenario = seed.scenario.unwrap_or_else(|| json!({}));
            let result = actus_wasm::dto::evaluate_json_with_risk(
                &seed.terms.to_string(),
                &scenario.to_string(),
            )
            .unwrap_or_else(|e| panic!("seed {name}: {e}"));
            let parsed: Value = serde_json::from_str(&result).expect("result JSON");
            let events = parsed["events"].as_array().expect("events");
            assert!(!events.is_empty(), "seed {name} produced no events");
        }
    }

    #[test]
    fn seed_inputs_replay_every_seed() {
        let inputs = seed_inputs();
        let cases = seed_cases();
        assert_eq!(inputs.len(), cases.len());
        for (input, case) in inputs.iter().zip(&cases) {
            let replayed = generate(input);
            let expected: Value = serde_json::from_str(&case.terms).expect("seed terms");
            let actual: Value = serde_json::from_str(&replayed.terms).expect("generated terms");
            assert_eq!(actual, expected);
            if case.scenario != "{}" {
                assert_eq!(replayed.scenario, case.scenario);
            }
        }
    }

    #[test]
    fn dictionary_loads_with_types_and_enum_values() {
        let dict = dictionary();
        assert!(dict.len() > 100);
        assert_eq!(dict["notionalPrincipal"].kind, Kind::Real);
        assert!(dict["dayCountConvention"]
            .allowed
            .contains(&"A365".to_string()));
    }

    #[test]
    fn generation_is_deterministic_and_total() {
        for data in [&b""[..], &[0xff; 3], &[7; 300]] {
            let a = generate(data);
            let b = generate(data);
            assert_eq!(a.terms, b.terms);
            assert_eq!(a.scenario, b.scenario);
            assert!(serde_json::from_str::<Value>(&a.terms).is_ok());
        }
    }
}
