use anyhow::Result;
use ayx_core::envelope::Envelope;

use crate::{
    OneDoctorCommand, cmd::RuntimeCtx, one_doctor_discover_envelope, one_doctor_identity_envelope,
    one_doctor_plans_envelope, one_doctor_scheduling_envelope, one_platform_auth_diagnose_envelope,
};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneDoctorCommand) -> Result<Envelope> {
    Ok(match command {
        OneDoctorCommand::Auth { profile, migrate } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let mut envelope = one_platform_auth_diagnose_envelope(&config)?;
            let inline_fields = ayx_core::auth::inline_secret_fields(&config);
            let migration = if migrate && !inline_fields.is_empty() {
                let path = ayx_core::profile::profile_storage_path(&config.profile_name)?;
                let output = crate::onboard::migrate_inline_auth_secrets(&path)?;
                Some(serde_json::json!({
                    "applied": true,
                    "migrated_fields": output.refs.keys().collect::<Vec<_>>(),
                    "inline_fields": output.inline_fields,
                }))
            } else {
                Some(serde_json::json!({
                    "applied": false,
                    "available": !inline_fields.is_empty(),
                    "inline_fields": inline_fields,
                    "hint": "rerun with --migrate when secure storage is available",
                }))
            };
            if let Some(data) = envelope.data.as_object_mut() {
                data.insert(
                    "inline_secret_fields".to_string(),
                    serde_json::json!(inline_fields),
                );
                data.insert("migration".to_string(), migration.unwrap_or_default());
            }
            envelope
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
