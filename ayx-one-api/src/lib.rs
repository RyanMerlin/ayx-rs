use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ayx_core::envelope::Envelope;
use ayx_core::observability::{
    ApiEvent, record_api_event, redact_text, response_shape, transport_error_summary,
};
use ayx_core::profile::Config;
use ayx_core::sensitive::write_sensitive_file;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use serde_json::{Value, json};
use url::form_urlencoded::Serializer;
const ONE_API_BASE_URL: &str = "https://us1.alteryxcloud.com";

mod coverage;
pub mod email_otp;
mod inventory;
pub mod types;

pub use coverage::{CoverageReport, MissingEndpoint, StaleEndpoint, coverage};
pub use email_otp::{OtpAuthResult, email_otp_login};

thread_local! {
    static ONE_APPLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NO_VERIFY_TLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DEBUG_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ONE_HTTP_CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
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

fn env_file_value(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }

    let cwd = std::env::current_dir().ok()?;
    let mut candidates = vec![cwd.join(".env")];
    if let Some(parent) = cwd.parent() {
        candidates.push(parent.join(".env"));
    }
    for candidate in candidates {
        let content = std::fs::read_to_string(&candidate).ok()?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains('=') {
                continue;
            }
            let mut parts = trimmed.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let value = parts.next().unwrap_or("").trim();
            if key == name {
                let value = value
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim()
                    .to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }

    None
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
        body.cloned().unwrap_or(Value::Null),
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
    Envelope::err_coded(
        code,
        format!("{} {} failed", surface, operation),
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

/// Pagination parameters for One list endpoints.
///
/// `limit` caps results per page. `page_token` (when present) requests the
/// next page using the `nextPageToken` value the server returned previously.
/// `auto_all` enables client-side auto-pagination — the helper follows
/// `nextPageToken` until exhausted or `max_pages` is hit.
#[derive(Debug, Clone, Default)]
pub struct OneListParams {
    pub limit: Option<u32>,
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

    loop {
        if pages_fetched >= max_pages {
            break;
        }
        let mut endpoint_with_query = endpoint.to_string();
        let mut q: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = params.limit {
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
                current_token = Some(t.clone());
            }
            _ => break,
        }
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
    for key in ["items", "results", "data", "records", "value"] {
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

    // Workspace identity preflight: when --apply is set and the profile pins
    // an expected workspace id, fail closed if the token's current workspace
    // doesn't match. Avoids "right command, wrong tenant" disasters.
    if mutating
        && let Some(expected) = config
            .alteryx_one
            .as_ref()
            .and_then(|o| o.expected_workspace_id.as_deref())
    {
        verify_workspace_identity(config, surface, operation, &url, expected)?;
    }

    let client = build_client()?;
    trace_one(format!(
        "{surface} {operation}: resolving access token for {url}"
    ));
    let mut access_token = resolve_one_access_token(config, &client)?;
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
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
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
                if status == StatusCode::UNAUTHORIZED && !refreshed_once {
                    match refresh_one_access_token(config, &client) {
                        Ok(token) => {
                            access_token = token;
                            refreshed_once = true;
                            continue;
                        }
                        Err(refresh_err) => {
                            trace_one(
                                "401 refresh failed; trying service principal fallback for live request",
                            );
                            match service_principal_access_token(config, &client) {
                                Ok(token) => {
                                    access_token = token;
                                    refreshed_once = true;
                                    continue;
                                }
                                Err(_) => return Err(refresh_err),
                            }
                        }
                    }
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
        .header(AUTHORIZATION, format!("Bearer {}", access_token))
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
        .header(AUTHORIZATION, format!("Bearer {}", access_token))
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
            return Ok(client.clone());
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
        *cache.borrow_mut() = Some(client.clone());
        Ok(client)
    })
}

fn resolve_one_access_token(config: &Config, client: &Client) -> Result<String> {
    use ayx_core::profile::AuthMode;

    let auth_mode = config
        .alteryx_one
        .as_ref()
        .map(|one| &one.auth_mode)
        .cloned()
        .unwrap_or_default();

    // Service-principal mode: skip user/refresh flow entirely.
    if auth_mode == AuthMode::ServicePrincipal {
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
        match refresh_one_access_token(config, client) {
            Ok(token) => return Ok(token),
            Err(refresh_err) => {
                trace_one("refresh token flow failed; trying service principal fallback");
                if let Ok(token) = service_principal_access_token(config, client) {
                    return Ok(token);
                }
                return Err(refresh_err);
            }
        }
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
) -> Result<()> {
    let client = build_client()?;
    let token = resolve_one_access_token(config, &client)?;
    let url = format!("{}/v4/workspaces/current", resolve_one_base_url(config));
    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
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
    let actual = body
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("workspaceId").and_then(|v| v.as_str()))
        .or_else(|| body.get("workspace_id").and_then(|v| v.as_str()))
        .unwrap_or("");
    if actual.is_empty() {
        bail!(
            "workspace preflight: /v4/workspaces/current succeeded but the response had no id/workspaceId field. Body shape: {}. Refusing to send mutating request to {mutation_url}.",
            response_shape(&body)
        );
    }
    if actual != expected {
        bail!(
            "workspace mismatch: expected '{expected}', token is authenticated for '{actual}'. Refusing to send mutating request to {mutation_url}. Either re-authenticate against the expected workspace or update alteryx_one.expected_workspace_id."
        );
    }
    Ok(())
}

pub fn refresh_one_access_token(config: &Config, client: &Client) -> Result<String> {
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
    let workspace_context = workspace_context_header_value(config);
    if let Some(ref workspace_context) = workspace_context {
        trace_one(format!(
            "refresh token request to {} using workspace context {}",
            token_endpoint_url, workspace_context
        ));
    }

    let mut request = client
        .post(&token_endpoint_url)
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
        .with_context(|| format!("refresh token request to '{}' failed", token_endpoint_url))?;
    let status = response.status();
    trace_one(format!(
        "refresh token request to {} returned {}",
        token_endpoint_url,
        status.as_u16()
    ));
    if !status.is_success() {
        std::mem::forget(response);
        bail!(
            "{}: refresh token request to '{}' returned {}",
            token_failure_prefix(status),
            token_endpoint_url,
            status.as_u16()
        );
    }
    let text = response
        .text()
        .context("failed to read refresh token response body")?;
    let token_json: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "auth failed: refresh token response from '{}' was not valid JSON. Body preview: '{}'",
            token_endpoint_url,
            response_body_preview(&text)
        )
    })?;
    format_refresh_token_response(&token_json)
}

