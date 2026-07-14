use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OnePlansCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OnePlansCommand,
) -> Result<Envelope> {
    Ok(match command {
        OnePlansCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        } => {
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
        OnePlansCommand::Create { profile, body } => {
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
        OnePlansCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "detail",
                "GET",
                "/plans/v1/plans/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Full { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "full",
                "GET",
                "/v4/plans/{id}/full",
                false,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Run { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "run",
                "POST",
                "/plans/v1/plans/{id}/run",
                true,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Count { profile } => {
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
        OnePlansCommand::RunParameters { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "run-parameters",
                "GET",
                "/plans/v1/plans/{id}/runParameters",
                false,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Schedules { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "schedules",
                "GET",
                "/plans/v1/plans/{id}/schedules",
                false,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Export { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "plans",
                "export",
                "GET",
                "/plans/v1/plans/{id}/package",
                false,
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Update { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "plans",
                "update",
                "PATCH",
                "/v4/plans/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OnePlansCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &format!(
                        "About to DELETE plan id='{id}' on profile '{}'. This cannot be undone.",
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
                &[("id", id.as_str())],
            )?
        }
        OnePlansCommand::Share { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "plans",
                "share",
                "POST",
                "/v4/plans/{id}/permissions",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OnePlansCommand::Import { profile } => {
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
        OnePlansCommand::Permissions {
            profile,
            id,
            subject_id,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let subject_id = subject_id.unwrap_or_default();
            if subject_id.is_empty() {
                one_api_live_request(
                    &config,
                    "plans",
                    "permissions",
                    "GET",
                    "/plans/v1/plans/{id}/permissions",
                    false,
                    &[("id", id.as_str())],
                )?
            } else {
                one_api_live_request(
                    &config,
                    "plans",
                    "permissions",
                    "DELETE",
                    "/plans/v1/plans/{id}/permissions/{subjectId}",
                    true,
                    &[("id", id.as_str()), ("subjectId", subject_id.as_str())],
                )?
            }
        }
    })
}
