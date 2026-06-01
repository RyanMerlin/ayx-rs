use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::{api_diagnose_envelope, api_status_envelope};
use ayx_one_api::one_api_live_request;

use crate::{cmd::RuntimeCtx, OnePlatformApiCommand};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OnePlatformApiCommand,
) -> Result<Envelope> {
    Ok(match command {
        OnePlatformApiCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            api_status_envelope(&config, "one platform")?
        }
        OnePlatformApiCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            api_diagnose_envelope(&config, "one platform")?
        }
        OnePlatformApiCommand::OpenApiSpec { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "platform",
                "open-api-spec",
                "GET",
                "/v4/open-api-spec",
                false,
                &[],
            )?
        }
    })
}
