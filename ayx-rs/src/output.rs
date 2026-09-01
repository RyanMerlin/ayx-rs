//! Central output contracts and renderers.
//!
//! The command handlers return the lossless core `Envelope`; this module is
//! the only place that projects it for terminal or compact JSON output.

use ayx_core::envelope::Envelope;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::render;

pub const COMPACT_SCHEMA_VERSION: &str = "ayx.output.v1";
pub const DEFAULT_OUTPUT_LIMIT: usize = 20;

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorFormat {
    #[default]
    Text,
    Json,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
    JsonFull,
    Yaml,
    Table,
}

impl std::fmt::Display for OutputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::JsonFull => "json-full",
            Self::Yaml => "yaml",
            Self::Table => "table",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    List,
    Detail,
    Result,
    Diagnostic,
    Export,
    Raw,
}

/// Presentation metadata is deliberately separate from `Envelope`: command
/// handlers keep their complete result, while the CLI owns display policy.
#[derive(Debug, Clone, Copy)]
pub struct OutputDescriptor {
    pub command: &'static str,
    pub kind: ViewKind,
    pub fields: &'static [&'static str],
}

impl OutputDescriptor {
    pub const fn new(command: &'static str, kind: ViewKind) -> Self {
        Self {
            command,
            kind,
            fields: &[],
        }
    }

    pub const fn with_fields(mut self, fields: &'static [&'static str]) -> Self {
        self.fields = fields;
        self
    }
}

#[derive(Serialize)]
struct CompactEnvelope {
    schema_version: &'static str,
    command: String,
    ok: bool,
    message: String,
    timestamp_utc: chrono::DateTime<chrono::Utc>,
    error_code: Option<ayx_core::envelope::ErrorCode>,
    data: Value,
}

pub fn render_envelope(
    envelope: &Envelope,
    mode: OutputMode,
    descriptor: OutputDescriptor,
    output_limit: usize,
) -> anyhow::Result<String> {
    let clean = redacted_envelope(envelope);
    match mode {
        OutputMode::Json => Ok(serde_json::to_string_pretty(&compact_envelope(
            &clean,
            descriptor,
            output_limit,
        ))?),
        OutputMode::JsonFull => Ok(serde_json::to_string_pretty(&clean)?),
        OutputMode::Yaml => serde_yaml::to_string(&clean)
            .map_err(|e| anyhow::anyhow!("failed to serialize envelope to yaml: {e}")),
        OutputMode::Text | OutputMode::Table => {
            // Text/table retain their established renderer, but list views use
            // the same bounded projection as compact JSON so --output-limit
            // has one predictable cross-format meaning.
            if clean.ok && descriptor.kind == ViewKind::List {
                let projected = Envelope {
                    ok: clean.ok,
                    message: clean.message.clone(),
                    timestamp_utc: clean.timestamp_utc,
                    data: compact_data(
                        &clean.data,
                        descriptor.kind,
                        descriptor.fields,
                        output_limit,
                        false,
                    ),
                    error_code: clean.error_code,
                };
                Ok(render::render_text(&projected))
            } else {
                Ok(render::render_text(&clean))
            }
        }
    }
}

fn compact_envelope(
    envelope: &Envelope,
    descriptor: OutputDescriptor,
    output_limit: usize,
) -> CompactEnvelope {
    let kind = if envelope.ok {
        descriptor.kind
    } else {
        ViewKind::Raw
    };
    CompactEnvelope {
        schema_version: COMPACT_SCHEMA_VERSION,
        command: descriptor.command.to_string(),
        ok: envelope.ok,
        message: envelope.message.clone(),
        timestamp_utc: envelope.timestamp_utc,
        error_code: envelope.error_code,
        data: compact_data(
            &envelope.data,
            kind,
            descriptor.fields,
            output_limit,
            !envelope.ok,
        ),
    }
}

fn compact_data(
    data: &Value,
    kind: ViewKind,
    descriptor_fields: &[&str],
    limit: usize,
    is_error: bool,
) -> Value {
    if is_error {
        let fields = selected_fields(descriptor_fields);
        return json!({
            "kind": "error",
            "fields": project_object(data.as_object(), &fields),
            "omitted_fields": omitted_fields(data.as_object(), &fields),
        });
    }
    match kind {
        ViewKind::List => compact_list(data, descriptor_fields, limit),
        ViewKind::Detail => compact_object("detail", data, descriptor_fields),
        ViewKind::Result => compact_object("result", data, descriptor_fields),
        ViewKind::Diagnostic => compact_object("diagnostic", data, descriptor_fields),
        ViewKind::Export => compact_object("export", data, descriptor_fields),
        ViewKind::Raw => compact_object("raw", data, descriptor_fields),
    }
}

