use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::ops::Add;
use std::time::SystemTime;

/// A Unix timestamp represented as a `u64`, used in payment authorization windows.
///
/// This type encodes the number of seconds since the Unix epoch (1970-01-01T00:00:00Z).
/// It is used in time-bounded ERC-3009 `transferWithAuthorization` messages to specify
/// the validity window (`validAfter` and `validBefore`) of a payment authorization.
///
/// Serialized as a stringified integer to avoid loss of precision in JSON.
/// For example, `1699999999` becomes `"1699999999"` in the wire format.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct UnixTimestamp(u64);

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
    pub fn from_secs(secs: u64) -> Self {
        Self(secs)
    }

    pub fn now() -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime before UNIX epoch?!?")
            .as_secs();
        Self(now)
    }

    pub fn as_secs(&self) -> u64 {
        self.0
    }
}
