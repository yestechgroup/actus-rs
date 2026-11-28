//! Serde helpers for ACTUS wire formats.
//!
//! Testbed values arrive as strings, JSON integers or JSON floats; dates use
//! minute or second precision and sometimes bare-day precision. These helper
//! modules are wired in via `#[serde(deserialize_with = ...)]` on the
//! `Option<T>` fields of [`crate::terms::ContractTerms`].
//!
//! # Singleton arrays
//!
//! The dictionary types the `array*` attributes as arrays (`Timestamp[]`,
//! `Cycle[]`, `Real[]`, `Enum[]`), one element per schedule segment. The
//! official LAX testbed writes a one-segment array as a bare scalar
//! (`"arrayCycleOfPrincipalRedemption": "P1YL1"`,
//! `"arrayNextPrincipalRedemptionPayment": 20`) in 14 of its 18 cases, and
//! mixes both shapes in one bag (lax16). Every array helper here therefore
//! reads a bare scalar as a one-element array and serializes the canonical
//! array form (a testbed encoding quirk, `vendor/actus/README.md`).

use chrono::{Datelike, NaiveDateTime, NaiveTime};
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

/// The years an ACTUS timestamp may carry: ISO 8601 four-digit years.
///
/// Bounding input dates keeps every date the engine derives from them (at
/// most one cycle past the latest input date) far inside the representable
/// calendar, so schedule arithmetic never meets the calendar's edge.
pub const YEAR_RANGE: std::ops::RangeInclusive<i32> = 1..=9999;

/// Parse an ACTUS timestamp in any of the three accepted shapes.
///
/// # Errors
/// The text matches none of the shapes, or the year is outside
/// [`YEAR_RANGE`] (signed or zero years such as `-0001` or `+262142`).
pub fn parse_timestamp(raw: &str) -> Result<NaiveDateTime, TimestampError> {
    let s = raw.trim();
    let parsed = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_time(NaiveTime::MIN))
        })
        .map_err(TimestampError::Format)?;
    if !YEAR_RANGE.contains(&parsed.year()) {
        return Err(TimestampError::YearOutOfRange(parsed.year()));
    }
    Ok(parsed)
}

/// Why a timestamp was rejected by [`parse_timestamp`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimestampError {
    /// Not an ACTUS timestamp shape.
    #[error("{0}")]
    Format(chrono::format::ParseError),
    /// A well-formed date whose year is outside [`YEAR_RANGE`].
    #[error("year {0} is outside 1..=9999")]
    YearOutOfRange(i32),
}

/// Serialize a timestamp in the canonical second-precision ACTUS form.
pub fn timestamp_to_string(value: &NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Deserialize `Option<Decimal>` from a JSON string, integer, float or null.
pub mod decimal_option {
    use super::*;

    /// Serde `deserialize_with` entry point of [`decimal_option`](self).
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

    /// Serde `serialize_with` entry point of [`decimal_option`](self).
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

    /// Serde `deserialize_with` entry point of [`string_option`](self).
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

    /// Serde `deserialize_with` entry point of [`string_vec_option`](self).
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

    /// Serde `deserialize_with` entry point of [`timestamp_option`](self).
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

    /// Serde `serialize_with` entry point of [`timestamp_option`](self).
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

/// Deserialize `Option<Vec<NaiveDateTime>>` from a JSON array of ACTUS
/// timestamp strings or null; serialize back to the canonical
/// second-precision strings. Array-valued attributes (`ARIPANX`, `ARPRANX`,
/// `ARRRANX`) use this helper.
pub mod timestamp_vec_option {
    use super::*;
    use serde::de::Error as DeError;

    /// Serde `deserialize_with` entry point of [`timestamp_vec_option`](self).
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<NaiveDateTime>>, D::Error> {
        struct TimestampVecOptionVisitor;

        impl<'de> Visitor<'de> for TimestampVecOptionVisitor {
            type Value = Option<Vec<NaiveDateTime>>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    f,
                    "an array of ACTUS timestamps, a single timestamp, or null"
                )
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Option<Vec<NaiveDateTime>>, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(raw) = seq.next_element::<String>()? {
                    let timestamp = parse_timestamp(&raw).map_err(|_| {
                        DeError::invalid_value(Unexpected::Str(&raw), &"an ISO 8601 timestamp")
                    })?;
                    out.push(timestamp);
                }
                Ok(Some(out))
            }

            /// A bare timestamp is a one-element array (SINGLETON_ARRAYS).
            fn visit_str<E: DeError>(self, v: &str) -> Result<Option<Vec<NaiveDateTime>>, E> {
                let timestamp = parse_timestamp(v).map_err(|_| {
                    DeError::invalid_value(Unexpected::Str(v), &"an ISO 8601 timestamp")
                })?;
                Ok(Some(vec![timestamp]))
            }

            fn visit_unit<E: DeError>(self) -> Result<Option<Vec<NaiveDateTime>>, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Option<Vec<NaiveDateTime>>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(TimestampVecOptionVisitor)
    }

    /// Serde `serialize_with` entry point of [`timestamp_vec_option`](self).
    pub fn serialize<S: Serializer>(
        value: &Option<Vec<NaiveDateTime>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.collect_seq(v.iter().map(timestamp_to_string)),
            None => serializer.serialize_none(),
        }
    }
}

