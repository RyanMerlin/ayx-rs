//! Workspace ResourceKind — maps `/v4/workspaces` items to rows (read-only
//! browse). No per-id detail endpoint is wired; workspace *switching* is a
//! later phase. Fields per ayx-one-api/src/types.rs:256-281 (WorkspaceSummary).
use super::{
    Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, items_array,
    str_field,
};
use serde_json::Value;

pub struct WorkspaceKind;

const COLS: &[Column] = &[
    Column {
        title: "NAME",
        width: 30,
    },
    Column {
        title: "OWNER",
        width: 28,
    },
    Column {
        title: "STATUS",
        width: 12,
    },
    Column {
        title: "ID",
        width: 22,
    },
];

fn status_tone(status: &str) -> StatusTone {
    match status.to_ascii_lowercase().as_str() {
        "active" | "ready" => StatusTone::Ok,
        "suspended" | "disabled" => StatusTone::Danger,
        _ => StatusTone::Neutral,
    }
}

impl ResourceKind for WorkspaceKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id", "workspaceId", "workspace_id"])
            .unwrap_or_default()
            .to_string();
        let name = str_field(item, &["name", "workspaceName", "workspace_name"])
            .unwrap_or("(unnamed)")
            .to_string();
        let owner = str_field(item, &["ownerEmail", "owner_email"])
            .unwrap_or_default()
            .to_string();
        let status_text = str_field(item, &["status"]).unwrap_or("—").to_string();
        let tone = status_tone(&status_text);
        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(name),
                Cell::plain(owner),
                Cell::toned(status_text, tone),
                Cell::plain(id),
            ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(WorkspaceKind.columns().len(), 4);
        assert_eq!(WorkspaceKind.columns()[0].title, "NAME");
    }

    #[test]
    fn row_prefers_id_then_workspace_id() {
        let item = json!({
            "workspaceId": "w_1", "workspaceName": "Marketing",
            "ownerEmail": "ops@alteryx.com", "status": "active"
        });
        let row = WorkspaceKind.row(&item);
        assert_eq!(row.id, "w_1");
        assert_eq!(row.cells[0].text, "Marketing");
        assert_eq!(row.cells[1].text, "ops@alteryx.com");
        assert_eq!(row.cells[2].text, "active");
        assert_eq!(row.cells[2].tone, StatusTone::Ok);
        assert_eq!(row.cells[3].text, "w_1");
    }

    #[test]
    fn no_detail_endpoint() {
        assert!(WorkspaceKind.detail_endpoint().is_none());
        assert_eq!(WorkspaceKind.list_endpoint().path, "/v4/workspaces");
    }
}
