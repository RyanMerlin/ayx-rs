use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{
    OneDoctorCommand, cmd::RuntimeCtx, one_doctor_discover_envelope, one_doctor_identity_envelope,
    one_doctor_plans_envelope, one_doctor_scheduling_envelope, one_platform_auth_diagnose_envelope,
};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneDoctorCommand) -> Result<Envelope> {
    Ok(match command {
        OneDoctorCommand::Auth { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_diagnose_envelope(&config)?
        }
        OneDoctorCommand::Discover { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_discover_envelope(&config)?
        }
        OneDoctorCommand::Identity { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_identity_envelope(&config)?
        }
        OneDoctorCommand::Plans { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_plans_envelope(&config)?
        }
        OneDoctorCommand::Scheduling { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_scheduling_envelope(&config)?
        }
    })
}