/// Deserialize `Option<Vec<Decimal>>` from a JSON array of ACTUS decimals
/// (strings or numbers) or null; serialize back to decimal strings.
/// Array-valued attributes (`ARPRNXT`, `ARRATE`) use this helper. A `null`
/// element is rejected: the arrays are positional, one value per segment.
pub mod decimal_vec_option {
    use super::*;
    use serde::de::Error as DeError;

    /// Serde `deserialize_with` entry point of [`decimal_vec_option`](self).
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<Decimal>>, D::Error> {
        struct DecimalVecOptionVisitor;

        impl<'de> Visitor<'de> for DecimalVecOptionVisitor {
            type Value = Option<Vec<Decimal>>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an array of ACTUS decimals, a single decimal, or null")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Option<Vec<Decimal>>, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(raw) = seq.next_element::<serde_json::Value>()? {
                    // Array attributes are positional: element `i` belongs to
                    // schedule segment `i`. A missing element cannot be
                    // dropped (every later value would move one segment
                    // earlier), so `null` is an error.
                    match decimal_from_value(&raw) {
                        Ok(Some(value)) => out.push(value),
                        Ok(None) => {
                            return Err(DeError::custom(format!(
                                "null element at index {} of a decimal array; \
                                 array attributes need one value per segment",
                                out.len()
                            )))
                        }
                        Err(e) => return Err(DeError::custom(e.to_string())),
                    }
                }
                Ok(Some(out))
            }

            /// A bare decimal is a one-element array (SINGLETON_ARRAYS).
            fn visit_str<E: DeError>(self, v: &str) -> Result<Option<Vec<Decimal>>, E> {
                parse_decimal(v)
                    .map(|d| Some(vec![d]))
                    .map_err(|_| DeError::invalid_value(Unexpected::Str(v), &"a decimal value"))
            }

            fn visit_f64<E: DeError>(self, v: f64) -> Result<Option<Vec<Decimal>>, E> {
                Decimal::try_from(v)
                    .map(|d| Some(vec![d]))
                    .map_err(DeError::custom)
            }

            fn visit_i64<E: DeError>(self, v: i64) -> Result<Option<Vec<Decimal>>, E> {
                Ok(Some(vec![Decimal::from(v)]))
            }

            fn visit_u64<E: DeError>(self, v: u64) -> Result<Option<Vec<Decimal>>, E> {
                Ok(Some(vec![Decimal::from(v)]))
            }

            fn visit_unit<E: DeError>(self) -> Result<Option<Vec<Decimal>>, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Option<Vec<Decimal>>, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(DecimalVecOptionVisitor)
    }

    /// Serde `serialize_with` entry point of [`decimal_vec_option`](self).
    pub fn serialize<S: Serializer>(
        value: &Option<Vec<Decimal>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.collect_seq(v.iter().map(|d| d.to_string())),
            None => serializer.serialize_none(),
        }
    }
}

/// Deserialize `Option<Vec<T>>` from a JSON array of `T`, a single bare `T`
/// (a one-element array, see "Singleton arrays" above) or null. Used by the
/// cycle- and enum-valued array attributes (`ARIPCL`, `ARPRCL`, `ARRRCL`,
/// `ARINCDEC`, `ARFIXVAR`), whose elements deserialize from strings.
pub mod one_or_many_option {
    use serde::de::value::StrDeserializer;
    use serde::de::{Deserialize, Deserializer, Error as DeError, SeqAccess, Visitor};
    use std::fmt;
    use std::marker::PhantomData;

    /// Serde `deserialize_with` entry point of [`one_or_many_option`](self).
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        struct OneOrManyVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for OneOrManyVisitor<T> {
            type Value = Option<Vec<T>>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an array, a single array element, or null")
            }

            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut out = Vec::new();
                while let Some(item) = seq.next_element::<T>()? {
                    out.push(item);
                }
                Ok(Some(out))
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<Self::Value, E> {
                T::deserialize(StrDeserializer::<E>::new(v)).map(|item| Some(vec![item]))
            }

            fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
                Ok(None)
            }

            fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
                Ok(None)
            }
        }

        deserializer.deserialize_any(OneOrManyVisitor(PhantomData))
    }
}
