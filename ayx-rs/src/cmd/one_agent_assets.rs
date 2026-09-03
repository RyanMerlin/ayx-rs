//! Agent Studio MCP asset registration.
//!
//! These endpoints are used by the Agent Studio Set Up Assets page. They are
//! intentionally kept separate from the public `/v4` One API inventory: the
//! public API creates and reads assets, while this service registers them for
//! the Insights and Apps MCP toolsets.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use ayx_core::envelope::{Envelope, ErrorCode};
use ayx_one_api::{
    one_api_live_request, one_api_live_request_with_body, one_api_live_request_with_query,
};
use serde_json::{Value, json};

use crate::{
    OneAgentAssetsCommand, OneAgentDatasetsCommand, OneAgentWorkflowsCommand, OneAgentsCommand,
    cmd::{self, RuntimeCtx},
};

const DATASETS_ENDPOINT: &str = "/ai-agents/backend/agents/ayx-datasets";
const AGENTS_ENDPOINT: &str = "/ai-agents/backend/agents";
const WORKFLOWS_ENDPOINT: &str = "/ai-agents/backend/agentyx/workflows";
const TOOLS_ENDPOINT: &str = "/ai-agents/backend/agentyx/tools";
const TOOL_CREATIONS_ENDPOINT: &str = "/ai-agents/backend/agentyx/toolCreations";
const MAX_AGENT_PROMPT_BYTES: usize = 32 * 1024;

fn response_body(envelope: &Envelope) -> &Value {
    envelope.data.get("response").unwrap_or(&Value::Null)
}

fn query<'a>(
    page: u32,
    page_size: u32,
    search_term: Option<&'a str>,
    mcp_enabled_only: bool,
    sort_field: Option<&'a str>,
    sort_order: Option<&'a str>,
) -> Vec<(&'static str, String)> {
    let mut values = vec![
        ("page", page.to_string()),
        ("pageSize", page_size.to_string()),
    ];
    if let Some(value) = search_term {
        values.push(("searchTerm", value.to_string()));
    }
    if mcp_enabled_only {
        values.push(("mcpEnabledOnly", "true".to_string()));
    }
    if let Some(value) = sort_field {
        values.push(("sortField", value.to_string()));
    }
    if let Some(value) = sort_order {
        values.push(("sortOrder", value.to_string()));
    }
    values
}

fn list_datasets(
    config: &ayx_core::profile::Config,
    page: u32,
    page_size: u32,
    search_term: Option<&str>,
    mcp_enabled_only: bool,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
) -> Result<Envelope> {
    let values = query(
        page,
        page_size,
        search_term,
        mcp_enabled_only,
        sort_field,
        sort_order,
    );
    let refs: Vec<(&str, &str)> = values
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    one_api_live_request_with_query(
        config,
        "agent-assets",
        "datasets-list",
        "GET",
        DATASETS_ENDPOINT,
        false,
        &[],
        &refs,
    )
}

fn datasets_from(envelope: &Envelope) -> Vec<Value> {
    response_body(envelope)
        .get("datasets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn find_dataset(envelope: &Envelope, id: &str) -> Option<Value> {
    datasets_from(envelope)
        .into_iter()
        .find(|dataset| dataset.get("ayxDatasetId").and_then(Value::as_str) == Some(id))
}

fn agent_query(
    page: u32,
    page_size: u32,
    search_term: Option<&str>,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut values = vec![
        ("page", page.to_string()),
        ("pageSize", page_size.to_string()),
    ];
    if let Some(value) = search_term {
        values.push(("searchTerm", value.to_string()));
    }
    if let Some(value) = sort_field {
        values.push(("sortField", value.to_string()));
    }
    if let Some(value) = sort_order {
        values.push(("sortOrder", value.to_string()));
    }
    values
}

fn agent_payload(mut payload: Value, id: Option<&str>) -> Result<Value> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("agent JSON body must be an object"))?;
    match id {
        Some(id) => {
            object.insert("id".to_string(), Value::String(id.to_string()));
        }
        None => {
            object.remove("id");
        }
    }
    Ok(payload)
}

