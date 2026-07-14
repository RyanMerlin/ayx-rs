use std::path::Path;

use anyhow::Result;
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
    command: OneFlowsCommand,
) -> Result<Envelope> {
    Ok(match command {
        OneFlowsCommand::List {
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
            ayx_one_api::one_api_list_request(&config, "flow", "list", "/v4/flows", &[], &params)?
        }
        OneFlowsCommand::Count { profile } => {
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
        OneFlowsCommand::Library { command } => match command {
            OneFlowLibraryCommand::List {
                profile,
                limit,
                offset,
            } => {
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
            OneFlowLibraryCommand::Count { profile } => {
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
        OneFlowsCommand::Folders { command } => match command {
            OneFlowFoldersCommand::List {
                profile,
                limit,
                offset,
            } => {
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
            OneFlowFoldersCommand::Count { profile } => {
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
            OneFlowFoldersCommand::Detail { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                one_api_live_request(
                    &config,
                    "flow",
                    "folders-detail",
                    "GET",
                    "/v4/folders/{id}",
                    false,
                    &[("id", id.as_str())],
                )?
            }
            OneFlowFoldersCommand::Create { profile, body } => {
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
            OneFlowFoldersCommand::Update { profile, id, body } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                let payload = load_payload(&body)?;
                one_api_live_request_with_body(
                    &config,
                    "flow",
                    "folders-update",
                    "PATCH",
                    "/v4/folders/{id}",
                    true,
                    &[("id", id.as_str())],
                    Some(payload),
                )?
            }
            OneFlowFoldersCommand::Delete { profile, id } => {
                let config = runtime.load_profile_lenient(profile.as_deref())?;
                if apply {
                    cmd::confirm::require_tty_confirmation(
                        yes,
                        &cmd::confirm::destructive_action_message(
                            "delete",
                            &format!("folder id='{id}'"),
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
                    &[("id", id.as_str())],
                )?
            }
            OneFlowFoldersCommand::Flows { command } => match command {
                OneFlowFolderFlowsCommand::List {
                    profile,
                    id,
                    limit,
                    offset,
                } => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
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
                        &[("id", id.as_str())],
                    )?
                }
                OneFlowFolderFlowsCommand::Count { profile, id } => {
                    let config = runtime.load_profile_lenient(profile.as_deref())?;
                    one_api_live_request(
                        &config,
                        "flow",
                        "folder-flows-count",
                        "GET",
                        "/v4/folders/{id}/flows/count",
                        false,
                        &[("id", id.as_str())],
                    )?
                }
            },
        },
        OneFlowsCommand::Create { profile, body } => {
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
        OneFlowsCommand::Detail { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "detail",
                "GET",
                "/v4/flows/{id}",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::Update { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "update",
                "PATCH",
                "/v4/flows/{id}",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneFlowsCommand::Delete { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            if apply {
                cmd::confirm::require_tty_confirmation(
                    yes,
                    &cmd::confirm::destructive_action_message(
                        "delete",
                        &format!("flow id='{id}'"),
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
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::Copy { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = body.map(|path| load_payload(&path)).transpose()?;
            match payload {
                Some(payload) => one_api_live_request_with_body(
                    &config,
                    "flow",
                    "copy",
                    "POST",
                    "/v4/flows/{id}/copy",
                    true,
                    &[("id", id.as_str())],
                    Some(payload),
                )?,
                None => one_api_live_request(
                    &config,
                    "flow",
                    "copy",
                    "POST",
                    "/v4/flows/{id}/copy",
                    true,
                    &[("id", id.as_str())],
                )?,
            }
        }
        OneFlowsCommand::Run { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = body.map(|path| load_payload(&path)).transpose()?;
            match payload {
                Some(payload) => one_api_live_request_with_body(
                    &config,
                    "flow",
                    "run",
                    "POST",
                    "/v4/flows/{id}/run",
                    true,
                    &[("id", id.as_str())],
                    Some(payload),
                )?,
                None => one_api_live_request(
                    &config,
                    "flow",
                    "run",
                    "POST",
                    "/v4/flows/{id}/run",
                    true,
                    &[("id", id.as_str())],
                )?,
            }
        }
        OneFlowsCommand::Validate { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "validate",
                "GET",
                "/v4/flows/{id}/validate",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::Parameters {
            profile,
            id,
            output_object_type,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let endpoint = if let Some(value) = output_object_type.as_deref() {
                format!(
                    "/v4/flows/{}/recipeParameters?outputObjectType={}",
                    id, value
                )
            } else {
                format!("/v4/flows/{}/recipeParameters", id)
            };
            one_api_live_request(&config, "flow", "parameters", "GET", &endpoint, false, &[])?
        }
        OneFlowsCommand::Inputs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "inputs",
                "GET",
                "/v4/flows/{id}/inputs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::Outputs { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "outputs",
                "GET",
                "/v4/flows/{id}/outputs",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::PermissionsGet { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            one_api_live_request(
                &config,
                "flow",
                "permissions-get",
                "GET",
                "/v4/flows/{id}/permissions",
                false,
                &[("id", id.as_str())],
            )?
        }
        OneFlowsCommand::Permissions { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "permissions",
                "POST",
                "/v4/flows/{id}/permissions",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneFlowsCommand::Move { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "move",
                "POST",
                "/v4/flows/{id}/move",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneFlowsCommand::ReplaceDataset { profile, id, body } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            let payload = load_payload(&body)?;
            one_api_live_request_with_body(
                &config,
                "flow",
                "replace-dataset",
                "PATCH",
                "/v4/flows/{id}/replaceDataset",
                true,
                &[("id", id.as_str())],
                Some(payload),
            )?
        }
        OneFlowsCommand::Import {
            profile,
            input,
            folder_id,
            from_ui,
            override_js_udfs,
        } => {
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
        OneFlowsCommand::ImportDryRun {
            profile,
            input,
            folder_id,
            from_ui,
            override_js_udfs,
        } => {
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
        OneFlowsCommand::Export {
            profile,
            id,
            output_file,
        } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            flow_export_package_envelope(&config, &id, &output_file, false)?
        }
        OneFlowsCommand::ExportDryRun { profile, id } => {
            let config = runtime.load_profile_lenient(profile.as_deref())?;
            flow_export_package_envelope(&config, &id, Path::new("unused"), true)?
        }
    })
}
