use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::{one_api_live_request, one_api_live_request_with_body};

use crate::{OneJobGroupCommand, cmd::RuntimeCtx, load_payload};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    command: Option<OneJobGroupCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one job-group commands available: list, count, pdf-results, run, publish, detail, cancel, status, inputs, outputs, jobs, publications, profile, profile-results",
        ),
        Some(OneJobGroupCommand::List {
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
                "jobGroup",
                "list",
                "/v4/jobLibrary",
                &[],
                &params,
            )?
        }
        Some(OneJobGroupCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobGroup",
                "count",
                "GET",
                "/v4/jobLibrary/count",
                false,
                &[],
            )?
        }
        Some(OneJobGroupCommand::Run { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "jobGroup",
                "run",
                "POST",
                "/v4/jobGroups",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OneJobGroupCommand::Publish {
            profile,
            job_group_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "jobGroup",
                "publish",
                "PUT",
                "/v4/jobGroups/{id}/publish",
                true,
                &[("id", job_group_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneJobGroupCommand::PdfResults {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "pdf-results",
                "GET",
                "/v4/jobGroups/{id}/pdfResults",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Detail {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "detail",
                "GET",
                "/v4/jobGroups/{id}",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Cancel {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "cancel",
                "POST",
                "/v4/jobGroups/{id}/cancel",
                true,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Status {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "status",
                "GET",
                "/v4/jobGroups/{id}/status",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Inputs {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "inputs",
                "GET",
                "/v4/jobGroups/{id}/inputs",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Outputs {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "outputs",
                "GET",
                "/v4/jobGroups/{id}/outputs",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Jobs {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "jobs",
                "GET",
                "/v4/jobGroups/{id}/jobs",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Publications {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "publications",
                "GET",
                "/v4/jobGroups/{id}/publications",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::Profile {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "profile",
                "GET",
                "/v4/jobGroups/{id}/profile",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
        Some(OneJobGroupCommand::ProfileResults {
            profile,
            job_group_id,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let job_group_id = job_group_id.ok_or_else(|| anyhow!("--job-group-id is required"))?;
            one_api_live_request(
                &config,
                "jobGroup",
                "profile-results",
                "GET",
                "/v4/jobGroups/{id}/profileResults",
                false,
                &[("id", job_group_id.as_str())],
            )?
        }
    })
}
