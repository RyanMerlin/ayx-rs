//! Connection ResourceKind. Real impl lands in Task 3.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct ConnectionKind;
const COLS: &[Column] = &[Column {
    title: "NAME",
    width: 30,
}];

impl ResourceKind for ConnectionKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, _item: &Value) -> Row {
        Row {
            id: String::new(),
            cells: Vec::new(),
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
