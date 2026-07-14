use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OneTokenCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OneTokenCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None | Some(OneTokenCommand::List) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "token",
                "api-access-tokens-list",
                "GET",
                "/v4/apiAccessTokens",
                false,
                &[],
            )?
        }
        Some(OneTokenCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "token",
                "api-access-tokens-create",
                "POST",
                "/v4/apiAccessTokens",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OneTokenCommand::Detail { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "token",
                "api-access-tokens-detail",
                "GET",
                "/v4/apiAccessTokens/{tokenId}",
                false,
                &[("tokenId", &id)],
            )?
        }
        Some(OneTokenCommand::Delete { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "delete",
                        &format!("token id='{id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "token",
                "api-access-tokens-delete",
                "DELETE",
                "/v4/apiAccessTokens/{tokenId}",
                true,
                &[("tokenId", &id)],
            )?
        }
    })
}
