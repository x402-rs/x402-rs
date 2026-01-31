//! Base64 encoding and decoding utilities.
//!
//! This module provides [`Base64Bytes`], a wrapper type for working with
//! base64-encoded data in the x402 protocol.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as b64;
use std::borrow::Cow;
use std::fmt::Display;

/// A wrapper for base64-encoded byte data.
///
/// This type holds bytes that represent base64-encoded data and provides
/// methods for encoding and decoding. It uses copy-on-write semantics
/// to avoid unnecessary allocations.
///
/// # Example
///
/// ```rust
/// use x402_types::util::Base64Bytes;
///
/// // Encode some data
/// let encoded = Base64Bytes::encode(b"hello world");
/// assert_eq!(encoded.to_string(), "aGVsbG8gd29ybGQ=");
///
/// // Decode it back
/// let decoded = encoded.decode().unwrap();
/// assert_eq!(decoded, b"hello world");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64Bytes<'a>(pub Cow<'a, [u8]>);

impl Base64Bytes<'_> {
    /// Decodes the base64 string bytes to raw binary data.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is not valid base64.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64.decode(&self.0)
    }

    /// Encodes raw binary data into base64 string bytes.
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
