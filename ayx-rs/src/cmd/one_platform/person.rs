use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OnePlatformPersonCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OnePlatformPersonCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => {
            // Bare `ayx one platform person` runs an unpaginated list
            // against the default config.yaml for back-compat.
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "person-list",
                "GET",
                "/v4/people",
                false,
                &[],
            )?
        }
        Some(OnePlatformPersonCommand::List {
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
                "platform",
                "person-list",
                "/v4/people",
                &[],
                &params,
            )?
        }
        Some(OnePlatformPersonCommand::Count) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "person-count",
                "GET",
                "/v4/people/count",
                false,
                &[],
            )?
        }
        Some(OnePlatformPersonCommand::Current) => {
            let config = runtime.load_profile_lenient(None)?;
            one_api_live_request(
                &config,
                "platform",
                "person-current",
                "GET",
                "/v4/people/current",
                false,
                &[],
            )?
        }
        Some(OnePlatformPersonCommand::Detail { profile, person_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "platform",
                "person-detail",
                "GET",
                "/v4/people/{id}",
                false,
                &[("id", &person_id)],
            )?
        }
        Some(OnePlatformPersonCommand::Update {
            profile,
            person_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
                "person-update",
                "PUT",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
                Some(payload),
            )?
        }
        Some(OnePlatformPersonCommand::Patch {
            profile,
            person_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
                "person-patch",
                "PATCH",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
                Some(payload),
            )?
        }
        Some(OnePlatformPersonCommand::Delete { profile, person_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &format!(
                        "About to DELETE person id='{person_id}' on profile '{}'. This cannot be undone.",
                        config.profile_name
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "platform",
                "person-delete",
                "DELETE",
                "/v4/people/{id}",
                true,
                &[("id", &person_id)],
            )?
        }
        Some(OnePlatformPersonCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
                "person-create",
                "POST",
                "/v4/people",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePlatformPersonCommand::UpdatePassword { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
                "person-update-password",
                "PATCH",
                "/v4/people/current/updatePassword",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OnePlatformPersonCommand::PasswordResetRequest { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "platform",
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