fn compact_list(data: &Value, descriptor_fields: &[&str], limit: usize) -> Value {
    let (items, source_key) = list_items(data).unwrap_or((&[], "items"));
    let shown = if limit == 0 {
        items.len()
    } else {
        items.len().min(limit)
    };
    let projection = if descriptor_fields.is_empty() {
        list_projection(items)
    } else {
        descriptor_fields.to_vec()
    };
    let projected: Vec<Value> = items[..shown]
        .iter()
        .map(|item| match item.as_object() {
            Some(object) => Value::Object(project_object(Some(object), &projection)),
            None => scalar_projection(item),
        })
        .collect();
    let total = known_total(data);
    let truncated = shown < items.len() || total.is_some_and(|n| n > shown);
    let omitted = data
        .as_object()
        .map(|object| {
            object
                .keys()
                .filter(|key| {
                    key.as_str() != source_key
                        && !matches!(
                            key.as_str(),
                            "count" | "total" | "total_count" | "next_page_token"
                        )
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "kind": "list",
        "items": projected,
        "total_count": total,
        "shown_count": shown,
        "truncated": truncated,
        "next_page_token": data.get("next_page_token").cloned().unwrap_or(Value::Null),
        "omitted_fields": omitted,
    })
}

fn compact_object(kind: &str, data: &Value, descriptor_fields: &[&str]) -> Value {
    let fields = selected_fields(descriptor_fields);
    match data.as_object() {
        Some(object) => json!({
            "kind": kind,
            "fields": project_object(Some(object), &fields),
            "omitted_fields": omitted_fields(Some(object), &fields),
        }),
        None => {
            json!({ "kind": kind, "fields": { "value": scalar_projection(data) }, "omitted_fields": [] })
        }
    }
}

fn selected_fields<'a>(descriptor_fields: &'a [&'a str]) -> Vec<&'a str> {
    if descriptor_fields.is_empty() {
        priority_fields()
    } else {
        descriptor_fields.to_vec()
    }
}

fn priority_fields() -> Vec<&'static str> {
    vec![
        "id",
        "name",
        "title",
        "status",
        "ok",
        "count",
        "total",
        "total_count",
        "dry_run",
        "applied",
        "mutating",
        "safety",
        "profile",
        "workspace_id",
        "audit_artifact",
        "output_file",
        "path",
        "hint",
        "error",
        "error_code",
    ]
}

fn list_projection(items: &[Value]) -> Vec<&'static str> {
    let preferred = priority_fields();
    preferred
        .into_iter()
        .filter(|field| items.iter().any(|item| item.get(*field).is_some()))
        .take(6)
        .collect()
}

fn list_items(data: &Value) -> Option<(&[Value], &str)> {
    if let Some(items) = data.as_array() {
        return Some((items, "items"));
    }
    for key in [
        "items",
        "actions",
        "workflows",
        "hits",
        "endpoints",
        "people",
        "flows",
        "plans",
        "connections",
    ] {
        if let Some(items) = data.get(key).and_then(Value::as_array) {
            return Some((items, key));
        }
    }
    None
}

fn known_total(data: &Value) -> Option<usize> {
    ["total_count", "total", "count"].iter().find_map(|key| {
        data.get(*key)
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
    })
}

fn project_object(object: Option<&Map<String, Value>>, fields: &[&str]) -> Map<String, Value> {
    let mut projected = Map::new();
    if let Some(object) = object {
        for field in fields {
            if let Some(value) = object.get(*field) {
                projected.insert((*field).to_string(), scalar_projection(value));
            }
        }
    }
    projected
}

fn omitted_fields(object: Option<&Map<String, Value>>, fields: &[&str]) -> Vec<String> {
    object
        .map(|object| {
            object
                .keys()
                .filter(|key| !fields.contains(&key.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn scalar_projection(value: &Value) -> Value {
    match value {
        Value::Array(items)
            if items
                .iter()
                .all(|item| !item.is_object() && !item.is_array()) =>
        {
            value.clone()
        }
        Value::Array(items) => Value::String(format!(
            "{} item(s); use --output json-full for details",
            items.len()
        )),
        Value::Object(object) => Value::String(format!(
            "{} field(s); use --output json-full for details",
            object.len()
        )),
        _ => value.clone(),
    }
}

pub fn redacted_envelope(envelope: &Envelope) -> Envelope {
    Envelope {
        ok: envelope.ok,
        message: envelope.message.clone(),
        timestamp_utc: envelope.timestamp_utc,
        data: redact_value(&envelope.data, None),
        error_code: envelope.error_code,
    }
}

fn redact_value(value: &Value, key: Option<&str>) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String("[REDACTED]".to_string());
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(k, v)| (k.clone(), redact_value(v, Some(k))))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| redact_value(v, key)).collect()),
        Value::String(text) if is_sensitive_value(text) => Value::String("[REDACTED]".to_string()),
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    // A `has_`/`has-` prefix marks a boolean presence flag, not a secret
    // itself; check it on the separator-aware form before word boundaries
    // are lost below (e.g. `hashed_password` must not match).
    let has_prefix = lower.starts_with("has_") || lower.starts_with("has-");
    let key = lower.replace(['-', '_'], "");
    // Keys that merely describe a credential (its presence, its claims
    // summary, where it points) are diagnostics, not secrets — `ayx one
    // doctor auth` depends on them surviving redaction. `is_sensitive_value`
    // only catches a narrow set of `k=v`/bearer forms, not arbitrary secret
    // values, so only add suffixes that real callers use for non-secret
    // metadata (counts, booleans, lengths) — never widen speculatively.
    const METADATA_SUFFIXES: &[&str] = &[
        "present", "claims", "url", "endpoint", "count", "fields", "mode", "enabled", "ref",
        "length",
    ];
    if has_prefix || METADATA_SUFFIXES.iter().any(|suffix| key.ends_with(suffix)) {
        return false;
    }
    [
        "authorization",
        "token",
        "password",
        "secret",
        "cookie",
        "connectionstring",
        "clientkey",
        "apikey",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn is_sensitive_value(value: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "password=",
        "pwd=",
        "access_token=",
        "refresh_token=",
        "id_token=",
        "client_secret=",
    ];
    let lower = value.to_ascii_lowercase();
    lower.starts_with("bearer ") || NEEDLES.iter().any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_list_caps_items_and_reports_it() {
        let items: Vec<Value> = (0..21)
            .map(|n| json!({"id": n, "name": format!("n{n}"), "body": {"large": true}}))
            .collect();
        let value = compact_data(&json!({"items": items}), ViewKind::List, &[], 20, false);
        assert_eq!(value["shown_count"], 20);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["items"].as_array().unwrap().len(), 20);
    }

    #[test]
    fn recursively_redacts_common_secrets() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({"authorization": "Bearer abc", "nested": [{"password": "x"}], "url": "x?password=y"}),
        );
        let clean = redacted_envelope(&env);
        assert_eq!(clean.data["authorization"], "[REDACTED]");
        assert_eq!(clean.data["nested"][0]["password"], "[REDACTED]");
        assert_eq!(clean.data["url"], "[REDACTED]");
    }

    #[test]
    fn flags_oauth_bearing_values() {
        for value in [
            "access_token=xyz",
            "https://h/p?access_token=xyz",
            "refresh_token=xyz",
            "ID_TOKEN=xyz",
            "client_secret=xyz",
            "Bearer abc",
            "pwd=abc",
        ] {
            assert!(is_sensitive_value(value), "expected sensitive: {value}");
        }
        for value in ["hello world", "workspace-1", "token_count=5"] {
            assert!(!is_sensitive_value(value), "unexpected sensitive: {value}");
        }
    }

    #[test]
    fn credential_metadata_keys_survive_redaction() {
        for key in [
            "access_token_present",
            "refresh_token_present",
            "access_token_claims",
            "token_endpoint_url",
            "inline_secret_fields",
            "access_token_ref",
            "token_count",
            "has_refresh_token",
            "has-refresh-token",
            "token_length",
        ] {
            assert!(!is_sensitive_key(key), "metadata key redacted: {key}");
        }
        for key in [
            "access_token",
            "client_secret",
            "x-api-key",
            "password",
            "hashed_password",
        ] {
            assert!(is_sensitive_key(key), "secret key not redacted: {key}");
        }
    }

    #[test]
    fn redacts_access_token_bearing_string_values() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({"detail": "GET https://h/p?access_token=abc123 failed"}),
        );
        let clean = redacted_envelope(&env);
        assert_eq!(clean.data["detail"], "[REDACTED]");
    }
}
