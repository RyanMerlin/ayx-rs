use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;

use crate::{cmd::RuntimeCtx, OneSchedulingCommand};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneSchedulingCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => {
            Envelope::ok("one scheduling commands available: list, detail, enable, disable, count")
        }
        Some(OneSchedulingCommand::List {
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
                "scheduling",
                "list",
                "/scheduling/v1/schedules",
                &[],
                &params,
            )?
        }
        Some(OneSchedulingCommand::Detail {
            profile,
            schedule_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneSchedulingCommand::Enable {
            profile,
            schedule_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        Some(OneSchedulingCommand::Disable {
            profile,
            schedule_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "scheduling",
                "count",
                "GET",
                "/scheduling/v1/schedules/count",
                false,
                &[],
            )?
        }
    })
}