fn prompt_payload(agent_id: &str, prompt: &str) -> Result<Value> {
    if prompt.trim().is_empty() {
        bail!("validation: --prompt must not be empty");
    }
    if prompt.len() > MAX_AGENT_PROMPT_BYTES {
        bail!("validation: --prompt must be at most {MAX_AGENT_PROMPT_BYTES} UTF-8 bytes");
    }
    Ok(json!({ "agentId": agent_id }))
}

fn chat_payload(conversation_id: &str, prompt: &str) -> Value {
    json!({
        "conversationId": conversation_id,
        "parts": [{"kind": "text", "text": prompt}],
    })
}

fn conversation_id(envelope: &Envelope) -> Option<String> {
    let response = response_body(envelope);
    response
        .get("id")
        .or_else(|| response.get("conversationId"))
        .or_else(|| response.get("conversation_id"))
        .or_else(|| {
            response
                .get("conversation")
                .and_then(|value| value.get("id"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn prompt_agent(
    config: &ayx_core::profile::Config,
    agent_id: &str,
    prompt: &str,
) -> Result<Envelope> {
    let conversation = one_api_live_request_with_body(
        config,
        "agent-assets",
        "agents-prompt-conversation",
        "POST",
        "/copilot/v2/conversations",
        true,
        &[],
        Some(prompt_payload(agent_id, prompt)?),
    )?;
    if !conversation.ok || conversation.data.get("dry_run") == Some(&Value::Bool(true)) {
        return Ok(conversation);
    }
    let Some(conversation_id) = conversation_id(&conversation) else {
        return Ok(Envelope::err_coded(
            ErrorCode::OutputClassification,
            "Agent Studio did not return a conversation id",
            json!({ "conversation": response_body(&conversation) }),
        ));
    };
    let mut chat = one_api_live_request_with_body(
        config,
        "agent-assets",
        "agents-prompt-chat",
        "POST",
        "/copilot/v2/chats",
        true,
        &[],
        Some(chat_payload(&conversation_id, prompt)),
    )?;
    if let Value::Object(data) = &mut chat.data {
        data.insert(
            "conversation_id".to_string(),
            Value::String(conversation_id),
        );
    }
    Ok(chat)
}

fn workflow_tools(
    config: &ayx_core::profile::Config,
    workflow_limit: u32,
) -> Result<(Envelope, Envelope, Envelope)> {
    let workflow_limit = workflow_limit.clamp(1, 1000).to_string();
    let workflows = one_api_live_request_with_query(
        config,
        "agent-assets",
        "workflows-list",
        "GET",
        WORKFLOWS_ENDPOINT,
        false,
        &[],
        &[("limit", workflow_limit.as_str())],
    )?;
    if !workflows.ok {
        return Ok((
            workflows,
            Envelope::ok("not requested"),
            Envelope::ok("not requested"),
        ));
    }
    let tools = one_api_live_request_with_query(
        config,
        "agent-assets",
        "tools-list",
        "GET",
        TOOLS_ENDPOINT,
        false,
        &[],
        &[("limit", "1000")],
    )?;
    let creations = one_api_live_request_with_query(
        config,
        "agent-assets",
        "tool-creations-list",
        "GET",
        TOOL_CREATIONS_ENDPOINT,
        false,
        &[],
        &[("showArchived", "false")],
    )?;
    Ok((workflows, tools, creations))
}

fn creation_status(envelope: &Envelope) -> Option<&str> {
    response_body(envelope)
        .get("status")
        .and_then(Value::as_str)
}

fn poll_tool_creation(
    config: &ayx_core::profile::Config,
    job_id: &str,
    timeout_seconds: u64,
) -> Result<Envelope> {
    let started = Instant::now();
    let timeout = Duration::from_secs(timeout_seconds);
    loop {
        let status = one_api_live_request(
            config,
            "agent-assets",
            "tool-creation-status",
            "GET",
            "/ai-agents/backend/agentyx/toolCreations/{id}",
            false,
            &[("id", job_id)],
        )?;
        if !status.ok {
            return Ok(status);
        }
        match creation_status(&status) {
            Some("COMPLETE") => return Ok(status),
            Some("FAILED" | "CANCELED" | "SKIPPED" | "UNKNOWN") => return Ok(status),
            _ if started.elapsed() >= timeout => {
                return Ok(Envelope::err_coded(
                    ErrorCode::Incomplete,
                    format!(
                        "Agent Studio tool creation did not complete within {timeout_seconds}s"
                    ),
                    json!({
                        "surface": "agent-assets",
                        "operation": "tool-creation-status",
                        "job_id": job_id,
                        "timeout_seconds": timeout_seconds,
                        "last_status": response_body(&status),
                    }),
                ));
            }
            _ => thread::sleep(Duration::from_millis(1500)),
        }
    }
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OneAgentAssetsCommand,
) -> Result<Envelope> {
    match command {
        OneAgentAssetsCommand::Agents { command } => match command {
            OneAgentsCommand::List {
                profile,
                page,
                page_size,
                search_term,
                sort_field,
                sort_order,
            } => {
                if let Some(order) = sort_order.as_deref()
                    && !matches!(order, "ASCENDING" | "DESCENDING")
                {
                    bail!("validation: --sort-order must be ASCENDING or DESCENDING");
                }
                if let Some(field) = sort_field.as_deref()
                    && !matches!(field, "description" | "lastUpdatedAt" | "name")
                {
                    bail!("validation: --sort-field must be description, lastUpdatedAt, or name");
                }
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let values = agent_query(
                    page,
                    page_size,
                    search_term.as_deref(),
                    sort_field.as_deref(),
                    sort_order.as_deref(),
                );
                let refs: Vec<(&str, &str)> = values
                    .iter()
                    .map(|(key, value)| (*key, value.as_str()))
                    .collect();
                Ok(one_api_live_request_with_query(
                    &config,
                    "agent-assets",
                    "agents-list",
                    "GET",
                    AGENTS_ENDPOINT,
                    false,
                    &[],
                    &refs,
                )?)
            }
            OneAgentsCommand::Detail { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                Ok(one_api_live_request(
                    &config,
                    "agent-assets",
                    "agents-detail",
                    "GET",
                    "/ai-agents/backend/agents/{id}",
                    false,
                    &[("id", id.as_str())],
                )?)
            }
            OneAgentsCommand::Prompt {
                profile,
                id,
                prompt,
            } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "submit a prompt to",
                            &format!("Agent Studio agent '{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(prompt_agent(&config, &id, &prompt)?)
            }
            OneAgentsCommand::Create { profile, body } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = agent_payload(crate::load_payload(&body)?, None)?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "create",
                            "Agent Studio agent",
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(one_api_live_request_with_body(
                    &config,
                    "agent-assets",
                    "agents-create",
                    "POST",
                    AGENTS_ENDPOINT,
                    true,
                    &[],
                    Some(payload),
                )?)
            }
            OneAgentsCommand::Update { profile, id, body } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = agent_payload(crate::load_payload(&body)?, Some(&id))?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "update",
                            &format!("Agent Studio agent '{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(one_api_live_request_with_body(
                    &config,
                    "agent-assets",
                    "agents-update",
                    "PATCH",
                    "/ai-agents/backend/agents/{id}",
                    true,
                    &[("id", id.as_str())],
                    Some(payload),
                )?)
            }
            OneAgentsCommand::Delete { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "delete",
                            &format!("Agent Studio agent '{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(one_api_live_request(
                    &config,
                    "agent-assets",
                    "agents-delete",
                    "DELETE",
                    "/ai-agents/backend/agents/{id}",
                    true,
                    &[("id", id.as_str())],
                )?)
            }
        },
        OneAgentAssetsCommand::Datasets { command } => match command {
            OneAgentDatasetsCommand::List {
                profile,
                page,
                page_size,
                search_term,
                mcp_enabled_only,
                sort_field,
                sort_order,
            } => {
                if let Some(order) = sort_order.as_deref()
                    && !matches!(order, "ASCENDING" | "DESCENDING")
                {
                    bail!("validation: --sort-order must be ASCENDING or DESCENDING");
                }
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                list_datasets(
                    &config,
                    page,
                    page_size,
                    search_term.as_deref(),
                    mcp_enabled_only,
                    sort_field.as_deref(),
                    sort_order.as_deref(),
                )
            }
            OneAgentDatasetsCommand::Set {
                profile,
                id,
                enable,
                disable,
            } => {
                if enable == disable {
                    bail!("validation: pass exactly one of --enable or --disable");
                }
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let listing =
                    list_datasets(&config, 0, 1000, Some(id.as_str()), false, None, None)?;
                if !listing.ok {
                    return Ok(listing);
                }
                let Some(dataset) = find_dataset(&listing, &id) else {
                    return Ok(Envelope::err_coded(
                        ErrorCode::NotFound,
                        format!("Agent Studio dataset not found: {id}"),
                        json!({
                            "surface": "agent-assets",
                            "operation": "datasets-set",
                            "dataset_id": id,
                        }),
                    ));
                };
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            if enable { "enable" } else { "disable" },
                            &format!("Agent Studio Insights for dataset '{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(one_api_live_request_with_body(
                    &config,
                    "agent-assets",
                    "datasets-set",
                    "PATCH",
                    "/ai-agents/backend/agents/ayx-datasets/{id}/mcp-enabled",
                    true,
                    &[("id", id.as_str())],
                    Some(json!({ "dataset": dataset, "isMcpEnabled": enable })),
                )?)
            }
        },
        OneAgentAssetsCommand::Workflows { command } => match command {
            OneAgentWorkflowsCommand::List { profile, limit } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let (workflows, tools, creations) = workflow_tools(&config, limit)?;
                if !workflows.ok {
                    return Ok(workflows);
                }
                let mut data = workflows.data;
                data["tools"] = response_body(&tools).clone();
                data["tool_creations"] = response_body(&creations).clone();
                data["requested_limit"] = json!(limit);
                Ok(Envelope::ok_with_data(
                    "Agent Studio workflow shortcuts listed",
                    data,
                ))
            }
            OneAgentWorkflowsCommand::Enable {
                profile,
                id,
                timeout_seconds,
            } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "register",
                            &format!("workflow '{id}' as an Agent Studio Apps shortcut"),
                            &config.profile_name,
                        ),
                    )?;
                }
                let creation = one_api_live_request_with_body(
                    &config,
                    "agent-assets",
                    "workflows-enable",
                    "POST",
                    TOOL_CREATIONS_ENDPOINT,
                    true,
                    &[],
                    Some(json!({ "workflowId": id })),
                )?;
                if !creation.ok || creation.data.get("dry_run") == Some(&Value::Bool(true)) {
                    return Ok(creation);
                }
                let Some(job_id) = response_body(&creation).get("id").and_then(Value::as_str)
                else {
                    return Ok(Envelope::err_coded(
                        ErrorCode::OutputClassification,
                        "Agent Studio did not return a tool creation job id",
                        json!({ "creation": response_body(&creation) }),
                    ));
                };
                if timeout_seconds == 0 {
                    bail!("validation: --timeout-seconds must be greater than zero");
                }
                let status = poll_tool_creation(&config, job_id, timeout_seconds)?;
                if !status.ok {
                    return Ok(status);
                }
                let mut data = creation.data;
                data["tool_creation"] = response_body(&status).clone();
                Ok(Envelope::ok_with_data(
                    "Agent Studio workflow shortcut registered",
                    data,
                ))
            }
            OneAgentWorkflowsCommand::Disable { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let tools = one_api_live_request_with_query(
                    &config,
                    "agent-assets",
                    "tools-list",
                    "GET",
                    TOOLS_ENDPOINT,
                    false,
                    &[],
                    &[("limit", "1000")],
                )?;
                if !tools.ok {
                    return Ok(tools);
                }
                let tool_id = response_body(&tools)
                    .get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|tool| tool.get("workflowId").and_then(Value::as_str) == Some(&id))
                    .and_then(|tool| tool.get("id").and_then(Value::as_str))
                    .map(str::to_string);
                let Some(tool_id) = tool_id else {
                    return Ok(Envelope::err_coded(
                        ErrorCode::NotFound,
                        format!("no Agent Studio tool found for workflow {id}"),
                        json!({
                            "surface": "agent-assets",
                            "operation": "workflows-disable",
                            "workflow_id": id,
                        }),
                    ));
                };
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "remove",
                            &format!("Agent Studio Apps shortcut for workflow '{id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                Ok(one_api_live_request(
                    &config,
                    "agent-assets",
                    "workflows-disable",
                    "DELETE",
                    "/ai-agents/backend/agentyx/tools/{id}",
                    true,
                    &[("id", tool_id.as_str())],
                )?)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{agent_payload, chat_payload, datasets_from, find_dataset, prompt_payload, query};
    use ayx_core::envelope::Envelope;
    use serde_json::json;

    #[test]
    fn dataset_query_matches_agent_studio_wire_names() {
        assert_eq!(
            query(2, 50, Some("sales"), true, Some("name"), Some("DESCENDING")),
            vec![
                ("page", "2".to_string()),
                ("pageSize", "50".to_string()),
                ("searchTerm", "sales".to_string()),
                ("mcpEnabledOnly", "true".to_string()),
                ("sortField", "name".to_string()),
                ("sortOrder", "DESCENDING".to_string()),
            ]
        );
    }

    #[test]
    fn dataset_lookup_uses_ayx_dataset_id() {
        let envelope = Envelope::ok_with_data(
            "fixture",
            json!({"response": {"datasets": [{"ayxDatasetId": "42", "name": "demo"}]}}),
        );
        assert_eq!(datasets_from(&envelope).len(), 1);
        assert_eq!(find_dataset(&envelope, "42").unwrap()["name"], "demo");
        assert!(find_dataset(&envelope, "99").is_none());
    }

    #[test]
    fn agent_payload_strips_create_id_and_binds_update_id() {
        let payload = agent_payload(json!({"id": "wrong", "name": "demo"}), None).unwrap();
        assert_eq!(payload, json!({"name": "demo"}));

        let payload = agent_payload(json!({"name": "demo"}), Some("agent-42")).unwrap();
        assert_eq!(payload["id"], "agent-42");
    }

    #[test]
    fn agent_payload_rejects_non_object_bodies() {
        let error = agent_payload(json!(["not", "an", "agent"]), None).unwrap_err();
        assert!(error.to_string().contains("must be an object"));
    }

    #[test]
    fn prompt_payload_and_chat_payload_match_copilot_wire_shape() {
        assert_eq!(
            prompt_payload("agent-42", "Summarize the dataset").unwrap(),
            json!({"agentId": "agent-42"})
        );
        assert_eq!(
            chat_payload("conversation-7", "Summarize the dataset"),
            json!({
                "conversationId": "conversation-7",
                "parts": [{"kind": "text", "text": "Summarize the dataset"}]
            })
        );
        assert!(prompt_payload("agent-42", "  ").is_err());
        assert!(prompt_payload("agent-42", &"x".repeat(32 * 1024 + 1)).is_err());
    }
}
