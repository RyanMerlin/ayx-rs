//! Output-format renderers for envelopes.
//!
//! `text` (default), `json`, `yaml`, and `table`. Lifted out of `main.rs`
//! so the renderers are pure/testable.
//!
//! Text mode used to print just `envelope.message` — a single line that
//! discarded the entire `data` payload. That meant `ayx actions list` in
//! text mode gave "10 action(s)" and nothing else. This module renders
//! known data shapes (`{items: [...]}` lists, single objects, scalar
//! arrays) into something operators can actually read at the terminal.
//!
//! Convention: every renderer returns `String` and never panics. Unknown
//! shapes fall back to `envelope.message`.

use std::env;
use std::io::IsTerminal;

use ayx_core::envelope::Envelope;
use chrono::{DateTime, SecondsFormat};
use clap::builder::styling::{AnsiColor, Color, RgbColor, Style};
use serde_json::Value;

const ALTERYX_BLUE: Color = Color::Rgb(RgbColor(0, 103, 185));

/// Pretty-print an envelope for human reading at a terminal.
///
/// Inspects `envelope.data` and selects:
/// - **Table** when `data.items` is an array of homogeneous objects
///   (auto-detected columns, preferential field ordering, capped at 6).
/// - **Vertical key:value** when data is a single object.
/// - **Newline-joined** when data is a scalar array.
/// - **Fallback** to `envelope.message` for anything else.
pub fn render_text(envelope: &Envelope) -> String {
    if is_doctor_shape(&envelope.data) {
        return format_doctor(&envelope.data, color_enabled());
    }
    let mut out = String::new();
    out.push_str(&envelope.message);
    if !envelope.message.is_empty() && !matches!(envelope.data, Value::Null) {
        out.push('\n');
    }
    out.push_str(&render_data_text(&envelope.data));
    // Trailing notice if there's a pagination token. Keeps the operator
    // honest about whether they're seeing all results.
    if let Some(token) = envelope
        .data
        .get("next_page_token")
        .and_then(|v| v.as_str())
        && !token.is_empty()
    {
        out.push_str("\n(more results available — use --all to fetch all, --max-pages N to cap)");
    }
    out
}

fn color_enabled() -> bool {
    std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none()
}

fn is_doctor_shape(data: &Value) -> bool {
    data.get("checks").is_some_and(Value::is_object)
        && data.get("sequence").is_some_and(Value::is_array)
}

fn paint(text: &str, style: Style, color: bool) -> String {
    if color {
        format!("{}{text}{}", style.render(), style.render_reset())
    } else {
        text.to_string()
    }
}

fn doctor_status_visuals(status: &str) -> (&'static str, Style) {
    match status {
        "ok" => (
            "✔",
            Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Green)))
                .bold(),
        ),
        "warn" => (
            "⚠",
            Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
                .bold(),
        ),
        "fail" => (
            "✘",
            Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Red)))
                .bold(),
        ),
        "skip" => (
            "–",
            Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::BrightBlack)))
                .dimmed(),
        ),
        _ => (
            "?",
            Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::BrightBlack)))
                .dimmed(),
        ),
    }
}

fn doctor_overall(data: &Value) -> String {
    data.get("overall")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_uppercase()
}

fn doctor_fix_applied(data: &Value) -> bool {
    data.get("fix_applied")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn doctor_check<'a>(data: &'a Value, name: &str) -> Option<&'a serde_json::Map<String, Value>> {
    data.get("checks")
        .and_then(Value::as_object)
        .and_then(|checks| checks.get(name))
        .and_then(Value::as_object)
}

fn doctor_sequence(data: &Value) -> Vec<&str> {
    data.get("sequence")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .collect()
}

