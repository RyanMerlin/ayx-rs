//! Job ResourceKind — maps `/v4/jobLibrary` job-group rows to display rows.
//! Fields per ayx-one-api/src/types.rs:319-349 (JobGroupSummary).
use super::{
    Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, date_only,
    items_array, str_field,
};
use serde_json::Value;

pub struct JobKind;

const COLS: &[Column] = &[
    Column {
        title: "FLOW",
        width: 32,
    },
    Column {
        title: "STATUS",
        width: 12,
    },
    Column {
        title: "STARTED",
        width: 12,
    },
    Column {
        title: "ID",
        width: 22,
    },
];

/// Map a One job-group status string to a tone. Case-insensitive.
pub(crate) fn status_tone(status: &str) -> StatusTone {
    match status.to_ascii_lowercase().as_str() {
        "succeeded" => StatusTone::Ok,
        "running" | "queued" => StatusTone::Warn,
        "failed" | "cancelled" | "canceled" => StatusTone::Danger,
        _ => StatusTone::Neutral,
    }
}

impl ResourceKind for JobKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let flow = str_field(item, &["flowName", "flow_name", "flowId", "flow_id"])
            .unwrap_or("(unknown flow)")
            .to_string();
        let status_text = str_field(item, &["status"]).unwrap_or("—").to_string();
        let tone = status_tone(&status_text);
        let started = str_field(
            item,
            &["startedAt", "started_at", "createdAt", "created_at"],
        )
        .map(date_only)
        .unwrap_or_default();
        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(flow),
                Cell::toned(status_text, tone),
                Cell::plain(started),
                Cell::plain(id),
            ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(JobKind.columns().len(), 4);
        assert_eq!(JobKind.columns()[0].title, "FLOW");
        assert_eq!(JobKind.columns()[1].title, "STATUS");
    }

    #[test]
    fn row_maps_flow_status_started_id_with_tone() {
        let item = json!({
            "id": "jg_1", "flowName": "Daily ETL", "status": "Succeeded",
            "startedAt": "2026-06-21T02:00:00Z"
        });
        let row = JobKind.row(&item);
        assert_eq!(row.id, "jg_1");
        assert_eq!(row.cells[0].text, "Daily ETL");
        assert_eq!(row.cells[1].text, "Succeeded");
        assert_eq!(row.cells[1].tone, StatusTone::Ok);
        assert_eq!(row.cells[2].text, "2026-06-21");
        assert_eq!(row.cells[3].text, "jg_1");
    }

    #[test]
    fn status_tone_mapping() {
        assert_eq!(status_tone("Succeeded"), StatusTone::Ok);
        assert_eq!(status_tone("running"), StatusTone::Warn);
        assert_eq!(status_tone("Queued"), StatusTone::Warn);
        assert_eq!(status_tone("Failed"), StatusTone::Danger);
        assert_eq!(status_tone("Cancelled"), StatusTone::Danger);
        assert_eq!(status_tone("weird"), StatusTone::Neutral);
    }

    #[test]
    fn row_falls_back_to_flow_id_then_placeholder() {
        let by_id = JobKind.row(&json!({ "id": "jg_2", "flowId": "fl_9", "status": "Running" }));
        assert_eq!(by_id.cells[0].text, "fl_9");
        let none = JobKind.row(&json!({ "id": "jg_3" }));
        assert_eq!(none.cells[0].text, "(unknown flow)");
        assert_eq!(none.cells[1].text, "—");
        assert_eq!(none.cells[1].tone, StatusTone::Neutral);
    }
}
