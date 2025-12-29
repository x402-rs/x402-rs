use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

/// Represents a price-like numeric value in human-readable currency format.
/// Accepts strings like "$0.01", "1,000", "€20", or raw numbers.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct MoneyAmount(pub Decimal);

#[allow(dead_code)] // Public for consumption by downstream crates.
impl MoneyAmount {
    /// Returns the number of digits after the decimal point in the original input.
    ///
    /// This is useful for checking precision constraints when converting
    /// human-readable amounts (e.g., `$0.01`) to on-chain token values.
    pub fn scale(&self) -> u32 {
        self.0.scale()
    }

    /// Returns the absolute mantissa of the decimal value as an unsigned integer.
    ///
    /// For example, the mantissa of `-12.34` is `1234`.
    /// Used when scaling values to match token decimal places.
    pub fn mantissa(&self) -> u128 {
        self.0.mantissa().unsigned_abs()
    }
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub enum MoneyAmountParseError {
    #[error("Invalid number format")]
    InvalidFormat,
    #[error(
        "Amount must be between {} and {}",
        money_amount::MIN_STR,
        money_amount::MAX_STR
    )]
    OutOfRange,
    #[error("Negative value is not allowed")]
    Negative,
    #[error("Too big of a precision: {money} vs {token} on token")]
    WrongPrecision { money: u32, token: u32 },
}

mod money_amount {
    use super::*;
    use once_cell::sync::Lazy;

    pub const MIN_STR: &str = "0.000000001";
    pub const MAX_STR: &str = "999999999";

    pub static MIN: Lazy<Decimal> =
        Lazy::new(|| Decimal::from_str(MIN_STR).expect("valid decimal"));
    pub static MAX: Lazy<Decimal> =
        Lazy::new(|| Decimal::from_str(MAX_STR).expect("valid decimal"));
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl MoneyAmount {
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

        if parsed < *money_amount::MIN || parsed > *money_amount::MAX {
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
        if decimal < *money_amount::MIN || decimal > *money_amount::MAX {
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
