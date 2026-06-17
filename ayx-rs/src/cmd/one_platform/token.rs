use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OnePlatformTokenCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OnePlatformTokenCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None | Some(OnePlatformTokenCommand::List) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "api-access-tokens-list",
                "GET",
                "/v4/apiAccessTokens",
                false,
                &[],
            )?
        }
        Some(OnePlatformTokenCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
                "api-access-tokens-create",
                "POST",
                "/v4/apiAccessTokens",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePlatformTokenCommand::Detail { profile, token_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "platform",
                "api-access-tokens-detail",
                "GET",
                "/v4/apiAccessTokens/{tokenId}",
                false,
                &[("tokenId", &token_id)],
            )?
        }
        Some(OnePlatformTokenCommand::Delete { profile, token_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "platform",
                "api-access-tokens-delete",
                "DELETE",
                "/v4/apiAccessTokens/{tokenId}",
                true,
                &[("tokenId", &token_id)],
            )?
        }
    })
}
