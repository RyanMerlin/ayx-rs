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

    /// Parse a wire-format code string (the `as_str` output) back into a code.
    ///
    /// Needed because `ayx-server-api` embeds `error_code=<code>` in the anyhow
    /// message it bails with, and the outer dispatcher has to recover the
    /// classification rather than re-guessing it from prose.
    pub fn parse_code(s: &str) -> Option<Self> {
        Some(match s {
            "config_missing" => ErrorCode::ConfigMissing,
            "auth_failed" => ErrorCode::AuthFailed,
            "permission_denied" => ErrorCode::PermissionDenied,
            "not_found" => ErrorCode::NotFound,
            "validation" => ErrorCode::Validation,
            "conflict" => ErrorCode::Conflict,
            "rate_limited" => ErrorCode::RateLimited,
            "network" => ErrorCode::Network,
            "upstream" => ErrorCode::Upstream,
            "workspace_mismatch" => ErrorCode::WorkspaceMismatch,
            "internal" => ErrorCode::Internal,
            _ => return None,
        })
    }

    /// Classify an HTTP status code into an `ErrorCode`. 2xx returns `None`.
    ///
    /// Only statuses with an unambiguous home in this enum are mapped. The rest
    /// deliberately fall through to `Internal`, because for a CLI the usual
    /// cause of a `405`, `415`, `426`, or `431` is that `ayx` built the request
    /// wrong — wrong method, wrong content type, oversized header — which is
    /// exactly what `Internal` means. Guessing a friendlier code for those
    /// would trade one wrong signal for another and send the caller to check
    /// their flags for a bug in this binary.
    pub fn from_http_status(status: u16) -> Option<Self> {
        match status {
            200..=299 => None,
            401 => Some(ErrorCode::AuthFailed),
            // 402 is an entitlement/plan-tier denial and 451 a policy denial;
            // both are "you may not have this", not "your input was malformed".
            402 | 403 | 451 => Some(ErrorCode::PermissionDenied),
            // A deleted resource answers 410, not 404 — Alteryx One returns
            // `GoneException` for a flow that existed and was removed
            // (live-verified against a flow create/delete/read cycle). For a
            // caller the outcome is identical to 404: the object is not there.
            404 | 410 => Some(ErrorCode::NotFound),
            // Precondition/lock failures are state conflicts, which is what
            // `Conflict` already covers for 409.
            409 | 412 | 423 | 428 => Some(ErrorCode::Conflict),
            429 => Some(ErrorCode::RateLimited),
            // A request timeout is a transport outcome, and `Network` names
            // timeouts explicitly. Classifying it as `Validation` would tell an
            // agent to inspect its flags for something a retry may well fix.
            408 => Some(ErrorCode::Network),
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

    /// Found live: deleting a One flow and then reading it back returns 410
    /// `GoneException`, which fell through the catch-all and was reported as
    /// `error_code: "internal"`. That tells an agent branching on the code that
    /// `ayx` malfunctioned and the call is worth retrying, when in fact the
    /// object is gone and the answer will never change.
    #[test]
    fn a_deleted_resource_is_not_found_not_internal() {
        assert_eq!(ErrorCode::from_http_status(410), Some(ErrorCode::NotFound));
    }

    /// Each mapped 4xx must land in the bucket a caller can act on. An earlier
    /// version of this fix swept every unenumerated 4xx into `Validation` on
    /// the theory that "the server rejected the request, so it is not our
    /// fault". That traded one wrong signal for another: `Validation`'s own
    /// contract is "malformed input, bad flag combination", and the CLI hint
    /// for it tells the user to check their flags and `--help`. Sending someone
    /// there for a `423 Locked` or a `408 Request Timeout` is misdirection.
    #[test]
    fn mapped_client_errors_land_in_an_actionable_bucket() {
        // Entitlement and policy denials are "you may not have this".
        for status in [402u16, 403, 451] {
            assert_eq!(
                ErrorCode::from_http_status(status),
                Some(ErrorCode::PermissionDenied),
                "{status} is a denial, not malformed input"
            );
        }
        // Precondition and lock failures are state conflicts, like 409.
        for status in [409u16, 412, 423, 428] {
            assert_eq!(
                ErrorCode::from_http_status(status),
                Some(ErrorCode::Conflict),
                "{status} is a state conflict"
            );
        }
        // A timeout is a transport outcome and may succeed on retry; calling it
        // `Validation` would point the caller at their flags instead.
        assert_eq!(ErrorCode::from_http_status(408), Some(ErrorCode::Network));
    }

    /// The statuses left on `Internal` are there on purpose, not by neglect.
    ///
    /// For a CLI, a `405`, `415`, `426`, or `431` almost always means `ayx`
    /// built the request wrong -- wrong method, wrong content type, oversized
    /// header. That is precisely what `Internal` means, and dressing it up as a
    /// caller-side validation failure would send someone hunting through their
    /// own flags for a bug in this binary.
    #[test]
    fn request_construction_failures_stay_internal() {
        for status in [405u16, 415, 426, 431] {
            assert_eq!(
                ErrorCode::from_http_status(status),
                Some(ErrorCode::Internal),
                "{status} indicates ayx built the request wrong"
            );
        }
    }

    /// `parse_code` must round-trip every variant, or the Server API path
    /// silently loses the classification `from_http_status` just computed.
    #[test]
    fn every_error_code_round_trips_through_its_wire_string() {
        for code in [
            ErrorCode::ConfigMissing,
            ErrorCode::AuthFailed,
            ErrorCode::PermissionDenied,
            ErrorCode::NotFound,
            ErrorCode::Validation,
            ErrorCode::Conflict,
            ErrorCode::RateLimited,
            ErrorCode::Network,
            ErrorCode::Upstream,
            ErrorCode::WorkspaceMismatch,
            ErrorCode::Internal,
        ] {
            assert_eq!(
                ErrorCode::parse_code(code.as_str()),
                Some(code),
                "{} does not round-trip",
                code.as_str()
            );
        }
        assert_eq!(ErrorCode::parse_code("not a code"), None);
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
