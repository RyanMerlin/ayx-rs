use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{
    cmd::RuntimeCtx, one_platform_auth_diagnose_envelope, one_platform_auth_status_envelope,
    OnePlatformAuthCommand,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OnePlatformAuthCommand,
) -> Result<Envelope> {
    Ok(match command {
        OnePlatformAuthCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_status_envelope(&config)?
        }
        OnePlatformAuthCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_diagnose_envelope(&config)?
        }
    })
}
