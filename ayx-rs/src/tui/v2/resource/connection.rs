//! Connection ResourceKind — maps `/v4/connections` items to rows.
//! Fields per ayx-one-api/src/types.rs:153-168 (ConnectionSummary).
use super::{
    Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, date_only, items_array,
    str_field,
};
use serde_json::Value;

pub struct ConnectionKind;

const COLS: &[Column] = &[
    Column {
        title: "NAME",
        width: 36,
    },
    Column {
        title: "CONNECTOR",
        width: 16,
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

impl ResourceKind for ConnectionKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let name = str_field(item, &["name"])
            .unwrap_or("(unnamed)")
            .to_string();
        let connector = str_field(item, &["connectorId", "connector_id"])
            .unwrap_or_default()
            .to_string();
        let updated = str_field(item, &["updatedAt", "updated_at"])
            .map(date_only)
            .unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(name),
                Cell::plain(connector),
                Cell::plain(updated),
                Cell::plain(id),
            ],
        }
    }

    fn list_endpoint(&self) -> ListEndpoint {
        ListEndpoint {
            surface: "connection",
            operation: "tui-connection-list",
            path: "/v4/connections",
        }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint {
            surface: "connection",
            operation: "tui-connection-detail",
            path: "/v4/connections/{id}",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::ResourceKind;
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(ConnectionKind.columns().len(), 4);
        assert_eq!(ConnectionKind.columns()[0].title, "NAME");
        assert_eq!(ConnectionKind.columns()[1].title, "CONNECTOR");
    }

    #[test]
    fn row_maps_name_connector_updated_id() {
        let item = json!({
            "id": "cn_1", "name": "Prod Snowflake",
            "connectorId": "snowflake", "updatedAt": "2026-06-18T08:30:00Z"
        });
        let row = ConnectionKind.row(&item);
        assert_eq!(row.id, "cn_1");
        assert_eq!(row.cells[0].text, "Prod Snowflake");
        assert_eq!(row.cells[1].text, "snowflake");
        assert_eq!(row.cells[2].text, "2026-06-18");
        assert_eq!(row.cells[3].text, "cn_1");
    }

    #[test]
    fn row_handles_missing_fields() {
        let row = ConnectionKind.row(&json!({ "id": "cn_x" }));
        assert_eq!(row.cells[0].text, "(unnamed)");
        assert_eq!(row.cells[1].text, "");
        assert_eq!(row.id, "cn_x");
    }

    #[test]
    fn list_endpoint_is_v4_connections() {
        assert_eq!(ConnectionKind.list_endpoint().path, "/v4/connections");
        assert_eq!(
            ConnectionKind.detail_endpoint().unwrap().path,
            "/v4/connections/{id}"
        );
    }
}
