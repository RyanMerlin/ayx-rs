use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneWebhookFlowTaskCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OneWebhookFlowTaskCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneWebhookFlowTaskCommand::Create { profile, body } => {
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
        OneWebhookFlowTaskCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "webhookFlowTask",
                "detail",
                "GET",
                "/v4/webhookFlowTasks/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneWebhookFlowTaskCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "webhookFlowTask",
                "delete",
                "DELETE",
                "/v4/webhookFlowTasks/{id}",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneWebhookFlowTaskCommand::Test { profile, body } => {
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
