use std::cell::RefCell;
use std::fs;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ayx_core::auth::CredentialBinding;
use ayx_core::envelope::Envelope;
use ayx_core::observability::{
    ApiEvent, is_secret_key, record_api_event, redact_text, response_shape, transport_error_summary,
};
use ayx_core::one_endpoint::OneEndpoint;
use ayx_core::profile::Config;
use ayx_core::sensitive::write_sensitive_file;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use serde_json::{Value, json};
use std::collections::HashSet;
use url::form_urlencoded::Serializer;
const ONE_API_BASE_URL: &str = "https://us1.alteryxcloud.com";

/// Canonical identity returned by `/v4/workspaces/current`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentWorkspaceIdentity {
    pub workspace_id: String,
    pub workspace_gid: String,
    pub display_name: Option<String>,
}

/// Parse the current-workspace response in one place for login, switch, and
/// mutation preflight. Numeric IDs may arrive as JSON numbers or strings, but
/// the canonical representation is always a non-empty decimal string.
pub fn parse_current_workspace_identity(value: &Value) -> Result<CurrentWorkspaceIdentity> {
    let value = value.get("response").unwrap_or(value);
    let workspace_id = value
        .get("id")
        .or_else(|| value.get("workspaceId"))
        .or_else(|| value.get("workspace_id"))
        .and_then(|id| {
            id.as_u64()
                .map(|id| id.to_string())
                .or_else(|| id.as_str().map(str::to_string))
        })
        .filter(|id| !id.is_empty() && id.chars().all(|character| character.is_ascii_digit()))
        .ok_or_else(|| {
            anyhow::anyhow!("current workspace response did not include a numeric workspace ID")
        })?;
    let workspace_gid = value
        .get("gid")
        .or_else(|| value.get("workspaceGid"))
        .or_else(|| value.get("workspace_gid"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|gid| !gid.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!("current workspace response did not include a workspace GID")
        })?;
    let display_name = ["displayName", "name"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(str::to_string);
    Ok(CurrentWorkspaceIdentity {
        workspace_id,
        workspace_gid,
        display_name,
    })
}

/// Probe the authoritative current workspace for a bearer token. This is the
/// single bounded network operation used to establish workspace identity before
/// credentials are activated or persisted.
pub fn probe_current_workspace(
    base_url: &str,
    access_token: &str,
) -> Result<CurrentWorkspaceIdentity> {
    let endpoint = format!("{}/v4/workspaces/current", base_url.trim_end_matches('/'));
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("could not create workspace verification client")?;
    let response = client
        .get(&endpoint)
        .header(AUTHORIZATION, bearer_authorization_value(access_token))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .context("workspace verification request failed")?;
    let status = response.status();
    let text = response
        .text()
        .context("failed to read workspace verification response")?;
    if !status.is_success() {
        bail!(
            "workspace verification failed with HTTP {} ({})",
            status.as_u16(),
            redact_text(&text.chars().take(200).collect::<String>())
        );
    }
    let body: Value =
        serde_json::from_str(&text).context("workspace verification returned invalid JSON")?;
    parse_current_workspace_identity(&body)
}

/// Probe the active profile's token against the authoritative workspace
/// endpoint. Callers should compare the returned identity before persisting
/// active-workspace changes or sending a mutation.
pub fn probe_config_current_workspace(config: &Config) -> Result<CurrentWorkspaceIdentity> {
    let client = build_client()?;
    let token = resolve_one_access_token(config, &client)?;
    probe_current_workspace(&resolve_one_base_url(config), &token)
}

#[cfg(test)]
mod current_workspace_identity_tests {
    use super::parse_current_workspace_identity;
    use serde_json::json;

    #[test]
    fn accepts_gateway_aliases_and_numeric_id_strings() {
        let identity = parse_current_workspace_identity(&json!({
            "response": {
                "workspace_id": "42",
                "workspace_gid": "gid-42",
                "displayName": "Finance"
            }
        }))
        .expect("identity should parse");
        assert_eq!(identity.workspace_id, "42");
        assert_eq!(identity.workspace_gid, "gid-42");
        assert_eq!(identity.display_name.as_deref(), Some("Finance"));
    }

    #[test]
    fn rejects_missing_or_non_numeric_identity() {
        for value in [
            json!({"id": "workspace-name", "gid": "gid-1"}),
            json!({"id": 42}),
            json!({"id": 42, "gid": ""}),
        ] {
            assert!(parse_current_workspace_identity(&value).is_err());
        }
    }
}

/// Parses a production authentication endpoint.  Unit tests that exercise the
/// HTTP transport use explicit loopback fixtures; that exception is compiled
/// only for this crate's own test binary and is never available to consumers.
pub(crate) fn trusted_one_endpoint(value: &str) -> Result<OneEndpoint> {
    #[cfg(test)]
    {
        if let Ok(endpoint) = OneEndpoint::for_test_localhost(value) {
            return Ok(endpoint);
        }
    }
    OneEndpoint::parse(value).map_err(anyhow::Error::from)
}

mod coverage;
pub mod email_otp;
mod inventory;
pub mod otp_compat;
pub mod platform;
pub mod types;

pub use coverage::{CoverageReport, MissingEndpoint, StaleEndpoint, coverage};
pub use email_otp::{
    OtpAuthResult, WizardOtpSession, email_otp_login, email_otp_login_with_password,
};
pub use otp_compat::{
    LEGACY_OTP_COMPATIBILITY_VERSION, LegacyOtpAdapter, LegacyOtpCompatibilityContract,
    WizardOtpAdapter,
};

thread_local! {
    static ONE_APPLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NO_VERIFY_TLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DEBUG_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // `ManuallyDrop` is deliberate, not an oversight: `reqwest::blocking::Client`
    // is a thin handle around an `Arc<InnerClientHandle>` whose `Drop` joins the
    // dedicated background thread that runs its internal tokio runtime. When
    // the *last* reference — this cached one — is dropped as part of this
    // thread-local's own teardown, that join runs from inside a thread-local
    // destructor callback invoked by the OS during main-thread/process exit
    // (on Windows, via FLS; this fires whether `main()` returns normally or
    // calls `std::process::exit()` — that call only skips libstd's own
    // at-exit/destructor bookkeeping, not OS-level TLS/FLS callbacks tied to a
    // non-trivially-droppable thread-local). By the time that callback runs,
    // Windows may already be tearing down other threads, so joining one here
    // is fragile and was the actual cause of the "thread local panicked on
    // drop, aborting" crash on successful commands (see docs/releases notes /
    // tokio-rs/tokio#593 for the same class of bug). `ManuallyDrop` makes this
    // thread-local's stored value provably non-drop-needing, so no destructor
    // is ever registered for it at all — the background thread (and its
    // channel/JoinHandle) is intentionally leaked for the life of the
    // process, which is fine for a short-lived CLI invocation.
    static ONE_HTTP_CLIENT: RefCell<Option<ManuallyDrop<Client>>> = const { RefCell::new(None) };
}

/// Disable TLS certificate verification for the calling thread. Lab/dev only.
pub fn set_no_verify_tls(disabled: bool) {
    NO_VERIFY_TLS.with(|c| c.set(disabled));
}

pub fn no_verify_tls() -> bool {
    NO_VERIFY_TLS.with(|c| c.get())
}

/// Enable debug-level per-call tracing to stderr (redacted).
pub fn set_debug_trace(on: bool) {
    DEBUG_TRACE.with(|c| c.set(on));
}

pub fn debug_trace() -> bool {
    DEBUG_TRACE.with(|c| c.get())
}

fn trace_one(message: impl AsRef<str>) {
    if debug_trace() {
        eprintln!("[one-debug] {}", redact_text(message.as_ref()));
    }
}

/// Build the Authorization header from either a raw access token or a token
/// persisted by an older CLI version that already contains the Bearer scheme.
/// Keeping this normalization at the request boundary prevents a stale saved
/// value from becoming `Bearer Bearer <token>`.
fn bearer_authorization_value(access_token: &str) -> String {
    let token = access_token.trim();
    let token = token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(token)
        .trim();
    format!("Bearer {token}")
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let token = token
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| token.trim().strip_prefix("bearer "))
        .unwrap_or(token.trim());
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
    serde_json::from_slice::<Value>(&decoded)
        .ok()
        .filter(|value| value.is_object())
}

fn workspace_context_from_claims(claims: &Value) -> Option<String> {
    let scope = claims.get("scope")?;
    let candidates: Vec<&str> = match scope {
        Value::String(value) => value.split_whitespace().collect(),
        Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    candidates
        .into_iter()
        .find(|value| value.starts_with("w:"))
        .map(|value| value.to_string())
}

fn workspace_context_from_token(token: Option<&str>) -> Option<String> {
    let claims = decode_jwt_claims(token?)?;
    workspace_context_from_claims(&claims)
}

fn workspace_context_header_value(config: &Config) -> Option<String> {
    let one = config.alteryx_one.as_ref()?;

    if let Some(workspace_id) = one
        .active_workspace_id()
        .filter(|value| value.starts_with("w:"))
    {
        return Some(workspace_id.to_string());
    }

    if let Some(workspace_id) = one
        .expected_workspace_id
        .as_deref()
        .filter(|value| value.starts_with("w:"))
    {
        return Some(workspace_id.to_string());
    }

    workspace_context_from_token(one.resolved_access_token())
        .or_else(|| workspace_context_from_token(one.resolved_refresh_token()))
}

/// Set the One API apply gate for the current thread.
///
/// When `false` (the default), mutating One API calls (POST/PUT/PATCH/DELETE)
/// short-circuit to a structured dry-run envelope without contacting the
/// server. Callers must call this from the CLI entrypoint before dispatching
/// any One command.
pub fn set_one_apply(apply: bool) {
    ONE_APPLY.with(|c| c.set(apply));
}

/// Current apply gate state for the calling thread.
pub fn one_apply() -> bool {
    ONE_APPLY.with(|c| c.get())
}

fn one_dry_run_envelope(
    surface: &str,
    operation: &str,
    method: &str,
    url: &str,
    endpoint_template: &str,
    body: Option<&Value>,
) -> Envelope {
    let mut data = one_response_metadata(
        surface,
        operation,
        method,
        url,
        endpoint_template,
        0,
        None,
        None,
        true,
        "dry_run",
        None,
        true,
        true,
    );
    data.insert("dry_run".to_string(), Value::Bool(true));
    data.insert("apply".to_string(), Value::Bool(false));
    data.insert(
        "would_send".to_string(),
        body.map(redact_json_value).unwrap_or(Value::Null),
    );
    data.insert("response".to_string(), Value::Null);
    data.insert(
        "validation_target".to_string(),
        Value::String(url.to_string()),
    );
    Envelope::ok_with_data(
        format!(
            "{} {} dry-run (pass --apply to execute)",
            surface, operation
        ),
        Value::Object({
            let mut map = data;
            map.insert(
                "message".to_string(),
                Value::String(format!(
                    "{} {} would be sent. Re-run with --apply to execute.",
                    surface, operation
                )),
            );
            map
        }),
    )
}

fn one_http_envelope(status: StatusCode, message: String, data: Value) -> Envelope {
    match ayx_core::envelope::ErrorCode::from_http_status(status.as_u16()) {
        Some(code) => Envelope::err_coded(code, message, data),
        None => Envelope::ok_with_data(message, data),
    }
}

fn token_failure_prefix(status: StatusCode) -> &'static str {
    match ayx_core::envelope::ErrorCode::from_http_status(status.as_u16()) {
        Some(ayx_core::envelope::ErrorCode::AuthFailed) => "auth failed",
        Some(ayx_core::envelope::ErrorCode::PermissionDenied) => "permission denied",
        _ => "token request failed",
    }
}

/// Return an allowlisted, bounded summary of a failed OAuth token exchange.
///
/// Require a complete, verified workspace identity before an applied One
/// mutation leaves the process, and refresh the access token if it is stale.
///
/// Returns the refreshed access token when one was minted, so the caller can
/// reuse it instead of resolving a second time.
///
/// Shared rather than inlined because every applied mutation needs it and a
/// path that forgets it sends the write against whatever workspace the ambient
/// token happens to resolve to. The multipart upload path was added without it.
fn preflight_applied_mutation(
    config: &Config,
    surface: &str,
    operation: &str,
    url: &str,
    mutating: bool,
) -> Result<Option<String>> {
    if !mutating {
        return Ok(None);
    }
    let mut preflight_access_token = None;
    // The old guard was conditional on the legacy `expected_workspace_id` field
    // and could therefore send a mutation with only an unverified numeric ID or
    // top-level token context.
    let one = config.alteryx_one.as_ref().ok_or_else(|| {
        anyhow::anyhow!("workspace identity is required for an applied One mutation")
    })?;
    let key = one
        .active_workspace_id()
        // A legacy profile that only carries top-level tokens cannot be
        // promoted here: it has no numeric workspace ID and no verified
        // workspace name, so it can never satisfy `WorkspaceTarget`.
        // Re-running login is the only way to obtain a verified credential.
        .ok_or_else(|| anyhow::anyhow!("no active verified workspace credential; authenticate or select a workspace before applying a mutation (run `ayx one login`, or `ayx one login --workspace-id <numeric-id-or-gid>` to select among saved workspace credentials)"))?;
    let credential = one
        .workspace_credentials
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("active workspace credential is missing"))?;
    let target = ayx_core::profile::WorkspaceTarget::from_credential(
        key,
        credential,
        ayx_core::profile::WorkspaceResolutionSource::ActiveProfile,
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "active workspace credential lacks complete verified ID, GID, or name metadata"
        )
    })?;
    if one_access_token_needs_refresh(config) {
        let refresh_client = build_client()?;
        preflight_access_token = Some(refresh_one_access_token_for_request(
            config,
            &refresh_client,
        )?);
    }
    verify_workspace_identity(
        config,
        surface,
        operation,
        url,
        &target.workspace_id,
        &target.workspace_gid,
        preflight_access_token.as_deref(),
    )?;
    Ok(preflight_access_token)
}

/// Mask credential-shaped runs inside provider-controlled prose.
///
/// `redact_text` masks `key=value` assignments and JWT shapes. A token endpoint
/// is free to write a bare opaque token into `error_description`, where neither
/// pattern matches and the value would be echoed verbatim. Replace any run long
/// enough to be a credential and not a plain alphabetic word, which keeps the
/// sentence readable while making it unable to carry a secret.
fn mask_opaque_runs(value: &str) -> String {
    const MIN_OPAQUE_CHARS: usize = 20;

    value
        .split_inclusive(char::is_whitespace)
        .map(|chunk| {
            let trimmed = chunk.trim_end();
            let trailing = &chunk[trimmed.len()..];
            let core = trimmed.trim_matches(|character: char| !character.is_ascii_alphanumeric());
            if core.chars().count() >= MIN_OPAQUE_CHARS
                && !core
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
            {
                format!("***{trailing}")
            } else {
                chunk.to_string()
            }
        })
        .collect()
}

