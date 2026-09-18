use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{
    OnePersonCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

fn normalize_provider_disabled_delete(id: &str, envelope: Envelope) -> Envelope {
    if envelope.error_code == Some(ayx_core::envelope::ErrorCode::Gone)
        && envelope
            .data
            .pointer("/response/exception/code")
            .and_then(serde_json::Value::as_str)
            == Some("IAM_ENDPOINT_SCREAM_TEST")
    {
        Envelope::err_coded(
            ayx_core::envelope::ErrorCode::Gone,
            "global person deletion is temporarily unavailable because the provider disabled this endpoint",
            serde_json::json!({
                "person_id": id,
                "provider_code": "IAM_ENDPOINT_SCREAM_TEST",
                "provider_response": envelope.data.get("response").cloned().unwrap_or(serde_json::Value::Null),
            }),
        )
        .with_remediation(
            "The provider has temporarily disabled global-person deletion. Use workspace-member removal only when the intended operation is to remove workspace membership.",
            vec![],
        )
    } else {
        envelope
    }
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
                .with_page_size(runtime.page_size)
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
        Some(OnePersonCommand::Current) => current(runtime, None)?,
        Some(OnePersonCommand::Detail { profile, id }) => {
            let id = crate::cmd::select::resolve_selector(
                "person id",
                "ayx one person list -o json",
                id,
                crate::cmd::select::SelectPolicy::from_runtime(runtime.no_input),
                || {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(Some(200))
                        .with_all(true, Some(10));
                    let listed = ayx_one_api::one_api_list_request(
                        &config,
                        "person",
                        "picker-list",
                        "/v4/people",
                        &[],
                        &params,
                    )?;
                    crate::cmd::select::items_from_envelope(
                        &listed,
                        &["email", "fullName", "full_name"],
                    )
                },
            )?;
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
            let envelope = one_api_live_request(
                &config,
                "person",
                "person-delete",
                "DELETE",
                "/v4/people/{id}",
                true,
                &[("id", id.as_str())],
            )?;
            normalize_provider_disabled_delete(&id, envelope)
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
    use ayx_core::envelope::ErrorCode;
    use serde_json::json;

    use super::normalize_provider_disabled_delete;

    #[test]
    fn provider_disabled_global_delete_is_gone_not_not_found() {
        let source = ayx_core::envelope::Envelope::err_coded(
            ErrorCode::Gone,
            "request failed",
            json!({"response": {"exception": {"code": "IAM_ENDPOINT_SCREAM_TEST"}}}),
        );
        let result = normalize_provider_disabled_delete("person-1", source);
        assert_eq!(result.error_code, Some(ErrorCode::Gone));
        assert_eq!(result.data["person_id"], "person-1");
        assert!(result.remediation.unwrap().commands.is_empty());
    }
}
