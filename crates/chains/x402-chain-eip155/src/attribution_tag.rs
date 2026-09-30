//! ERC-8021 attribution tags for settlement transactions.
//!
//! Facilitator side of the x402 extension keyed `builder-code`
//! (`docs/specs/extensions/builder_code.md`). ERC-8021 calls the result
//! transaction attribution; `builder-code` is only the key clients and
//! resource servers put on the wire, so it is kept verbatim. The facilitator appends a small,
//! self-describing attribution suffix to the calldata of the settlement
//! transaction it submits. The EVM ignores trailing calldata, so execution is
//! byte-for-byte unchanged, but indexers can decode who was involved in
//! producing the payment:
//!
//! - `a` — app code: the resource server / seller whose paid endpoint was hit
//! - `w` — wallet code: this facilitator
//! - `s` — service codes: additional providers (e.g. the paying client)
//!
//! Wire format (ERC-8021 Schema 2), reading left-to-right at the END of calldata:
//!
//! ```text
//! [ CBOR map {a,w,s} ] [ len : u16 big-endian ] [ schema : 0x02 ] [ marker : 16 bytes ]
//! ```
//!
//! The CBOR map uses the spec's short keys and is emitted in `a,w,s` order (only
//! present keys), which makes the bytes identical to the reference
//! `@x402/extensions` encoder.
//!
//! Codes must match `^[a-z0-9_]{1,32}$`. `a` and `s` arrive in a request body,
//! so a code that fails the pattern is dropped and the payment still settles.
//! If no valid code is present the suffix is `None` and the calldata is left
//! untouched — so the whole feature is a no-op unless configured or requested.

use alloy_primitives::Bytes;
use serde::Deserialize;
use serde_json::Value;
use x402_types::proto::v2::ExtensionsJson;
use x402_types::scheme::ExtensionKey;

/// ERC-8021 marker: the 4-byte pattern `0x80218021` repeated to 16 bytes.
const MARKER: [u8; 16] = [
    0x80, 0x21, 0x80, 0x21, 0x80, 0x21, 0x80, 0x21, 0x80, 0x21, 0x80, 0x21, 0x80, 0x21, 0x80, 0x21,
];

const SCHEMA_ID: u8 = 0x02;

/// Maximum length of a single code (bytes). Also the ceiling that keeps a CBOR
/// text-string header to at most two bytes.
const MAX_CODE_LEN: usize = 32;

/// Maximum number of service codes read from a payment: the reference
/// implementation reserves 5 for the client and 5 for the resource server.
/// Also keeps the CBOR array header to a single byte.
pub const MAX_ECHOED_SERVICE_CODES: usize = 10;

/// Returns true if `code` matches `^[a-z0-9_]{1,32}$`.
pub fn is_valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_CODE_LEN
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The attribution tag for a single settlement. All fields optional; an all-empty
/// (or all-invalid) set encodes to no suffix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionTag {
    /// `a` — application / resource-server code.
    pub app: Option<String>,
    /// `w` — wallet / facilitator code.
    pub wallet: Option<String>,
    /// `s` — service / client codes.
    pub service: Vec<String>,
}

impl AttributionTag {
    /// Build from the facilitator-configured code plus any `a`/`s` codes the
    /// payment carries in its `builder-code` extension.
    pub fn from_config_and_extensions(
        facilitator_code: Option<&str>,
        extensions: &ExtensionsJson,
    ) -> Self {
        let declared = extensions
            .get::<AttributionTagExtension>()
            .unwrap_or_default();
        AttributionTag {
            app: declared.app(),
            wallet: facilitator_code.map(str::to_string),
            service: declared.service(),
        }
    }

