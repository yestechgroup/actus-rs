//! Wire-format tests for `actus_model::serde_helpers`: every accepted and
//! rejected input shape of the decimal, timestamp, string and array
//! helpers, plus serialize/deserialize round trips.
//!
//! These helpers are the boundary where untrusted JSON (testbeds, WASM
//! callers) becomes typed contract terms, so each branch is pinned here.

use std::str::FromStr;

use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use actus_model::serde_helpers::{
    decimal_from_value, decimal_option, decimal_to_string, decimal_vec_option, parse_decimal,
    parse_timestamp, string_option, string_vec_option, timestamp_from_value, timestamp_option,
    timestamp_to_string, timestamp_vec_option,
};
use actus_model::ModelError;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).expect("decimal literal")
}

fn ts(y: i32, m: u32, day: u32, h: u32, min: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, day)
        .and_then(|date| date.and_hms_opt(h, min, s))
        .expect("valid timestamp")
}

/// A struct wiring every helper module onto a field, as `ContractTerms`
/// does. `#[serde(default)]` mirrors the optional attributes.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Wire {
    #[serde(
        default,
        with = "decimal_option",
        skip_serializing_if = "Option::is_none"
    )]
    decimal: Option<Decimal>,
    #[serde(
        default,
        with = "timestamp_option",
        skip_serializing_if = "Option::is_none"
    )]
    timestamp: Option<NaiveDateTime>,
    #[serde(
        default,
        deserialize_with = "string_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    string: Option<String>,
    #[serde(
        default,
        deserialize_with = "string_vec_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    strings: Option<Vec<String>>,
    #[serde(
        default,
        with = "timestamp_vec_option",
        skip_serializing_if = "Option::is_none"
    )]
    timestamps: Option<Vec<NaiveDateTime>>,
    #[serde(
        default,
        with = "decimal_vec_option",
        skip_serializing_if = "Option::is_none"
    )]
    decimals: Option<Vec<Decimal>>,
}

fn wire(value: Value) -> Result<Wire, serde_json::Error> {
    serde_json::from_value(value)
}

// ---------------------------------------------------------------- decimals

#[test]
fn parse_decimal_accepts_plain_trimmed_and_scientific_forms() {
    assert_eq!(parse_decimal("0.05").unwrap(), d("0.05"));
    assert_eq!(parse_decimal("  1000  ").unwrap(), d("1000"));
    assert_eq!(parse_decimal("-12.5").unwrap(), d("-12.5"));
    assert_eq!(parse_decimal("1e-3").unwrap(), d("0.001"));
    assert_eq!(parse_decimal("2.5E2").unwrap(), d("250"));
}

#[test]
fn parse_decimal_rejects_non_numbers() {
    for raw in ["", "abc", "1,000", "0.05%", "NaN"] {
        assert!(parse_decimal(raw).is_err(), "{raw:?} should be rejected");
    }
}

#[test]
fn decimal_from_value_covers_every_json_shape() {
    assert_eq!(decimal_from_value(&json!("0.07")).unwrap(), Some(d("0.07")));
    assert_eq!(decimal_from_value(&json!(-3)).unwrap(), Some(d("-3")));
    assert_eq!(
        decimal_from_value(&json!(u64::MAX)).unwrap(),
        Some(Decimal::from(u64::MAX))
    );
    assert_eq!(decimal_from_value(&json!(0.25)).unwrap(), Some(d("0.25")));
    assert_eq!(decimal_from_value(&Value::Null).unwrap(), None);
    assert!(matches!(
        decimal_from_value(&json!("x")),
        Err(ModelError::InvalidDecimal(raw)) if raw == "x"
    ));
    for other in [json!(true), json!([1]), json!({"v": 1})] {
        assert!(
            matches!(
                decimal_from_value(&other),
                Err(ModelError::InvalidDecimal(_))
            ),
            "{other}"
        );
    }
}

