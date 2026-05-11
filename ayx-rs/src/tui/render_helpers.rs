//! Pure rendering helpers for the TUI.
//!
//! These are free functions with no shared state — they take a value and
//! produce a `PanelState` or a derived view. Pulled out of `app.rs` so
//! navigation isn't drowning in formatters.

use anyhow::Result;
use serde_json::Value;

use super::app::{OneBrowserItem, OneBrowserResource, PanelState};

/// Wrap a `Result<Value>` into a `PanelState` for the TUI to render.
pub fn render_envelope_panel(title: &str, value: Result<Value>) -> PanelState {
    match value {
        Ok(value) => PanelState {
            title: title.to_string(),
            lines: pretty_yaml_lines(&value),
            is_error: false,
            raw: Some(value),
        },
        Err(err) => PanelState {
            title: title.to_string(),
            lines: vec![err.to_string()],
            is_error: true,
            raw: None,
        },
    }
}

pub fn pretty_yaml_lines(value: &Value) -> Vec<String> {
    serde_yaml::to_string(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(|line| line.to_string())
        .collect()
}

pub fn extract_one_browser_items(
    resource: OneBrowserResource,
    value: &Value,
) -> Vec<OneBrowserItem> {
    let array = preferred_item_array(resource, value)
        .or_else(|| find_first_object_array(value))
        .or_else(|| value.as_array().map(|v| v.as_slice()));

    let Some(array) = array else {
        return Vec::new();
    };

    array
        .iter()
        .map(|item| {
            let id = item.as_object().and_then(|object| {
                string_field(
                    object,
                    &[
                        "id",
                        "workspace_id",
                        "workspaceId",
                        "flow_id",
                        "flowId",
                        "connection_id",
                        "connectionId",
                        "person_id",
                        "personId",
                        "subjectId",
                        "roleId",
                        "planId",
                    ],
                )
            });
            let id = id.map(str::to_owned);
            let label = item
                .as_object()
                .and_then(|object| {
                    string_field(
                        object,
                        &[
                            "name",
                            "title",
                            "label",
                            "display_name",
                            "displayName",
                            "workspace_name",
                            "workspaceName",
                            "flow_name",
                            "flowName",
                            "connection_name",
                            "connectionName",
                            "email",
                            "status",
                        ],
                    )
                })
                .map(str::to_owned)
                .unwrap_or_else(|| id.clone().unwrap_or_else(|| item.to_string()));
            let summary = item
                .as_object()
                .and_then(|object| {
                    string_field(
                        object,
                        &[
                            "description",
                            "status",
                            "type",
                            "path",
                            "command",
                            "method",
                            "role",
                        ],
                    )
                })
                .map(str::to_owned)
                .unwrap_or_default();
            OneBrowserItem { id, label, summary }
        })
        .collect()
}

fn preferred_item_array(resource: OneBrowserResource, value: &Value) -> Option<&[Value]> {
    let object = value.as_object()?;
    let keys = match resource {
        OneBrowserResource::SurfaceInventory => [
            "surfaces",
            "partial_surfaces",
            "documented_only_surfaces",
            "deferred_surfaces",
        ]
        .as_slice(),
        OneBrowserResource::WorkspaceCurrentConfiguration
        | OneBrowserResource::WorkspaceCurrentConfigurationSchema => &["environments"],
        OneBrowserResource::WorkspaceList
        | OneBrowserResource::WorkspaceDetail
        | OneBrowserResource::ConnectionList
        | OneBrowserResource::ConnectionDetail
        | OneBrowserResource::FlowList
        | OneBrowserResource::FlowDetail => &["items", "data", "results", "rows"],
        _ => &["items", "data", "results", "rows", "surfaces", "endpoints"],
    };

    for key in keys {
        if let Some(array) = object.get(*key).and_then(|value| value.as_array()) {
            return Some(array.as_slice());
        }
    }
    None
}

fn find_first_object_array(value: &Value) -> Option<&[Value]> {
    let object = value.as_object()?;
    for key in [
        "items",
        "data",
        "results",
        "rows",
        "surfaces",
        "endpoints",
        "workspaces",
        "flows",
        "connections",
        "people",
    ] {
        if let Some(array) = object.get(key).and_then(|value| value.as_array()) {
            return Some(array.as_slice());
        }
    }
    None
}

fn string_field<'a>(object: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| object.get(*key)?.as_str())
}
