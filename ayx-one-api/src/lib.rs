use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ayx_core::envelope::Envelope;
use ayx_core::observability::{
    record_api_event, redact_text, response_shape, transport_error_summary, ApiEvent,
};
use ayx_core::profile::Config;
use ayx_core::sensitive::write_sensitive_file;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use reqwest::StatusCode;
use serde_json::{json, Value};
use url::form_urlencoded::Serializer;
const ONE_API_BASE_URL: &str = "https://us1.alteryxcloud.com";

mod inventory;
pub mod types;

thread_local! {
    static ONE_APPLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NO_VERIFY_TLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DEBUG_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    body: Option<&Value>,
) -> Envelope {
    Envelope::ok_with_data(
        format!(
            "{} {} dry-run (pass --apply to execute)",
            surface, operation
        ),
        json!({
            "surface": surface,
            "operation": operation,
            "method": method,
            "url": url,
            "ok": true,
            "dry_run": true,
            "apply": false,
            "mutating": true,
            "would_send": body.cloned().unwrap_or(Value::Null),
            "message": format!(
                "{} {} would be sent. Re-run with --apply to execute.",
                surface, operation
            ),
        }),
    )
}

fn one_http_envelope(status: StatusCode, message: String, data: Value) -> Envelope {
    match ayx_core::envelope::ErrorCode::from_http_status(status.as_u16()) {
        Some(code) => Envelope::err_coded(code, message, data),
        None => Envelope::ok_with_data(message, data),
    }
}

