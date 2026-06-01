use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{cmd::RuntimeCtx, load_payload, OneWriteSettingCommand};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneWriteSettingCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one write-setting commands available: list, count, create, detail, update, delete",
        ),
        Some(OneWriteSettingCommand::List {
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
                "writeSetting",
                "list",
                "/v4/writeSettings",
                &[],
                &params,
            )?
        }
        Some(OneWriteSettingCommand::Count { profile }) => {
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
        Some(OneWriteSettingCommand::Create { profile, body }) => {
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
        Some(OneWriteSettingCommand::Detail {
            profile,
            write_setting_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let write_setting_id =
                write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
            one_api_live_request(
                &config,
                "writeSetting",
                "detail",
                "GET",
                "/v4/writeSettings/{id}",
                false,
                &[("id", write_setting_id.as_str())],
            )?
        }
        Some(OneWriteSettingCommand::Update {
            profile,
            write_setting_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let write_setting_id =
                write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "writeSetting",
                "update",
                "PATCH",
                "/v4/writeSettings/{id}",
                true,
                &[("id", write_setting_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneWriteSettingCommand::Delete {
            profile,
            write_setting_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let write_setting_id =
                write_setting_id.ok_or_else(|| anyhow!("--write-setting-id is required"))?;
            one_api_live_request(
                &config,
                "writeSetting",
                "delete",
                "DELETE",
                "/v4/writeSettings/{id}",
                true,
                &[("id", write_setting_id.as_str())],
            )?
        }
    })
}
