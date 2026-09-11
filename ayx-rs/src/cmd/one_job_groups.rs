use anyhow::Result;
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneJobGroupCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(runtime: &RuntimeCtx<'_>, command: OneJobGroupCommand) -> Result<Envelope> {
    Ok(match command {
        OneJobGroupCommand::List {
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
                "jobs",
                "list",
                "/v4/jobLibrary",
                &[],
                &params,
            )?
        }
        OneJobGroupCommand::Count { profile } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "count",
                "GET",
                "/v4/jobLibrary/count",
                false,
                &[],
            )?
        }
        OneJobGroupCommand::Run { profile, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "jobs",
                "execute",
                "POST",
                "/v4/jobGroups",
                true,
                &[],
                Some(payload),
            )?
        }
        OneJobGroupCommand::Publish { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "jobs",
                "publish",
                "PUT",
                "/v4/jobGroups/{id}/publish",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneJobGroupCommand::PdfResults { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "pdf-results",
                "GET",
                "/v4/jobGroups/{id}/pdfResults",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Detail { profile, id } => {
            let id = crate::cmd::select::resolve_selector(
                "job id",
                "ayx one jobs list --output json",
                id,
                crate::cmd::select::SelectPolicy::from_runtime(runtime.no_input),
                || {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let params = ayx_one_api::OneListParams::new()
                        .with_limit(Some(200))
                        .with_all(true, Some(10));
                    let listed = ayx_one_api::one_api_list_request(
                        &config,
                        "jobs",
                        "picker-list",
                        "/v4/jobLibrary",
                        &[],
                        &params,
                    )?;
                    crate::cmd::select::items_from_envelope(
                        &listed,
                        &["name", "flowName", "flow_name"],
                    )
                },
            )?;
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "detail",
                "GET",
                "/v4/jobGroups/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Cancel { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "cancel",
                "POST",
                "/v4/jobGroups/{id}/cancel",
                true,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Status { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "status",
                "GET",
                "/v4/jobGroups/{id}/status",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Inputs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "inputs",
                "GET",
                "/v4/jobGroups/{id}/inputs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Outputs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "outputs",
                "GET",
                "/v4/jobGroups/{id}/outputs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Jobs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "runs",
                "GET",
                "/v4/jobGroups/{id}/jobs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Publications { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "publications",
                "GET",
                "/v4/jobGroups/{id}/publications",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Profile { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "profile",
                "GET",
                "/v4/jobGroups/{id}/profile",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::ProfileResults { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobs",
                "profile-results",
                "GET",
                "/v4/jobGroups/{id}/profileResults",
                false,
                &[("id", id.as_str())],
            )?
        }
    })
}

#[cfg(test)]
mod tests {}
