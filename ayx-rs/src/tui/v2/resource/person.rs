//! Person ResourceKind — maps `/v4/people` items to rows.
//! Fields per ayx-one-api/src/types.rs:202-222 (PersonSummary).
use super::{
    Cell, Column, DetailEndpoint, ListEndpoint, ResourceKind, Row, StatusTone, items_array,
    str_field,
};
use serde_json::Value;

pub struct PersonKind;

const COLS: &[Column] = &[
    Column {
        title: "NAME",
        width: 28,
    },
    Column {
        title: "EMAIL",
        width: 30,
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

fn bool_field(item: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .any(|k| item.get(*k).and_then(Value::as_bool).unwrap_or(false))
}

impl ResourceKind for PersonKind {
    fn columns(&self) -> &'static [Column] {
        COLS
    }

    fn extract_items(&self, payload: &Value) -> Vec<Value> {
        items_array(payload)
    }

    fn row(&self, item: &Value) -> Row {
        let id = str_field(item, &["id"]).unwrap_or_default().to_string();
        let email = str_field(item, &["email"]).unwrap_or_default().to_string();
        let name = str_field(item, &["fullName", "full_name"])
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| (!email.is_empty()).then(|| email.clone()))
            .unwrap_or_else(|| "(unnamed)".to_string());

        let (status_text, tone) = if bool_field(item, &["isSuspended", "is_suspended"]) {
            ("suspended", StatusTone::Danger)
        } else if bool_field(item, &["isAdmin", "is_admin"]) {
            ("admin", StatusTone::Ok)
        } else {
            ("active", StatusTone::Neutral)
        };

        Row {
            id: id.clone(),
            cells: vec![
                Cell::plain(name),
                Cell::plain(email),
                Cell::toned(status_text, tone),
                Cell::plain(id),
            ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::v2::resource::{ResourceKind, StatusTone};
    use serde_json::json;

    #[test]
    fn columns_are_four() {
        assert_eq!(PersonKind.columns().len(), 4);
        assert_eq!(PersonKind.columns()[0].title, "NAME");
        assert_eq!(PersonKind.columns()[1].title, "EMAIL");
    }

    #[test]
    fn row_uses_full_name_and_email() {
        let item = json!({ "id": "u_1", "fullName": "Ryan Merlin", "email": "ryan@alteryx.com" });
        let row = PersonKind.row(&item);
        assert_eq!(row.id, "u_1");
        assert_eq!(row.cells[0].text, "Ryan Merlin");
        assert_eq!(row.cells[1].text, "ryan@alteryx.com");
        assert_eq!(row.cells[2].text, "active");
        assert_eq!(row.cells[2].tone, StatusTone::Neutral);
        assert_eq!(row.cells[3].text, "u_1");
    }

    #[test]
    fn suspended_takes_priority_over_admin() {
        let row = PersonKind.row(&json!({ "id": "u_2", "isAdmin": true, "isSuspended": true }));
        assert_eq!(row.cells[2].text, "suspended");
        assert_eq!(row.cells[2].tone, StatusTone::Danger);
    }

    #[test]
    fn admin_when_not_suspended() {
        let row = PersonKind.row(&json!({ "id": "u_3", "isAdmin": true }));
        assert_eq!(row.cells[2].text, "admin");
        assert_eq!(row.cells[2].tone, StatusTone::Ok);
    }

    #[test]
    fn name_falls_back_to_email_then_placeholder() {
        let by_email = PersonKind.row(&json!({ "id": "u_4", "email": "x@y.com" }));
        assert_eq!(by_email.cells[0].text, "x@y.com");
        let none = PersonKind.row(&json!({ "id": "u_5" }));
        assert_eq!(none.cells[0].text, "(unnamed)");
    }
}