/// Token endpoints commonly return an OAuth error object for a 400 response.
/// The raw body must never be surfaced because proxies and providers are free
/// to include request data in it. Keep only the fields that identify the
/// failure class, and redact their values before attaching them to an error.
fn oauth_token_error_summary(body: &str, request_id: Option<&str>) -> String {
    const MAX_FIELD_CHARS: usize = 200;

    let bounded = |value: &str| {
        redact_text(
            &value
                .trim()
                .chars()
                .take(MAX_FIELD_CHARS)
                .collect::<String>(),
        )
    };
    let field = |json: &Value, name: &str| {
        json.get(name)
            .and_then(Value::as_str)
            .map(&bounded)
            .filter(|value| !value.is_empty())
    };

    let mut fields = Vec::new();
    if let Ok(json) = serde_json::from_str::<Value>(body) {
        if let Some(error) = field(&json, "error") {
            fields.push(format!("oauth_error={error}"));
        }
        if let Some(code) = field(&json, "error_code") {
            fields.push(format!("provider_error_code={code}"));
        }
        // `error` and `error_code` are short spec/provider enums. Only the
        // description is free prose, so only it needs the opaque-run pass.
        if let Some(description) = field(&json, "error_description") {
            fields.push(format!(
                "oauth_error_description={}",
                mask_opaque_runs(&description)
            ));
        }
    }
    if let Some(request_id) = request_id.map(bounded).filter(|value| !value.is_empty()) {
        fields.push(format!("request_id={request_id}"));
    }

    if fields.is_empty() {
        "provider did not return a recognized OAuth error".to_string()
    } else {
        fields.join("; ")
    }
}

#[allow(clippy::too_many_arguments)]
fn one_response_metadata(
    surface: &str,
    operation: &str,
    method: &str,
    url: &str,
    endpoint_template: &str,
    attempts: u32,
    status_code: Option<u16>,
    request_id: Option<String>,
    ok: bool,
    response_shape: &str,
    retry_after_seconds: Option<u64>,
    mutating: bool,
    dry_run: bool,
) -> serde_json::Map<String, Value> {
    let mut data = serde_json::Map::new();
    data.insert("surface".to_string(), Value::String(surface.to_string()));
    data.insert(
        "operation".to_string(),
        Value::String(operation.to_string()),
    );
    data.insert("method".to_string(), Value::String(method.to_string()));
    data.insert("url".to_string(), Value::String(url.to_string()));
    data.insert(
        "endpoint_template".to_string(),
        Value::String(endpoint_template.to_string()),
    );
    data.insert(
        "validation_target".to_string(),
        Value::String(url.to_string()),
    );
    data.insert("attempts".to_string(), Value::from(attempts));
    data.insert("ok".to_string(), Value::Bool(ok));
    data.insert(
        "response_shape".to_string(),
        Value::String(response_shape.to_string()),
    );
    data.insert("mutating".to_string(), Value::Bool(mutating));
    data.insert("dry_run".to_string(), Value::Bool(dry_run));
    if let Some(status_code) = status_code {
        data.insert("status_code".to_string(), Value::from(status_code));
    } else {
        data.insert("status_code".to_string(), Value::Null);
    }
    if let Some(request_id) = request_id {
        data.insert("request_id".to_string(), Value::String(request_id));
    } else {
        data.insert("request_id".to_string(), Value::Null);
    }
    if let Some(retry_after_seconds) = retry_after_seconds {
        data.insert(
            "retry_after_seconds".to_string(),
            Value::from(retry_after_seconds),
        );
    } else {
        data.insert("retry_after_seconds".to_string(), Value::Null);
    }
    data
}

fn response_body_preview(text: &str) -> String {
    redact_text(&text.chars().take(200).collect::<String>())
}

#[derive(Debug)]
enum ParsedOneResponse {
    Json {
        body: Value,
        response_shape: &'static str,
    },
    NonJson {
        response_kind: &'static str,
        body_preview: String,
        content_type: String,
        parse_error: Option<String>,
    },
}

fn parse_one_response(content_type: &str, text: &str) -> ParsedOneResponse {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return ParsedOneResponse::Json {
            body: Value::Null,
            response_shape: "null",
        };
    }

    let lower = content_type.to_lowercase();
    let looks_json = lower.contains("application/json")
        || lower.contains("+json")
        || matches!(trimmed.chars().next(), Some('{') | Some('['));
    if looks_json {
        match serde_json::from_str::<Value>(text) {
            Ok(body) => ParsedOneResponse::Json {
                response_shape: response_shape(&body),
                body,
            },
            Err(err) => ParsedOneResponse::NonJson {
                response_kind: "malformed_json",
                body_preview: response_body_preview(text),
                content_type: content_type.to_string(),
                parse_error: Some(err.to_string()),
            },
        }
    } else if lower.contains("text/html") || trimmed.starts_with('<') {
        ParsedOneResponse::NonJson {
            response_kind: "html",
            body_preview: response_body_preview(text),
            content_type: content_type.to_string(),
            parse_error: None,
        }
    } else {
        ParsedOneResponse::NonJson {
            response_kind: "non_json",
            body_preview: response_body_preview(text),
            content_type: content_type.to_string(),
            parse_error: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn one_transport_failure_envelope(
    status: Option<StatusCode>,
    surface: &str,
    operation: &str,
    method: &str,
    url: &str,
    endpoint_template: &str,
    attempts: u32,
    retry_after_seconds: Option<u64>,
    parsed: &ParsedOneResponse,
    mutating: bool,
    dry_run: bool,
) -> Envelope {
    let (response_shape, body_preview, content_type, parse_error) = match parsed {
        ParsedOneResponse::Json { .. } => ("null", String::new(), String::new(), None),
        ParsedOneResponse::NonJson {
            response_kind,
            body_preview,
            content_type,
            parse_error,
        } => (
            *response_kind,
            body_preview.clone(),
            content_type.clone(),
            parse_error.clone(),
        ),
    };
    let code = status
        .and_then(|status| ayx_core::envelope::ErrorCode::from_http_status(status.as_u16()))
        .unwrap_or(ayx_core::envelope::ErrorCode::Internal);
    let mut data = one_response_metadata(
        surface,
        operation,
        method,
        url,
        endpoint_template,
        attempts,
        status.map(|s| s.as_u16()),
        None,
        false,
        response_shape,
        retry_after_seconds,
        mutating,
        dry_run,
    );
    data.insert("response".to_string(), Value::Null);
    data.insert(
        "error_code".to_string(),
        Value::String(code.as_str().to_string()),
    );
    if !body_preview.is_empty() {
        data.insert("body_preview".to_string(), Value::String(body_preview));
    }
    if !content_type.is_empty() {
        data.insert("content_type".to_string(), Value::String(content_type));
    }
    if let Some(parse_error) = parse_error {
        data.insert("parse_error".to_string(), Value::String(parse_error));
    }
    // An HTML error body means we did not reach the JSON API gateway at all. The `/v4`
    // gateway answers unknown routes with a JSON `RouteNotFoundException`; the sibling
    // services (`/svc-workflow`, ...) fall through to an Express default handler that
    // renders HTML. Distinguishing the two is the difference between "the resource is
    // missing" and "this path does not exist on this service", so say so explicitly.
    if response_shape == "html" {
        data.insert(
            "error_hints".to_string(),
            Value::Array(vec![Value::String(
                "the service returned an HTML error page rather than the JSON API gateway — \
                 the route likely does not exist on this service; verify the path against \
                 docs/one-endpoint-matrix.md"
                    .to_string(),
            )]),
        );
    }
    Envelope::err_coded(
        code,
        format!("{} {} failed", surface, operation),
        Value::Object(data),
    )
}

/// Build a success envelope for a 2xx response whose body is not JSON.
///
/// Without this, a `200 text/csv` (or any other non-JSON success) fell through to
/// [`one_transport_failure_envelope`], where `ErrorCode::from_http_status` returns `None`
/// for a success status and the `unwrap_or` turned it into `Internal` — reporting a
/// perfectly good response as `ok: false`.
#[allow(clippy::too_many_arguments)]
fn one_non_json_success_envelope(
    status: StatusCode,
    surface: &str,
    operation: &str,
    method: &str,
    url: &str,
    endpoint_template: &str,
    attempts: u32,
    request_id: Option<String>,
    retry_after_seconds: Option<u64>,
    parsed: &ParsedOneResponse,
    mutating: bool,
    elapsed_ms: u64,
) -> Envelope {
    let (response_shape, body_preview, content_type, parse_error) = match parsed {
        ParsedOneResponse::Json { .. } => ("null", String::new(), String::new(), None),
        ParsedOneResponse::NonJson {
            response_kind,
            body_preview,
            content_type,
            parse_error,
        } => (
            *response_kind,
            body_preview.clone(),
            content_type.clone(),
            parse_error.clone(),
        ),
    };
    let mut data = one_response_metadata(
        surface,
        operation,
        method,
        url,
        endpoint_template,
        attempts,
        Some(status.as_u16()),
        request_id,
        true,
        response_shape,
        retry_after_seconds,
        mutating,
        false,
    );
    data.insert("elapsed_ms".to_string(), Value::from(elapsed_ms));
    data.insert("response".to_string(), Value::Null);
    data.insert("error_code".to_string(), Value::Null);
    if !body_preview.is_empty() {
        data.insert("body_preview".to_string(), Value::String(body_preview));
    }
    if !content_type.is_empty() {
        data.insert("content_type".to_string(), Value::String(content_type));
    }
    if let Some(parse_error) = parse_error {
        data.insert("parse_error".to_string(), Value::String(parse_error));
    }
    Envelope::ok_with_data(
        format!("{} {} ok (non-JSON body)", surface, operation),
        Value::Object(data),
    )
}

pub use inventory::{
    inventory_endpoints, inventory_endpoints_full, one_surface_inventory_envelope,
};

pub fn api_status_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    Ok(Envelope::ok_with_data(
        format!("{} api status", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": api.base_url,
            "has_credentials": {
                "client_id": api.auth.client_id.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "client_secret": api.auth.client_secret.as_ref().is_some_and(|v| !v.trim().is_empty()),
                "pat": api.auth.pat.as_ref().is_some_and(|v| !v.trim().is_empty()),
            },
            "timeout_ms": api.timeout_ms,
            "message": format!("{} api surface ready", product),
        }),
    ))
}

pub fn api_inventory_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    Ok(Envelope::ok_with_data(
        format!("{} api inventory", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": api.base_url,
            "inventory": [
                "connection posture",
                "auth posture",
                "follow-on command candidates",
            ],
            "message": format!("{} api inventory ready", product),
        }),
    ))
}

pub fn api_diagnose_envelope(config: &Config, product: &str) -> Result<Envelope> {
    let api = config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api/server_api section"))?;

    if api.base_url.trim().is_empty() {
        bail!("{} api base_url cannot be empty", product);
    }

    let normalized = if api.base_url.ends_with('/') {
        api.base_url.clone()
    } else {
        format!("{}/", api.base_url)
    };

    Ok(Envelope::ok_with_data(
        format!("{} api diagnose", product),
        json!({
            "product": product,
            "profile": config.profile_name,
            "base_url": normalized,
            "auth_mode": format!("{:?}", api.auth.mode),
            "checks": [
                "base URL present",
                "credential fields present",
                "token acquisition should be attempted by the CLI caller when a live endpoint is available"
            ],
            "next_step": format!("wire {}-specific reachability and endpoint checks once the API surface is defined", product),
        }),
    ))
}

pub fn one_api_live_request(
    config: &Config,
    surface: &str,
    operation: &str,
    method: &str,
    endpoint: &str,
    mutating: bool,
    path_params: &[(&str, &str)],
) -> Result<Envelope> {
    one_api_live_request_with_body(
        config,
        surface,
        operation,
        method,
        endpoint,
        mutating,
        path_params,
        None,
    )
}

fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    // Key matching is delegated to the single canonical
                    // matcher in ayx-core so every crate shares one list.
                    // Only the placeholder differs (`[REDACTED]` here vs
                    // `***` in `ayx_core::observability::redact_json`).
                    (
                        key.clone(),
                        if is_secret_key(key) {
                            Value::String("[REDACTED]".to_string())
                        } else {
                            redact_json_value(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.iter().map(redact_json_value).collect()),
        _ => value.clone(),
    }
}

/// Send a One API request with repeated URL query parameters.
///
/// OpenAPI array query parameters use the form style by default, so a value
/// such as `userIds=["1", "2"]` is encoded as `?userIds=1&userIds=2`.
/// Keeping this in the transport layer avoids each command hand-building an
/// unsafe query string.
#[allow(clippy::too_many_arguments)]
pub fn one_api_live_request_with_query(
    config: &Config,
    surface: &str,
    operation: &str,
    method: &str,
    endpoint: &str,
    mutating: bool,
    path_params: &[(&str, &str)],
    query_params: &[(&str, &str)],
) -> Result<Envelope> {
    if query_params.is_empty() {
        return one_api_live_request(
            config,
            surface,
            operation,
            method,
            endpoint,
            mutating,
            path_params,
        );
    }

    let endpoint_with_query = endpoint_with_query_params(endpoint, query_params);
    one_api_live_request(
        config,
        surface,
        operation,
        method,
        &endpoint_with_query,
        mutating,
        path_params,
    )
}

fn endpoint_with_query_params(endpoint: &str, query_params: &[(&str, &str)]) -> String {
    let mut endpoint_with_query = endpoint.to_string();
    let mut serializer = Serializer::new(String::new());
    for (key, value) in query_params {
        serializer.append_pair(key, value);
    }
    let separator = if endpoint_with_query.contains('?') {
        '&'
    } else {
        '?'
    };
    endpoint_with_query.push(separator);
    endpoint_with_query.push_str(&serializer.finish());
    endpoint_with_query
}

/// Pagination parameters for One list endpoints.
///
/// `limit` caps results per page. `page_token` (when present) requests the
/// next page using the `nextPageToken` value the server returned previously.
/// `auto_all` enables client-side auto-pagination — the helper follows
/// `nextPageToken` until exhausted or `max_pages` is hit.
#[derive(Debug, Clone, Default)]
pub struct OneListParams {
    pub limit: Option<u32>,
    /// Preferred spelling for the per-page size. `limit` remains accepted as
    /// a compatibility alias by the CLI.
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    pub auto_all: bool,
    pub max_pages: Option<u32>,
}

impl OneListParams {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_limit(mut self, limit: Option<u32>) -> Self {
        self.limit = limit;
        self
    }
    pub fn with_page_size(mut self, page_size: Option<u32>) -> Self {
        self.page_size = page_size;
        self
    }
    pub fn with_page_token(mut self, token: Option<String>) -> Self {
        self.page_token = token;
        self
    }
    pub fn with_all(mut self, all: bool, max_pages: Option<u32>) -> Self {
        self.auto_all = all;
        self.max_pages = max_pages;
        self
    }
}