    /// The ERC-8021 Schema 2 suffix for these codes, or `None` if no valid code
    /// is present. Append the returned bytes to settlement calldata.
    pub fn suffix(&self) -> Option<Bytes> {
        let app = self.app.as_deref().filter(|c| is_valid_code(c));
        let wallet = self.wallet.as_deref().filter(|c| is_valid_code(c));
        let service: Vec<&str> = self
            .service
            .iter()
            .map(String::as_str)
            .filter(|c| is_valid_code(c))
            .take(MAX_ECHOED_SERVICE_CODES)
            .collect();

        if app.is_none() && wallet.is_none() && service.is_empty() {
            return None;
        }

        // CBOR map with keys in a,w,s insertion order (only present keys).
        let mut cbor = Vec::new();
        let entry_count =
            app.is_some() as u8 + wallet.is_some() as u8 + (!service.is_empty()) as u8;
        cbor.push(0xa0 | entry_count); // map(entry_count) — count is always <= 3
        if let Some(a) = app {
            push_text(&mut cbor, "a");
            push_text(&mut cbor, a);
        }
        if let Some(w) = wallet {
            push_text(&mut cbor, "w");
            push_text(&mut cbor, w);
        }
        if !service.is_empty() {
            push_text(&mut cbor, "s");
            cbor.push(0x80 | service.len() as u8); // array(service.len()) — small
            for s in &service {
                push_text(&mut cbor, s);
            }
        }

        let mut out = cbor;
        let cbor_len = out.len() as u16;
        out.extend_from_slice(&cbor_len.to_be_bytes());
        out.push(SCHEMA_ID);
        out.extend_from_slice(&MARKER);
        Some(Bytes::from(out))
    }
}

/// Append an attribution suffix to calldata. `None` returns the calldata
/// unchanged (the common, unconfigured case allocates nothing).
pub fn append_suffix(calldata: Bytes, suffix: Option<&Bytes>) -> Bytes {
    match suffix {
        None => calldata,
        Some(s) => {
            let mut v = Vec::with_capacity(calldata.len() + s.len());
            v.extend_from_slice(&calldata);
            v.extend_from_slice(s);
            Bytes::from(v)
        }
    }
}

/// The `builder-code` extension as it appears in a payment payload.
///
/// The reference clients send the codes under `info`
/// (`{"builder-code": {"info": {"a": "...", "s": ["..."]}}}`); the spec's
/// examples show them unwrapped (`{"builder-code": {"a": "...", "s": "..."}}`).
/// Both are read, `info` first. `w` is set by the facilitator from config and
/// is intentionally never read from the payment.
///
/// Fields are kept as raw JSON so that one malformed field (say a numeric `a`)
/// does not discard the others.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AttributionTagExtension {
    #[serde(default)]
    info: Option<Value>,
    #[serde(default)]
    a: Option<Value>,
    #[serde(default)]
    s: Option<Value>,
}

impl AttributionTagExtension {
    fn field(&self, key: &str) -> Option<&Value> {
        match &self.info {
            Some(Value::Object(info)) => info.get(key),
            _ => match key {
                "a" => self.a.as_ref(),
                "s" => self.s.as_ref(),
                _ => None,
            },
        }
    }

    /// The app code `a`, if present and well-formed.
    pub fn app(&self) -> Option<String> {
        self.field("a")
            .and_then(Value::as_str)
            .filter(|c| is_valid_code(c))
            .map(str::to_string)
    }

    /// The service codes `s` (a string or an array of strings): well-formed
    /// entries in order, truncated to [`MAX_ECHOED_SERVICE_CODES`].
    pub fn service(&self) -> Vec<String> {
        let candidates: Vec<&Value> = match self.field("s") {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(single @ Value::String(_)) => vec![single],
            _ => vec![],
        };
        candidates
            .into_iter()
            .filter_map(Value::as_str)
            .filter(|c| is_valid_code(c))
            .take(MAX_ECHOED_SERVICE_CODES)
            .map(str::to_string)
            .collect()
    }
}

impl ExtensionKey for AttributionTagExtension {
    const EXTENSION_KEY: &'static str = "builder-code";
}

