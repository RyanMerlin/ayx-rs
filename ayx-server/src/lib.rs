pub mod logs;
pub mod mongo;
pub mod sqlserver;
pub mod upgrade;
pub mod util;

use std::{collections::HashMap, fs, path::Path, time::Duration};

use anyhow::{Context, Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_core::observability::{ApiEvent, record_api_event, response_shape};
use ayx_core::profile::{ObservabilityProfile, ServerProfile};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};
use url::form_urlencoded;

pub fn import_swagger(
    profile: &ServerProfile,
    observability: Option<&ObservabilityProfile>,
    url: &str,
    cache_dir: &Path,
    cache_name: &str,
) -> Result<Envelope> {
    let started = std::time::Instant::now();
    let client = build_client(profile.verify_tls())?;
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .with_context(|| format!("failed to download Swagger from '{}'", url))?
        .error_for_status()
        .context("swagger download returned error status")?;

    let swagger: Value = response
        .json()
        .context("failed to deserialize Swagger JSON")?;

    fs::create_dir_all(cache_dir).with_context(|| {
        format!(
            "failed to create swagger cache directory '{}'",
            cache_dir.display()
        )
    })?;
    let cache_path = cache_dir.join(cache_name);
    fs::write(&cache_path, serde_json::to_vec_pretty(&swagger)?).with_context(|| {
        format!(
            "failed to write swagger cache to '{}'",
            cache_path.display()
        )
    })?;

    let path_count = swagger
        .get("paths")
        .and_then(Value::as_object)
        .map(|map| map.len())
        .unwrap_or(0);
    let response_shape_name = "object";
    let envelope = Envelope::ok_with_data(
        "swagger imported",
        json!({
            "ok": true,
            "url": url,
            "cached_to": cache_path.display().to_string(),
            "path_count": path_count,
        }),
    );
    let _ = record_api_event(
        observability,
        ApiEvent {
            product: "server",
            surface: "api",
            operation: "import-swagger",
            method: "GET",
            endpoint_template: url,
            resolved_url: url,
            status_code: Some(200),
            duration_ms: started.elapsed().as_millis(),
            attempt: 1,
            retry_after_seconds: None,
            request_id: None,
            ok: true,
            error_class: None,
            response_shape: Some(response_shape_name),
            mutating: false,
            dry_run: false,
        },
    );

    Ok(envelope)
}

pub fn call_operation(
    profile: &ServerProfile,
    observability: Option<&ObservabilityProfile>,
    operation_id: &str,
    params: &HashMap<String, String>,
    body: Option<Value>,
    swagger_path: &Path,
) -> Result<Envelope> {
    let started = std::time::Instant::now();
    let spec = read_json(swagger_path)?;
    let (method, raw_path, operation) = find_operation(&spec, operation_id)?;
    let (resolved_path, query) = resolve_parameters(raw_path, operation.get("parameters"), params)?;
    let base_url = profile.webapi_url.trim_end_matches('/');
    let url = build_url(base_url, &resolved_path, &query);
    let client = build_client(profile.verify_tls())?;
    let token = fetch_token(profile, &client)?;

    let mut request_builder = client.request(method.clone(), &url);
    request_builder = request_builder
        .header(AUTHORIZATION, token)
        .header(ACCEPT, "application/json");

    if let Some(payload) = body {
        request_builder = request_builder
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
    }

    let response = request_builder
        .send()
        .with_context(|| format!("failed to call Server API operation '{}'", operation_id))?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|val| val.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().unwrap_or_else(|_| "".to_string());
    let response_body = parse_response_text(&content_type, &text);
    let envelope = Envelope::ok_with_data(
        "server API call executed",
        json!({
            "operation_id": operation_id,
            "url": url,
            "status_code": status.as_u16(),
            "ok": status.is_success(),
            "response": response_body,
        }),
    );
    let _ = record_api_event(
        observability,
        ApiEvent {
            product: "server",
            surface: "api",
            operation: operation_id,
            method: method.as_str(),
            endpoint_template: &resolved_path,
            resolved_url: &url,
            status_code: Some(status.as_u16()),
            duration_ms: started.elapsed().as_millis(),
            attempt: 1,
            retry_after_seconds: None,
            request_id: None,
            ok: status.is_success(),
            error_class: None,
            response_shape: Some(response_shape(&response_body)),
            mutating: !matches!(method, Method::GET),
            dry_run: false,
        },
    );
    Ok(envelope)
}

