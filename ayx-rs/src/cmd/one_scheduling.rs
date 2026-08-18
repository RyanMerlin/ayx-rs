use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneSchedulingCommand, cmd, cmd::RuntimeCtx, load_payload};

fn confirm_schedule_mutation(apply: bool, yes: bool, message: &str) -> Result<()> {
    if apply {
        cmd::confirm::require_tty_confirmation(yes, message)?;
    }
    Ok(())
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OneSchedulingCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneSchedulingCommand::List {
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
                "scheduling",
                "list",
                "/v4/schedules",
                &[],
                &params,
            )?
        }
        OneSchedulingCommand::Create { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to CREATE a schedule from '{}' on profile '{}'. Review the task and trigger before proceeding.",
                    body.display(),
                    config.profile_name
                ),
            )?;
            one_api_live_request_with_body(
                &config,
                "scheduling",
                "create",
                "POST",
                "/v4/schedules",
                true,
                &[],
                Some(payload),
            )?
        }
        OneSchedulingCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "scheduling",
                "detail",
                "GET",
                "/v4/schedules/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneSchedulingCommand::Update { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to UPDATE schedule id='{id}' on profile '{}'. Review the task and trigger before proceeding.",
                    config.profile_name
                ),
            )?;
            one_api_live_request_with_body(
                &config,
                "scheduling",
                "update",
                "PUT",
                "/v4/schedules/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneSchedulingCommand::Enable { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to ENABLE schedule id='{id}' on profile '{}'. This may activate a live workflow schedule.",
                    config.profile_name
                ),
            )?;
            one_api_live_request(
                &config,
                "scheduling",
                "enable",
                "POST",
                "/v4/schedules/{id}/enable",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneSchedulingCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to DELETE schedule id='{id}' on profile '{}'. This cannot be undone.",
                    config.profile_name
                ),
            )?;
            one_api_live_request(
                &config,
                "scheduling",
                "delete",
                "DELETE",
                "/v4/schedules/{id}",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneSchedulingCommand::Disable { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to DISABLE schedule id='{id}' on profile '{}'.",
                    config.profile_name
                ),
            )?;
            one_api_live_request(
                &config,
                "scheduling",
                "disable",
                "POST",
                "/v4/schedules/{id}/disable",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneSchedulingCommand::Count { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "scheduling",
                "count",
                "GET",
                "/v4/schedules/count",
                false,
                &[],
            )?
        }
    })
}

#[cfg(test)]
mod tests {
    use std::io::IsTerminal;

    use super::confirm_schedule_mutation;

    #[test]
    fn dry_run_never_requires_schedule_confirmation() {
        confirm_schedule_mutation(false, false, "dry-run schedule mutation")
            .expect("dry run should not prompt");
    }

    #[test]
    fn yes_allows_applied_schedule_confirmation() {
        confirm_schedule_mutation(true, true, "applied schedule mutation")
            .expect("--yes should bypass the prompt");
    }

    #[test]
    fn non_tty_applied_schedule_mutation_requires_yes() {
        if !std::io::stdin().is_terminal() {
            let error = confirm_schedule_mutation(true, false, "applied schedule mutation")
                .expect_err("applied non-TTY mutation must require --yes");
            assert!(error.to_string().contains("--yes"));
        }
    }
}
