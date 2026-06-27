//! Job ResourceKind. Real impl lands in Task 4.
use super::{Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, items_array};
use serde_json::Value;

pub struct JobKind;
const COLS: &[Column] = &[Column {
    title: "STATUS",
    width: 12,
}];

impl ResourceKind for JobKind {
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
            surface: "jobGroup",
            operation: "tui-job-list",
            path: "/v4/jobLibrary",
        }
    }

    fn detail_endpoint(&self) -> Option<DetailEndpoint> {
        Some(DetailEndpoint {
            surface: "jobGroup",
            operation: "tui-job-detail",
            path: "/v4/jobGroups/{id}",
        })
    }
}
