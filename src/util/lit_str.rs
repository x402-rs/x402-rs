//! Compile-time string literal type generation.
//!
//! This module provides the [`lit_str!`] macro for creating types that
//! represent specific string literals at compile time. These types are
//! useful for ensuring type safety when working with fixed string values
//! in protocol messages.
//!
//! # Example
//!
//! ```ignore
//! use x402::lit_str;
//!
//! lit_str!(ExactScheme, "exact");
//!
//! // The type only accepts the exact string
//! let scheme: ExactScheme = "exact".parse().unwrap();
//! assert_eq!(scheme.to_string(), "exact");
//!
//! // Other strings are rejected
//! assert!("other".parse::<ExactScheme>().is_err());
//! ```

/// Creates a type that represents a specific string literal.
///
/// The generated type:
/// - Has a `VALUE` constant with the string
/// - Implements `FromStr` (only accepts the exact string)
/// - Implements `Serialize`/`Deserialize` (as the string)
/// - Implements `Display` (outputs the string)
#[macro_export]
macro_rules! lit_str {
    ($struct_name:ident, $val:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $struct_name;

        impl $struct_name {
            pub const VALUE: &'static str = $val;
        }

        impl AsRef<str> for $struct_name {
            fn as_ref(&self) -> &str {
                Self::VALUE
            }
        }

        impl std::str::FromStr for $struct_name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s == Self::VALUE {
                    Ok($struct_name)
                } else {
                    Err(format!("expected '{}', got '{}'", Self::VALUE, s))
                }
            }
        }

        impl serde::Serialize for $struct_name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(Self::VALUE)
            }
        }

        impl<'de> serde::Deserialize<'de> for $struct_name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                if s == Self::VALUE {
                    Ok($struct_name)
                } else {
                    Err(serde::de::Error::custom(format!(
                        "expected '{}', got '{}'",
                        Self::VALUE,
                        s
                    )))
                }
            }
        }

        impl Display for $struct_name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, $val)
            }
        }
    };
}
