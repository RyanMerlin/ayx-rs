//! Flow ResourceKind - maps `/v4/flows` list items to display rows.
use super::{Cell, Column, ListEndpoint, ResourceKind, Row};
use serde_json::Value;

pub struct FlowKind;

const FLOW_COLUMNS: &[Column] = &[
    Column {
        title: "NAME",
        width: 40,
    },
    Column {
        title: "UPDATED",
        width: 12,
    },
    Column {
        title: "ID",
        width: 24,
    },
];

/// One API list payloads wrap items under one of these keys depending on the
/// endpoint/version. Try them in order (same heuristic the legacy browser uses).
fn items_array(payload: &Value) -> Vec<Value> {
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

fn str_field<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| item.get(*k).and_then(Value::as_str))
}

/// `2026-06-20T10:00:00Z` -> `2026-06-20`; passthrough if not a timestamp.
fn date_only(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}

impl ResourceKind for FlowKind {
    fn columns(&self) -> &'static [Column] {
        FLOW_COLUMNS
    }

    fn extract_items(&self, _payload: &Value) -> Vec<Value> {
        items_array(_payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id", "flowId"])
            .unwrap_or_default()
            .to_string();
        let name = str_field(item, &["name", "displayName"])
            .unwrap_or("(unnamed)")
            .to_string();
        let updated = str_field(item, &["updatedAt", "updated_at", "modifiedAt"])
            .map(date_only)
            .unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![Cell::plain(name), Cell::plain(updated), Cell::plain(id)],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint {
            surface: "flow",
            operation: "tui-flow-list",
            path: "/v4/flows",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_items_reads_data_array() {
        let payload = json!({
            "data": [
                { "id": "fl_1", "name": "ETL Pipeline", "updatedAt": "2026-06-20T10:00:00Z" },
                { "id": "fl_2", "name": "Sales Rollup", "updatedAt": "2026-06-19T09:00:00Z" }
            ]
        });
        let items = FlowKind.extract_items(&payload);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn row_maps_name_updated_id() {
        let item = json!({
            "id": "fl_1", "name": "ETL Pipeline", "updatedAt": "2026-06-20T10:00:00Z"
        });
        let row = FlowKind.row(&item);
        assert_eq!(row.id, "fl_1");
        assert_eq!(row.cells[0].text, "ETL Pipeline");
        assert_eq!(row.cells[1].text, "2026-06-20"); // date only
        assert_eq!(row.cells[2].text, "fl_1");
    }

    #[test]
    fn row_handles_missing_name() {
        let item = json!({ "id": "fl_x" });
        let row = FlowKind.row(&item);
        assert_eq!(row.cells[0].text, "(unnamed)");
        assert_eq!(row.id, "fl_x");
    }

    #[test]
    fn columns_are_three() {
        assert_eq!(FlowKind.columns().len(), 3);
        assert_eq!(FlowKind.columns()[0].title, "NAME");
    }
}
