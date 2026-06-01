use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    cmd::{self, RuntimeCtx},
    load_payload, OnePlansCommand,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OnePlansCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one plans commands available: list, create, detail, full, run, count, run-parameters, schedules, export, update, delete, share, import, permissions",
        ),
        Some(OnePlansCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = ayx_one_api::OneListParams::new()
                .with_limit(limit)
                .with_page_token(page_token)
                .with_all(all, max_pages);
            ayx_one_api::one_api_list_request(
                &config,
                "plans",
                "list",
                "/plans/v1/plans",
                &[],
                &params,
            )?
        }
        Some(OnePlansCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "plans",
                "create",
                "POST",
                "/v4/plans",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePlansCommand::Detail { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "detail",
                "GET",
                "/plans/v1/plans/{id}",
                false,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Full { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "full",
                "GET",
                "/v4/plans/{id}/full",
                false,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Run { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "run",
                "POST",
                "/plans/v1/plans/{id}/run",
                true,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "count",
                "GET",
                "/plans/v1/plans/count",
                false,
                &[],
            )?
        }
        Some(OnePlansCommand::RunParameters { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "run-parameters",
                "GET",
                "/plans/v1/plans/{id}/runParameters",
                false,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Schedules { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "schedules",
                "GET",
                "/plans/v1/plans/{id}/schedules",
                false,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Export { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            one_api_live_request(
                &config,
                "plans",
                "export",
                "GET",
                "/plans/v1/plans/{id}/package",
                false,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Update {
            profile,
            plan_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "plans",
                "update",
                "PATCH",
                "/v4/plans/{id}",
                true,
                &[("id", plan_id.as_str())],
                Some(payload),
            )?
        }
        Some(OnePlansCommand::Delete { profile, plan_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &format!(
                        "About to DELETE plan id='{plan_id}' on profile '{}'. This cannot be undone.",
                        config.profile_name
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "plans",
                "delete",
                "DELETE",
                "/v4/plans/{id}",
                true,
                &[("id", plan_id.as_str())],
            )?
        }
        Some(OnePlansCommand::Share {
            profile,
            plan_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "plans",
                "share",
                "POST",
                "/v4/plans/{id}/permissions",
                true,
                &[("id", plan_id.as_str())],
                Some(payload),
            )?
        }
        Some(OnePlansCommand::Import { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "import",
                "POST",
                "/plans/v1/plans/package",
                true,
                &[],
            )?
        }
        Some(OnePlansCommand::Permissions {
            profile,
            plan_id,
            subject_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let plan_id = plan_id.ok_or_else(|| anyhow!("--plan-id is required"))?;
            let subject_id = subject_id.unwrap_or_default();
            if subject_id.is_empty() {
                one_api_live_request(
                    &config,
                    "plans",
                    "permissions",
                    "GET",
                    "/plans/v1/plans/{id}/permissions",
                    false,
                    &[("id", plan_id.as_str())],
                )?
            } else {
                one_api_live_request(
                    &config,
                    "plans",
                    "permissions",
                    "DELETE",
                    "/plans/v1/plans/{id}/permissions/{subjectId}",
                    true,
                    &[("id", plan_id.as_str()), ("subjectId", subject_id.as_str())],
                )?
            }
        }
    })
}
