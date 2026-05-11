//! Typed response structs for One API surfaces.
//!
//! The transport returns `serde_json::Value` for compatibility with the
//! existing dispatcher, but every wired surface should grow a typed counterpart
//! here so callers (TUI, downstream tooling, future agents) can pattern-match
//! against schema rather than poking at JSON.
//!
//! Convention:
//! - Every struct uses `#[serde(deny_unknown_fields = false)]` (the default) so
//!   the parser is forward-compatible with new fields the server adds.
//! - Every struct exposes a `raw: Option<serde_json::Value>` escape hatch via
//!   a sibling `*Raw` wrapper for callers that want both shapes.
//! - Date fields stay as `String` for now (Alteryx One uses ISO 8601); promote
//!   to `chrono::DateTime<Utc>` when callers actually need temporal logic.
//!
//! Start: flow surface. Adopt the pattern for plans, connections,
//! workspaces, etc. in follow-up PRs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One row from `/v4/flows` (list endpoint).
///
/// Field coverage mirrors what the inventory says the surface returns;
/// unknown fields are silently retained via `extra` for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(default, alias = "updatedAt", alias = "updated_at")]
    pub updated_at: Option<String>,
    #[serde(default, alias = "folderId", alias = "folder_id")]
    pub folder_id: Option<String>,
    #[serde(default, alias = "workspaceId", alias = "workspace_id")]
    pub workspace_id: Option<String>,
    /// Any fields the server returned that we don't model explicitly. Useful
    /// when the response shape evolves between releases.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Paginated wrapper for `/v4/flows`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowListPage {
    #[serde(default)]
    pub items: Vec<FlowSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl FlowListPage {
    /// Parse a `/v4/flows` response body into a typed page. The transport
    /// stores the raw response under `data.response`; pass that value here.
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

/// Shared parser for endpoints that may return either an `{items: [...]}`
/// object or a bare array. Used by every typed page wrapper.
fn from_value_or_array<T>(v: &Value) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned + Default + FromItems,
{
    if v.is_array() {
        let items: Vec<<T as FromItems>::Item> = serde_json::from_value(v.clone())?;
        return Ok(T::from_items(items));
    }
    serde_json::from_value(v.clone())
}

/// Adapter trait: lets [`from_value_or_array`] wrap a bare-array response
/// into the right `*ListPage` type without knowing about each concrete one.
pub trait FromItems: Sized {
    type Item: serde::de::DeserializeOwned;
    fn from_items(items: Vec<Self::Item>) -> Self;
}

impl FromItems for FlowListPage {
    type Item = FlowSummary;
    fn from_items(items: Vec<FlowSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── Plans ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(default, alias = "updatedAt", alias = "updated_at")]
    pub updated_at: Option<String>,
    #[serde(default, alias = "workspaceId", alias = "workspace_id")]
    pub workspace_id: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanListPage {
    #[serde(default)]
    pub items: Vec<PlanSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl PlanListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for PlanListPage {
    type Item = PlanSummary;
    fn from_items(items: Vec<PlanSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── Connections ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Connector identifier (e.g. `snowflake`, `s3`, `databricks`).
    #[serde(default, alias = "connectorId", alias = "connector_id")]
    pub connector_id: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(default, alias = "updatedAt", alias = "updated_at")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionListPage {
    #[serde(default)]
    pub items: Vec<ConnectionSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl ConnectionListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for ConnectionListPage {
    type Item = ConnectionSummary;
    fn from_items(items: Vec<ConnectionSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── People ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default, alias = "fullName", alias = "full_name")]
    pub full_name: Option<String>,
    #[serde(default, alias = "firstName", alias = "first_name")]
    pub first_name: Option<String>,
    #[serde(default, alias = "lastName", alias = "last_name")]
    pub last_name: Option<String>,
    #[serde(default, alias = "isAdmin", alias = "is_admin")]
    pub is_admin: Option<bool>,
    #[serde(default, alias = "isSuspended", alias = "is_suspended")]
    pub is_suspended: Option<bool>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonListPage {
    #[serde(default)]
    pub items: Vec<PersonSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl PersonListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for PersonListPage {
    type Item = PersonSummary;
    fn from_items(items: Vec<PersonSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_object_with_items_array() {
        let payload = json!({
            "items": [
                {"id": "f1", "name": "alpha", "createdAt": "2026-05-10T12:00:00Z"},
                {"id": "f2", "name": "beta", "extra_field": "preserved"},
            ],
            "nextPageToken": "abc"
        });
        let page = FlowListPage::from_value(&payload).expect("parses");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id.as_deref(), Some("f1"));
        assert_eq!(
            page.items[0].created_at.as_deref(),
            Some("2026-05-10T12:00:00Z")
        );
        assert_eq!(page.next_page_token.as_deref(), Some("abc"));
        // Unknown fields preserved via flatten.
        assert!(page.items[1].extra.contains_key("extra_field"));
    }

    #[test]
    fn parses_bare_array() {
        let payload = json!([
            {"id": "f1", "name": "alpha"},
        ]);
        let page = FlowListPage::from_value(&payload).expect("parses");
        assert_eq!(page.items.len(), 1);
        assert!(page.next_page_token.is_none());
    }

    #[test]
    fn empty_payload_yields_empty_page() {
        let page = FlowListPage::from_value(&json!({})).expect("parses");
        assert!(page.items.is_empty());
        assert!(page.next_page_token.is_none());
    }

    #[test]
    fn snake_case_alias_accepted() {
        let payload = json!({
            "items": [
                {"id": "f1", "created_at": "2026-05-10T12:00:00Z", "folder_id": "fldr"},
            ]
        });
        let page = FlowListPage::from_value(&payload).expect("parses");
        assert_eq!(
            page.items[0].created_at.as_deref(),
            Some("2026-05-10T12:00:00Z")
        );
        assert_eq!(page.items[0].folder_id.as_deref(), Some("fldr"));
    }

    #[test]
    fn plan_list_parses_object_and_array() {
        let obj = json!({"items":[{"id":"p1","name":"Q4 forecast"}],"nextPageToken":"abc"});
        let p = PlanListPage::from_value(&obj).expect("parses object");
        assert_eq!(p.items[0].id.as_deref(), Some("p1"));
        assert_eq!(p.next_page_token.as_deref(), Some("abc"));
        let arr = json!([{"id":"p2","name":"adhoc"}]);
        let p2 = PlanListPage::from_value(&arr).expect("parses array");
        assert_eq!(p2.items.len(), 1);
        assert!(p2.next_page_token.is_none());
    }

    #[test]
    fn connection_list_aliases() {
        let payload = json!({"items":[{"id":"c1","connector_id":"snowflake","name":"prod"}]});
        let p = ConnectionListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].connector_id.as_deref(), Some("snowflake"));
    }

    #[test]
    fn person_list_preserves_unknown_fields() {
        let payload = json!({"items":[{"id":"u1","email":"a@b.com","unmodeled_field":42}]});
        let p = PersonListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].email.as_deref(), Some("a@b.com"));
        assert!(p.items[0].extra.contains_key("unmodeled_field"));
    }

    #[test]
    fn person_list_is_admin_parsed() {
        let payload =
            json!({"items":[{"id":"u1","email":"a@b.com","isAdmin":true,"isSuspended":false}]});
        let p = PersonListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].is_admin, Some(true));
        assert_eq!(p.items[0].is_suspended, Some(false));
    }
}
