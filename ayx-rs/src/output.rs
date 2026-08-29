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
    match data.as_object() {
        Some(object) => {
            // A descriptor that declares no fields projects every key the object
            // has.  Nested values are still summarized by `scalar_projection`, so
            // the payload stays bounded.  The former `priority_fields()` fallback
            // was an allowlist of key *names*, which meant any command whose keys
            // were not on it (whoami, profile, doctor, secret) emitted `{}` and
            // reported its entire payload under `omitted_fields`.
            let fields: Vec<&str> = if descriptor_fields.is_empty() {
                object.keys().map(String::as_str).collect()
            } else {
                descriptor_fields.to_vec()
            };
            json!({
                "kind": kind,
                "fields": project_object(Some(object), &fields),
                "omitted_fields": omitted_fields(Some(object), &fields),
            })
        }
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
    if is_metadata_key(key) {
        return false;
    }
    let key = key.to_ascii_lowercase().replace(['-', '_'], "");
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

/// Keys that *describe* a credential rather than carry one.
///
/// The sensitive-key check is a substring match, so without this exception it
/// also swallows every field whose name merely mentions a credential:
/// `next_page_token` (part of the documented compact-list contract),
/// `access_token_present`, `refresh_token_source`, `inline_secret_risks`
/// (a list of field *names*), `secret_posture`, `token_type`, and so on.
/// Those carry no secret material, and redacting them makes the diagnostics
/// that exist to report credential posture unable to report it.
///
/// This is a rule rather than an enumeration so a newly added `*_source` or
/// `*_present` field does not silently regress.
fn is_metadata_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    const EXACT: &[&str] = &["next_page_token", "secret_values_returned"];
    const SUFFIXES: &[&str] = &[
        "_present",
        "_source",
        "_fields",
        "_risks",
        "_posture",
        "_length",
        "_type",
        "_claims",
        "_endpoint",
        "_endpoint_url",
        "_refs",
        "_env",
    ];
    EXACT.contains(&key.as_str())
        || key.starts_with("has_")
        || SUFFIXES.iter().any(|suffix| key.ends_with(suffix))
}

fn is_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("bearer ")
        // An `inline:` reference *is* the plaintext. This is a value-level
        // backstop for the key-level exemption in `is_metadata_key`: a field
        // named `*_refs`/`*_source` disables key matching at any depth, and
        // `docs/workspace-output-standard.md` actively tells contributors to
        // name new metadata fields into that shape. Today every such emitter
        // prints reference *names*, so nothing leaks — this keeps that true
        // when one of them starts printing reference *values*.
        || lower.starts_with("inline:")
        || has_populated_assignment(&lower, "password=")
        || has_populated_assignment(&lower, "pwd=")
}

