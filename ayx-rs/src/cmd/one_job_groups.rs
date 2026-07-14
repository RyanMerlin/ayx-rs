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
        OneJobGroupCommand::Count { profile } => {
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
        OneJobGroupCommand::Run { profile, body } => {
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
        OneJobGroupCommand::Publish { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "jobGroup",
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
                "jobGroup",
                "pdf-results",
                "GET",
                "/v4/jobGroups/{id}/pdfResults",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneJobGroupCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "jobGroup",
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
                "jobGroup",
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
                "jobGroup",
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
                "jobGroup",
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
                "jobGroup",
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
                "jobGroup",
                "jobs",
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
                "jobGroup",
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
                "jobGroup",
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
                "jobGroup",
                "profile-results",
                "GET",
                "/v4/jobGroups/{id}/profileResults",
                false,
                &[("id", id.as_str())],
            )?
        }
    })
}

/// Synthesize a display name for job-groups that have `name: null`.
///
/// Job-groups created from flow runs have no user-assigned name. The only
/// identifying context available is `flowRun.flowId` (or top-level `flowId`),
/// the group's own `id`, and optionally `createdAt`. This function patches
/// each null-name item in-place so downstream consumers always have a
/// non-null name to display.
///
/// Precedence (panic-safe, all field reads are `Option`-chained):
/// 1. `flow-{flowId} ({id})` when both flowId and id are present
/// 2. `flow-{flowId} @ {createdAt}` when flowId and createdAt are present but not id
/// 3. `flow-{flowId}` when only flowId is present
/// 4. `job-{id}` when only id is present
/// 5. `job-?` as a last resort
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
        let flow_id: Option<String> = obj
            .get("flowRun")
            .and_then(|fr| fr.get("flowId"))
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("flowId").and_then(|v| v.as_str()))
            .map(str::to_string);
        let id: Option<String> = obj.get("id").and_then(|v| v.as_str()).map(str::to_string);
        let created_at: Option<String> = obj
            .get("createdAt")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let synthesized = match (flow_id, id, created_at) {
            (Some(fid), Some(gid), _) => format!("flow-{fid} ({gid})"),
            (Some(fid), None, Some(ts)) => format!("flow-{fid} @ {ts}"),
            (Some(fid), None, None) => format!("flow-{fid}"),
            (None, Some(gid), _) => format!("job-{gid}"),
            (None, None, _) => "job-?".to_string(),
        };
        obj.insert("name".to_string(), serde_json::Value::String(synthesized));
    }
}

#[cfg(test)]
mod tests {
    use super::synthesize_job_group_names;
    use serde_json::json;

    fn run(items: serde_json::Value) -> serde_json::Value {
        let mut data = json!({ "items": items });
        synthesize_job_group_names(&mut data);
        data
    }

    fn first_name(data: &serde_json::Value) -> &str {
        data["items"][0]["name"]
            .as_str()
            .expect("name must be a string")
    }

    #[test]
    fn null_name_flowrun_flow_id_and_id_produces_flow_id_in_parens() {
        let data = run(json!([{
            "name": null,
            "id": "grp-1",
            "flowRun": { "flowId": "flow-abc" }
        }]));
        assert_eq!(first_name(&data), "flow-flow-abc (grp-1)");
    }

    #[test]
    fn null_name_flowrun_flow_id_no_id_but_created_at_uses_at_format() {
        let data = run(json!([{
            "name": null,
            "createdAt": "2024-01-15T10:00:00Z",
            "flowRun": { "flowId": "flow-xyz" }
        }]));
        assert_eq!(first_name(&data), "flow-flow-xyz @ 2024-01-15T10:00:00Z");
    }

    #[test]
    fn null_name_top_level_flow_id_no_nested_flow_run() {
        // No flowRun block — falls back to top-level flowId.
        // Both id and flowId present → flow-{flowId} ({id}).
        let data = run(json!([{
            "name": null,
            "id": "grp-9",
            "flowId": "flow-top"
        }]));
        assert_eq!(first_name(&data), "flow-flow-top (grp-9)");
    }

    #[test]
    fn null_name_only_flow_id_no_id_no_created_at() {
        let data = run(json!([{
            "name": null,
            "flowRun": { "flowId": "flow-only" }
        }]));
        assert_eq!(first_name(&data), "flow-flow-only");
    }

    #[test]
    fn null_name_no_flow_id_with_id_produces_job_id() {
        let data = run(json!([{
            "name": null,
            "id": "job-42"
        }]));
        assert_eq!(first_name(&data), "job-job-42");
    }

    #[test]
    fn null_name_nothing_at_all_produces_job_question_mark() {
        let data = run(json!([{
            "name": null
        }]));
        assert_eq!(first_name(&data), "job-?");
    }

    #[test]
    fn non_null_name_left_untouched() {
        let data = run(json!([{
            "name": "My Existing Group",
            "id": "grp-5",
            "flowRun": { "flowId": "flow-ignored" }
        }]));
        assert_eq!(first_name(&data), "My Existing Group");
    }

    #[test]
    fn missing_items_key_is_a_no_op() {
        let mut data = json!({ "other_key": [] });
        synthesize_job_group_names(&mut data);
        // Should not panic and data is unchanged.
        assert!(data.get("items").is_none());
    }

    #[test]
    fn non_object_element_in_items_is_skipped_no_panic() {
        // Scalars inside the items array must be silently skipped.
        let mut data = json!({ "items": [42, "string", null, { "name": null, "id": "g1" }] });
        synthesize_job_group_names(&mut data);
        // The object at index 3 should be synthesized; the rest are unchanged scalars.
        let items = data["items"].as_array().unwrap();
        assert_eq!(items[0], json!(42));
        assert_eq!(items[1], json!("string"));
        assert_eq!(items[2], json!(null));
        assert_eq!(items[3]["name"], json!("job-g1"));
    }
}
