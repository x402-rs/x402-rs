use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};

/// Enumerates payment schemes. Only "exact" is supported in this implementation,
/// meaning the amount to be transferred must match exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Exact,
}

impl Display for Scheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            Scheme::Exact => "exact",
        };
        write!(f, "{s}")
    }
}

impl Into<String> for Scheme {
    fn into(self) -> String {
        self.to_string()
    }
}
