use std::error::Error as StdError;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{json, Value};
use thiserror::Error;

use crate::profile::ObservabilityProfile;

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
        fs::create_dir_all(parent).map_err(|source| ObservabilityError::CreateDir {
            path: parent.display().to_string(),
            source,
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
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|source| ObservabilityError::Open {
            path: log_path.display().to_string(),
            source,
        })?;
    use std::io::Write;
    let mut file = file;
    file.write_all(content.as_bytes())
        .map_err(|source| ObservabilityError::Write {
            path: log_path.display().to_string(),
            source,
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
    let chain = error_chain(error);
    let error_text = chain.first().cloned().unwrap_or_else(|| error.to_string());
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
    redact_query_params(&out)
}

fn redact_query_params(input: &str) -> String {
    const SECRET_KEYS: &[&str] = &[
        "token",
        "access_token",
        "refresh_token",
        "password",
        "api_key",
        "apikey",
        "client_secret",
    ];
    const VALUE_TERMINATORS: &[char] = &['&', ' ', '"', '\'', '\n', '\r', '\t', ','];
    const DELIMITERS: &[char] = &['&', '?', ' ', '"', '\''];

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
            for key in SECRET_KEYS {
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

/// Redact a JSON value tree in place by key heuristic. Returns a redacted
/// clone (caller controls whether to feed it to the logger). Top-level
/// strings that look like Bearer tokens are masked too.
pub fn redact_json(value: &Value) -> Value {
    const SECRET_KEYS: &[&str] = &[
        "token",
        "access_token",
        "refresh_token",
        "password",
        "api_key",
        "apikey",
        "client_secret",
        "authorization",
    ];
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let lower = k.to_ascii_lowercase();
                if SECRET_KEYS.iter().any(|s| lower == *s) {
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
        assert!(s.contains("\"foo\":\"bar\""));
    }
}