pub fn client_credentials_one_access_token(
    token_endpoint_url: &str,
    client_id: &str,
    client_secret: &str,
    workspace_gid: Option<&str>,
    client: &Client,
) -> Result<String> {
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
        .post(token_endpoint_url)
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
    let fde_client_id = env_file_value("AYX_ONE_ALTERYX_FDE_SP007_CLIENT_ID");
    let fde_client_secret = env_file_value("AYX_ONE_ALTERYX_FDE_SA007_SECRET");
    let fde_token_endpoint = env_file_value("AYX_ONE_ALTERYX_FDE_TOKEN_ENDPOINT");

    if let (Some(client_id), Some(client_secret), Some(token_endpoint_url)) =
        (fde_client_id, fde_client_secret, fde_token_endpoint)
    {
        trace_one("service principal credentials resolved from process env");
        return Some((client_id, client_secret, token_endpoint_url, None));
    }

    let one = config.alteryx_one.as_ref()?;
    // Use the SP-specific client_id, NOT the user oauth_client_id.
    let client_id = one.resolved_sp_client_id()?.to_string();
    let client_secret = one.resolved_client_secret()?.to_string();
    // SP has its own regional token endpoint (pingauth-us1-4), separate from
    // the user flow endpoint.
    let token_endpoint_url = one.effective_sp_token_endpoint_url()?;
    let workspace_gid = one.resolved_workspace_gid().map(str::to_string);
    trace_one("service principal credentials resolved from config");

    Some((client_id, client_secret, token_endpoint_url, workspace_gid))
}