/// True when `needle` appears with an actual value after the `=`.
///
/// The point of the value-based check is catching a secret embedded in a URL or
/// connection string (`...?password=hunter2`). An empty assignment carries no
/// secret, and treating it as one redacts things that exist to be read: the
/// `ayx secret env-template` output is a list of `NAME=` lines, one of which
/// ends in `_PASSWORD`, so the whole non-secret template was replaced with
/// `[REDACTED]`.
///
/// A quote is the standard *opening* delimiter of a populated value in ODBC,
/// JDBC and `.env` syntax (`Password="hunter2"`), so it must not be read as
/// "no value follows" — doing so silently stopped redacting the quoted form,
/// which is the common one. The assignment is empty only when the quote closes
/// immediately, or when the line ends, or when a field delimiter follows.
fn has_populated_assignment(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(index, _)| {
        // Padding between `=` and the value is legal in connection strings, but
        // a line break ends the assignment — so skip blanks, not newlines.
        let rest = haystack[index + needle.len()..].trim_start_matches([' ', '\t']);
        // Take the assignment's own value: everything up to the next field
        // delimiter or end of line. Peeking a single character past the opening
        // quote was not enough — `password=""hunter2"` reads as an immediately
        // closed quote and slipped through with the secret still in the string.
        let value = rest
            .split(['&', ';', ',', '\n', '\r'])
            .next()
            .unwrap_or_default();
        !value.trim().trim_matches(['"', '\'']).is_empty()
    })
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

    /// Keys that merely *describe* a credential must survive redaction.
    ///
    /// The sensitive-key check is a substring match, so before the metadata
    /// exception every one of these was replaced with `[REDACTED]` in every
    /// output mode. `next_page_token` is part of the documented compact-list
    /// contract, and `inline_secret_risks` is the payload of the diagnostic
    /// whose entire job is reporting inline-secret posture.
    #[test]
    fn metadata_keys_describing_credentials_are_not_redacted() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({
                "next_page_token": "cursor-42",
                "access_token_present": true,
                "access_token_source": "keyring",
                "refresh_token_source": "env",
                "has_access_token": true,
                "token_type": "Bearer",
                "token_length": 128,
                "secret_posture": "secure",
                "secret_refs": ["keyring:acct"],
                "inline_secret_fields": ["client_secret"],
                "inline_secret_risks": ["inline secret detected for client_secret"],
                "curator_api_secret_present": false,
                "token_endpoint_url": "https://example.invalid/as/token",
                "password_env": "AYX_ONE_WS_PASSWORD",
                "secret_values_returned": false,
            }),
        );
        let clean = redacted_envelope(&env);
        for key in [
            "next_page_token",
            "access_token_present",
            "access_token_source",
            "refresh_token_source",
            "has_access_token",
            "token_type",
            "token_length",
            "secret_posture",
            "secret_refs",
            "inline_secret_fields",
            "inline_secret_risks",
            "curator_api_secret_present",
            "token_endpoint_url",
            "password_env",
            "secret_values_returned",
        ] {
            assert_ne!(
                clean.data[key], "[REDACTED]",
                "{key} carries no secret material and must not be redacted"
            );
        }
    }

    /// The metadata exception must not widen into the real credential fields.
    #[test]
    fn credential_bearing_keys_are_still_redacted() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({
                "access_token": "real-token",
                "refresh_token": "real-refresh",
                "client_secret": "real-secret",
                "sp_client_secret": "real-sp-secret",
                "workspace_password": "real-password",
                "api_key": "real-key",
                "cookie": "session=abc",
                "connection_string": "Server=x;Password=y",
            }),
        );
        let clean = redacted_envelope(&env);
        for key in [
            "access_token",
            "refresh_token",
            "client_secret",
            "sp_client_secret",
            "workspace_password",
            "api_key",
            "cookie",
            "connection_string",
        ] {
            assert_eq!(clean.data[key], "[REDACTED]", "{key} must stay redacted");
        }
    }

    /// Quoting is the norm for ODBC/JDBC connection strings and `.env` values,
    /// and the value-based check is the only guard for a secret embedded in a
    /// free-text string under a key the name-matcher does not flag (a driver
    /// error echoing the connection string it failed on, for instance). Reading
    /// the opening quote as "no value follows" silently stopped redacting the
    /// most common form.
    #[test]
    fn quoted_password_assignments_are_still_redacted() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({
                "double": "Server=db;User Id=sa;Password=\"hunter2\";",
                "single": "Server=db;Password='hunter2';",
                "mongo": "mongodb://sa:x@h/?password=\"s3cr3t\"",
                "padded": "Server=db;Password= hunter2",
                "pwd_quoted": "PWD='hunter2'",
                // Peeking one character past the opening quote read this as an
                // immediately-closed quote and let the secret through.
                "doubled_quote": "Server=db;Password=\"\"hunter2\"",
                // Still empty: the quote closes immediately, or the line ends.
                "empty_quoted": "AYX_ONE_WS_PASSWORD=\"\"",
                "empty_template": "AYX_ONE_WS_PASSWORD=\nAYX_SERVER_API_SECRET=",
            }),
        );
        let clean = redacted_envelope(&env);
        for key in [
            "double",
            "single",
            "mongo",
            "padded",
            "pwd_quoted",
            "doubled_quote",
        ] {
            assert_eq!(
                clean.data[key], "[REDACTED]",
                "{key} carries a populated password and must be redacted"
            );
        }
        assert_ne!(
            clean.data["empty_quoted"], "[REDACTED]",
            "an immediately-closed quote carries no secret"
        );
        assert_ne!(
            clean.data["empty_template"], "[REDACTED]",
            "a template of empty assignments carries no secret"
        );
    }

    /// `is_metadata_key` exempts any `*_refs`/`*_source`-shaped key from the
    /// name-based check, at any nesting depth. No emitter prints a reference
    /// *value* today, so nothing leaks — this pins the value-level backstop so
    /// that stays true if one ever does.
    #[test]
    fn inline_references_are_redacted_even_under_an_exempt_key() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({
                "secret_refs": ["inline:hunter2", "keyring:acct", "env:AYX_TOKEN"],
                "client_secret_source": "inline:hunter2",
            }),
        );
        let clean = redacted_envelope(&env);
        assert_eq!(clean.data["secret_refs"][0], "[REDACTED]");
        assert_eq!(
            clean.data["secret_refs"][1], "keyring:acct",
            "a keyring reference names an account and carries no secret"
        );
        assert_eq!(
            clean.data["secret_refs"][2], "env:AYX_TOKEN",
            "an env reference names a variable and carries no secret"
        );
        assert_eq!(clean.data["client_secret_source"], "[REDACTED]");
    }

    /// A descriptor with no declared fields previously projected against a
    /// hardcoded name allowlist, so any command whose keys were not on it
    /// emitted `{}` and reported its whole payload as omitted.
    #[test]
    fn object_view_without_descriptor_fields_projects_every_key() {
        let value = compact_data(
            &json!({
                "active_profile": "envtest",
                "account_email": "user@example.invalid",
                "config_home": "/tmp/ayx",
                "resolution": {"selected_profile": "envtest"},
            }),
            ViewKind::Detail,
            &[],
            20,
            false,
        );
        assert_eq!(value["omitted_fields"].as_array().unwrap().len(), 0);
        assert_eq!(value["fields"]["active_profile"], "envtest");
        assert_eq!(value["fields"]["account_email"], "user@example.invalid");
        assert_eq!(value["fields"]["config_home"], "/tmp/ayx");
        // Nested values stay summarized so the compact view remains bounded.
        assert_eq!(
            value["fields"]["resolution"],
            "1 field(s); use --output json-full for details"
        );
    }

    /// An empty `NAME=` assignment carries no secret. `ayx secret env-template`
    /// emits exactly that, including one slot ending in `_PASSWORD`, and the
    /// whole non-secret template was being replaced with `[REDACTED]`.
    #[test]
    fn empty_assignments_are_not_treated_as_secret_values() {
        let env = Envelope::ok_with_data(
            "ok",
            json!({
                "content": "AYX_ONE_WS_PASSWORD=\nAYX_SERVER_API_SECRET=\nAYX_ONE_CLIENT_SECRET=",
                "populated": "Server=x;Password=hunter2",
                "query": "https://example.invalid/?password=hunter2",
                "bearer": "Bearer abc123",
            }),
        );
        let clean = redacted_envelope(&env);
        assert_ne!(
            clean.data["content"], "[REDACTED]",
            "a template of empty assignments carries no secret"
        );
        assert!(
            clean.data["content"]
                .as_str()
                .unwrap()
                .contains("AYX_ONE_WS_PASSWORD=")
        );
        assert_eq!(clean.data["populated"], "[REDACTED]");
        assert_eq!(clean.data["query"], "[REDACTED]");
        assert_eq!(clean.data["bearer"], "[REDACTED]");
    }

    /// An explicit field list still narrows the view.
    #[test]
    fn descriptor_fields_still_restrict_the_projection() {
        let value = compact_data(
            &json!({"status": "ok", "noise": 1, "more_noise": 2}),
            ViewKind::Result,
            &["status"],
            20,
            false,
        );
        assert_eq!(value["fields"]["status"], "ok");
        assert_eq!(value["fields"].as_object().unwrap().len(), 1);
        assert_eq!(value["omitted_fields"].as_array().unwrap().len(), 2);
    }
}