pub use inventory::{inventory_endpoints, one_surface_inventory_envelope};

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
        if let Some(s) = response.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
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
        let envelope = one_dry_run_envelope(surface, operation, method, &url, body.as_ref());
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
    if mutating {
        if let Some(expected) = config
            .alteryx_one
            .as_ref()
            .and_then(|o| o.expected_workspace_id.as_deref())
        {
            verify_workspace_identity(config, surface, operation, &url, expected)?;
        }
    }

    let client = build_client()?;
    let mut access_token = resolve_one_access_token(config, &client)?;
    let method_name = method.to_string();
    let method = reqwest::Method::from_bytes(method_name.as_bytes())
        .map_err(|_| anyhow::anyhow!("unsupported one api method '{}'", method))?;
    let mut attempt = 0u32;
    let max_attempts = if mutating { 1 } else { 4 };
    let started = Instant::now();
    let mut last_status: Option<StatusCode> = None;
    let mut retry_after_seconds: Option<u64> = None;
    let mut refreshed_once = false;

    loop {
        attempt += 1;
        let mut request = client
            .request(method.clone(), &url)
            .header(AUTHORIZATION, format!("Bearer {}", access_token))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(ref payload) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .json(payload);
        } else if mutating {
            request = request.header(CONTENT_TYPE, "application/json");
        }

        let response = request.send();
        match response {
            Ok(response) => {
                let status = response.status();
                last_status = Some(status);
                retry_after_seconds = parse_retry_after(response.headers().get(RETRY_AFTER));
                if status == StatusCode::UNAUTHORIZED && !refreshed_once {
                    access_token = refresh_one_access_token(config, &client)?;
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
                    let response_body = parse_response_text(&content_type, &text);
                    let envelope = one_http_envelope(
                        status,
                        format!(
                            "{} {} {}",
                            surface,
                            operation,
                            if status.is_success() { "ok" } else { "failed" }
                        ),
                        json!({
                            "surface": surface,
                            "operation": operation,
                            "method": method_name,
                            "url": url,
                            "attempts": attempt,
                            "elapsed_ms": started.elapsed().as_millis(),
                            "status_code": status.as_u16(),
                            "ok": status.is_success(),
                            "request_id": request_id,
                            "retry_after_seconds": retry_after_seconds,
                            "response": response_body,
                            "error_code": ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                                .map(|c| c.as_str()),
                        }),
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
                            status_code: Some(status.as_u16()),
                            duration_ms: started.elapsed().as_millis(),
                            attempt,
                            retry_after_seconds,
                            request_id: request_id.as_deref(),
                            ok: status.is_success(),
                            error_class: None,
                            response_shape: Some(response_shape(&response_body)),
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
                let transport = transport_error_summary(&err);
                if mutating || attempt >= max_attempts {
                    let code = ayx_core::envelope::ErrorCode::Network;
                    let envelope = Envelope::err_coded(
                        code,
                        format!("{} {} failed", surface, operation),
                        json!({
                            "surface": surface,
                            "operation": operation,
                            "method": method_name,
                            "url": url,
                            "attempts": attempt,
                            "elapsed_ms": started.elapsed().as_millis(),
                            "ok": false,
                            "status_code": last_status.map(|s| s.as_u16()),
                            "retry_after_seconds": retry_after_seconds,
                            "error": transport["error"].clone(),
                            "error_kind": transport["error_kind"].clone(),
                            "error_hints": transport["error_hints"].clone(),
                            "error_chain": transport["error_chain"].clone(),
                            "request_url": transport["request_url"].clone(),
                            "response": Value::Null,
                            "error_code": code.as_str(),
                        }),
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
                            response_shape: Some("null"),
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
    let response_body = parse_response_text(&content_type, &text);
    let envelope = one_http_envelope(
        status,
        format!(
            "flow package {} {}",
            if dry_run { "dry-run" } else { "import" },
            if status.is_success() { "ok" } else { "failed" }
        ),
        json!({
            "surface": "flow",
            "operation": if dry_run { "import-dry-run" } else { "import" },
            "method": "POST",
            "url": url,
            "status_code": status.as_u16(),
            "ok": status.is_success(),
            "request_id": request_id,
            "response": response_body,
            "error_code": ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                .map(|c| c.as_str()),
        }),
    );
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
            response_shape: Some(response_shape(&response_body)),
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
            let response_body = parse_response_text(&content_type, &text);
            let envelope = Envelope::ok_with_data(
                "flow package export dry-run ok",
                json!({
                    "surface": "flow",
                    "operation": "export-dry-run",
                    "flow_id": flow_id,
                    "status_code": status.as_u16(),
                    "ok": true,
                    "request_id": request_id,
                    "response": response_body,
                }),
            );
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
                    response_shape: Some(response_shape(&response_body)),
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
            json!({
                "surface": "flow",
                "operation": "export",
                "flow_id": flow_id,
                "status_code": status.as_u16(),
                "ok": true,
                "request_id": request_id,
                "path": output_path.display().to_string(),
                "bytes": bytes.len(),
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
    let response_body = parse_response_text(&content_type, &text);
    let envelope = one_http_envelope(
        status,
        format!(
            "flow package {} failed",
            if dry_run { "dry-run" } else { "export" }
        ),
        json!({
            "surface": "flow",
            "operation": if dry_run { "export-dry-run" } else { "export" },
            "flow_id": flow_id,
            "status_code": status.as_u16(),
            "ok": false,
            "request_id": request_id,
            "response": response_body,
            "error_code": ayx_core::envelope::ErrorCode::from_http_status(status.as_u16())
                .map(|c| c.as_str()),
        }),
    );
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
            response_shape: Some(response_shape(&response_body)),
            mutating: false,
            dry_run,
        },
    );
    Ok(envelope)
}

fn build_client() -> Result<Client> {
    let timeout = Duration::from_secs(60);
    let mut builder = Client::builder().timeout(timeout);
    if no_verify_tls() {
        // Lab/dev only — operator opted in explicitly via --no-verify-tls.
        builder = builder.danger_accept_invalid_certs(true);
        eprintln!(
            "[warn] TLS certificate verification disabled for One API transport (--no-verify-tls). Never use this in production."
        );
    }
    builder
        .build()
        .context("failed to build one api HTTP client")
}

fn resolve_one_access_token(config: &Config, client: &Client) -> Result<String> {
    if let Some(access_token) = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.resolved_access_token())
    {
        return Ok(access_token.to_string());
    }

    refresh_one_access_token(config, client)
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
        bail!(
            "workspace preflight failed: GET /v4/workspaces/current returned {} ({}). Refusing to send mutating request to {mutation_url}. Verify token or unset alteryx_one.expected_workspace_id.",
            status.as_u16(),
            text.chars().take(200).collect::<String>()
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

    let response = client
        .post(&token_endpoint_url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .send()
        .with_context(|| format!("refresh token request to '{}' failed", token_endpoint_url))?
        .error_for_status()
        .context("refresh token request returned error status")?;

    let token_json: Value = response
        .json()
        .context("failed to parse refresh token response")?;
    format_refresh_token_response(&token_json)
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

fn parse_response_text(content_type: &str, text: &str) -> Value {
    let lower = content_type.to_lowercase();
    if lower.contains("application/json") || lower.contains("+json") {
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    } else if text.trim_start().starts_with('<') {
        json!({ "raw": text, "content_type": content_type, "response_kind": "html" })
    } else if text.trim().is_empty() {
        Value::Null
    } else {
        json!({ "raw": text })
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
/// Precedence: `alteryx_one.base_url` in the profile, then the `AYX_ONE_API_BASE_URL`
/// environment variable, then the `us1` default. Region-specific examples:
/// `https://us1.alteryxcloud.com`, `https://eu1.alteryxcloud.com`,
/// `https://ap1.alteryxcloud.com`.
pub fn resolve_one_base_url(config: &Config) -> String {
    if let Some(one) = config.alteryx_one.as_ref() {
        if let Some(url) = one.normalized_base_url() {
            let trimmed = url.trim().trim_end_matches('/').to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
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
    use ayx_core::profile::{AlteryxOneProfile, Config, WorkspaceCredential};
    use httpmock::prelude::*;
    use serde_yaml::from_str;
    use std::collections::BTreeMap;

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
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: None,
            access_token_ref: None,
            refresh_token: Some("refresh-123".to_string()),
            refresh_token_ref: None,
            workspace_credentials: Default::default(),
            expected_workspace_id: None,
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("refresh succeeds");

        mock.assert();
        assert_eq!(token, "Bearer fresh");
    }

    #[test]
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
                token_endpoint_url: Some(format!("{}/workspace-token", server.base_url())),
            },
        );
        config.alteryx_one = Some(AlteryxOneProfile {
            account_email: "tester@example.com".to_string(),
            base_url: Some(server.base_url()),
            oauth_client_id: Some("legacy-client".to_string()),
            token_endpoint_url: Some(format!("{}/as", server.base_url())),
            access_token: Some("legacy-stale".to_string()),
            access_token_ref: None,
            refresh_token: Some("legacy-refresh".to_string()),
            refresh_token_ref: None,
            workspace_credentials,
            expected_workspace_id: Some("ws-123".to_string()),
        });

        let client = reqwest::blocking::Client::new();
        let token = refresh_one_access_token(&config, &client).expect("workspace refresh succeeds");

        mock.assert();
        assert_eq!(token, "Bearer fresh");
    }
}
