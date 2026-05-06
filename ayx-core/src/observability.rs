use std::fs::{self, OpenOptions};
use std::error::Error as StdError;
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
        "resolved_url": event.resolved_url,
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
        "dns" => hints.push("DNS resolution failed; check VPN, corporate DNS, and proxy settings.".to_string()),
        "timeout" => hints.push("Request timed out; check network latency and proxy behavior.".to_string()),
        "tls" => hints.push("TLS handshake failed; verify trust roots and HTTPS interception.".to_string()),
        "connect" => hints.push("Connection failed before an HTTP response was received.".to_string()),
        _ => hints.push("Inspect the error chain and resolved URL for the underlying transport problem.".to_string()),
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
        .map(|value| value.trim_matches(|ch| matches!(ch, '\'' | '"' | ')' | '(' | ',' | '.')).to_string())
}
