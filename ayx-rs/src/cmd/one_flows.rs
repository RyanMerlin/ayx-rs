use std::path::Path;

use anyhow::{Result, anyhow};
use ayx_core::envelope::Envelope;
use ayx_one_api::{
    flow_export_package_envelope, flow_import_package_envelope, one_api_live_request,
    one_api_live_request_with_body,
};
use url::form_urlencoded::Serializer;

use crate::{
    OneFlowFolderFlowsCommand, OneFlowFoldersCommand, OneFlowLibraryCommand, OneFlowsCommand,
    cmd::{self, RuntimeCtx},
    load_payload,
};

fn append_query(endpoint: &str, query: &[(&str, String)]) -> String {
    if query.is_empty() {
        return endpoint.to_string();
    }
    let mut serializer = Serializer::new(String::new());
    for (key, value) in query {
        serializer.append_pair(key, value);
    }
    format!("{endpoint}?{}", serializer.finish())
}

pub(crate) fn execute(
    runtime: &RuntimeCtx<'_>,
    apply: bool,
    yes: bool,
    command: Option<OneFlowsCommand>,
) -> Result<Envelope> {
    Ok(match command {
        None => Envelope::ok(
            "one flows commands available: list, count, library, folders, detail, create, update, delete, copy, run, validate, parameters, inputs, outputs, permissions, move, replace-dataset, import, import-dry-run, export, export-dry-run",
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
        Some(OneFlowsCommand::Library { command }) => match command {
            None => Envelope::ok("one flows library commands available: list, count"),
            Some(OneFlowLibraryCommand::List {
                profile,
                limit,
                offset,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let mut query = Vec::new();
                if let Some(limit) = limit {
                    query.push(("limit", limit.to_string()));
                }
                if let Some(offset) = offset {
                    query.push(("offset", offset.to_string()));
                }
                let endpoint = append_query("/v4/flowsLibrary", &query);
                one_api_live_request(
                    &config,
                    "flow",
                    "library-list",
                    "GET",
                    &endpoint,
                    false,
                    &[],
                )?
            }
            Some(OneFlowLibraryCommand::Count { profile }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "flow",
                    "library-count",
                    "GET",
                    "/v4/flowsLibrary/count",
                    false,
                    &[],
                )?
            }
        },
        Some(OneFlowsCommand::Folders { command }) => match command {
            None => Envelope::ok(
                "one flows folders commands available: list, count, detail, create, update, delete, flows",
            ),
            Some(OneFlowFoldersCommand::List {
                profile,
                limit,
                offset,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let mut query = Vec::new();
                if let Some(limit) = limit {
                    query.push(("limit", limit.to_string()));
                }
                if let Some(offset) = offset {
                    query.push(("offset", offset.to_string()));
                }
                let endpoint = append_query("/v4/folders", &query);
                one_api_live_request(
                    &config,
                    "flow",
                    "folders-list",
                    "GET",
                    &endpoint,
                    false,
                    &[],
                )?
            }
            Some(OneFlowFoldersCommand::Count { profile }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "flow",
                    "folders-count",
                    "GET",
                    "/v4/folders/count",
                    false,
                    &[],
                )?
            }
            Some(OneFlowFoldersCommand::Detail { profile, folder_id }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let folder_id = folder_id.ok_or_else(|| anyhow!("--folder-id is required"))?;
                one_api_live_request(
                    &config,
                    "flow",
                    "folders-detail",
                    "GET",
                    "/v4/folders/{id}",
                    false,
                    &[("id", folder_id.as_str())],
                )?
            }
            Some(OneFlowFoldersCommand::Create { profile, body }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = load_payload(&body)?;
                one_api_live_request_with_body(
                    &config,
                    "flow",
                    "folders-create",
                    "POST",
                    "/v4/folders",
                    true,
                    &[],
                    Some(payload),
                )?
            }
            Some(OneFlowFoldersCommand::Update {
                profile,
                folder_id,
                body,
            }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let folder_id = folder_id.ok_or_else(|| anyhow!("--folder-id is required"))?;
                let payload = load_payload(&body)?;
                one_api_live_request_with_body(
                    &config,
                    "flow",
                    "folders-update",
                    "PATCH",
                    "/v4/folders/{id}",
                    true,
                    &[("id", folder_id.as_str())],
                    Some(payload),
                )?
            }
            Some(OneFlowFoldersCommand::Delete { profile, folder_id }) => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let folder_id = folder_id.ok_or_else(|| anyhow!("--folder-id is required"))?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "delete",
                            &format!("folder id='{folder_id}'"),
                            &config.profile_name,
                        ),
                    )?;
                }
                one_api_live_request(
                    &config,
                    "flow",
                    "folders-delete",
                    "DELETE",
                    "/v4/folders/{id}",
                    true,
                    &[("id", folder_id.as_str())],
                )?
            }
            Some(OneFlowFoldersCommand::Flows { command }) => match command {
                None => Envelope::ok("one flows folders flows commands available: list, count"),
                Some(OneFlowFolderFlowsCommand::List {
                    profile,
                    folder_id,
                    limit,
                    offset,
                }) => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let folder_id = folder_id.ok_or_else(|| anyhow!("--folder-id is required"))?;
                    let mut query = Vec::new();
                    if let Some(limit) = limit {
                        query.push(("limit", limit.to_string()));
                    }
                    if let Some(offset) = offset {
                        query.push(("offset", offset.to_string()));
                    }
                    let endpoint = append_query("/v4/folders/{id}/flows", &query);
                    one_api_live_request(
                        &config,
                        "flow",
                        "folder-flows-list",
                        "GET",
                        &endpoint,
                        false,
                        &[("id", folder_id.as_str())],
                    )?
                }
                Some(OneFlowFolderFlowsCommand::Count { profile, folder_id }) => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    let folder_id = folder_id.ok_or_else(|| anyhow!("--folder-id is required"))?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "folder-flows-count",
                        "GET",
                        "/v4/folders/{id}/flows/count",
                        false,
                        &[("id", folder_id.as_str())],
                    )?
                }
            },
        },
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
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::destructive_action_message(
                        "delete",
                        &format!("flow id='{flow_id}'"),
                        &config.profile_name,
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
        Some(OneFlowsCommand::Permissions {
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
                "permissions",
                "POST",
                "/v4/flows/{id}/permissions",
                true,
                &[("id", flow_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneFlowsCommand::Move {
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
                "move",
                "POST",
                "/v4/flows/{id}/move",
                true,
                &[("id", flow_id.as_str())],
                Some(payload),
            )?
        }
        Some(OneFlowsCommand::ReplaceDataset {
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
                "replace-dataset",
                "PATCH",
                "/v4/flows/{id}/replaceDataset",
                true,
                &[("id", flow_id.as_str())],
                Some(payload),
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
