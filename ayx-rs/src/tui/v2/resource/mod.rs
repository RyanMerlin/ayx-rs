//! Resource model: the k9s engine. Each browsable asset implements
//! `ResourceKind`, so the list/detail views and effect executor are written
//! once and work for every asset. Phase 0 ships `Kind::Flow` only.
use serde_json::Value;

pub mod connection;
pub mod flow;
pub mod job;
pub mod person;
pub mod workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flow,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flows",
        }
    }

    pub fn all() -> &'static [Kind] {
        &[Kind::Flow]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Ok,
    Warn,
    Danger,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub tone: StatusTone,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: StatusTone::Neutral,
        }
    }
    pub fn toned(text: impl Into<String>, tone: StatusTone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: u16,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy)]
pub struct ListEndpoint {
    pub surface: &'static str,
    pub operation: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DetailEndpoint {
    pub surface: &'static str,
    pub operation: &'static str,
    /// Path template with an `{id}` placeholder, interpolated by the worker via
    /// `&[("id", id)]` (the same convention the CLI detail commands use).
    pub path: &'static str,
}

/// Each browsable asset implements this. Pure data mapping — no I/O, no state.
pub trait ResourceKind: Sync {
    fn columns(&self) -> &'static [Column];
    /// Pull the array of item objects out of a raw list-endpoint payload.
    fn extract_items(&self, payload: &Value) -> Vec<Value>;
    /// Map one item object to a display row (cells + stable id).
    fn row(&self, item: &Value) -> Row;
    fn list_endpoint(&self) -> ListEndpoint;
    /// The single-item endpoint for drill-down, or `None` if the asset has no
    /// per-id detail endpoint (e.g. Workspaces, whose detail is the switcher's
    /// job in a later phase).
    fn detail_endpoint(&self) -> Option<DetailEndpoint>;
}

/// Registry: map a `Kind` to its static trait object. Filled per-asset.
pub fn kind_impl(kind: Kind) -> &'static dyn ResourceKind {
    match kind {
        Kind::Flow => &flow::FlowKind,
    }
}

/// One API list payloads wrap items under one of these keys depending on the
/// endpoint/version. Try them in order, then fall back to a bare array.
pub(crate) fn items_array(payload: &Value) -> Vec<Value> {
    for key in ["data", "items", "results"] {
        if let Some(arr) = payload.get(key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    if let Some(arr) = payload.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// First present string field among `keys`.
pub(crate) fn str_field<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| item.get(*k).and_then(Value::as_str))
}

/// "2026-06-20T10:00:00Z" -> "2026-06-20"; passthrough if not a timestamp.
pub(crate) fn date_only(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_name_and_all() {
        assert_eq!(Kind::Flow.name(), "flows");
        assert!(Kind::all().contains(&Kind::Flow));
    }

    #[test]
    fn cell_constructors_carry_tone() {
        let plain = Cell::plain("hello");
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.tone, StatusTone::Neutral);

        let toned = Cell::toned("failed", StatusTone::Danger);
        assert_eq!(toned.tone, StatusTone::Danger);
    }

    #[test]
    fn items_array_reads_each_wrapper_key() {
        use serde_json::json;
        assert_eq!(items_array(&json!({ "data": [ {"a":1} ] })).len(), 1);
        assert_eq!(
            items_array(&json!({ "items": [ {"a":1}, {"b":2} ] })).len(),
            2
        );
        assert_eq!(items_array(&json!({ "results": [ {"a":1} ] })).len(), 1);
        assert_eq!(items_array(&json!([ {"a":1}, {"b":2} ])).len(), 2);
        assert_eq!(items_array(&json!({ "nope": 1 })).len(), 0);
    }

    #[test]
    fn str_field_first_present_wins() {
        use serde_json::json;
        let v = json!({ "displayName": "Bob", "name": "Robert" });
        assert_eq!(str_field(&v, &["name", "displayName"]), Some("Robert"));
        assert_eq!(str_field(&v, &["missing", "displayName"]), Some("Bob"));
        assert_eq!(str_field(&v, &["missing"]), None);
    }

    #[test]
    fn date_only_strips_time() {
        assert_eq!(date_only("2026-06-20T10:00:00Z"), "2026-06-20");
        assert_eq!(date_only("not-a-date"), "not-a-date");
    }
}