#[test]
fn decimal_option_deserializes_strings_integers_floats_and_null() {
    assert_eq!(
        wire(json!({"decimal": "1.5"})).unwrap().decimal,
        Some(d("1.5"))
    );
    assert_eq!(wire(json!({"decimal": 42})).unwrap().decimal, Some(d("42")));
    assert_eq!(
        wire(json!({"decimal": -42})).unwrap().decimal,
        Some(d("-42"))
    );
    assert_eq!(
        wire(json!({"decimal": 0.1})).unwrap().decimal,
        Some(d("0.1"))
    );
    assert_eq!(wire(json!({"decimal": null})).unwrap().decimal, None);
    assert_eq!(wire(json!({})).unwrap().decimal, None);
}

#[test]
fn decimal_option_rejects_bad_values_with_a_useful_message() {
    let err = wire(json!({"decimal": "ten"})).unwrap_err().to_string();
    assert!(
        err.contains("ten") && err.contains("a decimal value"),
        "{err}"
    );
    let err = wire(json!({"decimal": true})).unwrap_err().to_string();
    assert!(err.contains("an ACTUS decimal"), "{err}");
    assert!(wire(json!({"decimal": [1]})).is_err());
}

#[test]
fn decimals_serialize_as_strings_to_avoid_float_loss() {
    let value = serde_json::to_value(Wire {
        decimal: Some(d("0.1000000000000000000000000001")),
        ..Wire::default()
    })
    .unwrap();
    assert_eq!(value, json!({"decimal": "0.1000000000000000000000000001"}));
    assert_eq!(decimal_to_string(&d("-7.25")), "-7.25");
}

// -------------------------------------------------------------- timestamps

#[test]
fn parse_timestamp_accepts_second_minute_and_day_precision() {
    assert_eq!(
        parse_timestamp("2013-01-01T12:30:45").unwrap(),
        ts(2013, 1, 1, 12, 30, 45)
    );
    assert_eq!(
        parse_timestamp("2013-01-01T12:30").unwrap(),
        ts(2013, 1, 1, 12, 30, 0)
    );
    assert_eq!(
        parse_timestamp("2013-01-01").unwrap(),
        ts(2013, 1, 1, 0, 0, 0)
    );
    assert_eq!(
        parse_timestamp("  2024-02-29  ").unwrap(),
        ts(2024, 2, 29, 0, 0, 0)
    );
}

#[test]
fn parse_timestamp_rejects_malformed_and_impossible_dates() {
    for raw in [
        "",
        "2013/01/01",
        "01-01-2013",
        "2013-13-01",
        "2013-02-30",
        "2023-02-29",
        "2013-01-01T25:00:00",
        "2013-01-01T00:00:00Z",
        "yesterday",
    ] {
        assert!(parse_timestamp(raw).is_err(), "{raw:?} should be rejected");
    }
}

#[test]
fn timestamp_from_value_covers_every_json_shape() {
    assert_eq!(
        timestamp_from_value(&json!("2015-06-01")).unwrap(),
        Some(ts(2015, 6, 1, 0, 0, 0))
    );
    assert_eq!(timestamp_from_value(&Value::Null).unwrap(), None);
    assert!(matches!(
        timestamp_from_value(&json!("2015-06-31")),
        Err(ModelError::InvalidDate(raw)) if raw == "2015-06-31"
    ));
    assert!(matches!(
        timestamp_from_value(&json!(20150601)),
        Err(ModelError::InvalidDate(_))
    ));
}

#[test]
fn timestamp_option_deserializes_and_rejects() {
    assert_eq!(
        wire(json!({"timestamp": "2013-01-01T00:00"}))
            .unwrap()
            .timestamp,
        Some(ts(2013, 1, 1, 0, 0, 0))
    );
    assert_eq!(wire(json!({"timestamp": null})).unwrap().timestamp, None);
    let err = wire(json!({"timestamp": "soon"})).unwrap_err().to_string();
    assert!(err.contains("an ISO 8601 timestamp"), "{err}");
    let err = wire(json!({"timestamp": 1})).unwrap_err().to_string();
    assert!(err.contains("an ACTUS timestamp"), "{err}");
}

