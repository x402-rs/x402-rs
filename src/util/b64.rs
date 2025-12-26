use base64::Engine;
use base64::engine::general_purpose::STANDARD as b64;
use std::borrow::Cow;
use std::fmt::Display;

/// Contains bytes of base64 encoded some other bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64Bytes<'a>(pub Cow<'a, [u8]>);

impl Base64Bytes<'_> {
    /// Decode base64 string bytes to raw binary payload.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64.decode(&self.0)
    }

    /// Encode raw binary input into base64 string bytes
    pub fn encode<T: AsRef<[u8]>>(input: T) -> Base64Bytes<'static> {
        let encoded = b64.encode(input.as_ref());
        Base64Bytes(Cow::Owned(encoded.into_bytes()))
    }
}

impl AsRef<[u8]> for Base64Bytes<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<'a> From<&'a [u8]> for Base64Bytes<'a> {
    fn from(slice: &'a [u8]) -> Self {
        Base64Bytes(Cow::Borrowed(slice))
    }
}

impl Display for Base64Bytes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.0.as_ref()))
    }
}
