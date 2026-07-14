use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{
    OneDoctorCommand, cmd::RuntimeCtx, one_doctor_billing_envelope, one_doctor_discover_envelope,
    one_doctor_identity_envelope, one_doctor_plans_envelope, one_doctor_scheduling_envelope,
    one_platform_auth_diagnose_envelope,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneDoctorCommand>,
) -> Result<Envelope> {
    Ok(match command {
        Some(OneDoctorCommand::Auth { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_platform_auth_diagnose_envelope(&config)?
        }
        Some(OneDoctorCommand::Discover { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_discover_envelope(&config)?
        }
        Some(OneDoctorCommand::Identity { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_identity_envelope(&config)?
        }
        Some(OneDoctorCommand::Plans { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_plans_envelope(&config)?
        }
        Some(OneDoctorCommand::Scheduling { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_scheduling_envelope(&config)?
        }
        Some(OneDoctorCommand::Billing { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_doctor_billing_envelope(&config)?
        }
        None => Envelope::ok(
            "one doctor commands available: auth, discover, identity, plans, scheduling, billing",
        ),
    })
}
