//! Checked decimal arithmetic for payoff and state functions.
//!
//! `rust_decimal`'s operators panic on overflow (beyond about ±7.9e28) and
//! on division by zero, and the WASM build aborts on any panic. Contract
//! terms are untrusted input (JSON in the ACTUS Explorer), so every
//! operation in this crate goes through [`Ck`] instead. `clippy::
//! arithmetic_side_effects` is denied for the crate's library code to keep
//! it that way.
//!
//! [`Ck`] behaves like a floating-point NaN: once an operation fails, every
//! later operation on the value fails too, and [`Ck::get`] turns the result
//! into [`EngineError::Arithmetic`]. A whole expression can therefore stay
//! in operator form and be checked once:
//!
//! ```text
//! let accrual = (ck(yf) * rate * notional).get()?;
//! ```
//!
//! Where nothing fails, the results are identical to the plain operators:
//! `Decimal`'s `Mul` is `checked_mul(..).expect(..)`, and so on for the
//! other operators.

use std::ops::{Add, Div, Mul, Neg, Sub};

use rust_decimal::Decimal;

use crate::EngineError;

/// Why a checked decimal operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticFailure {
    /// The result is outside the representable `Decimal` range.
    Overflow,
    /// The divisor was zero.
    DivisionByZero,
}

impl std::fmt::Display for ArithmeticFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ArithmeticFailure::Overflow => "decimal overflow",
            ArithmeticFailure::DivisionByZero => "division by zero",
        })
    }
}

/// A decimal that remembers the first failed operation that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ck(Result<Decimal, ArithmeticFailure>);

/// Lifts a decimal into checked arithmetic.
pub(crate) fn ck(value: Decimal) -> Ck {
    Ck(Ok(value))
}

impl Ck {
    /// The value, or [`EngineError::Arithmetic`] if any operation failed.
    pub(crate) fn get(self) -> Result<Decimal, EngineError> {
        self.0.map_err(EngineError::Arithmetic)
    }

    fn lift(
        self,
        rhs: Ck,
        op: fn(Decimal, Decimal) -> Option<Decimal>,
        failure: ArithmeticFailure,
    ) -> Ck {
        Ck(self
            .0
            .and_then(|a| rhs.0.and_then(|b| op(a, b).ok_or(failure))))
    }
}

impl From<Decimal> for Ck {
    fn from(value: Decimal) -> Self {
        ck(value)
    }
}

macro_rules! checked_op {
    ($trait:ident, $method:ident, $checked:path) => {
        impl $trait<Ck> for Ck {
            type Output = Ck;
            fn $method(self, rhs: Ck) -> Ck {
                self.lift(rhs, $checked, ArithmeticFailure::Overflow)
            }
        }
        impl $trait<Decimal> for Ck {
            type Output = Ck;
            fn $method(self, rhs: Decimal) -> Ck {
                self.lift(ck(rhs), $checked, ArithmeticFailure::Overflow)
            }
        }
        impl $trait<Ck> for Decimal {
            type Output = Ck;
            fn $method(self, rhs: Ck) -> Ck {
                ck(self).lift(rhs, $checked, ArithmeticFailure::Overflow)
            }
        }
    };
}

checked_op!(Add, add, Decimal::checked_add);
checked_op!(Sub, sub, Decimal::checked_sub);
checked_op!(Mul, mul, Decimal::checked_mul);

fn checked_div(a: Decimal, b: Decimal) -> Option<Decimal> {
    a.checked_div(b)
}

impl Div<Ck> for Ck {
    type Output = Ck;
    fn div(self, rhs: Ck) -> Ck {
        match rhs.0 {
            Ok(b) if b.is_zero() => Ck(self.0.and(Err(ArithmeticFailure::DivisionByZero))),
            _ => self.lift(rhs, checked_div, ArithmeticFailure::Overflow),
        }
    }
}

impl Div<Decimal> for Ck {
    type Output = Ck;
    fn div(self, rhs: Decimal) -> Ck {
        self / ck(rhs)
    }
}

impl Div<Ck> for Decimal {
    type Output = Ck;
    fn div(self, rhs: Ck) -> Ck {
        ck(self) / rhs
    }
}

impl Neg for Ck {
    type Output = Ck;
    fn neg(self) -> Ck {
        // Decimal's range is symmetric (MIN == -MAX), so negation cannot fail.
        #[allow(clippy::arithmetic_side_effects)]
        Ck(self.0.map(|v| -v))
    }
}

/// Checked sum of decimals.
pub(crate) fn sum(values: impl IntoIterator<Item = Decimal>) -> Result<Decimal, EngineError> {
    values
        .into_iter()
        .fold(ck(Decimal::ZERO), |acc, v| acc + v)
        .get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn results_match_the_plain_operators() {
        let (a, b) = (dec!(1234.5678), dec!(-0.0365));
        assert_eq!((ck(a) + b).get().unwrap(), a + b);
        assert_eq!((ck(a) - b).get().unwrap(), a - b);
        assert_eq!((ck(a) * b).get().unwrap(), a * b);
        assert_eq!((ck(a) / b).get().unwrap(), a / b);
        assert_eq!((a * ck(b)).get().unwrap(), a * b);
        assert_eq!((a / ck(b)).get().unwrap(), a / b);
        assert_eq!((-ck(a)).get().unwrap(), -a);
        assert_eq!(sum([a, b, a]).unwrap(), a + b + a);
    }

    #[test]
    fn overflow_and_division_by_zero_are_errors() {
        let max = Decimal::MAX;
        assert!(matches!(
            (ck(max) * dec!(2)).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::Overflow))
        ));
        assert!(matches!(
            (ck(max) + max).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::Overflow))
        ));
        assert!(matches!(
            (ck(-max) - max).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::Overflow))
        ));
        assert!(matches!(
            (ck(dec!(1)) / Decimal::ZERO).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::DivisionByZero))
        ));
        assert!(matches!(
            (ck(max) / dec!(0.1)).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::Overflow))
        ));
        assert!(sum([max, max]).is_err());
    }

    #[test]
    fn the_first_failure_sticks() {
        let failed = ck(Decimal::MAX) * dec!(10);
        assert!(((failed / Decimal::ZERO) * Decimal::ZERO + dec!(1))
            .get()
            .is_err());
        assert!(matches!(
            (failed / Decimal::ZERO).get(),
            Err(EngineError::Arithmetic(ArithmeticFailure::Overflow))
        ));
    }
}
