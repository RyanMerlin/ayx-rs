//! Flow ResourceKind. Real impl lands in Task 2.
use super::{Column, ListEndpoint, ResourceKind, Row};
use serde_json::Value;

pub struct FlowKind;

impl ResourceKind for FlowKind {
    fn columns(&self) -> &'static [Column] {
        &[]
    }
    fn extract_items(&self, _payload: &Value) -> Vec<Value> {
        Vec::new()
    }
    fn row(&self, _item: &Value) -> Row {
        Row {
            id: String::new(),
            cells: Vec::new(),
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
