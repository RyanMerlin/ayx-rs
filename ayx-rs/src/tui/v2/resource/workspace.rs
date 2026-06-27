//! Workspace ResourceKind. Real impl lands in Task 6.
//! No per-id detail endpoint is wired (the only proven endpoint is
//! `/v4/workspaces/current`); workspace detail is the switcher's job in a later
//! phase, so `detail_endpoint` is `None`.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct WorkspaceKind;
const COLS: &[Column] = &[Column {
    title: "NAME",
    width: 30,
}];

impl ResourceKind for WorkspaceKind {
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
            operation: "tui-workspace-list",
            path: "/v4/workspaces",
        }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        None
    }
}
