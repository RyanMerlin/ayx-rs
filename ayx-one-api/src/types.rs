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

// ─── Workspaces ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, alias = "workspaceId", alias = "workspace_id")]
    pub workspace_id: Option<String>,
    #[serde(
        default,
        alias = "name",
        alias = "workspaceName",
        alias = "workspace_name"
    )]
    pub name: Option<String>,
    #[serde(default, alias = "description")]
    pub description: Option<String>,
    #[serde(default, alias = "status")]
    pub status: Option<String>,
    #[serde(default, alias = "ownerEmail", alias = "owner_email")]
    pub owner_email: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(default, alias = "updatedAt", alias = "updated_at")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceListPage {
    #[serde(default)]
    pub items: Vec<WorkspaceSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl WorkspaceListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for WorkspaceListPage {
    type Item = WorkspaceSummary;
    fn from_items(items: Vec<WorkspaceSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── Job groups ────────────────────────────────────────────────────────────

/// One row from `/v4/jobLibrary` or `/v4/jobGroups`. The two list endpoints
/// return overlapping but not identical shapes; this struct unions both via
/// optional fields. Telemetry aggregates over `status`, `started_at`,
/// `finished_at`, and `flow_id` — anything else is parked in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobGroupSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, alias = "flowId", alias = "flow_id")]
    pub flow_id: Option<String>,
    #[serde(default, alias = "flowName", alias = "flow_name")]
    pub flow_name: Option<String>,
    #[serde(default, alias = "planId", alias = "plan_id")]
    pub plan_id: Option<String>,
    /// Queued / Running / Succeeded / Failed / Cancelled (per One UI strings).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(default, alias = "startedAt", alias = "started_at")]
    pub started_at: Option<String>,
    #[serde(default, alias = "finishedAt", alias = "finished_at")]
    pub finished_at: Option<String>,
    /// Some surfaces return a duration in milliseconds directly.
    #[serde(default, alias = "durationMs", alias = "duration_ms")]
    pub duration_ms: Option<u64>,
    #[serde(default, alias = "ownerId", alias = "owner_id")]
    pub owner_id: Option<String>,
    #[serde(default, alias = "ownerEmail", alias = "owner_email")]
    pub owner_email: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobGroupListPage {
    #[serde(default)]
    pub items: Vec<JobGroupSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl JobGroupListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for JobGroupListPage {
    type Item = JobGroupSummary;
    fn from_items(items: Vec<JobGroupSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── Schedules ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, alias = "planId", alias = "plan_id")]
    pub plan_id: Option<String>,
    #[serde(default, alias = "flowId", alias = "flow_id")]
    pub flow_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub recurrence: Option<String>,
    #[serde(default, alias = "nextRunAt", alias = "next_run_at")]
    pub next_run_at: Option<String>,
    #[serde(default, alias = "lastRunAt", alias = "last_run_at")]
    pub last_run_at: Option<String>,
    #[serde(default, alias = "ownerId", alias = "owner_id")]
    pub owner_id: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleListPage {
    #[serde(default)]
    pub items: Vec<ScheduleSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl ScheduleListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for ScheduleListPage {
    type Item = ScheduleSummary;
    fn from_items(items: Vec<ScheduleSummary>) -> Self {
        Self {
            items,
            next_page_token: None,
            total: None,
            extra: Default::default(),
        }
    }
}

// ─── Roles ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleSummary {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "workspaceId", alias = "workspace_id")]
    pub workspace_id: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleListPage {
    #[serde(default)]
    pub items: Vec<RoleSummary>,
    #[serde(default, alias = "nextPageToken", alias = "next_page_token")]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl RoleListPage {
    pub fn from_value(v: &Value) -> Result<Self, serde_json::Error> {
        from_value_or_array(v)
    }
}

impl FromItems for RoleListPage {
    type Item = RoleSummary;
    fn from_items(items: Vec<RoleSummary>) -> Self {
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

    #[test]
    fn workspace_list_parses_workspace_name_and_aliases() {
        let payload = json!({
            "items": [
                {
                    "id": "w1",
                    "workspaceName": "Prod",
                    "workspace_id": "ws-123",
                    "ownerEmail": "ops@example.com",
                    "status": "active",
                    "updatedAt": "2026-05-10T13:00:00Z"
                }
            ],
            "nextPageToken": "next-1"
        });
        let p = WorkspaceListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].name.as_deref(), Some("Prod"));
        assert_eq!(p.items[0].workspace_id.as_deref(), Some("ws-123"));
        assert_eq!(p.items[0].owner_email.as_deref(), Some("ops@example.com"));
        assert_eq!(p.items[0].status.as_deref(), Some("active"));
        assert_eq!(
            p.items[0].updated_at.as_deref(),
            Some("2026-05-10T13:00:00Z")
        );
        assert_eq!(p.next_page_token.as_deref(), Some("next-1"));
    }

    #[test]
    fn workspace_list_parses_bare_array() {
        let payload = json!([{"id":"w1","name":"Dev"}]);
        let p = WorkspaceListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].name.as_deref(), Some("Dev"));
    }

    #[test]
    fn job_group_list_parses_status_and_aliases() {
        let payload = json!({
            "items": [
                {
                    "id": "jg1",
                    "flowId": "f1",
                    "flowName": "Daily ETL",
                    "status": "Succeeded",
                    "startedAt": "2026-05-10T12:00:00Z",
                    "finishedAt": "2026-05-10T12:05:30Z",
                    "ownerEmail": "ops@example.com"
                },
                {
                    "id": "jg2",
                    "flow_id": "f2",
                    "status": "Failed",
                    "error": "timeout",
                    "duration_ms": 90000
                }
            ],
            "nextPageToken": "p2"
        });
        let p = JobGroupListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].flow_id.as_deref(), Some("f1"));
        assert_eq!(p.items[0].status.as_deref(), Some("Succeeded"));
        assert_eq!(p.items[1].flow_id.as_deref(), Some("f2"));
        assert_eq!(p.items[1].error.as_deref(), Some("timeout"));
        assert_eq!(p.items[1].duration_ms, Some(90000));
        assert_eq!(p.next_page_token.as_deref(), Some("p2"));
    }

    #[test]
    fn job_group_list_parses_bare_array() {
        let payload = json!([{"id": "jg1", "status": "Running"}]);
        let p = JobGroupListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].status.as_deref(), Some("Running"));
    }

    #[test]
    fn schedule_list_parses_cron_and_recurrence() {
        let payload = json!({
            "items": [
                {"id": "s1", "planId": "pl1", "enabled": true, "cron": "0 6 * * *",
                 "nextRunAt": "2026-05-12T06:00:00Z"}
            ]
        });
        let p = ScheduleListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].plan_id.as_deref(), Some("pl1"));
        assert_eq!(p.items[0].cron.as_deref(), Some("0 6 * * *"));
        assert_eq!(p.items[0].enabled, Some(true));
    }

    #[test]
    fn role_list_parses_basic_fields() {
        let payload = json!({
            "items": [{"id": "r1", "name": "Editor", "description": "edit flows"}]
        });
        let p = RoleListPage::from_value(&payload).expect("parses");
        assert_eq!(p.items[0].name.as_deref(), Some("Editor"));
    }
}