/// List-request helper. Appends `limit`/`pageToken` as query params and, when
/// `params.auto_all` is set, follows `nextPageToken` until exhausted.
///
/// Returns an envelope whose `data` includes:
///   - `items`: concatenated results across all fetched pages
///   - `pages_fetched`: number of pages
///   - `next_page_token`: the *unconsumed* token if pagination was bounded
///   - `page_envelopes`: per-page debug info (status, elapsed_ms, request_id)
pub fn one_api_list_request(
    config: &Config,
    surface: &str,
    operation: &str,
    endpoint: &str,
    path_params: &[(&str, &str)],
    params: &OneListParams,
) -> Result<Envelope> {
    let max_pages = params
        .max_pages
        .unwrap_or(if params.auto_all { 50 } else { 1 });
    let mut current_token = params.page_token.clone();
    let mut pages_fetched: u32 = 0;
    let mut aggregated_items: Vec<Value> = Vec::new();
    let mut page_envelopes: Vec<Value> = Vec::new();
    let mut last_next_token: Option<String> = None;
    let mut seen_tokens: HashSet<String> = HashSet::new();
    let mut incomplete_reason: Option<String> = None;

    loop {
        if pages_fetched >= max_pages {
            if params.auto_all && last_next_token.is_some() {
                incomplete_reason = Some(format!(
                    "pagination page cap ({max_pages}) reached before the collection was exhausted"
                ));
            }
            break;
        }
        let mut endpoint_with_query = endpoint.to_string();
        let mut q: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = params.page_size.or(params.limit) {
            q.push(("limit", limit.to_string()));
        }
        if let Some(ref token) = current_token {
            q.push(("pageToken", token.clone()));
        }
        if !q.is_empty() {
            let qs: String = q
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}={}",
                        k,
                        url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("&");
            let sep = if endpoint.contains('?') { '&' } else { '?' };
            endpoint_with_query.push(sep);
            endpoint_with_query.push_str(&qs);
        }

        let envelope = one_api_live_request(
            config,
            surface,
            operation,
            "GET",
            &endpoint_with_query,
            false,
            path_params,
        )?;

        // `one_api_live_request` returns transport failures as `Ok(envelope)` with
        // `ok: false` — it does not surface them through `?`. Without this check a
        // failed page (404/403/500/HTML/...) fell straight into `extract_items`,
        // which found no items in the `Null` response and this function reported a
        // perfectly successful, silently empty list. Never do that: a failure must
        // stay a failure.
        if !envelope.ok {
            if pages_fetched == 0 {
                // Nothing fetched yet — the caller gets the original failure as-is.
                return Ok(envelope);
            }
            // A later page failed after earlier pages already succeeded. Keep
            // reporting failure (partial data must never read as `ok: true`), but
            // don't throw away what was already fetched — attach it, labeled, so
            // the caller can see exactly how much was retrieved before the break.
            let mut data = envelope.data.clone();
            if let Value::Object(ref mut map) = data {
                map.insert("partial".to_string(), Value::Bool(true));
                map.insert(
                    "pages_fetched_before_failure".to_string(),
                    Value::from(pages_fetched),
                );
                map.insert("items".to_string(), json!(aggregated_items));
                map.insert("page_envelopes".to_string(), json!(page_envelopes));
            }
            let code = envelope
                .error_code
                .unwrap_or(ayx_core::envelope::ErrorCode::Internal);
            return Ok(Envelope::err_coded(
                code,
                format!(
                    "{} {} failed on page {}, after {} item{} fetched across {} prior page{}: {}",
                    surface,
                    operation,
                    pages_fetched + 1,
                    aggregated_items.len(),
                    if aggregated_items.len() == 1 { "" } else { "s" },
                    pages_fetched,
                    if pages_fetched == 1 { "" } else { "s" },
                    envelope.message,
                ),
                data,
            ));
        }
        pages_fetched += 1;

        let response = envelope
            .data
            .get("response")
            .cloned()
            .unwrap_or(Value::Null);
        let items = extract_items(&response);
        aggregated_items.extend(items);
        last_next_token = extract_next_token(&response);
        page_envelopes.push(json!({
            "status_code": envelope.data.get("status_code"),
            "elapsed_ms": envelope.data.get("elapsed_ms"),
            "request_id": envelope.data.get("request_id"),
            "next_page_token": last_next_token,
        }));

        if !params.auto_all {
            break;
        }
        match &last_next_token {
            Some(t) if !t.is_empty() => {
                if !seen_tokens.insert(t.clone()) {
                    incomplete_reason = Some(
                        "pagination returned a repeated continuation token; refusing to loop"
                            .to_string(),
                    );
                    break;
                }
                current_token = Some(t.clone());
            }
            _ => break,
        }
    }

    if let Some(reason) = incomplete_reason {
        return Ok(Envelope::err_coded(
            ayx_core::envelope::ErrorCode::Incomplete,
            format!("{surface} {operation} incomplete: {reason}"),
            json!({
                "surface": surface,
                "operation": operation,
                "partial": true,
                "items": aggregated_items,
                "pages_fetched": pages_fetched,
                "next_page_token": last_next_token,
                "page_envelopes": page_envelopes,
                "reason": reason,
            }),
        ));
    }

    Ok(Envelope::ok_with_data(
        format!(
            "{} {} ok ({} item{}, {} page{})",
            surface,
            operation,
            aggregated_items.len(),
            if aggregated_items.len() == 1 { "" } else { "s" },
            pages_fetched,
            if pages_fetched == 1 { "" } else { "s" }
        ),
        json!({
            "surface": surface,
            "operation": operation,
            "items": aggregated_items,
            "pages_fetched": pages_fetched,
            "next_page_token": last_next_token,
            "page_envelopes": page_envelopes,
        }),
    ))
}

fn extract_items(response: &Value) -> Vec<Value> {
    // Common shapes: { "items": [...] }, { "results": [...] }, plain array.
    if let Some(arr) = response.as_array() {
        return arr.clone();
    }
    // `assets` is the svc-workflow list key (`GET /svc-workflow/api/v1/assets`); the
    // `/v4` gateway uses `data`. Both are live-verified.
    for key in ["items", "results", "data", "records", "value", "assets"] {
        if let Some(arr) = response.get(key).and_then(|v| v.as_array()) {
            return arr.clone();
        }
    }
    Vec::new()
}

fn extract_next_token(response: &Value) -> Option<String> {
    for key in [
        "nextPageToken",
        "next_page_token",
        "nextToken",
        "next_token",
        "cursor",
    ] {
        if let Some(s) = response.get(key).and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub fn one_api_live_request_with_body(
    config: &Config,
    surface: &str,
    operation: &str,
    method: &str,
    endpoint: &str,
    mutating: bool,
    path_params: &[(&str, &str)],
    body: Option<Value>,
) -> Result<Envelope> {
    let observability = config.observability.as_ref();

    let base = resolve_one_base_url(config);
    let mut url = format!("{}{}", base, endpoint);
    for (key, value) in path_params {
        // Percent-encode path segments so an id containing '/', '#', '?',
        // or other reserved characters doesn't escape the path or corrupt
        // the URL structure. NON_ALPHANUMERIC over-encodes (RFC 3986 allows
        // some unreserved punctuation) but that's strictly safer than under.
        let encoded = percent_encode_path_segment(value);
        url = url.replace(&format!("{{{}}}", key), &encoded);
    }

    // Safety gate: mutating requests require an explicit --apply (one_apply()).
    // Without it we record a dry-run event and return a structured envelope
    // describing the request that would have been sent.
    if mutating && !one_apply() {
        let envelope =
            one_dry_run_envelope(surface, operation, method, &url, endpoint, body.as_ref());
        let _ = record_api_event(
            observability,
            ApiEvent {
                product: "one",
                surface,
                operation,
                method,
                endpoint_template: endpoint,
                resolved_url: &url,
                status_code: None,
                duration_ms: 0,
                attempt: 0,
                retry_after_seconds: None,
                request_id: None,
                ok: true,
                error_class: None,
                response_shape: Some("dry_run"),
                mutating: true,
                dry_run: true,
            },
        );
        return Ok(envelope);
    }

    let preflight_access_token =
        preflight_applied_mutation(config, surface, operation, &url, mutating)?;

    let client = build_client()?;
    trace_one(format!(
        "{surface} {operation}: resolving access token for {url}"
    ));
    let mut access_token = match preflight_access_token {
        Some(token) => token,
        None => resolve_one_access_token(config, &client)?,
    };
    trace_one(format!("{surface} {operation}: access token resolved"));
    let workspace_context = workspace_context_header_value(config);
    if let Some(ref workspace_context) = workspace_context {
        trace_one(format!(
            "{surface} {operation}: workspace context {workspace_context}"
        ));
    } else {
        trace_one(format!(
            "{surface} {operation}: no workspace context derived for request"
        ));
    }
    let method_name = method.to_string();
    let method = reqwest::Method::from_bytes(method_name.as_bytes())
        .map_err(|_| anyhow::anyhow!("unsupported one api method '{}'", method))?;
    let mut attempt = 0u32;
    let max_attempts = if mutating { 1 } else { 4 };
    let started = Instant::now();
    let mut last_status: Option<StatusCode> = None;
    let mut retry_after_seconds: Option<u64> = None;
    let mut refreshed_once = false;

    let workspace_gid = config
        .alteryx_one
        .as_ref()
        .and_then(|o| o.resolved_workspace_gid())
        .map(str::to_string);

    loop {
        attempt += 1;
        let mut request = client
            .request(method.clone(), &url)
            .header(AUTHORIZATION, bearer_authorization_value(&access_token))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(ref gid) = workspace_gid {
            request = request.header("x-alteryx-workspace-gid", gid);
        }
        if let Some(ref workspace_context) = workspace_context {
            request = request.header("x-trifacta-person-workspace-id", workspace_context.as_str());
        }
        if let Some(ref payload) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .json(payload);
        } else if mutating {
            request = request.header(CONTENT_TYPE, "application/json");
        }

        trace_one(format!(
            "{surface} {operation}: attempt {attempt}/{max_attempts} {method_name} {url}"
        ));
        let response = request.send();
        match response {
            Ok(response) => {
                let status = response.status();
                last_status = Some(status);
                retry_after_seconds = parse_retry_after(response.headers().get(RETRY_AFTER));
                if status == StatusCode::UNAUTHORIZED && !mutating && !refreshed_once {
                    let auth_mode = config
                        .alteryx_one
                        .as_ref()
                        .map(|one| one.auth_mode.clone())
                        .unwrap_or_default();
                    let refreshed = if auth_mode == ayx_core::profile::AuthMode::ServicePrincipal {
                        service_principal_access_token(config, &client)
                    } else {
                        refresh_one_access_token_for_request(config, &client)
                    };
                    access_token = refreshed?;
                    refreshed_once = true;
                    continue;
                }
                if status.is_success()
                    || !should_retry_status(status, mutating)
                    || attempt >= max_attempts
                {
                    let content_type = response
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|val| val.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let request_id = response
                        .headers()
                        .get("x-request-id")
                        .and_then(|val| val.to_str().ok())
                        .map(ToOwned::to_owned);
                    let text = response.text().unwrap_or_else(|_| String::new());
                    let parsed = parse_one_response(&content_type, &text);
                    trace_one(format!(
                        "{surface} {operation}: attempt {attempt} status {} response_kind {}",
                        status.as_u16(),
                        match &parsed {
                            ParsedOneResponse::Json { response_shape, .. } => *response_shape,
                            ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
                        }
                    ));
                    let parsed_response_shape = match &parsed {
                        ParsedOneResponse::Json { response_shape, .. } => *response_shape,
                        ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
                    };
                    let envelope = match parsed {
                        ParsedOneResponse::Json {
                            body: response_body,
                            response_shape: body_shape,
                        } => one_http_envelope(
                            status,
                            format!(
                                "{} {} {}",
                                surface,
                                operation,
                                if status.is_success() { "ok" } else { "failed" }
                            ),
                            Value::Object({
                                let mut data = one_response_metadata(
                                    surface,
                                    operation,
                                    &method_name,
                                    &url,
                                    endpoint,
                                    attempt,
                                    Some(status.as_u16()),
                                    request_id.clone(),
                                    status.is_success(),
                                    body_shape,
                                    retry_after_seconds,
                                    mutating,
                                    false,
                                );
                                data.insert(
                                    "elapsed_ms".to_string(),
                                    Value::from(started.elapsed().as_millis() as u64),
                                );
                                data.insert("response".to_string(), response_body);
                                data.insert(
                                    "error_code".to_string(),
                                    ayx_core::envelope::ErrorCode::from_http_status(
                                        status.as_u16(),
                                    )
                                    .map_or(Value::Null, |c| Value::String(c.as_str().to_string())),
                                );
                                data
                            }),
                        ),
                        // A 2xx response is only a success if we actually received the real
                        // payload. `html`/`malformed_json` at 2xx mean the body was NOT the
                        // API response (an SSO/gateway page, a truncated body, ...) even
                        // though the status looks fine, so those still go to the failure
                        // builder below. Only a genuinely non-JSON body (`non_json`, e.g.
                        // `text/csv`) is a legitimate 2xx success.
                        ParsedOneResponse::NonJson { response_kind, .. }
                            if status.is_success() && response_kind == "non_json" =>
                        {
                            one_non_json_success_envelope(
                                status,
                                surface,
                                operation,
                                &method_name,
                                &url,
                                endpoint,
                                attempt,
                                request_id.clone(),
                                retry_after_seconds,
                                &parsed,
                                mutating,
                                started.elapsed().as_millis() as u64,
                            )
                        }
                        ParsedOneResponse::NonJson { .. } => one_transport_failure_envelope(
                            Some(status),
                            surface,
                            operation,
                            &method_name,
                            &url,
                            endpoint,
                            attempt,
                            retry_after_seconds,
                            &parsed,
                            mutating,
                            false,
                        ),
                    };
                    let _ = record_api_event(
                        observability,
                        ApiEvent {
                            product: "one",
                            surface,
                            operation,
                            method: &method_name,
                            endpoint_template: endpoint,
                            resolved_url: &url,
                            status_code: Some(status.as_u16()),
                            duration_ms: started.elapsed().as_millis(),
                            attempt,
                            retry_after_seconds,
                            request_id: request_id.as_deref(),
                            ok: status.is_success(),
                            error_class: None,
                            response_shape: Some(parsed_response_shape),
                            mutating,
                            dry_run: false,
                        },
                    );
                    return Ok(envelope);
                }
                let delay = retry_delay(attempt, retry_after_seconds);
                thread::sleep(delay);
                continue;
            }
            Err(err) => {
                trace_one(format!(
                    "{surface} {operation}: attempt {attempt} transport error: {err}"
                ));
                let transport = transport_error_summary(&err);
                let parsed = ParsedOneResponse::NonJson {
                    response_kind: "transport_error",
                    body_preview: transport["error"].as_str().unwrap_or_default().to_string(),
                    content_type: String::new(),
                    parse_error: None,
                };
                let parsed_response_shape = match &parsed {
                    ParsedOneResponse::Json { response_shape, .. } => *response_shape,
                    ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
                };
                if mutating || attempt >= max_attempts {
                    let code = ayx_core::envelope::ErrorCode::Network;
                    let mut data = one_response_metadata(
                        surface,
                        operation,
                        &method_name,
                        &url,
                        endpoint,
                        attempt,
                        last_status.map(|s| s.as_u16()),
                        None,
                        false,
                        "transport_error",
                        retry_after_seconds,
                        mutating,
                        false,
                    );
                    data.insert(
                        "elapsed_ms".to_string(),
                        Value::from(started.elapsed().as_millis() as u64),
                    );
                    data.insert("error".to_string(), transport["error"].clone());
                    data.insert("error_kind".to_string(), transport["error_kind"].clone());
                    data.insert("error_hints".to_string(), transport["error_hints"].clone());
                    data.insert("error_chain".to_string(), transport["error_chain"].clone());
                    data.insert("request_url".to_string(), transport["request_url"].clone());
                    data.insert("response".to_string(), Value::Null);
                    data.insert(
                        "error_code".to_string(),
                        Value::String(code.as_str().to_string()),
                    );
                    let envelope = Envelope::err_coded(
                        code,
                        format!("{} {} failed", surface, operation),
                        Value::Object(data),
                    );
                    let _ = record_api_event(
                        observability,
                        ApiEvent {
                            product: "one",
                            surface,
                            operation,
                            method: &method_name,
                            endpoint_template: endpoint,
                            resolved_url: &url,
                            status_code: last_status.map(|s| s.as_u16()),
                            duration_ms: started.elapsed().as_millis(),
                            attempt,
                            retry_after_seconds,
                            request_id: None,
                            ok: false,
                            error_class: Some("transport"),
                            response_shape: Some(parsed_response_shape),
                            mutating,
                            dry_run: false,
                        },
                    );
                    return Ok(envelope);
                }
                let delay = retry_delay(attempt, retry_after_seconds);
                thread::sleep(delay);
            }
        }
    }
}

pub fn flow_import_package_envelope(
    config: &Config,
    input_path: &Path,
    folder_id: Option<&str>,
    from_ui: bool,
    override_js_udfs: bool,
    dry_run: bool,
) -> Result<Envelope> {
    let observability = config.observability.as_ref();
    let started = Instant::now();

    let endpoint = if dry_run {
        "/v4/flows/package/dryRun"
    } else {
        "/v4/flows/package"
    };
    let mut url = format!("{}{}", resolve_one_base_url(config), endpoint);
    // Live import is mutating (POST). The Alteryx-provided `dryRun` endpoint is
    // an API-level dry-run and is safe without --apply.
    let mutating = !dry_run;
    if mutating && !one_apply() {
        let envelope = one_dry_run_envelope(
            "flow",
            "import-package",
            "POST",
            &url,
            endpoint,
            Some(&json!({
                "input_path": input_path.display().to_string(),
                "folder_id": folder_id,
                "from_ui": from_ui,
                "override_js_udfs": override_js_udfs,
            })),
        );
        let _ = record_api_event(
            observability,
            ApiEvent {
                product: "one",
                surface: "flow",
                operation: "import-package",
                method: "POST",
                endpoint_template: endpoint,
                resolved_url: &url,
                status_code: None,
                duration_ms: started.elapsed().as_millis(),
                attempt: 0,
                retry_after_seconds: None,
                request_id: None,
                ok: true,
                error_class: None,
                response_shape: Some("dry_run"),
                mutating: true,
                dry_run: true,
            },
        );
        return Ok(envelope);
    }
    let client = build_client()?;
    let access_token = resolve_one_access_token(config, &client)?;
    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(value) = folder_id {
        query.push(("folderId", value.to_string()));
    }
    if from_ui {
        query.push(("fromUI", "true".to_string()));
    }
    if override_js_udfs {
        query.push(("overrideJsUdfs", "true".to_string()));
    }
    if !query.is_empty() {
        let mut serializer = Serializer::new(String::new());
        for (key, value) in &query {
            serializer.append_pair(key, value);
        }
        url.push('?');
        url.push_str(&serializer.finish());
    }

    let file_bytes = fs::read(input_path)
        .with_context(|| format!("failed to read flow package '{}'", input_path.display()))?;
    let file_name = input_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "flow-package.zip".to_string());
    let form = Form::new().part(
        "data",
        Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("application/octet-stream")
            .expect("mime literal is valid"),
    );

    let response = client
        .post(&url)
        .header(AUTHORIZATION, bearer_authorization_value(&access_token))
        .multipart(form)
        .send()
        .with_context(|| format!("flow package request to '{}' failed", url))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let text = response.text().unwrap_or_else(|_| String::new());
    let parsed = parse_one_response(&content_type, &text);
    let parsed_response_shape = match &parsed {
        ParsedOneResponse::Json { response_shape, .. } => *response_shape,
        ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
    };
    let envelope = match parsed {
        ParsedOneResponse::Json {
            body: response_body,
            response_shape: body_shape,
        } => one_http_envelope(
            status,
            format!(
                "flow package {} {}",
                if dry_run { "dry-run" } else { "import" },
                if status.is_success() { "ok" } else { "failed" }
            ),
            Value::Object({
                let mut data = one_response_metadata(
                    "flow",
                    if dry_run { "import-dry-run" } else { "import" },
                    "POST",
                    &url,
                    endpoint,
                    1,
                    Some(status.as_u16()),
                    request_id.clone(),
                    status.is_success(),
                    body_shape,
                    None,
                    !dry_run,
                    dry_run,
                );
                data.insert(
                    "elapsed_ms".to_string(),
                    Value::from(started.elapsed().as_millis() as u64),
                );
                data.insert("response".to_string(), response_body);
                data.insert(
                    "error_code".to_string(),
                    ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                        .map_or(Value::Null, |c| Value::String(c.as_str().to_string())),
                );
                data
            }),
        ),
        ParsedOneResponse::NonJson { .. } => one_transport_failure_envelope(
            Some(status),
            "flow",
            if dry_run { "import-dry-run" } else { "import" },
            "POST",
            &url,
            endpoint,
            1,
            None,
            &parsed,
            !dry_run,
            dry_run,
        ),
    };
    let _ = record_api_event(
        observability,
        ApiEvent {
            product: "one",
            surface: "flow",
            operation: if dry_run { "import-dry-run" } else { "import" },
            method: "POST",
            endpoint_template: endpoint,
            resolved_url: &url,
            status_code: Some(status.as_u16()),
            duration_ms: started.elapsed().as_millis(),
            attempt: 1,
            retry_after_seconds: None,
            request_id: request_id.as_deref(),
            ok: status.is_success(),
            error_class: None,
            response_shape: Some(parsed_response_shape),
            mutating: !dry_run,
            dry_run,
        },
    );
    Ok(envelope)
}

