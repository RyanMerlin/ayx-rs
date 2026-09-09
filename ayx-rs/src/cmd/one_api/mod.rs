//! Alteryx One API introspection commands (`one api`). One only.
use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_diagnose_envelope, one_api_live_request, one_api_status_envelope};

use crate::{OneApiCommand, cmd::RuntimeCtx};

pub(crate) mod coverage;

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneApiCommand) -> Result<Envelope> {
    Ok(match command {
        OneApiCommand::Status { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_status_envelope(&config)?
        }
        OneApiCommand::Diagnose { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_diagnose_envelope(&config)?
        }
        OneApiCommand::OpenApiSpec { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "api",
                "open-api-spec",
                "GET",
                "/v4/open-api-spec",
                false,
                &[],
            )?
        }
        OneApiCommand::Coverage {
            profile,
            spec,
            check,
        } => coverage::execute(runtime, profile, spec, check)?,
    })
}
