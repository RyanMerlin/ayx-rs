use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;

use crate::{OneBillingCommand, cmd::RuntimeCtx};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneBillingCommand) -> Result<Envelope> {
    Ok(match command {
        OneBillingCommand::CurrentAccount { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "billing",
                "current-account",
                "GET",
                "/billing/v1/my/billing-accounts/current",
                false,
                &[],
            )?
        }
        OneBillingCommand::UsageExport { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "billing",
                "usage-export",
                "GET",
                "/billing/v1/usage/export",
                false,
                &[],
            )?
        }
    })
}
