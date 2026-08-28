use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneOutputObjectCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: OneOutputObjectCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneOutputObjectCommand::List {
            profile,
            limit,
            page_token,
            all,
            max_pages,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let params = ayx_one_api::OneListParams::new()
                .with_page_size(runtime.page_size)
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
        OneOutputObjectCommand::Count { profile } => {
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
        OneOutputObjectCommand::Create { profile, body } => {
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
        OneOutputObjectCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "outputObject",
                "detail",
                "GET",
                "/v4/outputObjects/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneOutputObjectCommand::Update { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "outputObject",
                "update",
                "PATCH",
                "/v4/outputObjects/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneOutputObjectCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "outputObject",
                "delete",
                "DELETE",
                "/v4/outputObjects/{id}",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneOutputObjectCommand::Inputs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "outputObject",
                "inputs",
                "GET",
                "/v4/outputObjects/{id}/inputs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneOutputObjectCommand::WrangleToPython { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            // One call for both arms, always `mutating: true`.
            //
            // These used to be two arms that disagreed: with `--body` it was
            // mutating (and so `--apply`-gated), without `--body` it passed
            // `mutating: false` — meaning a POST executed for real with no apply
            // gate and up to 4 retries on 5xx. Whether the endpoint mutates cannot
            // depend on whether the caller supplied a body, so the two arms are
            // collapsed rather than merely corrected: there is no longer a second
            // arm that can drift.
            let payload = body.map(|path| load_payload(&path)).transpose()?;
            one_api_live_request_with_body(
                &config,
                "outputObject",
                "wrangle-to-python",
                "POST",
                "/v4/outputObjects/{id}/wrangleToPython",
                true,
                &[("id", id.as_str())],
                payload,
            )?
        }
    })
}