fn service_principal_access_token(config: &Config, client: &Client) -> Result<String> {
    let (client_id, client_secret, token_endpoint_url, workspace_gid) =
        service_principal_credentials(config).ok_or_else(|| {
            anyhow::anyhow!(
                "service-principal auth requires alteryx_one.sp_client_id (or AYX_ONE_SP_CLIENT_ID / AYX_ONE_ALTERYX_FDE_SP007_CLIENT_ID), client_secret, and sp_token_endpoint_url"
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

pub fn format_refresh_token_response(token_json: &Value) -> Result<String> {
    let token_type = token_json
        .get("token_type")
        .and_then(Value::as_str)
        .unwrap_or("Bearer");
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("refresh token response missing access_token"))?;

    Ok(format!("{token_type} {access_token}"))
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
        "expired_token" => bail!("device code expired — run `ayx one platform auth login` again"),
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
    getrandom::getrandom(&mut verifier_bytes)
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
    getrandom::getrandom(&mut bytes)
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
            account_email: "tester@example.com".to_string(),
            base_url: Some(base_url.to_string()),
            oauth_client_id: Some("client-id".to_string()),
            client_secret: Some("client-secret".to_string()),
            client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", base_url)),
            access_token: Some("bearer-token".to_string()),
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });
        config
    }

    #[test]
    fn refresh_token_response_formats_access_token() {
        let token = format_refresh_token_response(&serde_json::json!({
            "token_type": "Bearer",
            "access_token": "fresh-token"
        }))
        .expect("response should format");
        assert_eq!(token, "Bearer fresh-token");
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
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: Some("client-id".to_string()),
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: None,
            access_token_ref: None,
            refresh_token: Some("refresh-123".to_string()),
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("refresh succeeds");

        mock.assert();
        assert_eq!(token, "Bearer fresh");
    }

    #[test]
    fn workspace_context_is_derived_from_jwt_scope_claim() {
        let payload = URL_SAFE_NO_PAD.encode(r#"{"scope":"w:01KMGF85WTTEJWZ397MW1RBD9ZB"}"#);
        let token = format!("eyJhbGciOiJub25lIn0.{}.", payload);

        assert_eq!(
            workspace_context_from_token(Some(&token)),
            Some("w:01KMGF85WTTEJWZ397MW1RBD9ZB".to_string())
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
                access_token: Some("workspace-stale".to_string()),
                access_token_ref: None,
                refresh_token: Some("workspace-refresh".to_string()),
                refresh_token_ref: None,
                oauth_client_id: Some("workspace-client".to_string()),
                client_secret: None,
                client_secret_ref: None,
                token_endpoint_url: Some(format!("{}/workspace-token", server.base_url())),
                sp_client_id: None,
                workspace_gid: None,
                api_base_url: None,
            },
        );
        config.alteryx_one = Some(AlteryxOneProfile {
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: Some("legacy-client".to_string()),
            client_secret: None,
            client_secret_ref: None,
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: Some("legacy-stale".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_credentials,
            expected_workspace_id: Some("ws-123".to_string()),
            sp_client_id: None,
            sp_token_endpoint_url: None,
            workspace_gid: None,
            auth_mode: AuthMode::default(),
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("workspace refresh succeeds");

        mock.assert();
        assert_eq!(token, "Bearer fresh");
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
        assert_eq!(token, "Bearer fresh-sp");
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
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
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
    #[ignore = "httpmock hangs in this environment; live smoke covers live transport"]
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
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: None,
            client_secret: Some("sp-secret".to_string()),
            client_secret_ref: None,
            token_endpoint_url: None,
            access_token: None,
            access_token_ref: None,
            refresh_token: None,
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
            sp_client_id: Some("sp-client".to_string()),
            sp_token_endpoint_url: Some(format!("{}/as", server.base_url())),
            workspace_gid: None,
            auth_mode: AuthMode::ServicePrincipal,
        });

        let client = reqwest::blocking::Client::new();
        let token = resolve_one_access_token(&config, &client).expect("service principal token");

        mock.assert();
        assert_eq!(token, "Bearer fresh-sp");
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
    fn path_parameters_are_percent_encoded() {
        assert_eq!(percent_encode_path_segment("f/id?x=1"), "f%2Fid%3Fx%3D1");
        assert_eq!(percent_encode_path_segment("safe-_.~"), "safe-_.~");
    }
}
