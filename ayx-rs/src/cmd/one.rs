//! Dispatch for `ayx one ...`.
//!
//! The largest single dispatch arm in the original main.rs — ~2000 LOC
//! covering platform / workspace / role / person / token / api / auth /
//! plans / scheduling / billing / flows / connections / connector
//! metadata / job groups / output objects / webhook flow tasks / write
//! settings / doctor / auto-insights / desktop-exec.
//!
//! Each arm is verbatim from the original dispatch, wrapped in
//! `Ok(match command { ... })` so the function returns `Result<Envelope>`.
//! The `load_profile` closure replaces the same-named captured closure
//! in main.rs by delegating to the shared profile loader.

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one::{api_diagnose_envelope, api_inventory_envelope, api_status_envelope};
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use serde_json::json;

use crate::{
    load_payload, ui_command_envelope, OneBillingCommand, OneCommand, OneSchedulingCommand,
    OneWebhookFlowTaskCommand, OneWriteSettingCommand, UiCommand, UiDataCommand, UiJobsCommand,
    UiLibraryCommand, UiSchedulesCommand, UiSessionCommand, UiWorkflowCommand,
};

/// Borrow Cli's apply + yes for the TTY confirm prompts inside delete arms.
pub struct Ctx<'a> {
    pub apply: bool,
    pub yes: bool,
    pub environment: Option<&'a str>,
}

