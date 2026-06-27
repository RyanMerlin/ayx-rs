//! Person ResourceKind. Real impl lands in Task 5.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct PersonKind;
const COLS: &[Column] = &[Column {
    title: "NAME",
    width: 28,
}];

impl ResourceKind for PersonKind {
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
            surface: "platform",
            operation: "tui-person-list",
            path: "/v4/people",
        }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint {
            surface: "platform",
            operation: "tui-person-detail",
            path: "/v4/people/{id}",
        })
    }
}