/// Upload one file as a multipart `file` field to a One sibling service.
///
/// Agent Studio's workflow shortcut source uses the documented
/// `/svc-workflow/api/v1/workflows` upload route.  Keep this transport helper
/// separate from the legacy `/v4/flows/package` importer because the two
/// services use different multipart field names and represent different asset
/// families.
pub fn one_api_multipart_file_request(
    config: &Config,
    surface: &str,
    operation: &str,
    endpoint: &str,
    input_path: &Path,
    mutating: bool,
) -> Result<Envelope> {
    let file_bytes = fs::read(input_path)
        .with_context(|| format!("failed to read upload file '{}'", input_path.display()))?;
    let file_name = input_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload.bin".to_string());
    let url = format!("{}{}", resolve_one_base_url(config), endpoint);

    if mutating && !one_apply() {
        return Ok(one_dry_run_envelope(
            surface,
            operation,
            "POST",
            &url,
            endpoint,
            Some(&json!({
                "field": "file",
                "file_name": file_name,
                "bytes": file_bytes.len(),
                "input_path": input_path.display().to_string(),
            })),
        ));
    }

    // An applied upload is a write like any other: it must prove the selected
    // workspace identity before the bytes leave the process, or it lands in
    // whatever workspace the ambient token resolves to.
    let preflight_access_token =
        preflight_applied_mutation(config, surface, operation, &url, mutating)?;

    let client = build_client()?;
    let access_token = match preflight_access_token {
        Some(token) => token,
        None => resolve_one_access_token(config, &client)?,
    };
    let workspace_context = workspace_context_header_value(config);
    let workspace_gid = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.resolved_workspace_gid())
        .map(str::to_string);
    let started = Instant::now();
    let form = Form::new().part(
        "file",
        Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("application/octet-stream")
            .expect("mime literal is valid"),
    );
    let mut request = client
        .post(&url)
        .header(AUTHORIZATION, bearer_authorization_value(&access_token))
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(gid) = workspace_gid {
        request = request.header("x-alteryx-workspace-gid", gid);
    }
    if let Some(workspace_context) = workspace_context {
        request = request.header("x-trifacta-person-workspace-id", workspace_context);
    }
    let response = request
        .multipart(form)
        .send()
        .with_context(|| format!("{surface} upload request to '{url}' failed"))?;

    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().unwrap_or_default();
    let parsed = parse_one_response(&content_type, &text);
    let parsed_response_shape = match &parsed {
        ParsedOneResponse::Json { response_shape, .. } => *response_shape,
        ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
    };
    let envelope = match parsed {
        ParsedOneResponse::Json {
            body: response_body,
            response_shape: body_shape,
        } => one_http_envelope(
            status,
            format!(
                "{surface} {operation} {}",
                if status.is_success() { "ok" } else { "failed" }
            ),
            Value::Object({
                let mut data = one_response_metadata(
                    surface,
                    operation,
                    "POST",
                    &url,
                    endpoint,
                    1,
                    Some(status.as_u16()),
                    request_id.clone(),
                    status.is_success(),
                    body_shape,
                    None,
                    mutating,
                    false,
                );
                data.insert(
                    "elapsed_ms".to_string(),
                    Value::from(started.elapsed().as_millis() as u64),
                );
                data.insert("response".to_string(), response_body);
                data.insert(
                    "error_code".to_string(),
                    ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                        .map_or(Value::Null, |code| Value::String(code.as_str().to_string())),
                );
                data
            }),
        ),
        ParsedOneResponse::NonJson { .. } => one_transport_failure_envelope(
            Some(status),
            surface,
            operation,
            "POST",
            &url,
            endpoint,
            1,
            None,
            &parsed,
            mutating,
            false,
        ),
    };
    let _ = record_api_event(
        config.observability.as_ref(),
        ApiEvent {
            product: "one",
            surface,
            operation,
            method: "POST",
            endpoint_template: endpoint,
            resolved_url: &url,
            status_code: Some(status.as_u16()),
            duration_ms: started.elapsed().as_millis(),
            attempt: 1,
            retry_after_seconds: None,
            request_id: request_id.as_deref(),
            ok: status.is_success(),
            error_class: None,
            response_shape: Some(parsed_response_shape),
            mutating,
            dry_run: false,
        },
    );
    Ok(envelope)
}

pub fn flow_export_package_envelope(
    config: &Config,
    flow_id: &str,
    output_path: &Path,
    dry_run: bool,
) -> Result<Envelope> {
    let observability = config.observability.as_ref();
    let client = build_client()?;
    let access_token = resolve_one_access_token(config, &client)?;
    let started = Instant::now();

    let endpoint = if dry_run {
        "/v4/flows/{id}/package/dryRun"
    } else {
        "/v4/flows/{id}/package"
    };
    let mut url = format!("{}{}", resolve_one_base_url(config), endpoint);
    url = url.replace("{id}", &percent_encode_path_segment(flow_id));

    let response = client
        .get(&url)
        .header(AUTHORIZATION, bearer_authorization_value(&access_token))
        .header(reqwest::header::ACCEPT, "*/*")
        .send()
        .with_context(|| format!("flow package request to '{}' failed", url))?;

    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);

    if status.is_success() {
        if dry_run {
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let text = response.text().unwrap_or_else(|_| String::new());
            let parsed = parse_one_response(&content_type, &text);
            let parsed_response_shape = match &parsed {
                ParsedOneResponse::Json { response_shape, .. } => *response_shape,
                ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
            };
            let envelope = match parsed {
                ParsedOneResponse::Json {
                    body: response_body,
                    response_shape: body_shape,
                } => Envelope::ok_with_data(
                    "flow package export dry-run ok",
                    Value::Object({
                        let mut data = one_response_metadata(
                            "flow",
                            "export-dry-run",
                            "GET",
                            &url,
                            endpoint,
                            1,
                            Some(status.as_u16()),
                            request_id.clone(),
                            true,
                            body_shape,
                            None,
                            false,
                            true,
                        );
                        data.insert("flow_id".to_string(), Value::String(flow_id.to_string()));
                        data.insert(
                            "elapsed_ms".to_string(),
                            Value::from(started.elapsed().as_millis() as u64),
                        );
                        data.insert("response".to_string(), response_body);
                        data
                    }),
                ),
                ParsedOneResponse::NonJson { .. } => one_transport_failure_envelope(
                    Some(status),
                    "flow",
                    "export-dry-run",
                    "GET",
                    &url,
                    endpoint,
                    1,
                    None,
                    &parsed,
                    false,
                    true,
                ),
            };
            let _ = record_api_event(
                observability,
                ApiEvent {
                    product: "one",
                    surface: "flow",
                    operation: "export-dry-run",
                    method: "GET",
                    endpoint_template: endpoint,
                    resolved_url: &url,
                    status_code: Some(status.as_u16()),
                    duration_ms: started.elapsed().as_millis(),
                    attempt: 1,
                    retry_after_seconds: None,
                    request_id: request_id.as_deref(),
                    ok: true,
                    error_class: None,
                    response_shape: Some(parsed_response_shape),
                    mutating: false,
                    dry_run: true,
                },
            );
            return Ok(envelope);
        }

        let bytes = response
            .bytes()
            .with_context(|| format!("failed to read flow package download from '{}'", url))?;
        if let Some(parent) = output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create flow export parent directory '{}'",
                    parent.display()
                )
            })?;
        }
        write_sensitive_file(output_path, &bytes).with_context(|| {
            format!(
                "failed to write flow package to '{}'",
                output_path.display()
            )
        })?;
        let envelope = Envelope::ok_with_data(
            "flow package exported",
            Value::Object({
                let mut data = one_response_metadata(
                    "flow",
                    "export",
                    "GET",
                    &url,
                    endpoint,
                    1,
                    Some(status.as_u16()),
                    request_id.clone(),
                    true,
                    "binary",
                    None,
                    false,
                    false,
                );
                data.insert("flow_id".to_string(), Value::String(flow_id.to_string()));
                data.insert(
                    "path".to_string(),
                    Value::String(output_path.display().to_string()),
                );
                data.insert("bytes".to_string(), Value::from(bytes.len() as u64));
                data.insert(
                    "elapsed_ms".to_string(),
                    Value::from(started.elapsed().as_millis() as u64),
                );
                data.insert("response".to_string(), Value::Null);
                data
            }),
        );
        let _ = record_api_event(
            observability,
            ApiEvent {
                product: "one",
                surface: "flow",
                operation: "export",
                method: "GET",
                endpoint_template: endpoint,
                resolved_url: &url,
                status_code: Some(status.as_u16()),
                duration_ms: started.elapsed().as_millis(),
                attempt: 1,
                retry_after_seconds: None,
                request_id: request_id.as_deref(),
                ok: true,
                error_class: None,
                response_shape: Some("binary"),
                mutating: false,
                dry_run: false,
            },
        );
        return Ok(envelope);
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().unwrap_or_else(|_| String::new());
    let parsed = parse_one_response(&content_type, &text);
    let parsed_response_shape = match &parsed {
        ParsedOneResponse::Json { response_shape, .. } => *response_shape,
        ParsedOneResponse::NonJson { response_kind, .. } => *response_kind,
    };
    let envelope = match parsed {
        ParsedOneResponse::Json {
            body: response_body,
            response_shape: body_shape,
        } => one_http_envelope(
            status,
            format!(
                "flow package {} failed",
                if dry_run { "dry-run" } else { "export" }
            ),
            Value::Object({
                let mut data = one_response_metadata(
                    "flow",
                    if dry_run { "export-dry-run" } else { "export" },
                    "GET",
                    &url,
                    endpoint,
                    1,
                    Some(status.as_u16()),
                    request_id.clone(),
                    false,
                    body_shape,
                    None,
                    false,
                    dry_run,
                );
                data.insert("flow_id".to_string(), Value::String(flow_id.to_string()));
                data.insert(
                    "elapsed_ms".to_string(),
                    Value::from(started.elapsed().as_millis() as u64),
                );
                data.insert("response".to_string(), response_body);
                data.insert(
                    "error_code".to_string(),
                    ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                        .map_or(Value::Null, |c| Value::String(c.as_str().to_string())),
                );
                data
            }),
        ),
        ParsedOneResponse::NonJson { .. } => one_transport_failure_envelope(
            Some(status),
            "flow",
            if dry_run { "export-dry-run" } else { "export" },
            "GET",
            &url,
            endpoint,
            1,
            None,
            &parsed,
            false,
            dry_run,
        ),
    };
    let _ = record_api_event(
        observability,
        ApiEvent {
            product: "one",
            surface: "flow",
            operation: if dry_run { "export-dry-run" } else { "export" },
            method: "GET",
            endpoint_template: endpoint,
            resolved_url: &url,
            status_code: Some(status.as_u16()),
            duration_ms: 0,
            attempt: 1,
            retry_after_seconds: None,
            request_id: request_id.as_deref(),
            ok: false,
            error_class: None,
            response_shape: Some(parsed_response_shape),
            mutating: false,
            dry_run,
        },
    );
    Ok(envelope)
}