fn doctor_summary(check: Option<&serde_json::Map<String, Value>>) -> &str {
    check
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn doctor_status(check: Option<&serde_json::Map<String, Value>>) -> &str {
    check
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("skip")
}

fn format_doctor(data: &Value, color: bool) -> String {
    let sequence = doctor_sequence(data);
    let name_width = sequence.iter().map(|name| name.len()).max().unwrap_or(0);
    let header = format!("ayx doctor — {}", doctor_overall(data));
    let mut lines = vec![paint(
        &header,
        Style::new().fg_color(Some(ALTERYX_BLUE)).bold(),
        color,
    )];

    for name in sequence {
        let check = doctor_check(data, name);
        let status = doctor_status(check);
        let summary = doctor_summary(check);
        let (glyph, style) = doctor_status_visuals(status);
        let glyph = paint(glyph, style, color);
        let status = paint(&format!("{status:<4}"), style, color);
        lines.push(format!(
            "  {glyph} {name:<name_width$}   {status}   {summary}"
        ));
    }

    if doctor_fix_applied(data) {
        lines.push("  fixes applied: created missing config dirs/state".to_string());
    }

    lines.join("\n")
}

/// Pretty-print just the data payload. Used by both text and table modes.
fn render_data_text(data: &Value) -> String {
    if data
        .get("unrecognized_collection")
        .is_some_and(|value| value.as_bool() == Some(true))
    {
        return data
            .get("hint")
            .and_then(Value::as_str)
            .unwrap_or(
                "The service returned a collection shape this CLI version does not recognize.",
            )
            .to_string();
    }

    // A response the command declared as several sibling collections, already
    // projected one list per key under `collections`.
    if let Some(collections) = data.get("collections").and_then(Value::as_object) {
        let sections: Vec<String> = collections
            .iter()
            .filter_map(|(key, list)| {
                let items = list.get("items").and_then(Value::as_array)?;
                Some(render_section(&title_case(key), items))
            })
            .collect();
        if !sections.is_empty() {
            return sections.join("\n\n");
        }
    }

    // Catalog uses named collections rather than the usual `items` wrapper.
    // Keep its operator view useful without changing the lossless JSON
    // contract consumed by agents and scripts.
    if data.get("commands").is_some() || data.get("capabilities").is_some() {
        let mut sections = Vec::new();
        for (key, label) in [("commands", "Commands"), ("capabilities", "Capabilities")] {
            if let Some(items) = data.get(key).and_then(Value::as_array) {
                sections.push(render_section(label, items));
            }
        }
        if !sections.is_empty() {
            return sections.join("\n\n");
        }
    }

    // Highest-priority shape: { "items": [ {...}, {...} ] } — pagination wrapper.
    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            return "(no items)".to_string();
        }
        if items.iter().all(|item| item.is_object()) {
            return render_object_array(items);
        }
        return render_scalar_array(items);
    }
    // { "actions": [...] } / { "workflows": [...] } / { "hits": [...] } — same shape, different key.
    for key in [
        "actions",
        "workflows",
        "hits",
        "endpoints",
        "people",
        "flows",
        "plans",
        "connections",
    ] {
        if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
            if arr.is_empty() {
                return format!("(no {key})");
            }
            if arr.iter().all(|item| item.is_object()) {
                return render_object_array(arr);
            }
            return render_scalar_array(arr);
        }
    }

    // Bare array of objects.
    if let Some(arr) = data.as_array() {
        if arr.is_empty() {
            return "(empty)".to_string();
        }
        if arr.iter().all(|item| item.is_object()) {
            return render_object_array(arr);
        }
        return render_scalar_array(arr);
    }

    // Single object → vertical key:value listing.
    if let Some(obj) = data.as_object() {
        if obj.is_empty() {
            return String::new();
        }
        return render_object_fields(obj, 2).join("\n");
    }

    // Scalar / null — nothing to add.
    String::new()
}

/// A titled, counted section: `Label (n)` over a table, a list, or `(none)`.
fn render_section(label: &str, items: &[Value]) -> String {
    let body = if items.is_empty() {
        "(none)".to_string()
    } else if items.iter().all(Value::is_object) {
        render_object_array(items)
    } else {
        render_scalar_array(items)
    };
    format!("{label} ({})\n{body}", items.len())
}

