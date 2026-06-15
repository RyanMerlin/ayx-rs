use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;

use crate::{cmd::RuntimeCtx, OneRoleCommand};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneRoleCommand) -> Result<Envelope> {
    Ok(match command {
        OneRoleCommand::ListAssignments { role_id } => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "role-list-assignments",
                "GET",
                "/v4/authorization/roles/{id}/people",
                false,
                &[("id", &role_id)],
            )?
        }
        OneRoleCommand::Assign {
            role_id,
            subject_id,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "role-assign",
                "POST",
                "/v4/authorization/roles/{id}/people/{subjectId}",
                true,
                &[("id", &role_id), ("subjectId", &subject_id)],
            )?
        }
        OneRoleCommand::Unassign {
            role_id,
            subject_id,
        } => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "role-unassign",
                "DELETE",
                "/v4/authorization/roles/{id}/people/{subjectId}",
                true,
                &[("id", &role_id), ("subjectId", &subject_id)],
            )?
        }
    })
}
