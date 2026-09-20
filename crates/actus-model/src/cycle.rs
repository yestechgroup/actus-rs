//! ACTUS cycle terms (`IPCL`, `PRCL`, `RRCL`, `IPCBCL`, `SCCL`).
//!
//! A cycle encodes a recurring schedule interval, e.g. `P1ML0` = one month,
//! long-first stub, anchor index 0. The techspec grammar is
//! `P<n><D|W|M|Y><L|R|U><digit>`; the trailing stub/index pair positions
//! centric anchors relative to period start/end.

use crate::error::ModelError;
use serde::de::{Deserializer, Error as DeError, Unexpected, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Period unit of a [`Cycle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CyclePeriod {
    /// `D` — daily cycle.
    Day,
    /// `W` — weekly cycle.
    Week,
    /// `M` — monthly cycle.
    Month,
    /// `Y` — yearly cycle.
    Year,
}

impl CyclePeriod {
    /// Dictionary period character (`D`, `W`, `M`, `Y`).
    #[must_use]
    pub fn as_char(&self) -> char {
        match self {
            CyclePeriod::Day => 'D',
            CyclePeriod::Week => 'W',
            CyclePeriod::Month => 'M',
            CyclePeriod::Year => 'Y',
        }
    }

    fn from_char(c: char) -> Option<CyclePeriod> {
        match c.to_ascii_uppercase() {
            'D' => Some(CyclePeriod::Day),
            'W' => Some(CyclePeriod::Week),
            'M' => Some(CyclePeriod::Month),
            'Y' => Some(CyclePeriod::Year),
            _ => None,
        }
    }
}

impl fmt::Display for CyclePeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// Stub positioning of centric anchors in a [`Cycle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CycleStub {
    /// `L` — long first/last period.
    Long,
    /// `R` — short first/last period.
    Short,
    /// `U` — stub positioning undefined.
    Undefined,
}

impl CycleStub {
    /// Dictionary stub character (`L`, `R`, `U`).
    #[must_use]
    pub fn as_char(&self) -> char {
        match self {
            CycleStub::Long => 'L',
            CycleStub::Short => 'R',
            CycleStub::Undefined => 'U',
        }
    }

    fn from_char(c: char) -> Option<CycleStub> {
        match c.to_ascii_uppercase() {
            'L' => Some(CycleStub::Long),
            'R' => Some(CycleStub::Short),
            'U' => Some(CycleStub::Undefined),
            _ => None,
        }
    }
}

impl fmt::Display for CycleStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// An ACTUS cycle term, e.g. `P1ML0` (one month, long-first stub, index 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cycle {
    length: u32,
    period: CyclePeriod,
    stub: CycleStub,
    index: u8,
}

impl Cycle {
    /// Construct a cycle from its parts.
    ///
    /// `index` must fit in one digit (0-9) so that [`Display`](fmt::Display)
    /// round-trips.
    pub fn new(
        length: u32,
        period: CyclePeriod,
        stub: CycleStub,
        index: u8,
    ) -> Result<Cycle, ModelError> {
        if length == 0 {
            return Err(ModelError::InvalidCycle(
                "cycle length must be at least 1".to_string(),
            ));
        }
        if index > 9 {
            return Err(ModelError::InvalidCycle(
                "cycle index must be a single digit 0-9".to_string(),
            ));
        }
        Ok(Cycle {
            length,
            period,
            stub,
            index,
        })
    }

    /// Parse a cycle term such as `P1ML0` or `P27DL1`.
    ///
    /// Grammar: `P` `<non-zero digits>` `<D|W|M|Y>` `<L|R|U>` `<digit>`
    /// (case-insensitive; [`Display`](fmt::Display) emits the canonical
    /// uppercase form).
    pub fn parse(input: &str) -> Result<Cycle, ModelError> {
        let s = input.trim().to_ascii_uppercase();
        let invalid = || ModelError::InvalidCycle(input.to_string());
        let bytes = s.as_bytes();
        if bytes.first() != Some(&b'P') {
            return Err(invalid());
        }
        let rest = &s[1..];
        let digits_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits_end == 0 {
            return Err(invalid());
        }
        let length: u32 = rest[..digits_end].parse().map_err(|_| invalid())?;
        if length == 0 {
            return Err(invalid());
        }
        let remainder = &rest[digits_end..];
        let mut chars = remainder.chars();
        let period = chars
            .next()
            .and_then(CyclePeriod::from_char)
            .ok_or_else(invalid)?;
        let stub = chars
            .next()
            .and_then(CycleStub::from_char)
            .ok_or_else(invalid)?;
        let index_char = chars.next().ok_or_else(invalid)?;
        if !index_char.is_ascii_digit() {
            return Err(invalid());
        }
        let index: u8 = index_char
            .to_digit(10)
            .map(|d| d as u8)
            .ok_or_else(invalid)?;
        if chars.next().is_some() {
            return Err(invalid());
        }
        Cycle::new(length, period, stub, index)
    }

    /// Cycle length in [`CyclePeriod`] units (always >= 1).
    #[must_use]
    pub fn length(&self) -> u32 {
        self.length
    }

    /// Cycle period unit.
    #[must_use]
    pub fn period(&self) -> CyclePeriod {
        self.period
    }

    /// Stub positioning of centric anchors.
    #[must_use]
    pub fn stub(&self) -> CycleStub {
        self.stub
    }

    /// Anchor index digit.
    #[must_use]
    pub fn index(&self) -> u8 {
        self.index
    }
}

impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "P{}{}{}{}",
            self.length, self.period, self.stub, self.index
        )
    }
}

impl std::str::FromStr for Cycle {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Cycle, ModelError> {
        Cycle::parse(s)
    }
}

impl Serialize for Cycle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Cycle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Cycle, D::Error> {
        struct CycleVisitor;

        impl<'de> Visitor<'de> for CycleVisitor {
            type Value = Cycle;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an ACTUS cycle term such as P1ML0")
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<Cycle, E> {
                Cycle::parse(v).map_err(DeError::custom)
            }
        }

        deserializer.deserialize_str(CycleVisitor)
    }
}

impl<'de> Deserialize<'de> for CyclePeriod {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<CyclePeriod, D::Error> {
        let s = String::deserialize(deserializer)?;
        CyclePeriod::from_char(
            s.chars().next().ok_or_else(|| {
                DeError::invalid_value(Unexpected::Str(&s), &"a period character")
            })?,
        )
        .ok_or_else(|| DeError::invalid_value(Unexpected::Str(&s), &"one of D, W, M, Y"))
    }
}

impl Serialize for CyclePeriod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CycleStub {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<CycleStub, D::Error> {
        let s = String::deserialize(deserializer)?;
        CycleStub::from_char(
            s.chars()
                .next()
                .ok_or_else(|| DeError::invalid_value(Unexpected::Str(&s), &"a stub character"))?,
        )
        .ok_or_else(|| DeError::invalid_value(Unexpected::Str(&s), &"one of L, R, U"))
    }
}

impl Serialize for CycleStub {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
