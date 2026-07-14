use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::one_api_live_request;

use crate::{
    OneRoleCommand,
    cmd::{self, RuntimeCtx},
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: OneRoleCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneRoleCommand::ListAssignments { role_id } => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "role",
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
                "role",
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
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "unassign",
                        &format!("subject id='{subject_id}' from role id='{role_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "role",
                "role-unassign",
                "DELETE",
                "/v4/authorization/roles/{id}/people/{subjectId}",
                true,
                &[("id", &role_id), ("subjectId", &subject_id)],
            )?
        }
    })
}