pub fn diagnose_api(
    profile: &ServerProfile,
    observability: Option<&ObservabilityProfile>,
) -> Result<Envelope> {
    let started = std::time::Instant::now();
    let client = build_client(profile.verify_tls())?;
    let token_url = format!(
        "{}/webapi/oauth2/token",
        profile.webapi_url.trim_end_matches('/')
    );
    let token_result = client
        .post(&token_url)
        .basic_auth(&profile.curator_api_key, Some(&profile.curator_api_secret))
        .form(&[("grant_type", "client_credentials"), ("scope", "admin")])
        .send()
        .and_then(|resp| resp.error_for_status())
        .map(|resp| resp.status().as_u16());

    let envelope = Envelope::ok_with_data(
        "server API diagnostics generated",
        json!({
            "base_url": profile.webapi_url,
            "verify_tls": profile.verify_tls(),
            "token_url": token_url,
            "token_endpoint": match token_result {
                Ok(status) => json!({ "ok": true, "status_code": status }),
                Err(ref err) => json!({ "ok": false, "error": err.to_string() }),
            },
            "recommendations": [
                "Use server api import-swagger to cache the current Swagger spec",
                "Use server api call to validate a known operationId",
                "If token acquisition fails, verify client_id/client_secret and base_url"
            ]
        }),
    );
    let _ = record_api_event(
        observability,
        ApiEvent {
            product: "server",
            surface: "api",
            operation: "diagnose",
            method: "POST",
            endpoint_template: &token_url,
            resolved_url: &token_url,
            status_code: token_result.as_ref().ok().copied(),
            duration_ms: started.elapsed().as_millis(),
            attempt: 1,
            retry_after_seconds: None,
            request_id: None,
            ok: token_result.is_ok(),
            error_class: token_result.as_ref().err().map(|_| "auth"),
            response_shape: Some("object"),
            mutating: false,
            dry_run: false,
        },
    );
    Ok(envelope)
}

fn build_client(verify_tls: bool) -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .danger_accept_invalid_certs(!verify_tls)
        .build()
        .context("failed to build server HTTP client")
}

fn fetch_token(profile: &ServerProfile, client: &Client) -> Result<String> {
    let token_url = format!(
        "{}/webapi/oauth2/token",
        profile.webapi_url.trim_end_matches('/')
    );
    let response = client
        .post(&token_url)
        .basic_auth(&profile.curator_api_key, Some(&profile.curator_api_secret))
        .form(&[("grant_type", "client_credentials"), ("scope", "admin")])
        .send()
        .with_context(|| format!("token request to '{}' failed", token_url))?
        .error_for_status()
        .context("token request returned error status")?;

    let token_json: Value = response.json().context("failed to parse token response")?;
    let token_type = token_json
        .get("token_type")
        .and_then(Value::as_str)
        .unwrap_or("Bearer");
    let access_token = token_json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("token response missing access_token"))?;

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

fn read_json(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read Swagger file '{}'", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse Swagger JSON at '{}'", path.display()))
}

fn find_operation<'a>(spec: &'a Value, operation_id: &str) -> Result<(Method, String, &'a Value)> {
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("swagger document has no paths object"))?;

    for (path, methods) in paths {
        if let Value::Object(map) = methods {
            for (method, op) in map {
                if op
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map(|id| id == operation_id)
                    .unwrap_or(false)
                {
                    let parsed_method = Method::from_bytes(method.as_bytes())
                        .map_err(|_| anyhow!("unsupported HTTP method '{}'", method))?;
                    return Ok((parsed_method, path.clone(), op));
                }
            }
        }
    }

    Err(anyhow!(
        "operationId '{}' not found in Swagger spec",
        operation_id
    ))
}

fn resolve_parameters(
    raw_path: String,
    parameters: Option<&Value>,
    params: &HashMap<String, String>,
) -> Result<(String, Vec<(String, String)>)> {
    let mut resolved_path = raw_path;
    let mut query_items = Vec::new();

    if let Some(Value::Array(items)) = parameters {
        for param in items {
            let name = match param.get("name").and_then(Value::as_str) {
                Some(name) => name,
                None => continue,
            };
            match param.get("in").and_then(Value::as_str) {
                Some("path") => {
                    if let Some(value) = params.get(name) {
                        resolved_path = resolved_path.replace(&format!("{{{}}}", name), value);
                    } else if param
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        return Err(anyhow!("missing required path parameter '{}'", name));
                    }
                }
                Some("query") => {
                    if let Some(value) = params.get(name) {
                        query_items.push((name.to_string(), value.clone()));
                    }
                }
                _ => {}
            }
        }
    }

    Ok((resolved_path, query_items))
}

fn build_url(base_url: &str, raw_path: &str, query: &[(String, String)]) -> String {
    let mut url = if raw_path.starts_with("/webapi/") {
        format!("{base_url}{raw_path}")
    } else if raw_path.starts_with('/') {
        format!("{base_url}/webapi{raw_path}")
    } else {
        format!("{base_url}/webapi/{raw_path}")
    };

    if !query.is_empty() {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in query {
            serializer.append_pair(key, value);
        }
        url = format!("{url}?{}", serializer.finish());
    }

    url
}