/// Write a CBOR definite-length text string (`s.len() <= MAX_CODE_LEN`, so the
/// header is one byte for len <= 23, otherwise `0x78` + one length byte).
fn push_text(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len();
    if n <= 23 {
        out.push(0x60 | n as u8);
    } else {
        out.push(0x78);
        out.push(n as u8);
    }
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::hex;

    fn suffix_hex(codes: AttributionTag) -> String {
        hex::encode(codes.suffix().expect("expected a suffix"))
    }

    fn extensions(json: serde_json::Value) -> ExtensionsJson {
        ExtensionsJson::try_from(json).expect("extensions object")
    }

    #[test]
    fn code_pattern_boundaries() {
        for ok in ["a", "celo_facil", "0", "_", "abc_123", &"z".repeat(32)] {
            assert!(is_valid_code(ok), "{ok:?} should be valid");
        }
        for bad in [
            "",
            "Celo",
            "CELO_FACIL",
            "my-app",
            "my app",
            "my.app",
            "a,b",
            "caf\u{e9}",
            &"z".repeat(33),
        ] {
            assert!(!is_valid_code(bad), "{bad:?} should be invalid");
        }
    }

    // The two worked examples of docs/specs/extensions/builder_code.md.

    #[test]
    fn spec_example_single_app() {
        let got = suffix_hex(AttributionTag {
            app: Some("bc_myapp".into()),
            ..Default::default()
        });
        assert_eq!(
            got,
            "a161616862635f6d79617070000c0280218021802180218021802180218021"
        );
    }

    #[test]
    fn spec_example_app_and_facilitator() {
        let got = suffix_hex(AttributionTag {
            app: Some("bc_myapp".into()),
            wallet: Some("bc_myfacilitator".into()),
            service: vec![],
        });
        assert_eq!(
            got,
            "a261616862635f6d7961707061777062635f6d79666163696c697461746f72001f0280218021802180218021802180218021"
        );
    }

    #[test]
    fn reads_codes_under_info() {
        let ext = extensions(serde_json::json!({
            "builder-code": { "info": { "a": "my_app", "s": ["base_mcp", "demo_app"] } }
        }));
        assert_eq!(
            AttributionTag::from_config_and_extensions(Some("my_fac"), &ext),
            AttributionTag {
                app: Some("my_app".into()),
                wallet: Some("my_fac".into()),
                service: vec!["base_mcp".into(), "demo_app".into()],
            }
        );
    }

    #[test]
    fn reads_unwrapped_codes_and_a_single_string_service() {
        let ext = extensions(serde_json::json!({
            "builder-code": { "a": "my_app", "s": "my_client" }
        }));
        assert_eq!(
            AttributionTag::from_config_and_extensions(None, &ext),
            AttributionTag {
                app: Some("my_app".into()),
                wallet: None,
                service: vec!["my_client".into()],
            }
        );
    }

    #[test]
    fn wallet_code_is_never_read_from_the_payment() {
        let ext = extensions(serde_json::json!({
            "builder-code": { "info": { "a": "my_app", "w": "forged_fac" } }
        }));
        let codes = AttributionTag::from_config_and_extensions(None, &ext);
        assert_eq!(codes.app.as_deref(), Some("my_app"));
        assert_eq!(codes.wallet, None);
    }

    #[test]
    fn malformed_fields_are_dropped_one_by_one() {
        // A numeric `a` and mixed `s` entries: the well-formed entries survive.
        let ext = extensions(serde_json::json!({
            "builder-code": { "info": { "a": 42, "s": ["ok_one", "Not-Ok", 7, "", "ok_two"] } }
        }));
        assert_eq!(
            AttributionTag::from_config_and_extensions(None, &ext),
            AttributionTag {
                app: None,
                wallet: None,
                service: vec!["ok_one".into(), "ok_two".into()],
            }
        );
    }

    #[test]
    fn service_codes_are_capped() {
        let many: Vec<String> = (0..MAX_ECHOED_SERVICE_CODES + 3)
            .map(|i| format!("svc_{i}"))
            .collect();
        let ext = extensions(serde_json::json!({ "builder-code": { "info": { "s": many } } }));
        let codes = AttributionTag::from_config_and_extensions(None, &ext);
        assert_eq!(codes.service.len(), MAX_ECHOED_SERVICE_CODES);
        assert_eq!(codes.service[0], "svc_0");
        // The cap also holds for codes set directly on the struct.
        let direct = AttributionTag {
            service: (0..30).map(|i| format!("svc_{i}")).collect(),
            ..Default::default()
        };
        let suffix = direct.suffix().unwrap();
        // map(1) 's' -> a1 61 73, then array(10) -> 0x8a.
        assert_eq!(&suffix[0..4], &[0xa1, 0x61, 0x73, 0x8a]);
    }

    #[test]
    fn no_extension_and_no_config_leaves_calldata_identical() {
        let calldata = Bytes::from(hex::decode("e3ee160e0000").unwrap());
        for ext in [
            ExtensionsJson::new(),
            extensions(serde_json::json!({ "builder-code": "not an object" })),
            extensions(serde_json::json!({ "attribution": { "a": "my_app" } })),
        ] {
            let suffix = AttributionTag::from_config_and_extensions(None, &ext).suffix();
            assert_eq!(suffix, None);
            assert_eq!(append_suffix(calldata.clone(), suffix.as_ref()), calldata);
        }
    }

    // Vectors below are byte-for-byte outputs of @celo/attribution-tags 0.4.0
    // (ox/erc8021 Schema 2). Interop rests on these matching exactly.

    #[test]
    fn vector_app_and_wallet() {
        let got = suffix_hex(AttributionTag {
            app: Some("celo_b7k3p9da".into()),
            wallet: Some("celo_facil".into()),
            service: vec![],
        });
        assert_eq!(
            got,
            "a261616d63656c6f5f62376b337039646161776a63656c6f5f666163696c001e0280218021802180218021802180218021"
        );
    }

    #[test]
    fn vector_app_wallet_service() {
        let got = suffix_hex(AttributionTag {
            app: Some("celo_b7k3p9da".into()),
            wallet: Some("celo_facil".into()),
            service: vec!["celo_agent".into()],
        });
        assert_eq!(
            got,
            "a361616d63656c6f5f62376b337039646161776a63656c6f5f666163696c6173816a63656c6f5f6167656e74002c0280218021802180218021802180218021"
        );
    }

    #[test]
    fn vector_wallet_only() {
        let got = suffix_hex(AttributionTag {
            app: None,
            wallet: Some("celo_facil".into()),
            service: vec![],
        });
        assert_eq!(
            got,
            "a161776a63656c6f5f666163696c000e0280218021802180218021802180218021"
        );
    }

    #[test]
    fn no_codes_yields_no_suffix() {
        assert!(AttributionTag::default().suffix().is_none());
        // All-invalid also yields nothing.
        let invalid = AttributionTag {
            app: Some("Invalid Code".into()),
            wallet: Some("with,comma".into()),
            service: vec!["".into()],
        };
        assert!(invalid.suffix().is_none());
    }

    #[test]
    fn invalid_codes_are_dropped_not_encoded() {
        // A valid wallet code survives; an invalid app code is silently dropped,
        // producing the wallet-only vector rather than failing.
        let got = suffix_hex(AttributionTag {
            app: Some("BAD APP".into()),
            wallet: Some("celo_facil".into()),
            service: vec![],
        });
        assert_eq!(
            got,
            "a161776a63656c6f5f666163696c000e0280218021802180218021802180218021"
        );
    }

    #[test]
    fn appends_to_calldata_and_leaves_none_untouched() {
        let calldata = Bytes::from(hex::decode("a9059cbb0000").unwrap());
        let suffix = AttributionTag {
            wallet: Some("celo_facil".into()),
            ..Default::default()
        }
        .suffix();
        let tagged = append_suffix(calldata.clone(), suffix.as_ref());
        assert!(tagged.starts_with(&calldata));
        assert!(tagged.ends_with(&MARKER));
        // No suffix -> identical bytes.
        assert_eq!(append_suffix(calldata.clone(), None), calldata);
    }

    #[test]
    fn code_at_length_boundary_uses_two_byte_header() {
        // 24-char code crosses the CBOR 1-byte-header boundary (0x78 + len).
        let code = "a".repeat(24);
        let got = AttributionTag {
            wallet: Some(code.clone()),
            ..Default::default()
        }
        .suffix()
        .unwrap();
        // map(1) 'w' -> 61 77, then value header 0x78 0x18 (24), then 24 'a's.
        assert_eq!(&got[0..4], &[0xa1, 0x61, 0x77, 0x78]);
        assert_eq!(got[4], 24);
        // 33-char code is invalid -> dropped -> no suffix.
        assert!(
            AttributionTag {
                wallet: Some("a".repeat(33)),
                ..Default::default()
            }
            .suffix()
            .is_none()
        );
    }
}