#[allow(clippy::too_many_lines)]
pub fn execute(cli: Ctx<'_>, command: Option<OneCommand>) -> Result<Envelope> {
    // Capture `environment` up-front so `cli.environment` reads through the
    // helper don't conflict with `cli` itself being borrowed by other arms.
    let environment = cli.environment;
    let runtime = crate::cmd::RuntimeCtx::new(environment);
    macro_rules! load_profile {
        ($profile:expr, $environment:expr) => {
            runtime.load_profile_lenient($profile)
        };
    }
    Ok(match command {
            None => Envelope::ok(
                "one commands available: platform, plans, scheduling, billing, auto-insights, desktop-exec",
            ),
            Some(OneCommand::Doctor { command }) => super::one_doctor::execute(&runtime, command)?,
            Some(OneCommand::Platform { command }) => {
                super::one_platform::execute(&runtime, cli.apply, cli.yes, command)?
            }
            Some(OneCommand::JobGroups { command }) => {
                super::one_job_groups::execute(&runtime, command)?
            }
            Some(OneCommand::OutputObjects { command }) => {
                super::one_output_objects::execute(&runtime, command)?
            }
            Some(OneCommand::WebhookFlowTasks { command }) => match command {
                None => Envelope::ok("one webhook-flow-task commands available: create, detail, delete, test"),
                Some(OneWebhookFlowTaskCommand::Create { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "create",
                        "POST",
                        "/v4/webhookFlowTasks",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Detail {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "detail",
                        "GET",
                        "/v4/webhookFlowTasks/{id}",
                        false,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Delete {
                    profile,
                    webhook_flow_task_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let webhook_flow_task_id =
                        webhook_flow_task_id.ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
                    one_api_live_request(
                        &config,
                        "webhookFlowTask",
                        "delete",
                        "DELETE",
                        "/v4/webhookFlowTasks/{id}",
                        true,
                        &[("id", webhook_flow_task_id.as_str())],
                    )?
                }
                Some(OneWebhookFlowTaskCommand::Test { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "webhookFlowTask",
                        "test",
                        "POST",
                        "/v4/webhooks/test",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
            },
            Some(OneCommand::WriteSettings { command }) => match command {
                None => Envelope::ok("one write-setting commands available: list, count, create, detail, update, delete"),
                Some(OneWriteSettingCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "writeSetting",
                        "list",
                        "/v4/writeSettings",
                        &[],
                        &params,
                    )?
                }
                Some(OneWriteSettingCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "count",
                        "GET",
                        "/v4/writeSettings/count",
                        false,
                        &[],
                    )?
                }
                Some(OneWriteSettingCommand::Create { profile, body }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "create",
                        "POST",
                        "/v4/writeSettings",
                        true,
                        &[],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Detail {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "detail",
                        "GET",
                        "/v4/writeSettings/{id}",
                        false,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
                Some(OneWriteSettingCommand::Update {
                    profile,
                    write_setting_id,
                    body,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "writeSetting",
                        "update",
                        "PATCH",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                        Some(payload),
                    )?
                }
                Some(OneWriteSettingCommand::Delete {
                    profile,
                    write_setting_id,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let write_setting_id =
                        write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
                    one_api_live_request(
                        &config,
                        "writeSetting",
                        "delete",
                        "DELETE",
                        "/v4/writeSettings/{id}",
                        true,
                        &[("id", write_setting_id.as_str())],
                    )?
                }
            },
            Some(OneCommand::Status { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_status_envelope(&config, "one")?
            }
            Some(OneCommand::Inventory { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_inventory_envelope(&config, "one")?
            }
            Some(OneCommand::Connections { command }) => {
                super::one_connections::execute(&runtime, command)?
            },
            Some(OneCommand::Flows { command }) => {
                super::one_flows::execute(&runtime, cli.apply, cli.yes, command)?
            },
            Some(OneCommand::Plans { command }) => {
                super::one_plans::execute(&runtime, cli.apply, cli.yes, command)?
            },
            Some(OneCommand::Scheduling { command }) => match command {
                None => Envelope::ok(
                    "one scheduling commands available: list, detail, enable, disable, count",
                ),
                Some(OneSchedulingCommand::List {
                    profile,
                    limit,
                    page_token,
                    all,
                    max_pages,
                }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(limit)
                        .with_page_token(page_token)
                        .with_all(all, max_pages);
                    ayx_one_api::one_api_list_request(
                        &config,
                        "scheduling",
                        "list",
                        "/scheduling/v1/schedules",
                        &[],
                        &params,
                    )?
                }
                Some(OneSchedulingCommand::Detail { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "detail",
                        "GET",
                        "/scheduling/v1/schedules/{id}",
                        false,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Enable { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "enable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/enable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Disable { profile, schedule_id }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    let schedule_id = schedule_id.ok_or_else(|| anyhow!("--schedule-id is required"))?;
                    one_api_live_request(
                        &config,
                        "scheduling",
                        "disable",
                        "POST",
                        "/scheduling/v1/schedules/{id}/disable",
                        true,
                        &[("id", schedule_id.as_str())],
                    )?
                }
                Some(OneSchedulingCommand::Count { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "scheduling", "count", "GET", "/scheduling/v1/schedules/count", false, &[])?
                }
            },
            Some(OneCommand::Billing { command }) => match command {
                None => Envelope::ok("one billing commands available: current-account, usage-export"),
                Some(OneBillingCommand::CurrentAccount { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "billing", "current-account", "GET", "/billing/v1/my/billing-accounts/current", false, &[])?
                }
                Some(OneBillingCommand::UsageExport { profile }) => {
                    let config = load_profile!(profile.as_deref(), environment)?;
                    one_api_live_request(&config, "billing", "usage-export", "GET", "/billing/v1/usage/export", false, &[])?
                }
            },
            Some(OneCommand::Ui { command }) => match command {
                None => Envelope::ok("one ui commands available: session, workflow, data, library, schedules, jobs (experimental)"),
                Some(UiCommand::Session { command }) => match command {
                    None => Envelope::ok("one ui session commands available: status, ensure, attach, inventory (experimental)"),
                    Some(UiSessionCommand::Status) => Envelope::ok_with_data(
                        "one ui session status scaffolded",
                        ui_command_envelope("session", "status", json!({
                            "browser": "managed by ayx-rs",
                            "mode": "experimental hybrid pinned visible tabs plus background read-only pages",
                        })),
                    ),
                    Some(UiSessionCommand::Ensure) => Envelope::ok_with_data(
                        "one ui session ensure scaffolded",
                        ui_command_envelope("session", "ensure", json!({ "result": "scaffolded" })),
                    ),
                    Some(UiSessionCommand::Attach { tab }) => Envelope::ok_with_data(
                        "one ui session attach scaffolded",
                        ui_command_envelope("session", "attach", json!({ "tab": tab })),
                    ),
                    Some(UiSessionCommand::Inventory) => Envelope::ok_with_data(
                        "one ui session inventory scaffolded",
                        ui_command_envelope("session", "inventory", json!({
                            "tabs": ["workflow", "data"],
                            "policy": "foreground tabs are reusable; read-only tasks may use background pages",
                        })),
                    ),
                },
                Some(UiCommand::Workflow { command }) => match command {
                    None => Envelope::ok("one ui workflow commands available: open, create, inventory, pane-config, pane-results, tool-list, tool-select, tool-inspect, graph-get, graph-put (experimental)"),
                    Some(UiWorkflowCommand::Open { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow open scaffolded",
                        ui_command_envelope("workflow", "open", json!({ "workflow_id": workflow_id, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Create { name, foreground }) => Envelope::ok_with_data(
                        "one ui workflow create scaffolded",
                        ui_command_envelope("workflow", "create", json!({ "name": name, "foreground": foreground })),
                    ),
                    Some(UiWorkflowCommand::Inventory { workflow_id, foreground }) => Envelope::ok_with_data(
                        "one ui workflow inventory scaffolded",
                        ui_command_envelope("workflow", "inventory", json!({
                            "workflow_id": workflow_id,
                            "foreground": foreground,
                            "captures": ["canvas", "config-pane", "results-pane"],
                        })),
                    ),
                    Some(UiWorkflowCommand::PaneConfig { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-config scaffolded",
                        ui_command_envelope("workflow", "pane-config", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::PaneResults { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow pane-results scaffolded",
                        ui_command_envelope("workflow", "pane-results", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolList { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-list scaffolded",
                        ui_command_envelope("workflow", "tool-list", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::ToolSelect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-select scaffolded",
                        ui_command_envelope("workflow", "tool-select", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::ToolInspect { workflow_id, tool_id }) => Envelope::ok_with_data(
                        "one ui workflow tool-inspect scaffolded",
                        ui_command_envelope("workflow", "tool-inspect", json!({ "workflow_id": workflow_id, "tool_id": tool_id })),
                    ),
                    Some(UiWorkflowCommand::GraphGet { workflow_id }) => Envelope::ok_with_data(
                        "one ui workflow graph-get scaffolded",
                        ui_command_envelope("workflow", "graph-get", json!({ "workflow_id": workflow_id })),
                    ),
                    Some(UiWorkflowCommand::GraphPut { workflow_id, input }) => Envelope::ok_with_data(
                        "one ui workflow graph-put scaffolded",
                        ui_command_envelope("workflow", "graph-put", json!({ "workflow_id": workflow_id, "input": input.display().to_string() })),
                    ),
                },
                Some(UiCommand::Data { command }) => match command {
                    None => Envelope::ok("one ui data commands available: list-datasets, dataset-detail, dataset-preview, upload, list-connections (experimental)"),
                    Some(UiDataCommand::ListDatasets { foreground }) => Envelope::ok_with_data(
                        "one ui data list-datasets scaffolded",
                        ui_command_envelope("data", "list-datasets", json!({
                            "foreground": foreground,
                            "tab_policy": "use pinned tab when warm; background page for read-only refresh is allowed",
                        })),
                    ),
                    Some(UiDataCommand::DatasetDetail { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-detail scaffolded",
                        ui_command_envelope("data", "dataset-detail", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::DatasetPreview { dataset_id, foreground }) => Envelope::ok_with_data(
                        "one ui data dataset-preview scaffolded",
                        ui_command_envelope("data", "dataset-preview", json!({ "dataset_id": dataset_id, "foreground": foreground })),
                    ),
                    Some(UiDataCommand::Upload { input, foreground }) => Envelope::ok_with_data(
                        "one ui data upload scaffolded",
                        ui_command_envelope("data", "upload", json!({ "input": input.display().to_string(), "foreground": foreground })),
                    ),
                    Some(UiDataCommand::ListConnections { foreground }) => Envelope::ok_with_data(
                        "one ui data list-connections scaffolded",
                        ui_command_envelope("data", "list-connections", json!({ "foreground": foreground })),
                    ),
                },
                Some(UiCommand::Library { command }) => match command {
                    None => Envelope::ok("one ui library commands available: inventory (experimental)"),
                    Some(UiLibraryCommand::Inventory) => Envelope::ok_with_data(
                        "one ui library inventory scaffolded",
                        ui_command_envelope("library", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Schedules { command }) => match command {
                    None => Envelope::ok("one ui schedules commands available: inventory (experimental)"),
                    Some(UiSchedulesCommand::Inventory) => Envelope::ok_with_data(
                        "one ui schedules inventory scaffolded",
                        ui_command_envelope("schedules", "inventory", json!({})),
                    ),
                },
                Some(UiCommand::Jobs { command }) => match command {
                    None => Envelope::ok("one ui jobs commands available: inventory (experimental)"),
                    Some(UiJobsCommand::Inventory) => Envelope::ok_with_data(
                        "one ui jobs inventory scaffolded",
                        ui_command_envelope("jobs", "inventory", json!({})),
                    ),
                },
            },
            Some(OneCommand::AutoInsights { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_diagnose_envelope(&config, "one auto-insights")?
            }
            Some(OneCommand::DesktopExec { profile }) => {
                let config = load_profile!(profile.as_deref(), environment)?;
                api_status_envelope(&config, "one desktop-exec")?
            }
    })
}
