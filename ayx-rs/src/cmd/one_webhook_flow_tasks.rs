use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{cmd::RuntimeCtx, load_payload, OneWebhookFlowTaskCommand};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneWebhookFlowTaskCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => {
            Envelope::ok("one webhook-flow-task commands available: create, detail, delete, test")
        }
        Some(OneWebhookFlowTaskCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let webhook_flow_task_id = webhook_flow_task_id
                .ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
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
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let webhook_flow_task_id = webhook_flow_task_id
                .ok_or_else(|| anyhow!("--webhook-flow-task-id is required"))?;
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
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
    })
}