fn title_case(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Render a JSON object as indented terminal fields. JSON output remains the
/// lossless interface; this projection exists so a person does not have to
/// parse a serialized object embedded inside a `key: value` line.
fn render_object_fields(object: &serde_json::Map<String, Value>, indent: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(object.len());
    for (key, value) in object {
        render_human_field(&mut lines, key, value, indent);
    }
    lines
}

fn render_human_field(lines: &mut Vec<String>, key: &str, value: &Value, indent: usize) {
    let prefix = " ".repeat(indent);
    match value {
        Value::Object(object) if object.is_empty() => {
            lines.push(format!("{prefix}{key}: {{}}"));
        }
        Value::Object(object) => {
            lines.push(format!("{prefix}{key}:"));
            lines.extend(render_object_fields(object, indent + 2));
        }
        Value::Array(items) if items.is_empty() => {
            lines.push(format!("{prefix}{key}: []"));
        }
        Value::Array(items)
            if items
                .iter()
                .all(|item| !item.is_object() && !item.is_array()) =>
        {
            lines.push(format!("{prefix}{key}: {}", scalar_or_compact(value)));
        }
        Value::Array(items) => {
            lines.push(format!("{prefix}{key}:"));
            for item in items {
                match item {
                    Value::Object(object) if object.is_empty() => {
                        lines.push(format!("{}- {{}}", " ".repeat(indent + 2)));
                    }
                    Value::Object(object) => {
                        lines.push(format!("{}-", " ".repeat(indent + 2)));
                        lines.extend(render_object_fields(object, indent + 4));
                    }
                    Value::Array(_) => {
                        lines.push(format!(
                            "{}- {}",
                            " ".repeat(indent + 2),
                            scalar_or_compact(item)
                        ));
                    }
                    _ => lines.push(format!(
                        "{}- {}",
                        " ".repeat(indent + 2),
                        scalar_or_compact(item)
                    )),
                }
            }
        }
        _ => lines.push(format!("{prefix}{key}: {}", scalar_or_compact(value))),
    }
}

/// Render an array of objects as a tab-aligned table. Columns are
/// auto-detected from the union of keys, preferring identity-style fields
/// (id, name, title) first, then descriptors, then everything else.
/// Capped at 6 columns so wide objects still fit on a terminal.
pub fn render_object_array(items: &[Value]) -> String {
    if items.is_empty() {
        return String::new();
    }
    // Preferred column ordering — most-useful fields first.
    const PREFERRED: &[&str] = &[
        "id",
        "action_id",
        "workflow_id",
        "name",
        "title",
        "safety",
        "status",
        "score",
        "action_count",
        "step_count",
        "email",
        "tags",
        "summary",
        "method",
        "kind",
        "ok",
    ];
    let mut columns: Vec<String> = Vec::new();
    for &p in PREFERRED {
        if items
            .iter()
            .any(|i| i.as_object().is_some_and(|o| o.contains_key(p)))
            && !columns.iter().any(|c| c == p)
        {
            columns.push(p.to_string());
        }
        if columns.len() >= 6 {
            break;
        }
    }
    // Fill the rest with anything else still seen.
    if columns.len() < 6 {
        for item in items {
            if let Some(obj) = item.as_object() {
                for k in obj.keys() {
                    if !columns.iter().any(|c| c == k) {
                        columns.push(k.clone());
                        if columns.len() >= 6 {
                            break;
                        }
                    }
                }
            }
            if columns.len() >= 6 {
                break;
            }
        }
    }
    if columns.is_empty() {
        return "(no displayable columns)".to_string();
    }

    // Build cell matrix.
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len() + 1);
    rows.push(columns.iter().map(|c| display_column_name(c)).collect());
    for item in items {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        let row: Vec<String> = columns
            .iter()
            .map(|col| {
                obj.get(col)
                    .map(scalar_or_compact)
                    .unwrap_or_else(|| "-".to_string())
            })
            .collect();
        rows.push(row);
    }

    // Column widths.
    let col_count = columns.len();
    let mut widths = vec![0usize; col_count];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell).min(40));
        }
    }

    // Render.
    let mut out = String::new();
    for (ri, row) in rows.iter().enumerate() {
        for (ci, cell) in row.iter().enumerate() {
            let padded = pad_cell(cell, widths[ci]);
            out.push_str(&padded);
            if ci + 1 < col_count {
                out.push_str("  ");
            }
        }
        out.push('\n');
        if ri == 0 {
            // Separator line under the header.
            for (ci, w) in widths.iter().enumerate() {
                out.push_str(&"-".repeat(*w));
                if ci + 1 < col_count {
                    out.push_str("  ");
                }
            }
            out.push('\n');
        }
    }
    // Trim trailing newline so callers can append a footer.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Keep machine-facing field names stable while making a few common governance
