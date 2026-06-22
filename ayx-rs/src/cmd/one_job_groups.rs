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
            let mut envelope = ayx_one_api::one_api_list_request(
                &config,
                "jobGroup",
                "list",
                "/v4/jobLibrary",
                &[],
                &params,
            )?;
            synthesize_job_group_names(&mut envelope.data);
            envelope
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

/// Synthesize a display name for job-groups that have `name: null`.
///
/// Job-groups created from flow runs have no user-assigned name. The only
/// identifying context available is `flowRun.flowId` (or top-level `flowId`)
/// and the group's own `id`. This function patches each null-name item
/// in-place so downstream consumers always have a non-null name to display.
fn synthesize_job_group_names(data: &mut serde_json::Value) {
    let items = match data.get_mut("items").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => return,
    };
    for item in items.iter_mut() {
        let obj = match item.as_object_mut() {
            Some(o) => o,
            None => continue,
        };
        // Only synthesize when name is null or missing.
        let name_is_null = obj.get("name").map(|n| n.is_null()).unwrap_or(true);
        if !name_is_null {
            continue;
        }
        // Try flowRun.flowId first, then top-level flowId.
        let flow_id = obj
            .get("flowRun")
            .and_then(|fr| fr.get("flowId"))
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("flowId").and_then(|v| v.as_str()));
        let synthesized = match flow_id {
            Some(fid) => format!("flow-{fid}"),
            None => {
                let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                format!("job-{id}")
            }
        };
        obj.insert("name".to_string(), serde_json::Value::String(synthesized));
    }
}
