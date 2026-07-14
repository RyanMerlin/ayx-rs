use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneWriteSettingCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OneWriteSettingCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneWriteSettingCommand::List {
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
                "writeSetting",
                "list",
                "/v4/writeSettings",
                &[],
                &params,
            )?
        }
        OneWriteSettingCommand::Count { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        OneWriteSettingCommand::Create { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
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
        OneWriteSettingCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "writeSetting",
                "detail",
                "GET",
                "/v4/writeSettings/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneWriteSettingCommand::Update { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "writeSetting",
                "update",
                "PATCH",
                "/v4/writeSettings/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneWriteSettingCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "writeSetting",
                "delete",
                "DELETE",
                "/v4/writeSettings/{id}",
                true,
                &[("id", id.as_str())],
            )?
        }
    })
}
