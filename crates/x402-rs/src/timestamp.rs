use alloy::primitives::U256;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::ops::Add;
use std::time::{SystemTime, SystemTimeError};

/// A Unix timestamp represented as a `u64`, used in payment authorization windows.
///
/// This type encodes the number of seconds since the Unix epoch (1970-01-01T00:00:00Z).
/// It is used in time-bounded ERC-3009 `transferWithAuthorization` messages to specify
/// the validity window (`validAfter` and `validBefore`) of a payment authorization.
///
/// Serialized as a stringified integer to avoid loss of precision in JSON.
/// For example, `1699999999` becomes `"1699999999"` in the wire format.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct UnixTimestamp(pub u64);

impl Serialize for UnixTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for UnixTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let ts = s
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom("timestamp must be a non-negative integer"))?;
        Ok(UnixTimestamp(ts))
    }
}

impl From<UnixTimestamp> for U256 {
    fn from(value: UnixTimestamp) -> Self {
        U256::from(value.0)
    }
}

impl Display for UnixTimestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<u64> for UnixTimestamp {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        UnixTimestamp(self.0 + rhs)
    }
}

impl UnixTimestamp {
    pub fn try_now() -> Result<Self, SystemTimeError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();
        Ok(Self(now))
    }

    pub fn seconds_since_epoch(&self) -> u64 {
        self.0
    }
}