/// fields read naturally in the terminal table.
fn display_column_name(column: &str) -> String {
    match column {
        "owner" | "owner_id" => "OWNER".to_string(),
        "last_updated_at" => "LAST UPDATED".to_string(),
        "last_updated_by_id" => "LAST EDITOR".to_string(),
        "workflow_version" => "VERSION".to_string(),
        _ => column.to_uppercase(),
    }
}

fn render_scalar_array(items: &[Value]) -> String {
    items
        .iter()
        .map(scalar_or_compact)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render a JSON scalar as a compact terminal cell. Nested structures are
/// summarized here because tables need one line per cell; vertical detail
/// output expands those structures through `render_human_field` instead.
fn scalar_or_compact(v: &Value) -> String {
    match v {
        Value::Null => "-".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => human_timestamp_or_original(s),
        Value::Array(arr) => {
            // For short arrays of scalars, render comma-separated.
            if arr.iter().all(|v| !v.is_object() && !v.is_array()) {
                arr.iter()
                    .map(scalar_or_compact)
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(object) => format!("{{{} fields}}", object.len()),
    }
}

/// Keep exact timestamp values in JSON, but omit visual noise below a second
/// in human-facing output. A failed parse is deliberately not an error: this
/// renderer must faithfully display arbitrary service strings.
fn human_timestamp_or_original(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|_| value.to_string())
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn pad_cell(s: &str, width: usize) -> String {
    let truncated: String = if display_width(s) > width {
        let mut chars: String = s.chars().take(width.saturating_sub(1)).collect();
        chars.push('…');
        chars
    } else {
        s.to_string()
    };
    let pad = width.saturating_sub(display_width(&truncated));
    format!("{}{}", truncated, " ".repeat(pad))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env_with(message: &str, data: Value) -> Envelope {
        Envelope::ok_with_data(message, data)
    }

    #[test]
    fn empty_data_renders_message_only() {
        let env = Envelope::ok("done");
        let text = render_text(&env);
        assert_eq!(text, "done");
    }

    #[test]
    fn nested_objects_render_as_indented_human_fields() {
        let env = env_with(
            "auth status",
            json!({
                "access_token_claims": {"exp": 1_791_546_160u64, "expired": false},
                "workspace_probe": {
                    "ok": true,
                    "response": {"count": 0, "data": []}
                },
                "recommendations": ["Use one auth status", "Run one workspace current"],
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("  access_token_claims:\n    exp: 1791546160\n    expired: false"));
        assert!(text.contains(
            "  workspace_probe:\n    ok: true\n    response:\n      count: 0\n      data: []"
        ));
        assert!(text.contains("  recommendations: Use one auth status,Run one workspace current"));
        assert!(!text.contains("\"access_token_claims\""));
        assert!(!text.contains("\"workspace_probe\""));
    }

    #[test]
    fn nested_table_cells_are_summarized_not_serialized_json() {
        let env = env_with(
            "one item",
            json!({"items": [{"id": "a", "metadata": {"nested": true}}]}),
        );
        let text = render_text(&env);
        assert!(text.contains("{1 fields}"));
        assert!(!text.contains("{\"nested\":true}"));
    }

    #[test]
    fn human_output_omits_fractional_timestamp_seconds() {
        let env = env_with(
            "status",
            json!({
                "created_at": "2026-09-09T13:14:15.987654Z",
                "updated_at": "2026-09-09T13:14:15.123+02:00",
                "unparseable": "2026-09-09T13:14:15.not-a-time",
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("created_at: 2026-09-09T13:14:15Z"));
        assert!(text.contains("updated_at: 2026-09-09T13:14:15+02:00"));
        assert!(text.contains("unparseable: 2026-09-09T13:14:15.not-a-time"));
        assert!(!text.contains(".987654"));
        assert!(!text.contains(".123+02:00"));
    }

    #[test]
    fn items_array_renders_table_with_header() {
        let env = env_with(
            "2 action(s)",
            json!({
                "items": [
                    {"id": "a.b", "title": "A", "safety": "read_only"},
                    {"id": "c.d", "title": "C", "safety": "mutating"},
                ]
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("ID"));
        assert!(text.contains("TITLE"));
        assert!(text.contains("SAFETY"));
        assert!(text.contains("a.b"));
        assert!(text.contains("read_only"));
        // header separator
        assert!(text.contains("---"));
    }

    #[test]
    fn governance_columns_use_human_friendly_headers() {
        let text = render_object_array(&[json!({
            "id": "workflow-1",
            "owner_id": 42,
            "last_updated_at": "2026-08-27T22:00:00Z",
            "workflow_version": 7,
        })]);
        assert!(text.contains("OWNER"));
        assert!(text.contains("LAST UPDATED"));
        assert!(text.contains("VERSION"));
    }

    #[test]
    fn actions_key_renders_table() {
        let env = env_with(
            "1 action(s)",
            json!({
                "actions": [{"id": "mongo.doctor", "safety": "read_only"}]
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("mongo.doctor"));
        assert!(text.contains("ID"));
    }

    #[test]
    fn catalog_collections_render_as_separate_human_tables() {
        let env = env_with(
            "catalog entries listed",
            json!({
                "commands": [
                    {"name": "one workspace list", "path": "one/workspace/list", "kind": "command"}
                ],
                "capabilities": [
                    {"id": "one.workspace.list", "title": "List workspaces"}
                ]
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("Commands (1)"));
        assert!(text.contains("one workspace list"));
        assert!(text.contains("Capabilities (1)"));
        assert!(text.contains("one.workspace.list"));
        assert!(text.contains("---"));
    }

    #[test]
    fn single_object_renders_key_value_list() {
        let env = env_with(
            "current",
            json!({
                "profile": "prod",
                "account_email": "u@e.com",
                "workspace_id": "ws-123"
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("profile: prod"));
        assert!(text.contains("account_email: u@e.com"));
        assert!(text.contains("workspace_id: ws-123"));
    }

    #[test]
    fn next_page_token_emits_footer() {
        let env = env_with(
            "1 flow(s)",
            json!({
                "items": [{"id": "f1"}],
                "next_page_token": "abc"
            }),
        );
        let text = render_text(&env);
        assert!(text.contains("more results available"));
    }

    #[test]
    fn empty_items_renders_no_items() {
        let env = env_with("0 action(s)", json!({"items": []}));
        let text = render_text(&env);
        assert!(text.contains("no items"));
    }

    #[test]
    fn scalar_array_renders_one_per_line() {
        let env = env_with("names", json!(["alpha", "beta", "gamma"]));
        let text = render_text(&env);
        assert!(text.contains("alpha\nbeta\ngamma"));
    }

    #[test]
    fn long_string_cells_get_truncated_with_ellipsis() {
        let env = env_with(
            "1 item",
            json!({
                "items": [{
                    "id": "x",
                    "title": "a".repeat(80),
                }]
            }),
        );
        let text = render_text(&env);
        assert!(text.contains('…'));
    }

    #[test]
    fn doctor_renderer_is_plain_and_sequence_ordered() {
        let data = json!({
            "sequence": ["config", "auth", "network"],
            "fix_applied": true,
            "overall": "fail",
            "checks": {
                "config": {
                    "status": "ok",
                    "summary": "profile 'default' resolved; no inline secrets",
                },
                "auth": {
                    "status": "skip",
                    "summary": "No Alteryx One or Server auth configured",
                },
                "network": {
                    "status": "fail",
                    "summary": "One workspace probe failed",
                }
            }
        });

        let text = format_doctor(&data, false);
        let config_pos = text.find("config").unwrap();
        let auth_pos = text.find("auth").unwrap();
        let network_pos = text.find("network").unwrap();

        assert!(text.contains("ayx doctor — FAIL"));
        assert!(text.contains("ok"));
        assert!(text.contains("skip"));
        assert!(text.contains("fail"));
        assert!(text.contains("profile 'default' resolved; no inline secrets"));
        assert!(text.contains("No Alteryx One or Server auth configured"));
        assert!(text.contains("One workspace probe failed"));
        assert!(text.contains("fixes applied: created missing config dirs/state"));
        assert!(config_pos < auth_pos);
        assert!(auth_pos < network_pos);
        assert!(!text.contains('\u{1b}'));
    }
}
