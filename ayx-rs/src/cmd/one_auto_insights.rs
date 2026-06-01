use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one::api_diagnose_envelope;

use crate::cmd::RuntimeCtx;

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, profile: Option<String>) -> Result<Envelope> {
    let config = runtime.load_profile_lenient(profile.as_deref())?;
    api_diagnose_envelope(&config, "one auto-insights")
}
