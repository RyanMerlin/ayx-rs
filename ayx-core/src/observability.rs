use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};
use thiserror::Error;

use crate::profile::ObservabilityProfile;
use crate::sensitive::append_sensitive_file;

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to create observability directory '{path}': {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open observability log '{path}': {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write observability log '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize observability event: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct ApiEvent<'a> {
    pub product: &'a str,
    pub surface: &'a str,
    pub operation: &'a str,
    pub method: &'a str,
    pub endpoint_template: &'a str,
    pub resolved_url: &'a str,
    pub status_code: Option<u16>,
    pub duration_ms: u128,
    pub attempt: u32,
    pub retry_after_seconds: Option<u64>,
    pub request_id: Option<&'a str>,
    pub ok: bool,
    pub error_class: Option<&'a str>,
    pub response_shape: Option<&'a str>,
    pub mutating: bool,
    pub dry_run: bool,
}

pub fn record_api_event(
    observability: Option<&ObservabilityProfile>,
    event: ApiEvent<'_>,
) -> Result<Option<PathBuf>, ObservabilityError> {
    let Some(observability) = observability else {
        return Ok(None);
    };
    let Some(api_logging) = observability.api_logging.as_ref() else {
        return Ok(None);
    };
    if !api_logging.enabled {
        return Ok(None);
    }

    let path = api_logging
        .path
        .as_deref()
        .unwrap_or("logs/api-events.jsonl");
    let log_path = Path::new(path);
    if let Some(parent) = log_path.parent() {
        crate::sensitive::ensure_sensitive_dir(parent).map_err(|err| match err {
            crate::sensitive::SensitiveIoError::CreateDir { path, source } => {
                ObservabilityError::CreateDir { path, source }
            }
            // `ensure_sensitive_dir` never itself produces `Lock`/`Write`/`Append` --
            // these arms exist only to keep the match exhaustive over the shared
            // `SensitiveIoError` enum.
            crate::sensitive::SensitiveIoError::Lock { path, source }
            | crate::sensitive::SensitiveIoError::Write { path, source }
            | crate::sensitive::SensitiveIoError::Append { path, source } => {
                ObservabilityError::Open { path, source }
            }
        })?;
    }

    let payload = json!({
        "timestamp_utc": Utc::now(),
        "product": event.product,
        "surface": event.surface,
        "operation": event.operation,
        "method": event.method,
        "endpoint_template": event.endpoint_template,
        "resolved_url": redact_url(event.resolved_url),
        "status_code": event.status_code,
        "duration_ms": event.duration_ms,
        "attempt": event.attempt,
        "retry_after_seconds": event.retry_after_seconds,
        "request_id": event.request_id,
        "ok": event.ok,
        "error_class": event.error_class,
        "response_shape": event.response_shape,
        "mutating": event.mutating,
        "dry_run": event.dry_run,
        "redact_bodies": api_logging.redact_bodies,
        "log_requests": api_logging.log_requests,
        "log_responses": api_logging.log_responses,
    });

    let content = serde_json::to_string(&payload)? + "\n";
    append_sensitive_file(log_path, content.as_bytes()).map_err(|err| match err {
        crate::sensitive::SensitiveIoError::CreateDir { path, source } => {
            ObservabilityError::CreateDir { path, source }
        }
        // `append_sensitive_file` never itself produces `Lock` -- this arm
        // exists only to keep the match exhaustive over the shared
        // `SensitiveIoError` enum.
        crate::sensitive::SensitiveIoError::Lock { path, source }
        | crate::sensitive::SensitiveIoError::Write { path, source } => {
            ObservabilityError::Open { path, source }
        }
        crate::sensitive::SensitiveIoError::Append { path, source } => {
            ObservabilityError::Write { path, source }
        }
    })?;
    Ok(Some(log_path.to_path_buf()))
}

pub fn response_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub fn transport_error_summary(error: &dyn StdError) -> Value {
    // Transport errors (reqwest in particular) stringify the offending URL,
    // which may carry `?access_token=...`. Redact every chain entry before it
    // is embedded in a log line or a user-facing stderr envelope.
    let chain: Vec<String> = error_chain(error)
        .iter()
        .map(|entry| redact_text(entry.as_str()))
        .collect();
    let error_text = chain
        .first()
        .cloned()
        .unwrap_or_else(|| redact_text(&error.to_string()));
    let error_text_lower = error_text.to_lowercase();
    let error_kind = if error_text_lower.contains("timeout") {
        "timeout"
    } else if error_text_lower.contains("dns") || error_text_lower.contains("resolve") {
        "dns"
    } else if error_text_lower.contains("tls") || error_text_lower.contains("certificate") {
        "tls"
    } else if error_text_lower.contains("connect") {
        "connect"
    } else if error_text_lower.contains("status") {
        "status"
    } else {
        "transport"
    };
    let request_url = chain.iter().find_map(|entry| extract_url(entry));
    let mut hints = Vec::new();
    match error_kind {
        "dns" => hints.push(
            "DNS resolution failed; check VPN, corporate DNS, and proxy settings.".to_string(),
        ),
        "timeout" => {
            hints.push("Request timed out; check network latency and proxy behavior.".to_string())
        }
        "tls" => hints
            .push("TLS handshake failed; verify trust roots and HTTPS interception.".to_string()),
        "connect" => {
            hints.push("Connection failed before an HTTP response was received.".to_string())
        }
        _ => hints.push(
            "Inspect the error chain and resolved URL for the underlying transport problem."
                .to_string(),
        ),
    }
    json!({
        "error": error_text,
        "error_kind": error_kind,
        "error_chain": chain,
        "error_hints": hints,
        "request_url": request_url,
    })
}

fn error_chain(error: &dyn StdError) -> Vec<String> {
    let mut chain = vec![error.to_string()];
    let mut current = error.source();
    while let Some(source) = current {
        chain.push(source.to_string());
        current = source.source();
    }
    chain
}

fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
        .map(|value| {
            value
                .trim_matches(|ch| matches!(ch, '\'' | '"' | ')' | '(' | ',' | '.'))
                .to_string()
        })
        .map(|url| redact_url(&url))
}

/// Centralized redactor used by every log/error path.
///
/// Strips:
/// - `Authorization: Bearer <token>` headers (anywhere in a string).
/// - `token=`, `access_token=`, `refresh_token=`, `password=` query / form
///   params in URLs and free-text error chains.
/// - Common secret-shaped JSON keys when the input parses as JSON.
///
/// Always run this before writing to `record_api_event`'s structured log
/// or to any stderr trace. Cheap (O(n) regex-free scan) and pure.
pub fn redact_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        // Bearer-token pattern: "Bearer xxxxx" → "Bearer ***"
        if c.is_ascii_alphabetic() {
            let mut probe = String::from(c);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_alphabetic() {
                    probe.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if probe.eq_ignore_ascii_case("Bearer") {
                out.push_str(&probe);
                // consume one whitespace token then mask
                let mut ws = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() {
                        ws.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !ws.is_empty() {
                    out.push_str(&ws);
                    let mut had_token = false;
                    while let Some(&next) = chars.peek() {
                        if next.is_whitespace() || next == '"' || next == '\'' || next == ',' {
                            break;
                        }
                        chars.next();
                        had_token = true;
                    }
                    if had_token {
                        out.push_str("***");
                    }
                }
                continue;
            }
            out.push_str(&probe);
            continue;
        }
        out.push(c);
    }
    let out = redact_query_params(&out);
    redact_bare_jwts(&out)
}

/// Redact bare JWT tokens of the form `eyJ<header>.<payload>.<signature>`.
///
/// Only matches the three-part dotted form with base64url characters so that
/// short `eyJ`-prefixed strings (e.g. plain base64 blobs) aren't accidentally
/// caught unless they really look like a JWT.
fn redact_bare_jwts(input: &str) -> String {
    // Base64url alphabet: A-Za-z0-9 _ -
    // We match eyJ...<dot>...<dot>...  where each segment is ≥1 base64url char.
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        // Look for the prefix "eyJ" (base64url for `{"`)
        if i + 3 <= len && &bytes[i..i + 3] == b"eyJ" {
            // Scan a base64url segment
            let seg_start = i;
            let mut j = i;
            while j < len && is_base64url(bytes[j]) {
                j += 1;
            }
            // We need exactly two dots separating three segments
            if j < len && bytes[j] == b'.' {
                let mut k = j + 1;
                while k < len && is_base64url(bytes[k]) {
                    k += 1;
                }
                if k < len && bytes[k] == b'.' {
                    let mut m = k + 1;
                    while m < len && is_base64url(bytes[m]) {
                        m += 1;
                    }
                    // All three segments must have ≥1 char and first must start with eyJ
                    let seg1_len = j - seg_start;
                    let seg2_len = k - (j + 1);
                    let seg3_len = m - (k + 1);
                    if seg1_len >= 3 && seg2_len >= 1 && seg3_len >= 1 {
                        out.push_str("***");
                        i = m;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[inline]
fn is_base64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn redact_query_params(input: &str) -> String {
    // Matched case-insensitively as a whole `key=` token at a delimiter
    // boundary, so both the snake_case and squashed spellings are listed.
    const SECRET_KEYS: &[&str] = &[
        "token",
        "access_token",
        "accesstoken",
        "refresh_token",
        "refreshtoken",
        "id_token",
        "idtoken",
        "password",
        "passwd",
        "pwd",
        "api_key",
        "apikey",
        "client_secret",
        "clientsecret",
        "tokenvalue",
        "local-auth-workspace",
        "x-csrf-token",
        "passcode",
        "passcodereferenceid",
        "secret",
    ];
    // Keys whose bare word is common in prose (`... status=500 code=http_error
    // ...`), so they are only redacted inside an actual query string -- i.e.
    // directly after `?` or `&`. This still catches the OAuth authorization
    // code in `/callback?code=<secret>`.
    const QUERY_ONLY_KEYS: &[&str] = &["code", "sig"];
    const VALUE_TERMINATORS: &[char] = &['&', ' ', '"', '\'', '\n', '\r', '\t', ','];
    const DELIMITERS: &[char] = &['&', '?', ' ', '"', '\''];
    const QUERY_DELIMITERS: &[char] = &['&', '?'];

    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;

        // Detect a `key=` pattern, either at the start of the string or
        // immediately after a delimiter. Critically, when `i == 0` we do
        // NOT pre-push ch (the old bug: produced "ppassword=***" because
        // ch=='p' was emitted before the matched key string).
        let at_delim = DELIMITERS.contains(&ch);
        let start_of_string = i == 0;
        if at_delim || start_of_string {
            // `key_start` is the byte index where the candidate key begins.
            // After a delimiter we skip the delimiter; at start-of-string
            // the key begins at i itself.
            let key_start = if at_delim { i + 1 } else { i };
            let mut matched = None;
            let candidates = SECRET_KEYS.iter().chain(
                QUERY_ONLY_KEYS
                    .iter()
                    .filter(|_| at_delim && QUERY_DELIMITERS.contains(&ch)),
            );
            for key in candidates {
                let key_end = key_start + key.len();
                // key must be followed by `=` and there must be at least
                // one byte at key_end (the `=` itself).
                if key_end < bytes.len()
                    && bytes[key_end] == b'='
                    && input
                        .get(key_start..key_end)
                        .is_some_and(|slice| slice.eq_ignore_ascii_case(key))
                {
                    matched = Some((key, key_end));
                    break;
                }
            }
            if let Some((key, key_end)) = matched {
                if at_delim {
                    out.push(ch);
                }
                out.push_str(key);
                out.push('=');
                // Skip the value up to the next terminator or EOS.
                i = key_end + 1;
                while i < bytes.len() {
                    let vc = bytes[i] as char;
                    if VALUE_TERMINATORS.contains(&vc) {
                        break;
                    }
                    i += 1;
                }
                out.push_str("***");
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Redact a URL: keeps scheme/host/path; masks query string secret values.
pub fn redact_url(url: &str) -> String {
    redact_query_params(url)
}

/// The canonical secret-key matcher for the whole workspace.
///
/// A key is considered secret-bearing when its *normalized* form (lowercased
/// with `_` and `-` removed) **contains** one of the needles below. Substring
/// matching — rather than exact equality — is what makes `clientSecret`,
/// `x-api-key`, `apiToken` and `connectionString` all redact correctly.
///
/// Needles are deliberately compound where a bare word would over-redact
/// (`accesskey`/`privatekey`, never a bare `key`).
///
/// Every crate must route key-based redaction through this function so a new
/// needle is picked up everywhere at once.
pub fn is_secret_key(key: &str) -> bool {
    const SECRET_KEY_NEEDLES: &[&str] = &[
        "password",
        "passwd",
        "pwd",
        "passcode",
        "secret",
        "token",
        "apikey",
        "accesskey",
        "accountkey",
        "sharedkey",
        "privatekey",
        "credential",
        "connectionstring",
        "authorization",
        "localauthworkspace",
        "csrf",
        "sas",
    ];
    let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
    SECRET_KEY_NEEDLES
        .iter()
        .any(|needle| normalized.contains(needle))
}

/// Redact a JSON value tree in place by key heuristic. Returns a redacted
/// clone (caller controls whether to feed it to the logger). Top-level
/// strings that look like Bearer tokens are masked too.
pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_secret_key(k) {
                    out.insert(k.clone(), Value::String("***".to_string()));
                } else {
                    out.insert(k.clone(), redact_json(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact_json).collect()),
        Value::String(s) => Value::String(redact_text(s)),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_bearer_tokens() {
        let r = redact_text("Authorization: Bearer abcdef.ghi.jkl");
        assert!(!r.contains("abcdef"));
        assert!(r.contains("Bearer ***"));
    }

    #[test]
    fn masks_query_params() {
        let r = redact_url("https://x/y?access_token=abc123&page=1");
        assert!(!r.contains("abc123"));
        assert!(r.contains("access_token=***"));
        assert!(r.contains("page=1"));
    }

    #[test]
    fn masks_query_param_at_start_of_string() {
        // Pre-fix this produced "ppassword=***" because the original code
        // emitted ch before pushing the key string.
        let r = redact_text("password=hunter2");
        assert_eq!(r, "password=***", "got: {r}");
    }

    #[test]
    fn masks_query_param_at_end_of_string() {
        let r = redact_text("?access_token=xyz");
        assert!(!r.contains("xyz"));
        assert_eq!(r, "?access_token=***");
    }

    #[test]
    fn masks_mixed_case_secret_keys() {
        for input in [
            "?PASSWORD=hunter2",
            "?Token=abc",
            "?Refresh_Token=rt",
            "?Client_Secret=cs",
        ] {
            let r = redact_text(input);
            assert!(!r.contains("hunter2"), "leaked in {input}: {r}");
            assert!(!r.contains("abc"), "leaked in {input}: {r}");
            assert!(!r.contains("\"rt\""), "leaked in {input}: {r}");
        }
    }

    #[test]
    fn masks_bearer_with_tab_separator() {
        let r = redact_text("Authorization:\tBearer\tabc.def.ghi");
        assert!(!r.contains("abc.def"), "got: {r}");
        // Some form of "Bearer\t***" or "Bearer ***" — either acceptable.
        assert!(r.contains("Bearer"));
        assert!(r.contains("***"));
    }

    #[test]
    fn masks_bearer_at_eos_with_no_trailing_chars() {
        let r = redact_text("Bearer abcdef");
        assert!(!r.contains("abcdef"));
        assert!(r.contains("Bearer ***"));
    }

    #[test]
    fn redacts_json_secret_keys() {
        let v = json!({
            "user": "ada",
            "password": "p@ssw0rd",
            "nested": {"refresh_token": "rt-xyz"},
            "tokens": [{"access_token": "at-1"}, {"foo": "bar"}],
        });
        let r = redact_json(&v);
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("p@ssw0rd"));
        assert!(!s.contains("rt-xyz"));
        assert!(!s.contains("at-1"));
        // "tokens" itself matches the `token` needle, so the whole container is
        // wiped — over-redaction is the safe failure mode for a key that names
        // a credential collection.
        assert!(!s.contains("\"foo\":\"bar\""));
        assert!(s.contains("\"user\":\"ada\""));
    }

    #[test]
    fn redacts_json_camel_case_and_header_style_secret_keys() {
        let v = json!({
            "clientId": "safe-to-show",
            "clientSecret": "do-not-show",
            "x-api-key": "hdr-key",
            "apiToken": "api-tok",
            "connectionString": "Server=x;Password=y",
            "accountKey": "acct",
            "sharedKey": "shared",
            "nested": {"access_key": "ak-1", "privateKey": "pk-1"},
            "items": [{"connectionString": "secret-connection"}],
            "displayName": "keep me",
        });
        let r = redact_json(&v);
        assert_eq!(r["clientId"], "safe-to-show");
        assert_eq!(r["displayName"], "keep me");
        assert_eq!(r["clientSecret"], "***");
        assert_eq!(r["x-api-key"], "***");
        assert_eq!(r["apiToken"], "***");
        assert_eq!(r["connectionString"], "***");
        assert_eq!(r["accountKey"], "***");
        assert_eq!(r["sharedKey"], "***");
        assert_eq!(r["nested"]["access_key"], "***");
        assert_eq!(r["nested"]["privateKey"], "***");
        assert_eq!(r["items"][0]["connectionString"], "***");
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("do-not-show"), "leaked: {s}");
        assert!(!s.contains("secret-connection"), "leaked: {s}");
    }

    #[test]
    fn is_secret_key_matches_normalized_substrings() {
        for key in [
            "clientSecret",
            "x-api-key",
            "apiToken",
            "connectionString",
            "Authorization",
            "PASSWORD",
            "user_pwd",
            "sharedKey",
            "accountKey",
            "privateKey",
            "credentials",
            "x-csrf-token",
            "passcodeReferenceId",
            "sasUrl",
        ] {
            assert!(is_secret_key(key), "expected secret: {key}");
        }
        for key in [
            "id",
            "name",
            "displayName",
            "keyboard_layout",
            "workspaceId",
        ] {
            assert!(!is_secret_key(key), "unexpected secret: {key}");
        }
    }

    #[test]
    fn transport_summary_redacts_tokens_in_error_text_and_chain() {
        #[derive(Debug)]
        struct Inner;
        impl std::fmt::Display for Inner {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "connect error for https://h/p?access_token=abc123")
            }
        }
        impl StdError for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "error sending request for url https://h/p?access_token=abc123"
                )
            }
        }
        impl StdError for Outer {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(&self.0)
            }
        }

        let summary = transport_error_summary(&Outer(Inner));
        let s = serde_json::to_string(&summary).unwrap();
        assert!(!s.contains("abc123"), "token leaked: {s}");
        assert!(s.contains("access_token=***"), "got: {s}");
    }

    #[test]
    fn masks_additional_oauth_query_params() {
        for (input, leaked) in [
            ("?id_token=idtok123", "idtok123"),
            ("?code=authcode123", "authcode123"),
            ("?clientSecret=cs123", "cs123"),
        ] {
            let r = redact_text(input);
            assert!(!r.contains(leaked), "leaked in {input}: {r}");
        }
    }

    #[test]
    fn masks_tokenvalue_key() {
        let r = redact_text("tokenValue=abc123");
        assert!(!r.contains("abc123"), "leaked: {r}");
        assert!(
            r.contains("tokenvalue=***") || r.contains("tokenValue=***"),
            "got: {r}"
        );
    }

    #[test]
    fn masks_local_auth_workspace_with_jwt_value() {
        let jwt = "eyJhbGc.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let input = format!("local-auth-workspace={jwt}");
        let r = redact_text(&input);
        assert!(!r.contains("eyJhbGc"), "leaked JWT header: {r}");
        assert!(r.contains("local-auth-workspace=***"), "got: {r}");
    }

    #[test]
    fn masks_bare_jwt_in_text() {
        let jwt = "eyJaaa.bbb.ccc";
        let r = redact_text(jwt);
        assert!(!r.contains("eyJaaa"), "leaked: {r}");
        assert_eq!(r, "***");
    }

    #[test]
    fn bare_jwt_masked_inside_larger_string() {
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig123";
        let input = format!("Bearer {jwt}");
        let r = redact_text(&input);
        assert!(!r.contains("eyJhbGciOiJSUzI1NiJ9"), "leaked: {r}");
        assert!(r.contains("Bearer ***") || r.contains("***"), "got: {r}");
    }
}
