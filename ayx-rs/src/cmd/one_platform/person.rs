use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OnePersonCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OnePersonCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => {
            // Bare `ayx one person` runs an unpaginated list
            // against the default config.yaml for back-compat.
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "person",
                "person-list",
                "GET",
                "/v4/people",
                false,
                &[],
            )?
        }
        Some(OnePersonCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = ayx_one_api::OneListParams::new()
                .with_limit(limit)
                .with_page_token(page_token)
                .with_all(all, max_pages);
            ayx_one_api::one_api_list_request(
                &config,
                "person",
                "person-list",
                "/v4/people",
                &[],
                &params,
            )?
        }
        Some(OnePersonCommand::Count) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "person",
                "person-count",
                "GET",
                "/v4/people/count",
                false,
                &[],
            )?
        }
        Some(OnePersonCommand::Current) => current(runtime, None)?,
        Some(OnePersonCommand::Detail { profile, person_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "person",
                "person-detail",
                "GET",
                "/v4/people/{id}",
                false,
                &[("id", &person_id)],
            )?
        }
        Some(OnePersonCommand::Update {
            profile,
            person_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-update",
                "PUT",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::Patch {
            profile,
            person_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-patch",
                "PATCH",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::Delete { profile, person_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "delete",
                        &format!("person id='{person_id}'"),
                        &config.profile_name,
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "person",
                "person-delete",
                "DELETE",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
            )?
        }
        Some(OnePersonCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-create",
                "POST",
                "/v4/people",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::UpdatePassword { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-update-password",
                "PATCH",
                "/v4/people/current/updatePassword",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::PasswordResetRequest { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-password-reset-request",
                "POST",
                "/v4/passwordresetrequest",
                true,
                &[],
                Some(payload),
            )?
        }
    })
}

pub(crate) fn current(runtime: &RuntimeCtx<'_>, profile: Option<&str>) -> Result<Envelope> {
    let config = runtime.load_profile_lenient(profile)?;
    one_api_live_request(
        &config,
        "person",
        "person-current",
        "GET",
        "/v4/people/current",
        false,
        &[],
    )
}
