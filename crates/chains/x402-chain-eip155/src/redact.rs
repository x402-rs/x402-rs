//! Helpers for redacting credentials from RPC URLs.
//!
//! Most hosted RPC providers embed the API key in the endpoint URL — as a
//! path segment (`https://host/v2/<key>`), a query string (`?apikey=<key>`),
//! or userinfo (`https://user:token@host`). These helpers strip the
//! credential-bearing parts before a URL reaches a log line or a
//! client-facing error body.

/// Reduce an RPC endpoint URL to `scheme://host[:port]`.
///
/// Drops userinfo, path, query string, and fragment, which is where hosted
/// providers put API keys. Returns `"<redacted>"` when the input does not
/// look like a URL at all.
pub fn redact_rpc_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<redacted>".to_string();
    };
    // The authority ends at the first '/', '?', '#', or whitespace.
    let authority_end = rest
        .find(&['/', '?', '#', ' ', '\t', '\n', '\r'][..])
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    // Strip userinfo (`user:token@`) — keep only the host part.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() {
        return "<redacted>".to_string();
    }
    format!("{scheme}://{host}")
}

/// Redact URLs embedded in free-form error text.
///
/// Every `scheme://authority[/path][?query][#fragment]` occurrence becomes
/// `scheme://host/<redacted>`, so API keys embedded in transport error
/// messages never reach client-facing response bodies. Text without URLs is
/// returned unchanged.
pub fn redact_urls_in_text(text: &str) -> String {
    const SCHEMES: &[&str] = &["https://", "http://", "wss://", "ws://"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let found = SCHEMES
            .iter()
            .filter_map(|scheme| rest.find(scheme).map(|idx| (idx, *scheme)))
            .min_by_key(|(idx, _)| *idx);
        let Some((idx, scheme)) = found else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..idx]);
        let after_scheme = &rest[idx + scheme.len()..];
        // The authority runs until '/', '?', '#', or any delimiter/whitespace.
        let authority_end = after_scheme
            .find(
                &[
                    '/', '?', '#', ' ', '\t', '\n', '\r', '"', '\'', '<', '>', '(', ')', '[',
                    ']', '{', '}',
                ][..],
            )
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..authority_end];
        let host = authority.rsplit('@').next().unwrap_or(authority);
        out.push_str(scheme);
        out.push_str(host);
        out.push_str("/<redacted>");
        // Skip the remainder of the original URL (path/query/fragment).
        let url_end = after_scheme
            .find(
                &[
                    ' ', '\t', '\n', '\r', '"', '\'', '<', '>', '(', ')', '[', ']', '{', '}',
                ][..],
            )
            .unwrap_or(after_scheme.len());
        rest = &after_scheme[url_end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_rpc_url_strips_api_key_forms() {
        assert_eq!(
            redact_rpc_url("https://eth-mainnet.g.alchemy.com/v2/SECRET"),
            "https://eth-mainnet.g.alchemy.com"
        );
        assert_eq!(
            redact_rpc_url("https://mainnet.infura.io/v3/SECRET"),
            "https://mainnet.infura.io"
        );
        assert_eq!(
            redact_rpc_url("https://host.example/path?apikey=SECRET"),
            "https://host.example"
        );
        assert_eq!(
            redact_rpc_url("https://user:token@host.example:8545/v2/x"),
            "https://host.example:8545"
        );
        assert_eq!(
            redact_rpc_url("http://127.0.0.1:9/v2/SECRETKEY123"),
            "http://127.0.0.1:9"
        );
        assert_eq!(redact_rpc_url("not-a-url"), "<redacted>");
        assert_eq!(redact_rpc_url("https://"), "<redacted>");
    }

    #[test]
    fn redact_urls_in_text_redacts_embedded_urls() {
        let input = "error sending request for url (http://127.0.0.1:9/v2/SECRETKEY123)";
        let out = redact_urls_in_text(input);
        assert!(!out.contains("SECRETKEY123"), "{out}");
        assert!(
            out.contains("http://127.0.0.1:9/<redacted>"),
            "{out}"
        );

        let input = "failed https://user:tok3n@host.example/v2/key?apikey=zzz then http://plain.example/path";
        let out = redact_urls_in_text(input);
        assert!(!out.contains("tok3n"), "{out}");
        assert!(!out.contains("/v2/key"), "{out}");
        assert!(!out.contains("apikey=zzz"), "{out}");
        assert!(out.contains("https://host.example/<redacted>"), "{out}");
        assert!(out.contains("http://plain.example/<redacted>"), "{out}");

        // Text without URLs is returned unchanged.
        assert_eq!(redact_urls_in_text("plain error, no url"), "plain error, no url");
        assert_eq!(redact_urls_in_text(""), "");
    }
}
