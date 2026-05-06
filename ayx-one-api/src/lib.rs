use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ayx_core::envelope::Envelope;
use ayx_core::observability::{record_api_event, response_shape, transport_error_summary, ApiEvent};
use ayx_core::profile::Config;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use reqwest::StatusCode;
use serde_json::{json, Value};
use url::form_urlencoded::Serializer;
const ONE_API_BASE_URL: &str = "https://api.us1.alteryxcloud.com";

mod inventory;

pub use inventory::one_surface_inventory_envelope;

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
    let client = build_client()?;
    let mut access_token = resolve_one_access_token(config, &client)?;

    let mut url = format!("{}{}", normalized_base_url(), endpoint);
    for (key, value) in path_params {
        url = url.replace(&format!("{{{}}}", key), value);
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
                    let envelope = Envelope::ok_with_data(
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
                    let envelope = Envelope::ok_with_data(
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
    let client = build_client()?;
    let access_token = resolve_one_access_token(config, &client)?;
    let started = Instant::now();

    let endpoint = if dry_run {
        "/v4/flows/package/dryRun"
    } else {
        "/v4/flows/package"
    };
    let mut url = format!("{}{}", normalized_base_url(), endpoint);
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
    let envelope = Envelope::ok_with_data(
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
    let mut url = format!("{}{}", normalized_base_url(), endpoint);
    url = url.replace("{id}", flow_id);

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
        fs::write(output_path, &bytes).with_context(|| {
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
    let envelope = Envelope::ok_with_data(
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
    Client::builder()
        .timeout(timeout)
        .build()
        .context("failed to build one api HTTP client")
}

fn resolve_one_access_token(config: &Config, client: &Client) -> Result<String> {
    if let Some(access_token) = config
        .alteryx_one
        .as_ref()
        .and_then(|one| one.access_token.as_ref())
        .filter(|token| !token.trim().is_empty())
    {
        return Ok(access_token.clone());
    }

    refresh_one_access_token(config, client)
}

pub fn refresh_one_access_token(config: &Config, client: &Client) -> Result<String> {
    let one = config
        .alteryx_one
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing alteryx_one section"))?;
    let client_id = one
        .oauth_client_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("alteryx_one.oauth_client_id is required for refresh_token support")
        })?;
    let refresh_token = one
        .refresh_token
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("alteryx_one.refresh_token is required to refresh access tokens")
        })?;
    let token_endpoint_url = one
        .token_endpoint_url
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("alteryx_one.token_endpoint_url is required to refresh access tokens")
        })?;

    let response = client
        .post(token_endpoint_url)
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
    if content_type.to_lowercase().contains("application/json") {
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
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
    let base_ms = 250u64.saturating_mul(multiplier);
    Duration::from_millis(base_ms.min(8_000))
}

fn normalized_base_url() -> &'static str {
    ONE_API_BASE_URL
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        AlteryxOneProfile, Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn sample_config_for_refresh(server_url: String, access_token: Option<String>) -> Config {
        Config {
            profile_name: "test".to_string(),
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: None,
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            alteryx_one: Some(AlteryxOneProfile {
                account_email: "test@example.com".to_string(),
                oauth_client_id: Some("client-123".to_string()),
                token_endpoint_url: Some(server_url),
                access_token,
                access_token_ref: None,
                refresh_token: Some("refresh-abc".to_string()),
                refresh_token_ref: None,
            }),
            observability: None,
            server_api: None,
            api: None,
            server: None,
            sqlserver: None,
            upgrade: None,
        }
    }

    #[test]
    fn refresh_token_path_resolves_access_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("connection should arrive");
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf).expect("request should be readable");
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: application/json\r\n",
                "Connection: close\r\n",
                "\r\n",
                r#"{"token_type":"Bearer","access_token":"fresh-token"}"#
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should write");
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client should build");
        let config = sample_config_for_refresh(
            format!("http://{}/token", addr),
            Some("existing-token".to_string()),
        );
        let token = refresh_one_access_token(&config, &client).expect("refresh should succeed");
        assert_eq!(token, "Bearer fresh-token");
        server.join().expect("server should join");
    }
}
