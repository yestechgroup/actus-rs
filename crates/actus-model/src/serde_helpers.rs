//! Serde helpers for ACTUS wire formats.
//!
//! Testbed values arrive as strings, JSON integers or JSON floats; dates use
//! minute or second precision and sometimes bare-day precision. These helper
//! modules are wired in via `#[serde(deserialize_with = ...)]` on the
//! `Option<T>` fields of [`crate::terms::ContractTerms`].

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::de::Error as DeError;
use serde::de::{Deserializer, Unexpected, Visitor};
use serde::ser::Serializer;
use std::fmt;

/// Parse an ACTUS decimal wire value (string, integer or float).
///
/// Strings are trimmed first; scientific notation falls back to
/// [`Decimal::from_scientific`] (`30E360`-style enum tokens never reach this
/// helper because enum fields use their own deserializers).
pub fn parse_decimal(raw: &str) -> Result<Decimal, rust_decimal::Error> {
    let s = raw.trim();
    s.parse::<Decimal>()
        .or_else(|_| Decimal::from_scientific(s))
}

/// Parse a JSON value (string, number or null) as an ACTUS decimal.
pub fn decimal_from_value(
    value: &serde_json::Value,
) -> Result<Option<Decimal>, crate::error::ModelError> {
    match value {
        serde_json::Value::String(s) => parse_decimal(s)
            .map(Some)
            .map_err(|_| crate::error::ModelError::InvalidDecimal(s.clone())),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Some(Decimal::from(i)))
            } else if let Some(u) = n.as_u64() {
                Ok(Some(Decimal::from(u)))
            } else {
                Decimal::try_from(n.as_f64().unwrap_or_default())
                    .map(Some)
                    .map_err(|e| crate::error::ModelError::InvalidDecimal(e.to_string()))
            }
        }
        serde_json::Value::Null => Ok(None),
        other => Err(crate::error::ModelError::InvalidDecimal(other.to_string())),
    }
}

/// Parse a JSON string as an ACTUS timestamp.
pub fn timestamp_from_value(
    value: &serde_json::Value,
) -> Result<Option<NaiveDateTime>, crate::error::ModelError> {
    match value {
        serde_json::Value::String(s) => parse_timestamp(s)
            .map(Some)
            .map_err(|_| crate::error::ModelError::InvalidDate(s.clone())),
        serde_json::Value::Null => Ok(None),
        other => Err(crate::error::ModelError::InvalidDate(other.to_string())),
    }
}

/// Parse an ACTUS timestamp in any of the three accepted shapes.
pub fn parse_timestamp(raw: &str) -> Result<NaiveDateTime, chrono::format::ParseError> {
    let s = raw.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Ok(dt);
    }
    let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    Ok(date.and_hms_opt(0, 0, 0).expect("midnight is a valid time"))
}

/// Serialize a timestamp in the canonical second-precision ACTUS form.
pub fn timestamp_to_string(value: &NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Deserialize `Option<Decimal>` from a JSON string, integer, float or null.
pub mod decimal_option {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Decimal>, D::Error> {
        struct DecimalOptionVisitor;

        impl<'de> Visitor<'de> for DecimalOptionVisitor {
            type Value = Option<Decimal>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an ACTUS decimal (string, number or null)")
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<Option<Decimal>, E> {
                parse_decimal(v)
                    .map(Some)
                    .map_err(|_| DeError::invalid_value(Unexpected::Str(v), &"a decimal value"))
            }

            fn visit_f64<E: DeError>(self, v: f64) -> Result<Option<Decimal>, E> {
                Decimal::try_from(v).map(Some).map_err(DeError::custom)
            }

            fn visit_i64<E: DeError>(self, v: i64) -> Result<Option<Decimal>, E> {
                Ok(Some(Decimal::from(v)))
            }

            fn visit_u64<E: DeError>(self, v: u64) -> Result<Option<Decimal>, E> {
                Ok(Some(Decimal::from(v)))
            }

            fn visit_unit<E: DeError>(self) -> Result<Option<Decimal>, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Option<Decimal>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(DecimalOptionVisitor)
    }

    pub fn serialize<S: Serializer>(
        value: &Option<Decimal>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_str(&v.to_string()),
            None => serializer.serialize_none(),
        }
    }
}

/// Deserialize `Option<String>` from a JSON string or null.
pub mod string_option {
    use serde::de::{Deserializer, Visitor};
    use std::fmt;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<String>, D::Error> {
        struct StringOptionVisitor;

        impl<'de> Visitor<'de> for StringOptionVisitor {
            type Value = Option<String>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a string or null")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Option<String>, E> {
                Ok(Some(v.to_string()))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Option<String>, E> {
                Ok(None)
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Option<String>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(StringOptionVisitor)
    }
}

/// Deserialize `Option<Vec<String>>` from a JSON array of strings or null.
pub mod string_vec_option {
    use serde::de::{Deserializer, Error as DeError, Visitor};
    use std::fmt;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<String>>, D::Error> {
        struct StringVecOptionVisitor;

        impl<'de> Visitor<'de> for StringVecOptionVisitor {
            type Value = Option<Vec<String>>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an array of strings or null")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Option<Vec<String>>, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(item) = seq.next_element::<String>()? {
                    out.push(item);
                }
                Ok(Some(out))
            }

            fn visit_unit<E: DeError>(self) -> Result<Option<Vec<String>>, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Option<Vec<String>>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(StringVecOptionVisitor)
    }
}

/// Deserialize `Option<NaiveDateTime>` from a JSON string or null.
pub mod timestamp_option {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<NaiveDateTime>, D::Error> {
        struct TimestampOptionVisitor;

        impl<'de> Visitor<'de> for TimestampOptionVisitor {
            type Value = Option<NaiveDateTime>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an ACTUS timestamp (ISO 8601 date or datetime)")
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<Option<NaiveDateTime>, E> {
                parse_timestamp(v).map(Some).map_err(|_| {
                    DeError::invalid_value(Unexpected::Str(v), &"an ISO 8601 timestamp")
                })
            }

            fn visit_unit<E: DeError>(self) -> Result<Option<NaiveDateTime>, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Option<NaiveDateTime>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(TimestampOptionVisitor)
    }

    pub fn serialize<S: Serializer>(
        value: &Option<NaiveDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_str(&timestamp_to_string(v)),
            None => serializer.serialize_none(),
        }
    }
}

/// Serialize a plain decimal as its canonical string form.
pub fn decimal_to_string(value: &Decimal) -> String {
    value.to_string()
}