fn build_client() -> Result<Client> {
    ONE_HTTP_CLIENT.with(|cache| {
        if let Some(client) = cache.borrow().as_ref() {
            return Ok((**client).clone());
        }
        let timeout = Duration::from_secs(60);
        let mut builder = Client::builder().timeout(timeout);
        if no_verify_tls() {
            // Lab/dev only — operator opted in explicitly via --no-verify-tls.
            builder = builder.danger_accept_invalid_certs(true);
            eprintln!(
                "[warn] TLS certificate verification disabled for One API transport (--no-verify-tls). Never use this in production."
            );
        }
        let client = builder
            .build()
            .context("failed to build one api HTTP client")?;
        *cache.borrow_mut() = Some(ManuallyDrop::new(client.clone()));
        Ok(client)
    })
}

fn resolve_one_access_token(config: &Config, client: &Client) -> Result<String> {
    use ayx_core::profile::AuthMode;

    validate_active_credential_bindings(config)?;

    let auth_mode = config
        .alteryx_one
        .as_ref()
        .map(|one| &one.auth_mode)
        .cloned()
        .unwrap_or_default();

    // Service-principal mode: skip user/refresh flow entirely.
    if auth_mode == AuthMode::ServicePrincipal {
        if config
            .alteryx_one
            .as_ref()
            .and_then(|one| one.resolved_credential_kind())
            .is_some()
        {
            bail!(
                "alteryx_one.auth_mode=service-principal conflicts with the selected workspace user credential method; use auth_mode=user for email-OTP or OAuth refresh authentication"
            );
        }
        return service_principal_access_token(config, client);
    }

    // User mode (default): access_token → refresh → no SP fallthrough.
    if let Some(access_token) = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.resolved_access_token())
    {
        return Ok(access_token.to_string());
    }

    if config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.resolved_refresh_token())
        .is_some()
    {
        return refresh_one_access_token_for_request(config, client);
    }

    Err(anyhow::anyhow!(
        "no Alteryx One credentials configured — set alteryx_one.access_token / refresh_token for user auth, or set alteryx_one.auth_mode: service-principal with sp_client_id + client_secret + sp_token_endpoint_url for SP auth"
    ))
}

/// Confirm the token's current workspace matches `expected_workspace_id`.
///
/// Returns `Ok(())` on match. Returns `Err` on mismatch or when the lookup
/// itself fails — both are fail-closed for safety. Network errors during the
/// check also abort the mutation; that's intentional (we can't verify, so we
/// don't mutate).
fn verify_workspace_identity(
    config: &Config,
    surface: &str,
    operation: &str,
    mutation_url: &str,
    expected: &str,
    expected_gid: &str,
    token_override: Option<&str>,
) -> Result<()> {
    let client = build_client()?;
    let token = match token_override {
        Some(token) => token.to_string(),
        None => resolve_one_access_token(config, &client)?,
    };
    let url = format!("{}/v4/workspaces/current", resolve_one_base_url(config));
    let response = client
        .get(&url)
        .header(AUTHORIZATION, bearer_authorization_value(&token))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .with_context(|| format!("workspace preflight failed for {surface} {operation}"))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        let preview = redact_text(&text.chars().take(200).collect::<String>());
        bail!(
            "workspace preflight failed: GET /v4/workspaces/current returned {} ({}). Refusing to send mutating request to {mutation_url}. Verify token or unset alteryx_one.expected_workspace_id.",
            status.as_u16(),
            preview
        );
    }
    let body: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(err) => {
            // Don't silently treat a non-JSON 200 response as missing-id —
            // surface the actual payload prefix so the operator can see
            // whether they hit an HTML error page from a proxy.
            let preview = redact_text(&text.chars().take(200).collect::<String>());
            bail!(
                "workspace preflight parse failure: /v4/workspaces/current returned a non-JSON {} response ({}). Body preview: '{}'. Refusing to send mutating request to {mutation_url}.",
                status.as_u16(),
                err,
                preview
            );
        }
    };
    let identity = parse_current_workspace_identity(&body).map_err(|err| {
        anyhow::anyhow!(
            "workspace preflight identity parse failure: {err}. Refusing to send mutating request to {mutation_url}"
        )
    })?;
    if identity.workspace_id != expected || identity.workspace_gid != expected_gid {
        bail!(
            "workspace mismatch: expected ('{expected}', '{expected_gid}'), token is authenticated for ('{}', '{}'). Refusing to send mutating request to {mutation_url}",
            identity.workspace_id,
            identity.workspace_gid
        );
    }
    Ok(())
}

const ACCESS_TOKEN_REFRESH_SKEW_SECONDS: u64 = 30;

fn one_access_token_needs_refresh(config: &Config) -> bool {
    let Some(one) = config.alteryx_one.as_ref() else {
        return false;
    };
    if one.auth_mode == ayx_core::profile::AuthMode::ServicePrincipal
        || one.resolved_access_token().is_none()
    {
        return false;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map_or(0, |duration| duration.as_secs());
    let deadline = now.saturating_add(ACCESS_TOKEN_REFRESH_SKEW_SECONDS);
    if let Some(exp) = decode_jwt_claims(one.resolved_access_token().unwrap_or(""))
        .and_then(|claims| claims.get("exp").and_then(Value::as_u64))
    {
        return exp <= deadline;
    }
    one.resolved_access_token_expires_at()
        .is_some_and(|expires_at| expires_at <= deadline)
}

pub fn refresh_one_access_token(config: &Config, client: &Client) -> Result<String> {
    Ok(refresh_one_tokens(config, client)?.access_token)
}

/// The result of a One token-endpoint exchange. The access token is kept raw;
/// callers add the Authorization scheme when constructing an API request.
/// This type intentionally has no `Debug` implementation.
pub struct RefreshedOneTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: String,
}

/// Exchange the configured refresh token and preserve any replacement refresh
/// token returned by the provider. Persistence belongs to the CLI/core layer.
pub fn refresh_one_tokens(config: &Config, client: &Client) -> Result<RefreshedOneTokens> {
    validate_active_credential_bindings(config)?;
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing alteryx_one section"))?;
    let workspace_id = one.active_workspace_id();
    let client_id = one.resolved_oauth_client_id().ok_or_else(|| {
        anyhow::anyhow!("alteryx_one.oauth_client_id is required to refresh access tokens")
    })?;
    let refresh_token = one.resolved_refresh_token().ok_or_else(|| {
        anyhow::anyhow!("alteryx_one.refresh_token is required to refresh access tokens")
    })?;
    let token_endpoint_url = one
        .effective_token_endpoint_url_for_workspace(workspace_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "alteryx_one.base_url or token_endpoint_url is required to refresh access tokens"
            )
        })?;
    let token_endpoint = trusted_one_endpoint(&token_endpoint_url)
        .context("refresh token endpoint failed Alteryx One trust validation")?;
    let workspace_context = workspace_context_header_value(config);
    if let Some(ref workspace_context) = workspace_context {
        trace_one(format!(
            "refresh token request to {} using workspace context {}",
            token_endpoint_url, workspace_context
        ));
    }

    let mut request = client
        .post(token_endpoint.as_str())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(ref workspace_context) = workspace_context {
        request = request.header("x-trifacta-person-workspace-id", workspace_context.as_str());
    }
    let response = request
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .send()
        .with_context(|| format!("refresh token request to '{}' failed", token_endpoint))?;
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("x-correlation-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    trace_one(format!(
        "refresh token request to {} returned {}",
        token_endpoint_url,
        status.as_u16()
    ));
    let text = response
        .text()
        .context("failed to read refresh token response body")?;
    if !status.is_success() {
        bail!(
            "{}: refresh token request to '{}' returned {} ({})",
            token_failure_prefix(status),
            token_endpoint_url,
            status.as_u16(),
            oauth_token_error_summary(&text, request_id.as_deref())
        );
    }
    let token_json: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "auth failed: refresh token response from '{}' was not valid JSON. Body preview: '{}'",
            token_endpoint_url,
            response_body_preview(&text)
        )
    })?;
    parse_one_token_response(&token_json)
}

/// Refresh a token for an API request and persist a provider-issued
/// replacement when the credential is backed by a canonical keyring entry.
/// Environment, inline, and legacy references are intentionally not mutated.
fn refresh_one_access_token_for_request(config: &Config, client: &Client) -> Result<String> {
    let store = ayx_core::one_credential_store::OneCredentialStore::from_config(config)
        .map_err(|err| anyhow::anyhow!(err))?;
    if let Some(store) = store {
        let lease = store
            .acquire_refresh()
            .map_err(|err| anyhow::anyhow!(err))?;
        let refreshed = refresh_one_tokens(lease.config(), client)?;
        lease
            .commit_rotation(&refreshed.access_token, refreshed.refresh_token.as_deref())
            .map_err(|err| {
                anyhow::anyhow!(
                    "refresh exchange succeeded but local credential persistence failed; the provider may have rotated the refresh token, so do not retry this exchange blindly: {err}"
                )
            })?;
        return Ok(refreshed.access_token);
    }

    let refreshed = refresh_one_tokens(config, client)?;
    if refreshed.refresh_token.is_some() {
        bail!(
            "refresh token rotation cannot be persisted for this credential source; import the OAuth token into secure keyring storage with `ayx one login --auth-method oauth-refresh --refresh-token-env NAME --secret-policy secure`"
        );
    }
    Ok(refreshed.access_token)
}

fn auth_binding_for_workspace(
    config: &Config,
    workspace_id: Option<&str>,
) -> Result<CredentialBinding> {
    let one = config
        .alteryx_one
        .as_ref()
        .context("config missing alteryx_one section")?;
    let base_url = one
        .normalized_base_url()
        .context("alteryx_one.base_url is required for credential binding")?;
    let issuer = one
        .effective_token_endpoint_url_for_workspace(workspace_id)
        .unwrap_or_else(|| base_url.clone());
    let region = url::Url::parse(&base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .and_then(|host| host.split('.').next().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let workspace_gid = workspace_id
        .and_then(|id| one.workspace_credentials.get(id))
        .and_then(|credential| credential.workspace_gid.clone())
        .or_else(|| one.workspace_gid.clone());
    CredentialBinding::new(
        one.account_email.clone(),
        issuer,
        region,
        base_url,
        workspace_id.map(str::to_string),
        workspace_gid,
    )
    .map_err(anyhow::Error::from)
}

fn bound_reference_matches(
    reference: Option<&str>,
    bindings: &[(&CredentialBinding, &str)],
    field: &str,
) -> Result<()> {
    let Some(reference) = reference else {
        return Ok(());
    };
    let Some(account) = reference.strip_prefix("keyring:") else {
        // Inline, env, and legacy profile-scoped references remain readable.
        return Ok(());
    };
    if !account.starts_with("v1/") {
        return Ok(());
    }
    let expected = bindings.iter().any(|(binding, expected_field)| {
        *expected_field == field
            && account == ayx_core::secrets::bound_keyring_account(binding, field)
    });
    if expected {
        Ok(())
    } else {
        bail!(
            "credential binding mismatch for {field}; refusing to consume a credential bound to a different account, issuer, region, base URL, workspace, or workspace GID"
        )
    }
}

/// Enforce binding at the point where a One credential can actually be used.
/// Profile loading intentionally remains backward-compatible; this check is
/// fail-closed for the active workspace before access/refresh/client-secret
/// resolution can reach the network.
fn validate_active_credential_bindings(config: &Config) -> Result<()> {
    let Some(one) = config.alteryx_one.as_ref() else {
        return Ok(());
    };
    let active_workspace_id = one.active_workspace_id();
    let base_binding = auth_binding_for_workspace(config, None)?;
    let active_binding = active_workspace_id
        .map(|workspace_id| auth_binding_for_workspace(config, Some(workspace_id)))
        .transpose()?;

    let top_level_fields = [
        ("alteryx_one.access_token", one.access_token_ref.as_deref()),
        (
            "alteryx_one.refresh_token",
            one.refresh_token_ref.as_deref(),
        ),
        (
            "alteryx_one.workspace_password",
            one.workspace_password_ref.as_deref(),
        ),
        (
            "alteryx_one.client_secret",
            one.client_secret_ref.as_deref(),
        ),
        (
            "alteryx_one.sp_client_secret",
            one.sp_client_secret_ref.as_deref(),
        ),
    ];
    for (field, reference) in top_level_fields {
        let mut bindings = vec![(&base_binding, field)];
        if let Some(binding) = active_binding.as_ref() {
            bindings.push((binding, field));
        }
        bound_reference_matches(reference, &bindings, field)?;
    }

    if let Some(workspace_id) = active_workspace_id
        && let Some(credential) = one.workspace_credentials.get(workspace_id)
        && let Some(binding) = active_binding.as_ref()
    {
        let fields = [
            ("access_token", credential.access_token_ref.as_deref()),
            ("refresh_token", credential.refresh_token_ref.as_deref()),
            (
                "workspace_password",
                credential.workspace_password_ref.as_deref(),
            ),
            ("client_secret", credential.client_secret_ref.as_deref()),
            (
                "sp_client_secret",
                credential.sp_client_secret_ref.as_deref(),
            ),
        ];
        for (short_field, reference) in fields {
            let field =
                format!("alteryx_one.workspace_credentials['{workspace_id}'].{short_field}");
            bound_reference_matches(reference, &[(binding, field.as_str())], &field)?;
        }
    }
    // Only the active workspace is checked above; inactive credentials cannot
    // become an accidental fallback through this resolver.
    Ok(())
}

pub fn client_credentials_one_access_token(
    token_endpoint_url: &str,
    client_id: &str,
    client_secret: &str,
    workspace_gid: Option<&str>,
    client: &Client,
) -> Result<String> {
    let token_endpoint = trusted_one_endpoint(token_endpoint_url)
        .context("service-principal token endpoint failed Alteryx One trust validation")?;
    // Ping Identity requires client_secret_post (creds in the form body).
    // Basic auth returns 401 "Unsupported authentication method".
    // scope=w:<gid> is required; without it the token carries only "sp:auth"
    // which the API rejects.
    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    let scope_value;
    if let Some(gid) = workspace_gid {
        scope_value = format!("w:{}", gid);
        params.push(("scope", scope_value.as_str()));
    }
    let response = client
        .post(token_endpoint.as_str())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .with_context(|| {
            format!(
                "client credentials token request to '{}' failed",
                token_endpoint_url
            )
        })?;
    let status = response.status();
    trace_one(format!(
        "client credentials token request to {} returned {}",
        token_endpoint_url,
        status.as_u16()
    ));
    if !status.is_success() {
        std::mem::forget(response);
        bail!(
            "{}: client credentials token request to '{}' returned {}",
            token_failure_prefix(status),
            token_endpoint_url,
            status.as_u16()
        );
    }
    let text = response
        .text()
        .context("failed to read client credentials response body")?;
    let token_json: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "auth failed: client credentials response from '{}' was not valid JSON. Body preview: '{}'",
            token_endpoint_url,
            response_body_preview(&text)
        )
    })?;
    format_refresh_token_response(&token_json)
}

