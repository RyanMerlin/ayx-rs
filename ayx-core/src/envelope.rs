use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

/// Machine-readable error classification.
///
/// Callers (CLI parsers, agents, automation) can branch on this without
/// parsing the human-readable `message` string. Keep the variants stable —
/// they are part of the public CLI contract.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Profile/config missing, malformed, or incomplete.
    ConfigMissing,
    /// Authentication failed: missing/expired/revoked token, bad credentials.
    AuthFailed,
    /// Caller lacks permission for the requested resource.
    PermissionDenied,
    /// Resource (workflow, plan, person, …) was not found.
    NotFound,
    /// Caller-side validation failure: malformed input, bad flag combination.
    Validation,
    /// State conflict (409): resource already exists, version stale, etc.
    Conflict,
    /// Server rate-limited the request (429).
    RateLimited,
    /// Network transport error: DNS, TCP, TLS, timeout, connection reset.
    Network,
    /// 5xx server error from upstream.
    Upstream,
    /// Workspace identity preflight detected the wrong workspace.
    WorkspaceMismatch,
    /// Generic internal error / unexpected condition.
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ConfigMissing => "config_missing",
            ErrorCode::AuthFailed => "auth_failed",
            ErrorCode::PermissionDenied => "permission_denied",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Validation => "validation",
            ErrorCode::Conflict => "conflict",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Network => "network",
            ErrorCode::Upstream => "upstream",
            ErrorCode::WorkspaceMismatch => "workspace_mismatch",
            ErrorCode::Internal => "internal",
        }
    }

    /// Classify an HTTP status code into an `ErrorCode`. 2xx returns `None`.
    pub fn from_http_status(status: u16) -> Option<Self> {
        match status {
            200..=299 => None,
            401 => Some(ErrorCode::AuthFailed),
            403 => Some(ErrorCode::PermissionDenied),
            404 => Some(ErrorCode::NotFound),
            409 => Some(ErrorCode::Conflict),
            429 => Some(ErrorCode::RateLimited),
            400 | 422 => Some(ErrorCode::Validation),
            500..=599 => Some(ErrorCode::Upstream),
            _ => Some(ErrorCode::Internal),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub message: String,
    pub timestamp_utc: DateTime<Utc>,
    pub data: Value,
    /// Machine-readable error classification. Absent on success envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
}

impl Envelope {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data: Value::Null,
            error_code: None,
        }
    }

    pub fn ok_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: true,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data,
            error_code: None,
        }
    }

    pub fn err_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: false,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data,
            error_code: Some(ErrorCode::Internal),
        }
    }

    /// Build an error envelope with an explicit `ErrorCode`. Prefer this over
    /// `err_with_data` for any new code path so callers can branch on the
    /// classification.
    pub fn err_coded(code: ErrorCode, message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: false,
            message: message.into(),
            timestamp_utc: Utc::now(),
            data,
            error_code: Some(code),
        }
    }

    /// Set/override the error_code on an existing envelope. Useful at the
    /// outer dispatch layer when classifying anyhow errors.
    pub fn with_error_code(mut self, code: ErrorCode) -> Self {
        self.error_code = Some(code);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_http_statuses() {
        assert!(ErrorCode::from_http_status(200).is_none());
        assert_eq!(
            ErrorCode::from_http_status(401),
            Some(ErrorCode::AuthFailed)
        );
        assert_eq!(
            ErrorCode::from_http_status(403),
            Some(ErrorCode::PermissionDenied)
        );
        assert_eq!(ErrorCode::from_http_status(404), Some(ErrorCode::NotFound));
        assert_eq!(ErrorCode::from_http_status(409), Some(ErrorCode::Conflict));
        assert_eq!(
            ErrorCode::from_http_status(429),
            Some(ErrorCode::RateLimited)
        );
        assert_eq!(
            ErrorCode::from_http_status(422),
            Some(ErrorCode::Validation)
        );
        assert_eq!(ErrorCode::from_http_status(502), Some(ErrorCode::Upstream));
    }

    #[test]
    fn err_coded_emits_code_in_serialization() {
        let env = Envelope::err_coded(ErrorCode::WorkspaceMismatch, "nope", Value::Null);
        let serialized = serde_json::to_string(&env).unwrap();
        assert!(serialized.contains("\"error_code\":\"workspace_mismatch\""));
        assert!(serialized.contains("\"ok\":false"));
    }

    #[test]
    fn ok_envelope_skips_error_code_field() {
        let env = Envelope::ok("done");
        let serialized = serde_json::to_string(&env).unwrap();
        assert!(!serialized.contains("error_code"));
    }
}
