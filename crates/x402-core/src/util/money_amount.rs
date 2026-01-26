//! Human-readable currency amount parsing.
//!
//! This module provides [`MoneyAmount`], a type for parsing human-readable
//! currency strings into precise decimal values suitable for conversion to
//! on-chain token amounts.
//!
//! # Supported Formats
//!
//! - Plain numbers: `"100"`, `"0.01"`
//! - With currency symbols: `"$10.50"`, `"€20"`
//! - With thousand separators: `"1,000"`, `"1,000,000.50"`
//!
//! # Example
//!
//! ```rust
//! use x402_rs::util::money_amount::MoneyAmount;
//!
//! let amount = MoneyAmount::parse("$10.50").unwrap();
//! assert_eq!(amount.scale(), 2);  // 2 decimal places
//! assert_eq!(amount.mantissa(), 1050);  // 10.50 as integer
//! ```

use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

/// A parsed monetary amount with decimal precision.
///
/// This type represents a non-negative decimal value parsed from a
/// human-readable string. It preserves the original precision, which
/// is important when converting to token amounts with specific decimal places.
///
/// # Precision
///
/// The [`scale`](MoneyAmount::scale) method returns the number of decimal places,
/// and [`mantissa`](MoneyAmount::mantissa) returns the value as an integer.
/// For example, `"10.50"` has scale 2 and mantissa 1050.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct MoneyAmount(pub Decimal);

#[allow(dead_code)] // Public for consumption by downstream crates.
impl MoneyAmount {
    /// Returns the number of decimal places in the original input.
    ///
    /// This is used to verify that the input precision doesn't exceed
    /// the token's decimal places.
    pub fn scale(&self) -> u32 {
        self.0.scale()
    }

    /// Returns the value as an unsigned integer (without decimal point).
    ///
    /// For example, `"12.34"` returns `1234`.
    pub fn mantissa(&self) -> u128 {
        self.0.mantissa().unsigned_abs()
    }
}

/// Errors that can occur when parsing a monetary amount.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub enum MoneyAmountParseError {
    /// The input string could not be parsed as a number.
    #[error("Invalid number format")]
    InvalidFormat,
    /// The value is outside the allowed range.
    #[error(
        "Amount must be between {} and {}",
        constants::MIN_STR,
        constants::MAX_STR
    )]
    OutOfRange,
    /// Negative values are not allowed.
    #[error("Negative value is not allowed")]
    Negative,
    /// The input has more decimal places than the token supports.
    #[error("Too big of a precision: {money} vs {token} on token")]
    WrongPrecision {
        /// Decimal places in the input.
        money: u32,
        /// Decimal places supported by the token.
        token: u32,
    },
}

mod constants {
    use super::*;
    use std::sync::LazyLock;

    pub const MIN_STR: &str = "0.000000001";
    pub const MAX_STR: &str = "999999999";

    pub static MIN: LazyLock<Decimal> =
        LazyLock::new(|| Decimal::from_str(MIN_STR).expect("valid decimal"));
    pub static MAX: LazyLock<Decimal> =
        LazyLock::new(|| Decimal::from_str(MAX_STR).expect("valid decimal"));
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl MoneyAmount {
    /// Parses a human-readable currency string into a [`MoneyAmount`].
    ///
    /// Currency symbols, thousand separators, and whitespace are stripped
    /// before parsing. The result must be a non-negative number within
    /// the allowed range.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string cannot be parsed as a number
    /// - The value is negative
    /// - The value is outside the allowed range
    pub fn parse(input: &str) -> Result<Self, MoneyAmountParseError> {
        // Remove anything that isn't digit, dot, minus
        let cleaned = Regex::new(r"[^\d\.\-]+")
            .unwrap()
            .replace_all(input, "")
            .to_string();

        let parsed =
            Decimal::from_str(&cleaned).map_err(|_| MoneyAmountParseError::InvalidFormat)?;

        if parsed.is_sign_negative() {
            return Err(MoneyAmountParseError::Negative);
        }

        if parsed < *constants::MIN || parsed > *constants::MAX {
            return Err(MoneyAmountParseError::OutOfRange);
        }

        Ok(MoneyAmount(parsed))
    }
}

impl FromStr for MoneyAmount {
    type Err = MoneyAmountParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MoneyAmount::parse(s)
    }
}

impl TryFrom<&str> for MoneyAmount {
    type Error = MoneyAmountParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        MoneyAmount::from_str(value)
    }
}

impl From<u128> for MoneyAmount {
    fn from(value: u128) -> Self {
        MoneyAmount(Decimal::from(value))
    }
}

impl TryFrom<f64> for MoneyAmount {
    type Error = MoneyAmountParseError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        let decimal = Decimal::from_f64(value).ok_or(MoneyAmountParseError::OutOfRange)?;
        if decimal.is_sign_negative() {
            return Err(MoneyAmountParseError::Negative);
        }
        if decimal < *constants::MIN || decimal > *constants::MAX {
            return Err(MoneyAmountParseError::OutOfRange);
        }
        Ok(MoneyAmount(decimal))
    }
}

impl Display for MoneyAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.normalize())
    }
}