/// Returns `(sp_client_id, client_secret, sp_token_endpoint_url, workspace_gid)`.
fn service_principal_credentials(
    config: &Config,
) -> Option<(String, String, String, Option<String>)> {
    // Environment-supplied SP credentials arrive through the profile loader
    // (`AYX_ONE_SP_CLIENT_ID` / `AYX_ONE_SP_CLIENT_SECRET` /
    // `AYX_ONE_SP_TOKEN_ENDPOINT_URL` in `apply_env_fallbacks`), which honours
    // AYX_CONFIG_HOME isolation and records secrets as `env:` references.
    // Reading them again here would bypass both.
    let one = config.alteryx_one.as_ref()?;
    // Use the SP-specific client_id, NOT the user oauth_client_id.
    let client_id = one.resolved_sp_client_id()?.to_string();
    let client_secret = one.resolved_sp_client_secret()?.to_string();
    // SP has its own regional token endpoint, separate from the user flow one.
    let token_endpoint_url = one.effective_sp_token_endpoint_url()?;
    let workspace_gid = one.resolved_workspace_gid().map(str::to_string);
    trace_one("service principal credentials resolved from config");

    Some((client_id, client_secret, token_endpoint_url, workspace_gid))
}

fn service_principal_access_token(config: &Config, client: &Client) -> Result<String> {
    let (client_id, client_secret, token_endpoint_url, workspace_gid) =
        service_principal_credentials(config).ok_or_else(|| {
            anyhow::anyhow!(
                "service-principal auth requires alteryx_one.sp_client_id (or AYX_ONE_SP_CLIENT_ID), client_secret, and sp_token_endpoint_url"
            )
        })?;
    trace_one(format!(
        "service principal token request using endpoint {} and client_id present",
        token_endpoint_url
    ));

    client_credentials_one_access_token(
        &token_endpoint_url,
        &client_id,
        &client_secret,
        workspace_gid.as_deref(),
        client,
    )
}

pub fn parse_one_token_response(token_json: &Value) -> Result<RefreshedOneTokens> {
    let token_type = token_json
        .get("token_type")
        .and_then(Value::as_str)
        .unwrap_or("Bearer")
        .trim();
    if !token_type.eq_ignore_ascii_case("bearer") {
        bail!("token response used unsupported token type")
    }
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("token response missing access_token"))?;
    let refresh_token = token_json
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(RefreshedOneTokens {
        access_token: access_token.to_string(),
        refresh_token,
        expires_in: token_json.get("expires_in").and_then(Value::as_u64),
        token_type: "Bearer".to_string(),
    })
}

/// Compatibility projection for callers that need only the raw access token.
pub fn format_refresh_token_response(token_json: &Value) -> Result<String> {
    // Callers add the Authorization scheme when constructing an API request.
    // Keep this helper's result raw; returning "Bearer ..." would produce
    // "Bearer Bearer ..." on the wire.
    Ok(parse_one_token_response(token_json)?.access_token)
}

// ---------------------------------------------------------------------------
// Device Authorization Grant (RFC 8628) — no browser redirect required.
// User visits a short URL on any device and enters a code.
// ---------------------------------------------------------------------------

/// Response from the device authorization endpoint.
#[derive(Debug)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Full URI with the code embedded — open this in a browser to skip typing.
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    /// Minimum poll interval in seconds.
    pub interval: u64,
}

/// Initiate a device authorization request.  Returns the codes and URI to show
/// the user.  Uses `scope=openid` which gives a workspace-scoped token.
pub fn initiate_device_auth(
    device_auth_endpoint: &str,
    client_id: &str,
    client: &Client,
) -> Result<DeviceAuthResponse> {
    let resp = client
        .post(device_auth_endpoint)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[("client_id", client_id), ("scope", "openid")])
        .send()
        .with_context(|| {
            format!("device authorization request to '{device_auth_endpoint}' failed")
        })?;
    let status = resp.status();
    let text = resp.text().context("failed to read device auth response")?;
    if !status.is_success() {
        bail!(
            "device authorization request returned {}: {}",
            status.as_u16(),
            response_body_preview(&text)
        );
    }
    let j: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "device auth response was not JSON: {}",
            response_body_preview(&text)
        )
    })?;
    Ok(DeviceAuthResponse {
        device_code: j["device_code"]
            .as_str()
            .context("device auth response missing device_code")?
            .to_string(),
        user_code: j["user_code"]
            .as_str()
            .context("device auth response missing user_code")?
            .to_string(),
        verification_uri: j["verification_uri"]
            .as_str()
            .context("device auth response missing verification_uri")?
            .to_string(),
        verification_uri_complete: j["verification_uri_complete"].as_str().map(str::to_string),
        expires_in: j["expires_in"].as_u64().unwrap_or(300),
        interval: j["interval"].as_u64().unwrap_or(5),
    })
}

/// Single poll attempt.  Returns `Ok(Some((access_token, refresh_token)))` on
/// success, `Ok(None)` when the user hasn't approved yet (authorization_pending
/// / slow_down), or `Err` on a terminal failure.
pub fn poll_device_token(
    token_endpoint: &str,
    client_id: &str,
    device_code: &str,
    client: &Client,
) -> Result<Option<(String, Option<String>)>> {
    let resp = client
        .post(token_endpoint)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
            ("client_id", client_id),
        ])
        .send()
        .context("device token poll request failed")?;
    let status = resp.status();
    let text = resp
        .text()
        .context("failed to read device token poll response")?;
    if status.is_success() {
        let j: Value = serde_json::from_str(&text).with_context(|| {
            format!(
                "device token response was not JSON: {}",
                response_body_preview(&text)
            )
        })?;
        let access_token = j["access_token"]
            .as_str()
            .context("device token response missing access_token")?
            .to_string();
        let refresh_token = j["refresh_token"].as_str().map(str::to_string);
        return Ok(Some((access_token, refresh_token)));
    }
    // 4xx errors from the token endpoint carry an `error` field.
    let j: Value = serde_json::from_str(&text).unwrap_or_default();
    let error = j["error"].as_str().unwrap_or("");
    match error {
        "authorization_pending" | "slow_down" => Ok(None),
        "expired_token" => bail!("device code expired — run `ayx one login` again"),
        "access_denied" => bail!("access denied — the user rejected the authorization request"),
        _ => bail!(
            "device token poll returned {}: {}",
            status.as_u16(),
            response_body_preview(&text)
        ),
    }
}

// ---------------------------------------------------------------------------
// Authorization Code + PKCE (RFC 7636) — browser redirect flow.
// ---------------------------------------------------------------------------

/// PKCE code_verifier + code_challenge pair (S256).
pub struct PkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
}

/// Generate a fresh PKCE challenge using 32 cryptographically random bytes (256-bit verifier).
///
/// Panics if the OS entropy source is unavailable — a weak verifier is worse than failing.
pub fn generate_pkce_challenge() -> PkceChallenge {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use sha2::{Digest, Sha256};

    let mut verifier_bytes = [0u8; 32];
    getrandom::fill(&mut verifier_bytes)
        .expect("OS entropy source unavailable — cannot generate PKCE verifier");
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(digest);
    PkceChallenge {
        code_verifier,
        code_challenge,
    }
}

/// Generate `n` cryptographically random bytes as a base64url string.
///
/// Panics if the OS entropy source is unavailable.
pub fn generate_random_state(n: usize) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes)
        .expect("OS entropy source unavailable — cannot generate OAuth state");
    URL_SAFE_NO_PAD.encode(&bytes)
}

/// Exchange an authorization code for tokens.
pub fn exchange_auth_code(
    token_endpoint: &str,
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    client: &Client,
) -> Result<(String, Option<String>)> {
    let resp = client
        .post(token_endpoint)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .context("authorization code exchange request failed")?;
    let status = resp.status();
    let text = resp
        .text()
        .context("failed to read token exchange response")?;
    if !status.is_success() {
        bail!(
            "authorization code exchange returned {}: {}",
            status.as_u16(),
            response_body_preview(&text)
        );
    }
    let j: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "token exchange response was not JSON: {}",
            response_body_preview(&text)
        )
    })?;
    let access_token = j["access_token"]
        .as_str()
        .context("token exchange response missing access_token")?
        .to_string();
    let refresh_token = j["refresh_token"].as_str().map(str::to_string);
    Ok((access_token, refresh_token))
}

#[allow(dead_code)]
fn parse_response_text(content_type: &str, text: &str) -> Value {
    match parse_one_response(content_type, text) {
        ParsedOneResponse::Json { body, .. } => body,
        ParsedOneResponse::NonJson {
            response_kind,
            body_preview,
            content_type,
            parse_error,
        } => json!({
            "raw": body_preview,
            "content_type": content_type,
            "response_kind": response_kind,
            "parse_error": parse_error,
        }),
    }
}

fn should_retry_status(status: StatusCode, mutating: bool) -> bool {
    if mutating {
        return false;
    }
    matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504)
}

fn parse_retry_after(header: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let value = header?.to_str().ok()?.trim();
    value.parse::<u64>().ok()
}

fn retry_delay(attempt: u32, retry_after_seconds: Option<u64>) -> Duration {
    if let Some(seconds) = retry_after_seconds {
        return Duration::from_secs(seconds.clamp(1, 60));
    }
    let shift = attempt.saturating_sub(1).min(6);
    let multiplier = 1u64 << shift;
    let base_ms = 250u64.saturating_mul(multiplier).min(8_000);
    // Add ±20% jitter to avoid thundering herd when many clients retry together.
    // Use a cheap pseudo-random source seeded from the system clock — we don't
    // need cryptographic randomness here.
    let jittered = apply_jitter(base_ms, 20);
    Duration::from_millis(jittered)
}

/// Percent-encode a path segment per RFC 3986. Conservative: any byte that
/// isn't an unreserved char (`ALPHA / DIGIT / '-' / '.' / '_' / '~'`) is
/// encoded as `%XX`. This is stricter than necessary (sub-delims like `!$&`
/// are technically allowed unencoded in path segments) but always safe.
///
/// Used before substituting `{id}`-style placeholders into endpoint paths
/// so an id containing `/`, `#`, `?`, etc. doesn't escape the segment.
pub(crate) fn percent_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        let is_unreserved = byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'.'
            || byte == b'_'
            || byte == b'~';
        if is_unreserved {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{:02X}", byte);
        }
    }
    out
}