#[test]
fn timestamps_serialize_in_canonical_second_precision() {
    assert_eq!(
        timestamp_to_string(&ts(2013, 1, 1, 0, 0, 0)),
        "2013-01-01T00:00:00"
    );
    let value = serde_json::to_value(Wire {
        timestamp: Some(ts(2013, 1, 1, 9, 5, 0)),
        ..Wire::default()
    })
    .unwrap();
    assert_eq!(value, json!({"timestamp": "2013-01-01T09:05:00"}));
}

// ----------------------------------------------------------------- strings

#[test]
fn string_option_accepts_strings_and_null_only() {
    assert_eq!(
        wire(json!({"string": "USD"})).unwrap().string.as_deref(),
        Some("USD")
    );
    assert_eq!(wire(json!({"string": null})).unwrap().string, None);
    let err = wire(json!({"string": 5})).unwrap_err().to_string();
    assert!(err.contains("a string or null"), "{err}");
}

#[test]
fn string_vec_option_accepts_string_arrays_and_null_only() {
    assert_eq!(
        wire(json!({"strings": ["INC", "DEC"]})).unwrap().strings,
        Some(vec!["INC".to_string(), "DEC".to_string()])
    );
    assert_eq!(wire(json!({"strings": []})).unwrap().strings, Some(vec![]));
    assert_eq!(wire(json!({"strings": null})).unwrap().strings, None);
    assert!(wire(json!({"strings": [1, 2]})).is_err());
    let err = wire(json!({"strings": "INC"})).unwrap_err().to_string();
    assert!(err.contains("an array of strings or null"), "{err}");
}

// ------------------------------------------------------------------ arrays

#[test]
fn timestamp_vec_option_parses_each_element() {
    assert_eq!(
        wire(json!({"timestamps": ["2013-01-01", "2014-01-01T00:00:00"]}))
            .unwrap()
            .timestamps,
        Some(vec![ts(2013, 1, 1, 0, 0, 0), ts(2014, 1, 1, 0, 0, 0)])
    );
    assert_eq!(wire(json!({"timestamps": null})).unwrap().timestamps, None);
    let err = wire(json!({"timestamps": ["2013-01-01", "nope"]}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("nope"), "{err}");
    assert!(wire(json!({"timestamps": [20130101]})).is_err());
    let err = wire(json!({"timestamps": "2013-01-01"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("an array of ACTUS timestamps"), "{err}");
}

#[test]
fn decimal_vec_option_parses_mixed_numeric_elements() {
    assert_eq!(
        wire(json!({"decimals": ["0.01", 2, 0.5, "1e2"]}))
            .unwrap()
            .decimals,
        Some(vec![d("0.01"), d("2"), d("0.5"), d("100")])
    );
    assert_eq!(wire(json!({"decimals": null})).unwrap().decimals, None);
    let err = wire(json!({"decimals": ["0.01", "bad"]}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("bad"), "{err}");
    let err = wire(json!({"decimals": 0.01})).unwrap_err().to_string();
    assert!(err.contains("an array of ACTUS decimals"), "{err}");
}

// -------------------------------------------------------------- round trip

#[test]
fn every_serializing_helper_round_trips() {
    let original = Wire {
        decimal: Some(d("1234.5678")),
        timestamp: Some(ts(2020, 2, 29, 23, 59, 59)),
        string: None,
        strings: None,
        timestamps: Some(vec![ts(2020, 1, 31, 0, 0, 0), ts(2020, 2, 29, 0, 0, 0)]),
        decimals: Some(vec![d("0.015"), d("-1"), d("0")]),
    };
    let encoded = serde_json::to_value(&original).unwrap();
    assert_eq!(
        encoded,
        json!({
            "decimal": "1234.5678",
            "timestamp": "2020-02-29T23:59:59",
            "timestamps": ["2020-01-31T00:00:00", "2020-02-29T00:00:00"],
            "decimals": ["0.015", "-1", "0"]
        })
    );
    assert_eq!(wire(encoded).unwrap(), original);
}
