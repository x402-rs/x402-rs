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
