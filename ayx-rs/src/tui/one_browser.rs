//! One API browser glue for the TUI.
//!
//! The TUI's "One Browser" pane is a thin wrapper around the same
//! `one_api_live_request` calls the CLI uses. This module holds the
//! blocking dispatcher that the background worker invokes off-thread —
//! it's not part of `app.rs` because it has no shared state and growing
//! it inside the App impl made navigation worse.

use anyhow::{anyhow, Result};
use ayx_core::profile::Config;
use serde_json::Value;

use super::app::OneBrowserResource;

/// Run the One API call backing a `OneBrowserResource` and return its
/// data payload. Called from the TUI worker thread (see `tui/worker.rs`).
pub fn request_for_one_browser_blocking(
    config: &Config,
    resource: OneBrowserResource,
    id: Option<&str>,
) -> Result<Value> {
    let envelope = match resource {
        OneBrowserResource::AuthStatus => crate::one_platform_auth_status_envelope(config)?,
        OneBrowserResource::AuthDiagnose => crate::one_platform_auth_diagnose_envelope(config)?,
        OneBrowserResource::SurfaceInventory => crate::one_surface_inventory_envelope(config)?,
        OneBrowserResource::WorkspaceCurrent => crate::one_api_live_request(
            config,
            "platform",
            "tui-workspace-current",
            "GET",
            "/v4/workspaces/current",
            false,
            &[],
        )?,
        OneBrowserResource::WorkspaceCurrentConfiguration => crate::one_api_live_request(
            config,
            "platform",
            "tui-workspace-current-configuration",
            "GET",
            "/v4/workspaces/current/configuration",
            false,
            &[],
        )?,
        OneBrowserResource::WorkspaceCurrentConfigurationSchema => crate::one_api_live_request(
            config,
            "platform",
            "tui-workspace-current-configuration-schema",
            "GET",
            "/v4/workspaces/current/configuration-schema",
            false,
            &[],
        )?,
        OneBrowserResource::WorkspaceList => crate::one_api_live_request(
            config,
            "platform",
            "tui-workspace-list",
            "GET",
            "/v4/workspaces",
            false,
            &[],
        )?,
        OneBrowserResource::WorkspaceDetail => crate::one_api_live_request(
            config,
            "platform",
            "tui-workspace-detail",
            "GET",
            "/v4/workspaces/{id}",
            false,
            &[("id", id.ok_or_else(|| anyhow!("workspace id required"))?)],
        )?,
        OneBrowserResource::FlowList => crate::one_api_live_request(
            config,
            "flow",
            "tui-flow-list",
            "GET",
            "/v4/flows",
            false,
            &[],
        )?,
        OneBrowserResource::FlowDetail => crate::one_api_live_request(
            config,
            "flow",
            "tui-flow-detail",
            "GET",
            "/v4/flows/{id}",
            false,
            &[("id", id.ok_or_else(|| anyhow!("flow id required"))?)],
        )?,
        OneBrowserResource::ConnectionList => crate::one_api_live_request(
            config,
            "connection",
            "tui-connection-list",
            "GET",
            "/v4/connections",
            false,
            &[],
        )?,
        OneBrowserResource::ConnectionDetail => crate::one_api_live_request(
            config,
            "connection",
            "tui-connection-detail",
            "GET",
            "/v4/connections/{id}",
            false,
            &[("id", id.ok_or_else(|| anyhow!("connection id required"))?)],
        )?,
    };
    Ok(envelope.data)
}