fn apply_jitter(base_ms: u64, pct: u64) -> u64 {
    if base_ms == 0 || pct == 0 {
        return base_ms;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let span = base_ms.saturating_mul(pct) / 100;
    if span == 0 {
        return base_ms;
    }
    let offset = nanos % (span * 2 + 1);
    base_ms.saturating_add(offset).saturating_sub(span)
}

/// Resolve the One API base URL for this profile.
///
/// Precedence for SP mode: active workspace credential's `api_base_url`, then
/// the profile `base_url`, then env var, then the `us1` default.
/// For user mode (and as the SP fallback): profile `base_url` → env var →
/// `https://us1.alteryxcloud.com`.
pub fn resolve_one_base_url(config: &Config) -> String {
    use ayx_core::profile::AuthMode;
    // In SP mode, honour the per-credential api_base_url if set (allows
    // routing to the regional cell that trusts the SP issuer's signing key).
    if config
        .alteryx_one
        .as_ref()
        .is_some_and(|one| one.auth_mode == AuthMode::ServicePrincipal)
        && let Some(url) = config
            .alteryx_one
            .as_ref()
            .and_then(|one| one.resolved_sp_api_base_url())
    {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    if let Some(one) = config.alteryx_one.as_ref()
        && let Some(url) = one.normalized_base_url()
    {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    if let Ok(env_url) = std::env::var("AYX_ONE_API_BASE_URL") {
        let trimmed = env_url.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    ONE_API_BASE_URL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::auth::CredentialBinding;
    use ayx_core::profile::{AlteryxOneProfile, AuthMode, Config, WorkspaceCredential};
    use httpmock::prelude::*;
    use serde_yaml::from_str;
    use std::collections::BTreeMap;

    fn one_profile(base_url: &str) -> Config {
        let mut config: Config = from_str(
            r#"
profile_name: test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
        )
        .expect("config parses");
        config.alteryx_one = Some(AlteryxOneProfile {
            schema_version: ayx_core::profile::CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "tester@example.com".to_string(),
            base_url: Some(base_url.to_string()),
            oauth_client_id: Some("client-id".to_string()),
            client_secret: Some("client-secret".to_string()),
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", base_url)),
            access_token: Some("bearer-token".to_string()),
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });
        config
    }

    #[test]
    fn token_consumption_rejects_a_bound_reference_for_another_identity() {
        let mut config = one_profile("https://us1.example.test");
        let wrong_binding = CredentialBinding::new(
            "tester@example.com",
            "https://pingauth.example.test/as/token",
            "eu1",
            "https://eu1.example.test",
            None,
            None,
        )
        .expect("test binding");
        config
            .alteryx_one
            .as_mut()
            .expect("One profile")
            .access_token_ref = Some(format!(
            "keyring:{}",
            wrong_binding.keyring_account("alteryx_one.access_token")
        ));

        let error = resolve_one_access_token(&config, &Client::new())
            .expect_err("a mismatched bound token must never reach the API client");
        assert!(error.to_string().contains("credential binding mismatch"));
    }

    #[test]
    fn token_consumption_accepts_a_matching_bound_reference() {
        let mut config = one_profile("https://us1.example.test");
        let binding = auth_binding_for_workspace(&config, None).expect("test binding");
        config
            .alteryx_one
            .as_mut()
            .expect("One profile")
            .access_token_ref = Some(format!(
            "keyring:{}",
            binding.keyring_account("alteryx_one.access_token")
        ));

        validate_active_credential_bindings(&config).expect("matching binding should pass");
    }

    #[test]
    fn refresh_token_response_formats_access_token() {
        let token = format_refresh_token_response(&serde_json::json!({
            "token_type": "Bearer",
            "access_token": "fresh-token"
        }))
        .expect("response should format");
        assert_eq!(token, "fresh-token");
    }

    #[test]
    fn token_response_preserves_rotation_metadata_without_debug_secrets() {
        let refreshed = parse_one_token_response(&serde_json::json!({
            "token_type": "bearer",
            "access_token": " fresh-access ",
            "refresh_token": " fresh-refresh ",
            "expires_in": 300
        }))
        .expect("response should parse");

        assert_eq!(refreshed.access_token, "fresh-access");
        assert_eq!(refreshed.refresh_token.as_deref(), Some("fresh-refresh"));
        assert_eq!(refreshed.expires_in, Some(300));
        assert_eq!(refreshed.token_type, "Bearer");
    }

    #[test]
    fn oauth_token_error_summary_keeps_only_allowlisted_redacted_fields() {
        let summary = oauth_token_error_summary(
            r#"{
                "error":"invalid_grant",
                "error_code":"internal",
                "error_description":"refresh_token=provider-refresh-secret",
                "access_token":"provider-access-secret",
                "refresh_token":"provider-refresh-secret",
                "client_secret":"provider-client-secret"
            }"#,
            Some("request-42"),
        );

        assert!(summary.contains("oauth_error=invalid_grant"), "{summary}");
        assert!(
            summary.contains("provider_error_code=internal"),
            "{summary}"
        );
        assert!(summary.contains("request_id=request-42"), "{summary}");
        assert!(summary.contains("refresh_token=***"), "{summary}");
        for secret in [
            "provider-access-secret",
            "provider-refresh-secret",
            "provider-client-secret",
        ] {
            assert!(!summary.contains(secret), "secret leaked in {summary}");
        }
    }

    /// A provider may write a bare token into the description, where neither the
    /// `key=value` nor the JWT pattern matches. Prose must survive; anything
    /// credential-shaped must not.
    #[test]
    fn oauth_token_error_summary_masks_opaque_tokens_in_provider_prose() {
        let summary = oauth_token_error_summary(
            r#"{
                "error":"invalid_grant",
                "error_description":"refresh credential aB3xY7pQ9wL2mN5kR8tZ was revoked"
            }"#,
            None,
        );

        assert!(
            !summary.contains("aB3xY7pQ9wL2mN5kR8tZ"),
            "an opaque token in prose leaked: {summary}"
        );
        assert!(summary.contains("oauth_error=invalid_grant"), "{summary}");
        assert!(
            summary.contains("refresh credential") && summary.contains("was revoked"),
            "the readable prose must survive: {summary}"
        );
    }

    #[test]
    fn oauth_token_error_summary_keeps_ordinary_long_words() {
        assert_eq!(
            mask_opaque_runs("internationalization and authentication failed"),
            "internationalization and authentication failed",
            "plain alphabetic words are not credentials"
        );
        assert_eq!(mask_opaque_runs("token abc123def456ghi789jkl"), "token ***");
    }

    #[test]
    fn oauth_token_error_summary_never_includes_unrecognized_body_content() {
        let summary = oauth_token_error_summary(
            "token endpoint copied request refresh_token=provider-refresh-secret",
            None,
        );

        assert_eq!(summary, "provider did not return a recognized OAuth error");
        assert!(!summary.contains("provider-refresh-secret"));
    }

    #[test]
    fn token_response_rejects_non_bearer_tokens() {
        let error = match parse_one_token_response(&serde_json::json!({
            "token_type": "mac",
            "access_token": "fresh-access"
        })) {
            Ok(_) => panic!("unsupported token types must fail closed"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("unsupported token type"));
    }

    #[test]
    fn bearer_authorization_value_normalizes_persisted_scheme() {
        assert_eq!(
            bearer_authorization_value("fresh-token"),
            "Bearer fresh-token"
        );
        assert_eq!(
            bearer_authorization_value("Bearer fresh-token"),
            "Bearer fresh-token"
        );
        assert_eq!(
            bearer_authorization_value("bearer fresh-token"),
            "Bearer fresh-token"
        );
    }

    // Characterization tests for the CSPRNG helpers. They lock the observable
    // behavior — output length/encoding, the PKCE S256 relationship, and that
    // independent draws differ — so the getrandom 0.2 → 0.4 migration (which
    // renames `getrandom::getrandom` to `getrandom::fill` under these functions)
    // is shown to change nothing observable here. These are regression guards,
    // not entropy assertions: `assert_ne!` catches a grossly broken backend
    // (constant/all-zero output), but a unit test cannot prove full-buffer
    // initialization or randomness quality — that is getrandom's contract.
    #[test]
    fn pkce_challenge_is_s256_of_a_256bit_random_verifier() {
        use base64::Engine as _;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use sha2::{Digest, Sha256};

        let a = generate_pkce_challenge();
        // 32 random bytes → 43-char base64url (no padding).
        assert_eq!(a.code_verifier.len(), 43, "verifier: {}", a.code_verifier);
        let decoded = URL_SAFE_NO_PAD
            .decode(a.code_verifier.as_bytes())
            .expect("verifier is base64url");
        assert_eq!(decoded.len(), 32, "verifier must be 256 bits");
        // S256: challenge == base64url(SHA256(ascii(verifier))).
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(a.code_verifier.as_bytes()));
        assert_eq!(
            a.code_challenge, expected,
            "challenge must be the S256 of the verifier"
        );
        // Independent draws must differ.
        assert_ne!(
            a.code_verifier,
            generate_pkce_challenge().code_verifier,
            "verifiers must be random"
        );
    }

    #[test]
    fn random_state_decodes_to_requested_length_and_varies() {
        use base64::Engine as _;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;

        for n in [16usize, 32] {
            let decoded = URL_SAFE_NO_PAD
                .decode(generate_random_state(n).as_bytes())
                .expect("state is base64url");
            assert_eq!(decoded.len(), n, "state must decode to n bytes");
        }
        assert_ne!(
            generate_random_state(32),
            generate_random_state(32),
            "state must be random"
        );
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn refresh_token_includes_client_id_and_refresh_token() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/as/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=refresh_token&client_id=client-id&refresh_token=refresh-123");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token_type":"Bearer","access_token":"fresh"}"#);
        });

        let mut config: Config = from_str(
            r#"
profile_name: test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
        )
        .expect("config parses");
        config.alteryx_one = Some(AlteryxOneProfile {
            schema_version: ayx_core::profile::CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: Some("client-id".to_string()),
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: None,
            access_token_ref: None,
            refresh_token: Some("refresh-123".to_string()),
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("refresh succeeds");

        mock.assert();
        assert_eq!(token, "fresh");
    }

    #[test]
    fn workspace_context_is_derived_from_jwt_scope_claim() {
        let payload = URL_SAFE_NO_PAD.encode(r#"{"scope":"w:01AAAAAAAAAAAAAAAAAAAAAAAA"}"#);
        let token = format!("eyJhbGciOiJub25lIn0.{}.", payload);

        assert_eq!(
            workspace_context_from_token(Some(&token)),
            Some("w:01AAAAAAAAAAAAAAAAAAAAAAAA".to_string())
        );
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn refresh_token_prefers_workspace_credential_fields() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/workspace-token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=refresh_token&client_id=workspace-client&refresh_token=workspace-refresh");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token_type":"Bearer","access_token":"fresh"}"#);
        });

        let mut config: Config = from_str(
            r#"
profile_name: test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
        )
        .expect("config parses");
        let mut workspace_credentials = BTreeMap::new();
        workspace_credentials.insert(
            "ws-123".to_string(),
            WorkspaceCredential {
                workspace_id: None,
                workspace_name: None,
                credential_health: None,
                access_token: Some("workspace-stale".to_string()),
                access_token_ref: None,
                refresh_token: Some("workspace-refresh".to_string()),
                refresh_token_ref: None,
                credential_kind: None,
                access_token_expires_at: None,
                workspace_password: None,
                workspace_password_ref: None,
                oauth_client_id: Some("workspace-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                sp_client_secret: None,
                sp_client_secret_ref: None,
                token_endpoint_url: Some(format!("{}/workspace-token", server.base_url())),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );
        config.alteryx_one = Some(AlteryxOneProfile {
            schema_version: ayx_core::profile::CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: Some("legacy-stale".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials,
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: Some("ws-123".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("workspace refresh succeeds");

        mock.assert();
        assert_eq!(token, "fresh");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn client_credentials_posts_grant_type_and_returns_token() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/as/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=client_credentials&client_id=sp-client&client_secret=sp-secret");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token_type":"Bearer","access_token":"fresh-sp"}"#);
        });

        let client = reqwest::blocking::Client::new();
        let token = client_credentials_one_access_token(
            &format!("{}/as/token", server.base_url()),
            "sp-client",
            "sp-secret",
            None,
            &client,
        )
        .expect("client credentials succeeds");

        mock.assert();
        assert_eq!(token, "fresh-sp");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn refresh_token_unauthorized_is_reported_as_auth_failure() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/as/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=refresh_token&client_id=client-id&refresh_token=refresh-123");
            then.status(401)
                .header("content-type", "text/html")
                .body("<html>unauthorized</html>");
        });

        let mut config = one_profile(&server.base_url());
        config.alteryx_one.as_mut().unwrap().oauth_client_id = Some("client-id".to_string());
        config.alteryx_one.as_mut().unwrap().refresh_token = Some("refresh-123".to_string());

        let client = reqwest::blocking::Client::new();
        let err = refresh_one_access_token(&config, &client).expect_err("refresh should fail");

        mock.assert();
        let message = err.to_string();
        assert!(message.contains("auth failed"), "{message}");
        assert!(message.contains("unauthorized"), "{message}");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn client_credentials_forbidden_is_reported_as_scope_failure() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/as/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=client_credentials");
            then.status(403)
                .header("content-type", "text/html")
                .body("<html>forbidden</html>");
        });

        let client = reqwest::blocking::Client::new();
        let err = client_credentials_one_access_token(
            &format!("{}/as/token", server.base_url()),
            "client-id",
            "client-secret",
            None,
            &client,
        )
        .expect_err("client credentials should fail");

        mock.assert();
        let message = err.to_string();
        assert!(message.contains("permission denied"), "{message}");
        assert!(message.contains("forbidden"), "{message}");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn live_request_includes_request_metadata_for_json_responses() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(200)
                .header("content-type", "application/json")
                .header("x-request-id", "req-123")
                .body(r#"{"id":"ws-1","name":"Workspace"}"#);
        });

        let config = one_profile(&server.base_url());
        eprintln!("test server base url: {}", server.base_url());
        eprintln!("sending request...");
        let envelope = one_api_live_request(
            &config,
            "platform",
            "workspace-current",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )
        .expect("request should succeed");
        eprintln!("request returned");

        mock.assert();
        assert!(envelope.ok);
        assert_eq!(envelope.data["endpoint_template"], "/v4/workspaces/current");
        assert_eq!(envelope.data["method"], "GET");
        assert_eq!(
            envelope.data["url"],
            format!("{}/v4/workspaces/current", server.base_url())
        );
        assert_eq!(envelope.data["status_code"], 200);
        assert_eq!(envelope.data["request_id"], "req-123");
        assert_eq!(envelope.data["attempts"], 1);
        assert_eq!(envelope.data["response_shape"], "object");
        assert_eq!(envelope.data["response"]["id"], "ws-1");
    }

    #[test]
    fn live_request_turns_html_responses_into_transport_failures() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(200)
                .header("content-type", "text/html")
                .body("<html><body>gateway error</body></html>");
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_live_request(
            &config,
            "platform",
            "workspace-current",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )
        .expect("request should return an envelope");

        mock.assert();
        assert!(!envelope.ok);
        assert_eq!(
            envelope.error_code,
            Some(ayx_core::envelope::ErrorCode::Internal)
        );
        assert_eq!(envelope.data["response_shape"], "html");
        assert_eq!(
            envelope.data["body_preview"],
            "<html><body>gateway error</body></html>"
        );
    }

    #[test]
    fn live_request_turns_malformed_json_responses_into_transport_failures() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(200)
                .header("content-type", "application/json")
                .body("{\"id\":");
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_live_request(
            &config,
            "platform",
            "workspace-current",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )
        .expect("request should return an envelope");

        mock.assert();
        assert!(!envelope.ok);
        assert_eq!(
            envelope.error_code,
            Some(ayx_core::envelope::ErrorCode::Internal)
        );
        assert_eq!(envelope.data["response_shape"], "malformed_json");
        assert_eq!(envelope.data["status_code"], 200);
        assert!(envelope.data["parse_error"].as_str().is_some());
    }

    /// T1 regression, end-to-end: commit 86965eb's whole point was that a 2xx
    /// with a genuinely non-JSON body (e.g. `text/csv` on an export endpoint) is
    /// a success, not a transport failure. The `html`/`malformed_json` fix above
    /// must not regress this — assert it through the same `one_api_live_request`
    /// entry point the html/malformed_json tests use, not just the lower-level
    /// envelope builder unit test.
    #[test]
    fn live_request_treats_2xx_csv_body_as_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/workflows/1/export");
            then.status(200)
                .header("content-type", "text/csv")
                .body("id,name\n1,alpha\n");
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_live_request(
            &config,
            "workflow",
            "export",
            "GET",
            "/v4/workflows/1/export",
            false,
            &[],
        )
        .expect("request should return an envelope");

        mock.assert();
        assert!(envelope.ok, "2xx non-JSON body must not be an error");
        assert_eq!(envelope.error_code, None);
        assert_eq!(envelope.data["response_shape"], "non_json");
        assert_eq!(envelope.data["status_code"], 200);
    }

    /// `one_api_live_request` returns transport failures as `Ok(envelope)` with
    /// `ok: false` -- `?` does not catch that. `one_api_list_request` must check
    /// `envelope.ok` itself, or a failed list (404 here) silently becomes a
    /// successful empty list.
    #[test]
    fn list_request_json_404_is_reported_as_failure_not_an_empty_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/widgets");
            then.status(404)
                .header("content-type", "application/json")
                .body(r#"{"error":"not found"}"#);
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_list_request(
            &config,
            "widget",
            "list",
            "/v4/widgets",
            &[],
            &OneListParams::new(),
        )
        .expect("request should return an envelope");

        mock.assert();
        assert!(!envelope.ok, "a 404 list page must not report ok: true");
        assert_eq!(
            envelope.error_code,
            Some(ayx_core::envelope::ErrorCode::NotFound)
        );
    }

    /// Same failure class as above, but the upstream answers with an HTML error
    /// page instead of JSON (the shape a gateway/SSO redirect actually returns).
    #[test]
    fn list_request_html_404_is_reported_as_failure_not_an_empty_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/widgets");
            then.status(404)
                .header("content-type", "text/html")
                .body("<html><body>not found</body></html>");
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_list_request(
            &config,
            "widget",
            "list",
            "/v4/widgets",
            &[],
            &OneListParams::new(),
        )
        .expect("request should return an envelope");

        mock.assert();
        assert!(
            !envelope.ok,
            "an HTML 404 list page must not report ok: true"
        );
        assert_eq!(envelope.data["response_shape"], "html");
    }

    /// A failure on page 2+ of an auto-paginated list must still report
    /// `ok: false` -- it must never silently truncate to a "successful" partial
    /// list -- but the already-fetched page(s) should still be visible, clearly
    /// labeled `partial`, rather than thrown away.
    #[test]
    fn list_request_failure_on_a_later_page_stays_a_failure_but_keeps_partial_data() {
        let server = MockServer::start();
        let page1 = server.mock(|when, then| {
            when.method(GET)
                .path("/v4/widgets")
                .query_param("limit", "1")
                .query_param_missing("pageToken");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"data":[{"id":"a"}],"nextPageToken":"tok2"}"#);
        });
        let page2 = server.mock(|when, then| {
            when.method(GET)
                .path("/v4/widgets")
                .query_param("limit", "1")
                .query_param("pageToken", "tok2");
            then.status(404)
                .header("content-type", "application/json")
                .body(r#"{"error":"not found"}"#);
        });

        let config = one_profile(&server.base_url());
        let params = OneListParams::new()
            .with_limit(Some(1))
            .with_all(true, Some(5));
        let envelope = one_api_list_request(&config, "widget", "list", "/v4/widgets", &[], &params)
            .expect("request should return an envelope");

        page1.assert();
        page2.assert();
        assert!(
            !envelope.ok,
            "partial data from a broken pagination run must never read as ok: true"
        );
        assert_eq!(envelope.data["partial"], true);
        assert_eq!(envelope.data["pages_fetched_before_failure"], 1);
        let items = envelope.data["items"]
            .as_array()
            .expect("partial items must still be attached");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "a");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn live_request_retries_gets_but_not_mutations() {
        let server = MockServer::start();
        let get_mock = server.mock(|when, then| {
            when.method(GET).path("/v4/workspaces/current");
            then.status(500)
                .header("content-type", "application/json")
                .body(r#"{"error":"boom"}"#);
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_live_request(
            &config,
            "platform",
            "workspace-current",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )
        .expect("request should return an envelope");

        get_mock.assert_calls(4);
        assert!(!envelope.ok);
        assert_eq!(envelope.data["status_code"], 500);
        assert_eq!(envelope.data["attempts"], 4);

        set_one_apply(true);
        let put_mock = server.mock(|when, then| {
            when.method(PUT).path("/v4/workspaces/current");
            then.status(500)
                .header("content-type", "application/json")
                .body(r#"{"error":"boom"}"#);
        });
        let mutating = one_api_live_request_with_body(
            &config,
            "platform",
            "workspace-update",
            "PUT",
            "/v4/workspaces/current",
            true,
            &[],
            Some(json!({"name":"Workspace"})),
        )
        .expect("mutation should return an envelope");
        put_mock.assert_calls(1);
        assert!(!mutating.ok);
        assert_eq!(mutating.data["attempts"], 1);
        set_one_apply(false);
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn path_parameters_are_percent_encoded_before_request_dispatch() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v4/flows/f%2Fid");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"f/id"}"#);
        });

        let config = one_profile(&server.base_url());
        let envelope = one_api_live_request(
            &config,
            "flow",
            "flow-detail",
            "GET",
            "/v4/flows/{id}",
            false,
            &[("id", "f/id")],
        )
        .expect("request should succeed");

        mock.assert();
        assert!(envelope.ok);
        assert_eq!(
            envelope.data["url"],
            format!("{}/v4/flows/f%2Fid", server.base_url())
        );
        assert_eq!(envelope.data["response"]["id"], "f/id");
    }

    #[test]
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
    fn resolve_one_access_token_uses_service_principal_config_when_no_bearer_token_is_configured() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/as/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body("grant_type=client_credentials&client_id=sp-client&client_secret=sp-secret");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token_type":"Bearer","access_token":"fresh-sp"}"#);
        });

        let mut config: Config = from_str(
            r#"
profile_name: test
mongo:
  mode: embedded
  databases:
    gallery_name: AlteryxGallery
    service_name: AlteryxService
  embedded: {}
"#,
        )
        .expect("config parses");
        config.alteryx_one = Some(AlteryxOneProfile {
            schema_version: ayx_core::profile::CURRENT_PROFILE_SCHEMA_VERSION,
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: None,
            client_secret: Some("sp-secret".to_string()),
            client_secret_ref: None,
            sp_client_secret: None,
            sp_client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_password: None,
            workspace_password_ref: None,
            workspace_credentials: Default::default(),
            active_workspace_id: None,
            auth_rollout: None,
            expected_workspace_id: None,
            sp_client_id: Some("sp-client".to_string()),
            sp_token_endpoint_url: Some(format!("{}/as", server.base_url())),
            workspace_gid: None,
            auth_mode: AuthMode::ServicePrincipal,
        });

        let client = reqwest::blocking::Client::new();
        let token = resolve_one_access_token(&config, &client).expect("service principal token");

        mock.assert();
        assert_eq!(token, "fresh-sp");
    }

    #[test]
    fn response_body_preview_redacts_and_truncates() {
        let preview = response_body_preview(
            "Bearer secret-token and password=super-secret "
                .repeat(10)
                .as_str(),
        );
        assert!(preview.len() <= 200);
        assert!(!preview.contains("secret-token"));
        assert!(!preview.contains("super-secret"));
    }

    #[test]
    fn dry_run_body_redacts_provider_secrets_recursively() {
        let body = json!({
            "clientId": "safe-to-show",
            "clientSecret": "do-not-show",
            "accountKey": "also-do-not-show",
            "sharedKey": "another-secret",
            "nested": {"access_key": "also-secret"},
            "items": [{"connectionString": "secret-connection"}],
        });
        let redacted = redact_json_value(&body);
        assert_eq!(redacted["clientId"], "safe-to-show");
        assert_eq!(redacted["clientSecret"], "[REDACTED]");
        assert_eq!(redacted["accountKey"], "[REDACTED]");
        assert_eq!(redacted["sharedKey"], "[REDACTED]");
        assert_eq!(redacted["nested"]["access_key"], "[REDACTED]");
        assert_eq!(redacted["items"][0]["connectionString"], "[REDACTED]");
    }

    #[test]
    fn parse_one_response_classifies_json_html_and_malformed() {
        match parse_one_response("application/json", r#"{"id":"1"}"#) {
            ParsedOneResponse::Json {
                body,
                response_shape,
            } => {
                assert_eq!(response_shape, "object");
                assert_eq!(body["id"], "1");
            }
            other => panic!("unexpected parse result: {other:?}"),
        }

        match parse_one_response("text/html", "<html>oops</html>") {
            ParsedOneResponse::NonJson {
                response_kind,
                body_preview,
                ..
            } => {
                assert_eq!(response_kind, "html");
                assert_eq!(body_preview, "<html>oops</html>");
            }
            other => panic!("unexpected parse result: {other:?}"),
        }

        match parse_one_response("application/json", "{\"id\":") {
            ParsedOneResponse::NonJson {
                response_kind,
                parse_error,
                ..
            } => {
                assert_eq!(response_kind, "malformed_json");
                assert!(parse_error.is_some());
            }
            other => panic!("unexpected parse result: {other:?}"),
        }
    }

    #[test]
    fn one_response_metadata_sets_common_fields() {
        let data = one_response_metadata(
            "platform",
            "workspace-current",
            "GET",
            "https://example.test/v4/workspaces/current",
            "/v4/workspaces/current",
            2,
            Some(200),
            Some("req-1".to_string()),
            true,
            "object",
            Some(5),
            false,
            false,
        );

        assert_eq!(data["surface"], "platform");
        assert_eq!(data["operation"], "workspace-current");
        assert_eq!(data["method"], "GET");
        assert_eq!(data["endpoint_template"], "/v4/workspaces/current");
        assert_eq!(
            data["validation_target"],
            "https://example.test/v4/workspaces/current"
        );
        assert_eq!(data["attempts"], 2);
        assert_eq!(data["status_code"], 200);
        assert_eq!(data["request_id"], "req-1");
        assert_eq!(data["ok"], true);
        assert_eq!(data["response_shape"], "object");
        assert_eq!(data["retry_after_seconds"], 5);
    }

    #[test]
    fn transport_failure_envelope_uses_status_and_preview() {
        let parsed = ParsedOneResponse::NonJson {
            response_kind: "html",
            body_preview: "<html>forbidden</html>".to_string(),
            content_type: "text/html".to_string(),
            parse_error: None,
        };
        let envelope = one_transport_failure_envelope(
            Some(StatusCode::FORBIDDEN),
            "platform",
            "workspace-current",
            "GET",
            "https://example.test/v4/workspaces/current",
            "/v4/workspaces/current",
            1,
            None,
            &parsed,
            false,
            false,
        );

        assert!(!envelope.ok);
        assert_eq!(
            envelope.error_code,
            Some(ayx_core::envelope::ErrorCode::PermissionDenied)
        );
        assert_eq!(envelope.data["response_shape"], "html");
        assert_eq!(envelope.data["body_preview"], "<html>forbidden</html>");
    }

    /// Sibling services (`/svc-workflow`) answer unknown routes with an Express HTML
    /// page instead of the `/v4` gateway's JSON `RouteNotFoundException`. Classify it
    /// as `html` so the transport can hint at a wrong path rather than a missing record.
    #[test]
    fn parse_one_response_classifies_express_html_error_page() {
        let body = "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
                    <title>Error</title></head><body><pre>Cannot POST /api/v0/workflows/x/share\
                    </pre></body></html>";
        match parse_one_response("text/html; charset=utf-8", body) {
            ParsedOneResponse::NonJson {
                response_kind,
                body_preview,
                ..
            } => {
                assert_eq!(response_kind, "html");
                assert!(body_preview.contains("Cannot POST"));
            }
            other => panic!("unexpected parse result: {other:?}"),
        }
    }

    /// T2: an HTML error body means we never reached the JSON API gateway, so the
    /// envelope must say the path is probably wrong for that service.
    #[test]
    fn transport_failure_envelope_hints_on_html_body() {
        let parsed = ParsedOneResponse::NonJson {
            response_kind: "html",
            body_preview: "<pre>Cannot GET /svc-workflow/api/v1/nope</pre>".to_string(),
            content_type: "text/html".to_string(),
            parse_error: None,
        };
        let envelope = one_transport_failure_envelope(
            Some(StatusCode::NOT_FOUND),
            "workflow",
            "detail",
            "GET",
            "https://example.test/svc-workflow/api/v1/nope",
            "/svc-workflow/api/v1/nope",
            1,
            None,
            &parsed,
            false,
            false,
        );

        assert!(!envelope.ok);
        assert_eq!(
            envelope.error_code,
            Some(ayx_core::envelope::ErrorCode::NotFound)
        );
        let hints = envelope.data["error_hints"]
            .as_array()
            .expect("error_hints must be present for an html error body");
        assert_eq!(hints.len(), 1);
        assert!(
            hints[0]
                .as_str()
                .unwrap_or_default()
                .contains("HTML error page"),
            "hint should explain the HTML page, got: {hints:?}"
        );

        // A JSON error body must NOT get the hint.
        let json_parsed = ParsedOneResponse::NonJson {
            response_kind: "malformed_json",
            body_preview: "{".to_string(),
            content_type: "application/json".to_string(),
            parse_error: Some("eof".to_string()),
        };
        let json_envelope = one_transport_failure_envelope(
            Some(StatusCode::NOT_FOUND),
            "workflow",
            "detail",
            "GET",
            "https://example.test/v4/x",
            "/v4/x",
            1,
            None,
            &json_parsed,
            false,
            false,
        );
        assert!(json_envelope.data.get("error_hints").is_none());
    }

    /// T1 regression: a 2xx whose body is not JSON is a SUCCESS. Before this, the
    /// failure builder's `unwrap_or(Internal)` turned every `200 text/csv` into
    /// `ok: false` with `error_code: "internal"`.
    #[test]
    fn non_json_success_envelope_is_ok_and_uncoded() {
        let parsed = ParsedOneResponse::NonJson {
            response_kind: "non_json",
            body_preview: "id,name\n1,alpha".to_string(),
            content_type: "text/csv".to_string(),
            parse_error: None,
        };
        let envelope = one_non_json_success_envelope(
            StatusCode::OK,
            "workflow",
            "export",
            "GET",
            "https://example.test/v4/thing",
            "/v4/thing",
            1,
            Some("req-9".to_string()),
            None,
            &parsed,
            false,
            42,
        );

        assert!(envelope.ok, "2xx non-JSON must not be an error");
        assert_eq!(envelope.error_code, None);
        assert_eq!(envelope.data["error_code"], Value::Null);
        assert_eq!(envelope.data["ok"], true);
        assert_eq!(envelope.data["status_code"], 200);
        assert_eq!(envelope.data["response_shape"], "non_json");
        assert_eq!(envelope.data["content_type"], "text/csv");
        assert_eq!(envelope.data["body_preview"], "id,name\n1,alpha");
        assert_eq!(envelope.data["elapsed_ms"], 42);
        assert_eq!(envelope.data["request_id"], "req-9");
    }

    /// T3: svc-workflow returns `{ "assets": [...] }`; the `/v4` gateway returns
    /// `{ "data": [...] }`. Both must page.
    #[test]
    fn extract_items_handles_assets_and_data_keys() {
        let assets = json!({ "assets": [{ "id": "a" }, { "id": "b" }] });
        assert_eq!(extract_items(&assets).len(), 2);

        let data = json!({ "data": [{ "id": "a" }], "count": 1 });
        assert_eq!(extract_items(&data).len(), 1);

        let bare = json!([{ "id": "a" }]);
        assert_eq!(extract_items(&bare).len(), 1);

        let none = json!({ "unrelated": 1 });
        assert!(extract_items(&none).is_empty());
    }

    #[test]
    fn token_failure_prefixes_are_stable() {
        assert_eq!(
            token_failure_prefix(StatusCode::UNAUTHORIZED),
            "auth failed"
        );
        assert_eq!(
            token_failure_prefix(StatusCode::FORBIDDEN),
            "permission denied"
        );
        assert_eq!(
            token_failure_prefix(StatusCode::BAD_GATEWAY),
            "token request failed"
        );
    }

    #[test]
    fn retry_policy_retries_gets_but_not_mutations() {
        assert!(should_retry_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            false
        ));
        assert!(should_retry_status(StatusCode::SERVICE_UNAVAILABLE, false));
        assert!(!should_retry_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            true
        ));
        assert!(!should_retry_status(StatusCode::NOT_FOUND, false));
    }

    #[test]
    fn repeated_query_values_are_form_encoded() {
        assert_eq!(
            endpoint_with_query_params(
                "/v4/groups/users",
                &[("userIds", "u/1"), ("userIds", "u?2")],
            ),
            "/v4/groups/users?userIds=u%2F1&userIds=u%3F2"
        );
    }

    #[test]
    fn path_parameters_are_percent_encoded() {
        assert_eq!(percent_encode_path_segment("f/id?x=1"), "f%2Fid%3Fx%3D1");
        assert_eq!(percent_encode_path_segment("safe-_.~"), "safe-_.~");
    }
}
