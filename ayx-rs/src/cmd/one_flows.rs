use std::path::Path;

use anyhow::{anyhow, Result};
use ayx_core::envelope::Envelope;
use ayx_one_api::{
    flow_export_package_envelope, flow_import_package_envelope, one_api_live_request,
    one_api_live_request_with_body,
};

use crate::{
    cmd::{self, RuntimeCtx},
    load_payload, OneFlowsCommand,
};

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OneFlowsCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one flows commands available: list, count, detail, create, update, delete, copy, run, validate, parameters, inputs, outputs, import, import-dry-run, export, export-dry-run",
        ),
        Some(OneFlowsCommand::List {
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
            ayx_one_api::one_api_list_request(&config, "flow", "list", "/v4/flows", &[], &params)?
        }
        Some(OneFlowsCommand::Count { profile }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "count",
                "GET",
                "/v4/flows/count",
                false,
                &[],
            )?
        }
        Some(OneFlowsCommand::Create { profile, body }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "create",
                "POST",
                "/v4/flows",
                true,
                &[],
                Some(payload),
            )?
        }
        Some(OneFlowsCommand::Detail { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            one_api_live_request(
                &config,
                "flow",
                "detail",
                "GET",
                "/v4/flows/{id}",
                false,
                &[("id", flow_id.as_str())],
            )?
        }
        Some(OneFlowsCommand::Update {
            profile,
            flow_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "update",
                "PUT",
                "/v4/flows/{id}",
                true,
                &[("id", flow_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneFlowsCommand::Delete { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            if apply {
                // Only gate on confirmation when actually applying.
                // Without --apply the transport short-circuits to a
                // dry-run envelope anyway; no need to prompt.
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &format!(
                        "About to DELETE flow id='{flow_id}' on profile '{}'. This cannot be undone.",
                        config.profile_name
                    ),
                )?;
            }
            one_api_live_request(
                &config,
                "flow",
                "delete",
                "DELETE",
                "/v4/flows/{id}",
                true,
                &[("id", flow_id.as_str())],
            )?
        }
        Some(OneFlowsCommand::Copy {
            profile,
            flow_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            let payload = body.map(|path| load_payload(&path)).transpose()?;
            match payload {
                Some(payload) => one_api_live_request_with_body(
                    &config,
                    "flow",
                    "copy",
                    "POST",
                    "/v4/flows/{id}/copy",
                    true,
                    &[("id", flow_id.as_str())],
                    Some(payload),
                )?,
                None => one_api_live_request(
                    &config,
                    "flow",
                    "copy",
                    "POST",
                    "/v4/flows/{id}/copy",
                    true,
                    &[("id", flow_id.as_str())],
                )?,
            }
        }
        Some(OneFlowsCommand::Run {
            profile,
            flow_id,
            body,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            let payload = body.map(|path| load_payload(&path)).transpose()?;
            match payload {
                Some(payload) => one_api_live_request_with_body(
                    &config,
                    "flow",
                    "run",
                    "POST",
                    "/v4/flows/{id}/run",
                    true,
                    &[("id", flow_id.as_str())],
                    Some(payload),
                )?,
                None => one_api_live_request(
                    &config,
                    "flow",
                    "run",
                    "POST",
                    "/v4/flows/{id}/run",
                    true,
                    &[("id", flow_id.as_str())],
                )?,
            }
        }
        Some(OneFlowsCommand::Validate { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            one_api_live_request(
                &config,
                "flow",
                "validate",
                "GET",
                "/v4/flows/{id}/validate",
                false,
                &[("id", flow_id.as_str())],
            )?
        }
        Some(OneFlowsCommand::Parameters {
            profile,
            flow_id,
            output_object_type,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            let endpoint = if let Some(value) = output_object_type.as_deref() {
                format!(
                    "/v4/flows/{}/recipeParameters?outputObjectType={}",
                    flow_id, value
                )
            } else {
                format!("/v4/flows/{}/recipeParameters", flow_id)
            };
            one_api_live_request(&config, "flow", "parameters", "GET", &endpoint, false, &[])?
        }
        Some(OneFlowsCommand::Inputs { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            one_api_live_request(
                &config,
                "flow",
                "inputs",
                "GET",
                "/v4/flows/{id}/inputs",
                false,
                &[("id", flow_id.as_str())],
            )?
        }
        Some(OneFlowsCommand::Outputs { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            one_api_live_request(
                &config,
                "flow",
                "outputs",
                "GET",
                "/v4/flows/{id}/outputs",
                false,
                &[("id", flow_id.as_str())],
            )?
        }
        Some(OneFlowsCommand::Import {
            profile,
            input,
            folder_id,
            from_ui,
            override_js_udfs,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            flow_import_package_envelope(
                &config,
                &input,
                folder_id.as_deref(),
                from_ui,
                override_js_udfs,
                false,
            )?
        }
        Some(OneFlowsCommand::ImportDryRun {
            profile,
            input,
            folder_id,
            from_ui,
            override_js_udfs,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            flow_import_package_envelope(
                &config,
                &input,
                folder_id.as_deref(),
                from_ui,
                override_js_udfs,
                true,
            )?
        }
        Some(OneFlowsCommand::Export {
            profile,
            flow_id,
            output,
        }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            flow_export_package_envelope(&config, &flow_id, &output, false)?
        }
        Some(OneFlowsCommand::ExportDryRun { profile, flow_id }) => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let flow_id = flow_id.ok_or_else(|| anyhow!("--flow-id is required"))?;
            flow_export_package_envelope(&config, &flow_id, Path::new("unused"), true)?
        }
    })
}
