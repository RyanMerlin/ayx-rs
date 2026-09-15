use anyhow::{Result, bail};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};
use chrono::DateTime;
use serde_json::{Value, json};

use crate::{
    OneSchedulingCommand, ScheduleTargetKind, ScheduleTriggerKind, cmd, cmd::RuntimeCtx,
    load_payload,
};

#[allow(clippy::too_many_arguments)]
fn typed_schedule_payload(
    target_kind: Option<ScheduleTargetKind>,
    target_id: Option<String>,
    trigger_kind: Option<ScheduleTriggerKind>,
    name: Option<String>,
    timezone: Option<String>,
    hour: Option<u8>,
    minute: Option<u8>,
    weekday: Vec<u8>,
    day_of_month: Option<u8>,
    at: Option<String>,
    body: Option<&std::path::Path>,
) -> Result<(Value, String)> {
    if let Some(body) = body {
        if target_kind.is_some()
            || target_id.is_some()
            || trigger_kind.is_some()
            || name.is_some()
            || timezone.is_some()
            || hour.is_some()
            || minute.is_some()
            || !weekday.is_empty()
            || day_of_month.is_some()
            || at.is_some()
        {
            bail!("validation: --body cannot be combined with typed scheduling arguments")
        }
        return Ok((load_payload(body)?, "raw".to_string()));
    }

    let target_kind = target_kind.ok_or_else(|| {
        anyhow::anyhow!(
            "validation: provide --body <FILE|JSON|-> or TARGET-KIND TARGET-ID TRIGGER-KIND"
        )
    })?;
    let target_id =
        target_id.ok_or_else(|| anyhow::anyhow!("validation: TARGET-ID is required"))?;
    if target_id.trim().is_empty() {
        bail!("validation: TARGET-ID must not be empty")
    }
    let trigger_kind =
        trigger_kind.ok_or_else(|| anyhow::anyhow!("validation: TRIGGER-KIND is required"))?;
    if !matches!(target_kind, ScheduleTargetKind::Workflow) {
        bail!(
            "validation: typed target '{target_kind:?}' is not provider-verified; use --body <FILE|JSON|-> until a live canary verifies it"
        );
    }
    if !matches!(trigger_kind, ScheduleTriggerKind::Daily) {
        bail!(
            "validation: typed trigger '{trigger_kind:?}' is not provider-verified; use --body <FILE|JSON|-> until a live canary verifies it"
        );
    }
    let name =
        name.ok_or_else(|| anyhow::anyhow!("validation: typed scheduling requires --name"))?;
    if name.trim().is_empty() {
        bail!("validation: --name must not be empty")
    }
    let timezone = timezone.unwrap_or_else(|| "UTC".to_string());
    timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| anyhow::anyhow!("validation: --timezone must be a valid IANA time zone"))?;

    let task = match target_kind {
        ScheduleTargetKind::Workflow => json!({"runWorkflow": {"workflowId": target_id}}),
        ScheduleTargetKind::Flow => json!({"runFlow": {"flowId": target_id}}),
        ScheduleTargetKind::Plan => json!({"runPlan": {"planId": target_id}}),
        ScheduleTargetKind::JobGroup => json!({"runJobGroup": {"jobGroupId": target_id}}),
    };
    let hour = hour.unwrap_or(0);
    let minute = minute.unwrap_or(0);
    if hour > 23 {
        bail!("validation: --hour must be between 0 and 23")
    }
    if minute > 59 {
        bail!("validation: --minute must be between 0 and 59")
    }
    let trigger = match trigger_kind {
        ScheduleTriggerKind::Daily => json!({
            "timeBased": {
                "daily": {"hourOfDay": hour, "minuteOfHour": minute},
                "timezone": timezone
            }
        }),
        ScheduleTriggerKind::Weekly => {
            if weekday.is_empty() || weekday.iter().any(|day| !(1..=7).contains(day)) {
                bail!("validation: weekly schedules require --weekday values from 1 through 7")
            }
            json!({"timeBased": {"weekly": {"daysOfWeek": weekday, "hourOfDay": hour, "minuteOfHour": minute}, "timezone": timezone}})
        }
        ScheduleTriggerKind::Monthly => {
            let day = day_of_month.ok_or_else(|| {
                anyhow::anyhow!("validation: monthly schedules require --day-of-month")
            })?;
            if !(1..=31).contains(&day) {
                bail!("validation: --day-of-month must be between 1 and 31")
            }
            json!({"timeBased": {"monthly": {"dayOfMonth": day, "hourOfDay": hour, "minuteOfHour": minute}, "timezone": timezone}})
        }
        ScheduleTriggerKind::OneTime => {
            let at = at.ok_or_else(|| {
                anyhow::anyhow!("validation: one-time schedules require --at <RFC3339>")
            })?;
            DateTime::parse_from_rfc3339(&at).map_err(|_| {
                anyhow::anyhow!("validation: --at must be a valid RFC3339 timestamp")
            })?;
            json!({"timeBased": {"oneTime": {"dateTime": at}, "timezone": timezone}})
        }
    };
    Ok((
        json!({"name": name, "tasks": [task], "triggers": [trigger]}),
        "typed".to_string(),
    ))
}

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
                .with_page_size(runtime.page_size)
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
        OneSchedulingCommand::Create {
            profile,
            target_kind,
            target_id,
            trigger_kind,
            name,
            timezone,
            hour,
            minute,
            weekday,
            day_of_month,
            at,
            body,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let (payload, source) = typed_schedule_payload(
                target_kind,
                target_id,
                trigger_kind,
                name,
                timezone,
                hour,
                minute,
                weekday,
                day_of_month,
                at,
                body.as_deref(),
            )?;
            confirm_schedule_mutation(
                apply,
                yes,
                &format!(
                    "About to CREATE a {source} schedule on profile '{}'. Review the task and trigger before proceeding.",
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

    use super::{confirm_schedule_mutation, typed_schedule_payload};
    use crate::{ScheduleTargetKind, ScheduleTriggerKind};

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

    #[test]
    fn typed_schedule_payload_supports_verified_workflow_daily_form() {
        let (payload, source) = typed_schedule_payload(
            Some(ScheduleTargetKind::Workflow),
            Some("target-1".to_string()),
            Some(ScheduleTriggerKind::Daily),
            Some("Morning run".to_string()),
            Some("America/Denver".to_string()),
            Some(6),
            Some(30),
            vec![],
            None,
            None,
            None,
        )
        .expect("typed schedule should validate");
        assert_eq!(source, "typed");
        assert_eq!(payload["tasks"][0]["runWorkflow"]["workflowId"], "target-1");
        assert_eq!(payload["triggers"][0]["timeBased"]["daily"]["hourOfDay"], 6);
    }

    #[test]
    fn typed_schedule_payload_rejects_unverified_targets_and_triggers() {
        let flow = typed_schedule_payload(
            Some(ScheduleTargetKind::Flow),
            Some("f".to_string()),
            Some(ScheduleTriggerKind::Daily),
            Some("Flow".to_string()),
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
        )
        .expect_err("flow target must remain raw-body only");
        assert!(flow.to_string().contains("--body"));

        let weekly = typed_schedule_payload(
            Some(ScheduleTargetKind::Workflow),
            Some("w".to_string()),
            Some(ScheduleTriggerKind::Weekly),
            Some("Weekly".to_string()),
            None,
            None,
            None,
            vec![1, 5],
            None,
            None,
            None,
        )
        .expect_err("weekly trigger must remain raw-body only");
        assert!(weekly.to_string().contains("--body"));
    }

    #[test]
    fn typed_schedule_payload_rejects_invalid_temporal_values() {
        let invalid_timezone = typed_schedule_payload(
            Some(ScheduleTargetKind::Workflow),
            Some("w".to_string()),
            Some(ScheduleTriggerKind::Daily),
            Some("Bad timezone".to_string()),
            Some("Not/AZone".to_string()),
            None,
            None,
            vec![],
            None,
            None,
            None,
        )
        .expect_err("invalid timezone must fail");
        assert!(invalid_timezone.to_string().contains("IANA"));

        let invalid_hour = typed_schedule_payload(
            Some(ScheduleTargetKind::Workflow),
            Some("w".to_string()),
            Some(ScheduleTriggerKind::Daily),
            Some("Bad hour".to_string()),
            None,
            Some(24),
            None,
            vec![],
            None,
            None,
            None,
        )
        .expect_err("invalid hour must fail");
        assert!(invalid_hour.to_string().contains("--hour"));
    }
}
