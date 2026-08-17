use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OnePersonCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

pub(crate) fn person_count_deprecation_message() -> &'static str {
    "one person count is deprecated and will be removed by the vendor (scream-test flag IAM_SCREAM_PEOPLE); there is no replacement count endpoint, so use 'one person list' for enumeration"
}

fn warn_person_count_deprecated() {
    eprintln!("warning: {}", person_count_deprecation_message());
}

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
            warn_person_count_deprecated();
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
        Some(OnePersonCommand::Detail { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "person",
                "person-detail",
                "GET",
                "/v4/people/{id}",
                false,
                &[("id", &id)],
            )?
        }
        Some(OnePersonCommand::Update { profile, id, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-update",
                "PUT",
                "/v4/people/{id}",
                true,
                &[("id", &id)],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::Patch { profile, id, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "person",
                "person-patch",
                "PATCH",
                "/v4/people/{id}",
                true,
                &[("id", &id)],
                Some(payload),
            )?
        }
        Some(OnePersonCommand::Delete { profile, id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::access_change_message(
                        "delete",
                        &format!("person id='{id}'"),
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
                &[("id", &id)],
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

#[cfg(test)]
mod tests {
    use super::person_count_deprecation_message;

    #[test]
    fn person_count_warning_mentions_the_vendor_removal_and_list_fallback() {
        let message = person_count_deprecation_message();
        assert!(message.contains("IAM_SCREAM_PEOPLE"));
        assert!(message.contains("no replacement count endpoint"));
        assert!(message.contains("one person list"));
    }
}
