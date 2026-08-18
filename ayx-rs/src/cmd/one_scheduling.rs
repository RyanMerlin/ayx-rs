use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;

use crate::{OneSchedulingCommand, cmd::RuntimeCtx};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneSchedulingCommand) -> Result<Envelope> {
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
        OneSchedulingCommand::Enable { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        OneSchedulingCommand::Disable { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
