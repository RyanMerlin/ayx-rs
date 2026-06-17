use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneOutputObjectCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneOutputObjectCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one output-object commands available: list, count, create, detail, update, delete, inputs, wrangle-to-python",
        ),
        Some(OneOutputObjectCommand::List {
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
                "outputObject",
                "list",
                "/v4/outputObjects",
                &[],
                &params,
            )?
        }
        Some(OneOutputObjectCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "outputObject",
                "count",
                "GET",
                "/v4/outputObjects/count",
                false,
                &[],
            )?
        }
        Some(OneOutputObjectCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "outputObject",
                "create",
                "POST",
                "/v4/outputObjects",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OneOutputObjectCommand::Detail {
            profile,
            output_object_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let output_object_id =
                output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
            one_api_live_request(
                &config,
                "outputObject",
                "detail",
                "GET",
                "/v4/outputObjects/{id}",
                false,
                &[("id", output_object_id.as_str())],
            )?
        }
        Some(OneOutputObjectCommand::Update {
            profile,
            output_object_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let output_object_id =
                output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "outputObject",
                "update",
                "PATCH",
                "/v4/outputObjects/{id}",
                true,
                &[("id", output_object_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneOutputObjectCommand::Delete {
            profile,
            output_object_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let output_object_id =
                output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
            one_api_live_request(
                &config,
                "outputObject",
                "delete",
                "DELETE",
                "/v4/outputObjects/{id}",
                true,
                &[("id", output_object_id.as_str())],
            )?
        }
        Some(OneOutputObjectCommand::Inputs {
            profile,
            output_object_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let output_object_id =
                output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
            one_api_live_request(
                &config,
                "outputObject",
                "inputs",
                "GET",
                "/v4/outputObjects/{id}/inputs",
                false,
                &[("id", output_object_id.as_str())],
            )?
        }
        Some(OneOutputObjectCommand::WrangleToPython {
            profile,
            output_object_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let output_object_id =
                output_object_id.ok_or_else(|| anyhow!("--output-object-id is required"))?;
            match body {
                Some(body) => {
                    let payload = load_payload(&body)?;
                    one_api_live_request_with_body(
                        &config,
                        "outputObject",
                        "wrangle-to-python",
                        "POST",
                        "/v4/outputObjects/{id}/wrangleToPython",
                        true,
                        &[("id", output_object_id.as_str())],
                        Some(payload),
                    )?
                }
                None => one_api_live_request(
                    &config,
                    "outputObject",
                    "wrangle-to-python",
                    "POST",
                    "/v4/outputObjects/{id}/wrangleToPython",
                    false,
                    &[("id", output_object_id.as_str())],
                )?,
            }
        }
    })
}
