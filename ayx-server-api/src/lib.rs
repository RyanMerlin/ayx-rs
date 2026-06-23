use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ayx_core::audit::write_audit_artifact;
use ayx_core::envelope::Envelope;
use ayx_core::profile::{ApiAuthMode, ApiProfile, Config};
use chrono::Utc;
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{Value, json};
use url::form_urlencoded;

const MAX_RETRIES: usize = 3;

static TOKEN_CACHE: OnceLock<Mutex<HashMap<String, CachedToken>>> = OnceLock::new();

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub fn status_envelope(config: &Config) -> Result<Envelope> {
    let api = api_profile(config)?;
    let client = build_client(api)?;
    let token = resolve_bearer_token(api, &client)?;

    let response = request_json(
        &client,
        "GET",
        &format!("{}v3/users?view=Default", normalized_base_url(api)),
        &token,
        None,
    )?;

    Ok(Envelope::ok_with_data(
        "api connection validated",
        json!({
            "profile": config.profile_name,
            "base_url": normalized_base_url(api),
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn users_list_envelope(config: &Config, view: &str) -> Result<Envelope> {
    list_envelope(
        config,
        "users",
        &format!("v3/users?view={}", view),
        "users retrieved",
    )
}

pub fn user_detail_envelope(config: &Config, user_id: &str) -> Result<Envelope> {
    let response = api_request(config, "GET", &format!("v3/users/{}", user_id), None)?;
    Ok(Envelope::ok_with_data(
        "user details retrieved",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn user_update_envelope(config: &Config, user_id: &str, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "PUT",
        &format!("v3/users/{}", user_id),
        Some(payload.clone()),
    )?;
    Ok(Envelope::ok_with_data(
        "user update requested",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_delete_envelope(config: &Config, user_id: &str) -> Result<Envelope> {
    let response = api_request(config, "DELETE", &format!("v3/users/{}", user_id), None)?;
    Ok(Envelope::ok_with_data(
        "user delete requested",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn user_assets_envelope(
    config: &Config,
    user_id: &str,
    asset_type: Option<&str>,
) -> Result<Envelope> {
    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(value) = asset_type {
        params.push(("assetType", value));
    }
    let relative = build_query_path(&format!("v3/users/{}/assets", user_id), &params);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "user assets retrieved",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn user_transfer_assets_envelope(
    config: &Config,
    user_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "PUT",
        &format!("v3/users/{}/assetTransfer", user_id),
        Some(payload.clone()),
    )?;
    Ok(Envelope::ok_with_data(
        "user assets transfer requested",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_deactivate_envelope(config: &Config, user_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "POST",
        &format!("v3/users/{}/deactivate", user_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "user deactivate requested",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn user_password_reset_envelope(config: &Config, user_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "POST",
        &format!("v3/users/{}/passwordReset", user_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "user password reset requested",
        json!({
            "profile": config.profile_name,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn workflows_list_envelope(config: &Config, view: &str) -> Result<Envelope> {
    list_envelope(
        config,
        "workflows",
        &format!("v3/workflows?view={}", view),
        "workflows retrieved",
    )
}

pub fn workflow_detail_envelope(config: &Config, workflow_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "GET",
        &format!("v3/workflows/{}", workflow_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "workflow details retrieved",
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn workflow_jobs_envelope(config: &Config, workflow_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "GET",
        &format!("v3/workflows/{}/jobs", workflow_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "workflow jobs retrieved",
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn workflow_questions_envelope(
    config: &Config,
    workflow_id: &str,
    version_id: Option<&str>,
) -> Result<Envelope> {
    let mut params = Vec::new();
    if let Some(value) = version_id {
        params.push(("versionId", value));
    }
    let relative = build_query_path(&format!("v3/workflows/{}/questions", workflow_id), &params);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "workflow questions retrieved",
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn workflow_package_envelope(
    config: &Config,
    workflow_id: &str,
    version_id: Option<&str>,
    output_path: Option<&Path>,
) -> Result<Envelope> {
    let api = api_profile(config)?;
    let client = build_client(api)?;
    let token = resolve_bearer_token(api, &client)?;

    let mut params = Vec::new();
    if let Some(value) = version_id {
        params.push(("versionId", value));
    }
    let relative = build_query_path(&format!("v3/workflows/{}/package", workflow_id), &params);
    let url = format!("{}{}", normalized_base_url(api), relative);

    let bytes = download_bytes(&client, &token, &url)?;

    let dest = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.yxzp", workflow_id)));

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
    }

    fs::write(&dest, &bytes)
        .with_context(|| format!("failed to write workflow package to '{}'", dest.display()))?;

    Ok(Envelope::ok_with_data(
        "workflow package downloaded",
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "path": dest.display().to_string(),
            "bytes": bytes.len(),
        }),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn workflow_version_upload_envelope(
    config: &Config,
    workflow_id: &str,
    name: &str,
    owner_id: &str,
    file_path: &Path,
    others_may_download: bool,
    others_can_execute: bool,
    execution_mode: &str,
    has_private_data_exemption: bool,
    comments: Option<&str>,
    make_published: bool,
    workflow_credential_type: &str,
    credential_id: Option<&str>,
    bypass_workflow_version_check: bool,
) -> Result<Envelope> {
    let api = api_profile(config)?;
    let client = build_client(api)?;
    let token = resolve_bearer_token(api, &client)?;

    let file_bytes = fs::read(file_path)
        .with_context(|| format!("failed to read workflow package '{}'", file_path.display()))?;
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}.yxzp", workflow_id));

    let url = format!(
        "{}v3/workflows/{}/versions",
        normalized_base_url(api),
        workflow_id
    );

    let form_builder = || {
        let mut builder = Form::new().part(
            "file",
            Part::bytes(file_bytes.clone())
                .file_name(file_name.clone())
                .mime_str("application/octet-stream")
                .expect("mime literal is valid"),
        );
        builder = builder
            .text("name", name.to_string())
            .text("ownerId", owner_id.to_string())
            .text("othersMayDownload", others_may_download.to_string())
            .text("othersCanExecute", others_can_execute.to_string())
            .text("executionMode", execution_mode.to_string())
            .text(
                "hasPrivateDataExemption",
                has_private_data_exemption.to_string(),
            )
            .text("makePublished", make_published.to_string())
            .text(
                "workflowCredentialType",
                workflow_credential_type.to_string(),
            )
            .text(
                "bypassWorkflowVersionCheck",
                bypass_workflow_version_check.to_string(),
            );

        if let Some(value) = comments {
            builder = builder.text("comments", value.to_string());
        }
        if let Some(value) = credential_id {
            builder = builder.text("credentialId", value.to_string());
        }

        builder
    };

    let json_response = request_multipart(&client, &token, &url, form_builder)?;

    Ok(Envelope::ok_with_data(
        "workflow version upload requested",
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "response": json_response,
        }),
    ))
}

pub fn schedules_list_envelope(config: &Config, view: &str) -> Result<Envelope> {
    list_envelope(
        config,
        "schedules",
        &format!("v3/schedules?view={}", view),
        "schedules retrieved",
    )
}

pub fn collections_list_envelope(config: &Config, view: &str) -> Result<Envelope> {
    list_envelope(
        config,
        "collections",
        &format!("v3/collections?view={}", view),
        "collections retrieved",
    )
}

pub fn dcm_connections_list_envelope(config: &Config) -> Result<Envelope> {
    // Known deployments vary; v3 endpoint here is attempted first.
    list_envelope(
        config,
        "dcm_connections",
        "v3/dcm/connections",
        "dcm connections retrieved",
    )
}

fn list_envelope(
    config: &Config,
    resource: &str,
    relative_path: &str,
    message: &str,
) -> Result<Envelope> {
    let api = api_profile(config)?;
    let client = build_client(api)?;
    let token = resolve_bearer_token(api, &client)?;

    let url = format!("{}{}", normalized_base_url(api), relative_path);
    let response = request_json(&client, "GET", &url, &token, None)?;

    Ok(Envelope::ok_with_data(
        message,
        json!({
            "profile": config.profile_name,
            "resource": resource,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn workflow_transfer_owner_envelope(
    config: &Config,
    workflow_id: &str,
    owner_id: &str,
    transfer_schedules: bool,
    apply: bool,
    audit_dir: &Path,
) -> Result<Envelope> {
    let api = api_profile(config)?;
    let applied = apply;

    let payload = json!({
        "ownerId": owner_id,
        "transferSchedules": transfer_schedules
    });

    let mut response = Value::Null;
    if applied {
        let client = build_client(api)?;
        let token = resolve_bearer_token(api, &client)?;
        let url = format!(
            "{}v3/workflows/{}/transfer",
            normalized_base_url(api),
            workflow_id
        );
        response = request_json(&client, "PUT", &url, &token, Some(payload.clone()))?;
    }

    let audit_payload = json!({
        "command": "api workflow transfer owner",
        "timestamp_utc": Utc::now(),
        "profile": config.profile_name,
        "workflow_id": workflow_id,
        "new_owner_id": owner_id,
        "transfer_schedules": transfer_schedules,
        "dry_run": !applied,
        "applied": applied,
        "request_payload": payload,
        "response": response,
        "safety_gate": { "apply": apply }
    });
    let audit_artifact = write_audit_artifact(audit_dir, "api-workflow-transfer", &audit_payload)?;

    Ok(Envelope::ok_with_data(
        if applied {
            "workflow owner transfer executed via API"
        } else {
            "dry-run only: pass --apply to execute workflow owner transfer"
        },
        json!({
            "profile": config.profile_name,
            "workflow_id": workflow_id,
            "new_owner_id": owner_id,
            "transfer_schedules": transfer_schedules,
            "dry_run": !applied,
            "applied": applied,
            "response": response,
            "audit_artifact": audit_artifact,
            "safety_gate": { "apply": apply }
        }),
    ))
}

pub fn schedule_detail_envelope(config: &Config, schedule_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "GET",
        &format!("v3/schedules/{}", schedule_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "schedule details retrieved",
        json!({
            "profile": config.profile_name,
            "schedule_id": schedule_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn schedule_delete_envelope(config: &Config, schedule_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "DELETE",
        &format!("v3/schedules/{}", schedule_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "schedule delete request issued",
        json!({
            "profile": config.profile_name,
            "schedule_id": schedule_id,
            "applied": true,
            "response": response,
        }),
    ))
}

pub fn schedule_create_envelope(config: &Config, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(config, "POST", "v3/schedules", Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "schedule creation requested",
        json!({
            "profile": config.profile_name,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn schedule_update_envelope(
    config: &Config,
    schedule_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "PUT",
        &format!("v3/schedules/{}", schedule_id),
        Some(payload.clone()),
    )?;
    Ok(Envelope::ok_with_data(
        "schedule update requested",
        json!({
            "profile": config.profile_name,
            "schedule_id": schedule_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn schedule_patch_envelope(
    config: &Config,
    schedule_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "PATCH",
        &format!("v3/schedules/{}", schedule_id),
        Some(payload.clone()),
    )?;
    Ok(Envelope::ok_with_data(
        "schedule patch requested",
        json!({
            "profile": config.profile_name,
            "schedule_id": schedule_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_detail_envelope(config: &Config, collection_id: &str) -> Result<Envelope> {
    let response = api_request(
        config,
        "GET",
        &format!("v3/collections/{}", collection_id),
        None,
    )?;
    Ok(Envelope::ok_with_data(
        "collection details retrieved",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn collection_create_envelope(config: &Config, name: &str) -> Result<Envelope> {
    let payload = json!({ "name": name });
    let response = api_request(config, "POST", "v3/collections", Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection creation requested",
        json!({
            "profile": config.profile_name,
            "name": name,
            "response": response,
            "request_payload": payload,
        }),
    ))
}

pub fn collection_update_envelope(
    config: &Config,
    collection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "PUT",
        &format!("v3/collections/{}", collection_id),
        Some(payload.clone()),
    )?;
    Ok(Envelope::ok_with_data(
        "collection update requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_delete_envelope(
    config: &Config,
    collection_id: &str,
    force_delete: bool,
) -> Result<Envelope> {
    let relative = if force_delete {
        format!("v3/collections/{collection_id}?forceDelete=true")
    } else {
        format!("v3/collections/{collection_id}")
    };
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "collection delete requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "force_delete": force_delete,
            "response": response,
        }),
    ))
}

pub fn collection_add_user_envelope(
    config: &Config,
    collection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/collections/{}/users", collection_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection add user requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_remove_user_envelope(
    config: &Config,
    collection_id: &str,
    user_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/collections/{}/users/{}", collection_id, user_id);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "collection remove user requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn collection_add_schedule_envelope(
    config: &Config,
    collection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/collections/{}/schedules", collection_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection add schedule requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_remove_schedule_envelope(
    config: &Config,
    collection_id: &str,
    schedule_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/collections/{}/schedules/{}", collection_id, schedule_id);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "collection remove schedule requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "schedule_id": schedule_id,
            "response": response,
        }),
    ))
}

pub fn collection_add_workflow_envelope(
    config: &Config,
    collection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/collections/{}/workflows", collection_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection add workflow requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_remove_workflow_envelope(
    config: &Config,
    collection_id: &str,
    workflow_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/collections/{}/workflows/{}", collection_id, workflow_id);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "collection remove workflow requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "workflow_id": workflow_id,
            "response": response,
        }),
    ))
}

pub fn collection_add_user_group_envelope(
    config: &Config,
    collection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/collections/{}/userGroups", collection_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection add user group requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_remove_user_group_envelope(
    config: &Config,
    collection_id: &str,
    user_group_id: &str,
) -> Result<Envelope> {
    let relative = format!(
        "v3/collections/{}/userGroups/{}",
        collection_id, user_group_id
    );
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "collection remove user group requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "user_group_id": user_group_id,
            "response": response,
        }),
    ))
}

pub fn collection_update_user_permissions_envelope(
    config: &Config,
    collection_id: &str,
    user_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!(
        "v3/collections/{}/users/{}/permissions",
        collection_id, user_id
    );
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection user permissions update requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "user_id": user_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn collection_update_user_group_permissions_envelope(
    config: &Config,
    collection_id: &str,
    user_group_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!(
        "v3/collections/{}/userGroups/{}/permissions",
        collection_id, user_group_id
    );
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "collection user group permissions update requested",
        json!({
            "profile": config.profile_name,
            "collection_id": collection_id,
            "user_group_id": user_group_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn dcm_connection_lookup_envelope(config: &Config, connection_id: &str) -> Result<Envelope> {
    let relative = format!("v3/dcm/connections/lookup?connectionId={}", connection_id);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "dcm connection lookup performed",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn dcm_connection_share_collaboration_envelope(
    config: &Config,
    connection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/dcm/connections/{}/sharing/collaboration", connection_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "dcm connection collaboration share requested",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn dcm_connection_share_execution_envelope(
    config: &Config,
    connection_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/dcm/connections/{}/sharing/execution", connection_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "dcm connection execution share requested",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credential_share_user_envelope(
    config: &Config,
    credential_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/credentials/{}/users", credential_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "credential shared with user",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credential_unshare_user_envelope(
    config: &Config,
    credential_id: &str,
    user_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/credentials/{}/users/{}", credential_id, user_id);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "credential unshared from user",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn credential_share_user_group_envelope(
    config: &Config,
    credential_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/credentials/{}/userGroups", credential_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "credential shared with user group",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credential_unshare_user_group_envelope(
    config: &Config,
    credential_id: &str,
    user_group_id: &str,
) -> Result<Envelope> {
    let relative = format!(
        "v3/credentials/{}/userGroups/{}",
        credential_id, user_group_id
    );
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "credential unshared from user group",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "user_group_id": user_group_id,
            "response": response,
        }),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn subscriptions_list_envelope(
    config: &Config,
    name: Option<&str>,
    can_share_schedule: Option<bool>,
    default_workflow_credential_id: Option<&str>,
    user_count_gte: Option<u32>,
    user_count_lte: Option<u32>,
    workflow_count_gte: Option<u32>,
    workflow_count_lte: Option<u32>,
) -> Result<Envelope> {
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(value) = name {
        params.push(("name".to_string(), value.to_string()));
    }
    if let Some(value) = can_share_schedule {
        params.push(("canShareSchedule".to_string(), value.to_string()));
    }
    if let Some(value) = default_workflow_credential_id {
        params.push(("defaultWorkflowCredentialId".to_string(), value.to_string()));
    }
    if let Some(value) = user_count_gte {
        params.push(("userCountGreaterThanEquals".to_string(), value.to_string()));
    }
    if let Some(value) = user_count_lte {
        params.push(("userCountLessThanEquals".to_string(), value.to_string()));
    }
    if let Some(value) = workflow_count_gte {
        params.push((
            "workflowCountGreaterThanEquals".to_string(),
            value.to_string(),
        ));
    }
    if let Some(value) = workflow_count_lte {
        params.push(("workflowCountLessThanEquals".to_string(), value.to_string()));
    }

    let param_refs: Vec<(&str, &str)> = params
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let relative = build_query_path("v3/subscriptions", &param_refs);
    let response = api_request(config, "GET", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "subscriptions retrieved",
        json!({
            "profile": config.profile_name,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn subscription_detail_envelope(config: &Config, subscription_id: &str) -> Result<Envelope> {
    let relative = format!("v3/subscriptions/{}", subscription_id);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "subscription details retrieved",
        json!({
            "profile": config.profile_name,
            "subscription_id": subscription_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn subscription_add_envelope(config: &Config, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(config, "POST", "v3/subscriptions", Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "subscription create requested",
        json!({
            "profile": config.profile_name,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn subscription_update_envelope(
    config: &Config,
    subscription_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/subscriptions/{}", subscription_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "subscription update requested",
        json!({
            "profile": config.profile_name,
            "subscription_id": subscription_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn subscription_delete_envelope(config: &Config, subscription_id: &str) -> Result<Envelope> {
    let relative = build_query_path("v3/subscriptions", &[("subscriptionId", subscription_id)]);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "subscription delete requested",
        json!({
            "profile": config.profile_name,
            "subscription_id": subscription_id,
            "response": response,
        }),
    ))
}

pub fn subscription_change_users_envelope(
    config: &Config,
    subscription_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/subscriptions/{}/users", subscription_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "subscription change users requested",
        json!({
            "profile": config.profile_name,
            "subscription_id": subscription_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credentials_list_envelope(
    config: &Config,
    view: Option<&str>,
    user_id: Option<&str>,
    user_group_id: Option<&str>,
) -> Result<Envelope> {
    let mut params = Vec::new();
    if let Some(value) = view {
        params.push(("view", value));
    }
    if let Some(value) = user_id {
        params.push(("userId", value));
    }
    if let Some(value) = user_group_id {
        params.push(("userGroupId", value));
    }
    let relative = build_query_path("v3/credentials", &params);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "credentials retrieved",
        json!({
            "profile": config.profile_name,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn credential_detail_envelope(config: &Config, credential_id: &str) -> Result<Envelope> {
    let relative = format!("v3/credentials/{}", credential_id);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "credential details retrieved",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn credential_add_envelope(config: &Config, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(config, "POST", "v3/credentials", Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "credential add requested",
        json!({
            "profile": config.profile_name,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credential_update_envelope(
    config: &Config,
    credential_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/credentials/{}", credential_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "credential update requested",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn credential_delete_envelope(
    config: &Config,
    credential_id: &str,
    force_delete: bool,
) -> Result<Envelope> {
    let relative = if force_delete {
        format!("v3/credentials/{credential_id}?force=true")
    } else {
        format!("v3/credentials/{}", credential_id)
    };
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "credential delete requested",
        json!({
            "profile": config.profile_name,
            "credential_id": credential_id,
            "force_delete": force_delete,
            "response": response,
        }),
    ))
}

pub fn dcm_admin_connections_list_envelope(
    config: &Config,
    connection_id: Option<&str>,
    visible_by: Option<&str>,
) -> Result<Envelope> {
    let mut params = Vec::new();
    if let Some(value) = connection_id {
        params.push(("connectionId", value));
    }
    if let Some(value) = visible_by {
        params.push(("visibleBy", value));
    }
    let relative = build_query_path("v3/dcm/admin/connections", &params);
    let response = api_request(config, "GET", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "dcm admin connections retrieved",
        json!({
            "profile": config.profile_name,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn dcm_admin_connection_upsert_envelope(config: &Config, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(
        config,
        "POST",
        "v3/dcm/admin/connections",
        Some(payload.clone()),
    )?;

    Ok(Envelope::ok_with_data(
        "dcm admin connection upsert requested",
        json!({
            "profile": config.profile_name,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn dcm_admin_connection_detail_envelope(
    config: &Config,
    connection_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/dcm/admin/connections/{}", connection_id);
    let response = api_request(config, "GET", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "dcm admin connection details retrieved",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn dcm_admin_connection_delete_envelope(
    config: &Config,
    connection_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/dcm/admin/connections/{}", connection_id);
    let response = api_request(config, "DELETE", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "dcm admin connection delete requested",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "response": response,
        }),
    ))
}

pub fn dcm_admin_connection_remove_collaboration_envelope(
    config: &Config,
    connection_id: &str,
) -> Result<Envelope> {
    let relative = format!(
        "v3/dcm/admin/connections/{}/sharing/collaboration",
        connection_id
    );
    let response = api_request(config, "DELETE", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "dcm admin collaboration sharing removed",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "response": response,
        }),
    ))
}

pub fn dcm_admin_connection_remove_execution_envelope(
    config: &Config,
    connection_id: &str,
) -> Result<Envelope> {
    let relative = format!(
        "v3/dcm/admin/connections/{}/sharing/execution",
        connection_id
    );
    let response = api_request(config, "DELETE", &relative, None)?;

    Ok(Envelope::ok_with_data(
        "dcm admin execution sharing removed",
        json!({
            "profile": config.profile_name,
            "connection_id": connection_id,
            "response": response,
        }),
    ))
}

pub fn user_groups_list_envelope(config: &Config) -> Result<Envelope> {
    let response = api_request(config, "GET", "v3/usergroups", None)?;
    Ok(Envelope::ok_with_data(
        "user groups retrieved",
        json!({
            "profile": config.profile_name,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn user_group_detail_envelope(config: &Config, user_group_id: &str) -> Result<Envelope> {
    let relative = format!("v3/usergroups/{}", user_group_id);
    let response = api_request(config, "GET", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "user group details retrieved",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "status": response["status"],
            "body": response["body"],
        }),
    ))
}

pub fn user_group_create_envelope(config: &Config, contract: Value) -> Result<Envelope> {
    let payload = contract.clone();
    let response = api_request(config, "POST", "v3/usergroups", Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "user group creation requested",
        json!({
            "profile": config.profile_name,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_group_update_envelope(
    config: &Config,
    user_group_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/usergroups/{}", user_group_id);
    let response = api_request(config, "PUT", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "user group update requested",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_group_delete_envelope(
    config: &Config,
    user_group_id: &str,
    force_delete: bool,
) -> Result<Envelope> {
    let relative = if force_delete {
        format!("v3/usergroups/{user_group_id}?forceDelete=true")
    } else {
        format!("v3/usergroups/{user_group_id}")
    };
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "user group delete requested",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "force_delete": force_delete,
            "response": response,
        }),
    ))
}

pub fn user_group_add_users_envelope(
    config: &Config,
    user_group_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/usergroups/{}/users", user_group_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "user group add users requested",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_group_remove_user_envelope(
    config: &Config,
    user_group_id: &str,
    user_id: &str,
) -> Result<Envelope> {
    let relative = format!("v3/usergroups/{}/users/{}", user_group_id, user_id);
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "user removed from group",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "user_id": user_id,
            "response": response,
        }),
    ))
}

pub fn user_group_add_ad_group_envelope(
    config: &Config,
    user_group_id: &str,
    contract: Value,
) -> Result<Envelope> {
    let payload = contract.clone();
    let relative = format!("v3/usergroups/{}/activedirectorygroups", user_group_id);
    let response = api_request(config, "POST", &relative, Some(payload.clone()))?;
    Ok(Envelope::ok_with_data(
        "user group add ad group requested",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "request_payload": payload,
            "response": response,
        }),
    ))
}

pub fn user_group_remove_ad_group_envelope(
    config: &Config,
    user_group_id: &str,
    ad_group_sid: &str,
) -> Result<Envelope> {
    let relative = format!(
        "v3/usergroups/{}/activedirectorygroups/{}",
        user_group_id, ad_group_sid
    );
    let response = api_request(config, "DELETE", &relative, None)?;
    Ok(Envelope::ok_with_data(
        "user group remove ad group requested",
        json!({
            "profile": config.profile_name,
            "user_group_id": user_group_id,
            "ad_group_sid": ad_group_sid,
            "response": response,
        }),
    ))
}

fn api_profile(config: &Config) -> Result<&ApiProfile> {
    config
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config missing api section"))
}

fn build_client(api: &ApiProfile) -> Result<Client> {
    let timeout = Duration::from_millis(api.timeout_ms.unwrap_or(30_000));
    let mut builder = Client::builder().timeout(timeout);
    // The new global --no-verify-tls flag (set via `set_no_verify_tls`) is
    // a thread-local override that takes effect only when explicitly opted
    // in. Server profile doesn't carry a per-profile verify_tls field
    // today; the global flag is the only knob.
    if no_verify_tls() {
        builder = builder.danger_accept_invalid_certs(true);
        eprintln!(
            "[warn] TLS certificate verification disabled for Server API transport (--no-verify-tls). Never use this in production."
        );
    }
    let _ = api; // verify_tls is not a per-profile field on Server API today.
    builder.build().context("failed to build API HTTP client")
}

thread_local! {
    static SERVER_APPLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SERVER_NO_VERIFY_TLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SERVER_DEBUG_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Set the Server API apply gate for the current thread.
///
/// When `false` (default), mutating Server API requests short-circuit to a
/// structured dry-run envelope. Mirrors the One API safety contract.
pub fn set_server_apply(apply: bool) {
    SERVER_APPLY.with(|c| c.set(apply));
}

pub fn server_apply() -> bool {
    SERVER_APPLY.with(|c| c.get())
}

/// Disable TLS certificate verification on Server API client builds for
/// this thread. Lab/dev only.
pub fn set_no_verify_tls(disabled: bool) {
    SERVER_NO_VERIFY_TLS.with(|c| c.set(disabled));
}

pub fn no_verify_tls() -> bool {
    SERVER_NO_VERIFY_TLS.with(|c| c.get())
}

pub fn set_debug_trace(on: bool) {
    SERVER_DEBUG_TRACE.with(|c| c.set(on));
}

pub fn debug_trace() -> bool {
    SERVER_DEBUG_TRACE.with(|c| c.get())
}

fn cache_key(api: &ApiProfile) -> String {
    let mode = match api.auth.mode {
        ApiAuthMode::Pat => "pat",
        ApiAuthMode::Oauth2ClientCredentials => "oauth2",
    };
    let client_id = api.auth.client_id.as_deref().unwrap_or_default();
    format!("{}|{}|{}", normalized_base_url(api), mode, client_id)
}

fn resolve_bearer_token(api: &ApiProfile, client: &Client) -> Result<String> {
    match api.auth.mode {
        ApiAuthMode::Pat => api
            .auth
            .pat
            .clone()
            .ok_or_else(|| anyhow::anyhow!("api.auth.pat is required for pat mode")),
        ApiAuthMode::Oauth2ClientCredentials => {
            let key = cache_key(api);
            let cache = TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

            if let Some(tok) = cache
                .lock()
                .map_err(|_| anyhow::anyhow!("token cache lock poisoned"))?
                .get(&key)
                .cloned()
                && tok.expires_at > Instant::now() + Duration::from_secs(10)
            {
                return Ok(tok.token);
            }

            let client_id = api
                .auth
                .client_id
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("api.auth.client_id is required"))?;
            let client_secret = api
                .auth
                .client_secret
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("api.auth.client_secret is required"))?;

            let token_url = format!("{}oauth2/token", normalized_base_url(api));

            let mut form = vec![("grant_type", "client_credentials")];
            if let Some(scope) = api.auth.scope.as_ref() {
                form.push(("scope", scope.as_str()));
            }

            let token_resp = client
                .post(token_url)
                .basic_auth(client_id, Some(client_secret))
                .form(&form)
                .send()
                .context("failed token request")?;

            let status = token_resp.status();
            let body_text = token_resp.text().unwrap_or_default();
            if !status.is_success() {
                bail!(
                    "token request failed with {} ({})",
                    status,
                    status_error_code(status.as_u16())
                );
            }

            let json_val: Value = serde_json::from_str(&body_text)
                .context("failed to parse token response as JSON")?;
            let token = json_val
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("access_token missing in token response"))?
                .to_string();

            let expires_in = json_val
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(300);

            cache
                .lock()
                .map_err(|_| anyhow::anyhow!("token cache lock poisoned"))?
                .insert(
                    key,
                    CachedToken {
                        token: token.clone(),
                        expires_at: Instant::now() + Duration::from_secs(expires_in),
                    },
                );

            Ok(token)
        }
    }
}

fn request_json(
    client: &Client,
    method: &str,
    url: &str,
    bearer_token: &str,
    body: Option<Value>,
) -> Result<Value> {
    let mutating = is_mutating_method(method);
    for attempt in 1..=MAX_RETRIES {
        // DELETE / PATCH were missing — added so the safety lift can route
        // every Server-API mutation through this single chokepoint.
        let req = match method.to_ascii_uppercase().as_str() {
            "GET" => client.get(url),
            "PUT" => client.put(url),
            "POST" => client.post(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => bail!("unsupported method '{}'", method),
        }
        .bearer_auth(bearer_token)
        .header("Accept", "application/json");

        let req = if let Some(body) = body.as_ref() {
            req.json(body)
        } else {
            req
        };

        let response = req
            .send()
            .with_context(|| format!("request failed for {} {}", method, url))?;
        let status = response.status().as_u16();
        let body_text = response.text().unwrap_or_default();
        let body_json = serde_json::from_str::<Value>(&body_text)
            .unwrap_or_else(|_| json!({ "raw": body_text }));

        // Mutating requests must not be silently retried (the server may
        // have processed the first attempt). Only retry GETs.
        let retryable = (status == 429 || status >= 500) && !mutating;
        if retryable && attempt < MAX_RETRIES {
            // Jittered backoff: deterministic 300 ms × attempt with ±20%
            // jitter from clock nanos to avoid thundering-herd retries.
            let base = (attempt as u64) * 300;
            let span = base / 5;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0);
            let offset = if span > 0 { nanos % (span * 2 + 1) } else { 0 };
            let sleep_ms = base.saturating_add(offset).saturating_sub(span);
            thread::sleep(Duration::from_millis(sleep_ms));
            continue;
        }

        if status >= 400 {
            // Use ayx-core's HTTP-status → ErrorCode mapping so anyhow chain
            // classification at the outer dispatcher picks it up cleanly.
            let code = ayx_core::envelope::ErrorCode::from_http_status(status)
                .map(|c| c.as_str())
                .unwrap_or("internal");
            bail!(
                "api request failed [{}] status={} code={} error_code={} url={} body={}",
                method,
                status,
                status_error_code(status),
                code,
                ayx_core::observability::redact_url(url),
                ayx_core::observability::redact_json(&body_json)
            );
        }

        return Ok(json!({ "status": status, "body": body_json }));
    }

    bail!("request exhausted retries")
}

fn status_error_code(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        409 => "conflict",
        429 => "rate_limited",
        500 => "server_error",
        502 => "bad_gateway",
        503 => "service_unavailable",
        504 => "gateway_timeout",
        _ => "http_error",
    }
}

fn normalized_base_url(api: &ApiProfile) -> String {
    let mut base = api.base_url.trim().to_string();
    if !base.ends_with('/') {
        base.push('/');
    }
    base
}

fn build_query_path(base: &str, params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        base.to_string()
    } else {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in params {
            serializer.append_pair(key, value);
        }
        format!("{}?{}", base, serializer.finish())
    }
}

fn download_bytes(client: &Client, bearer_token: &str, url: &str) -> Result<Vec<u8>> {
    for attempt in 1..=MAX_RETRIES {
        let response = client
            .get(url)
            .bearer_auth(bearer_token)
            .send()
            .with_context(|| format!("download failed for GET {}", url))?;
        let status = response.status().as_u16();

        if (status == 429 || status >= 500) && attempt < MAX_RETRIES {
            thread::sleep(Duration::from_millis((attempt as u64) * 300));
            continue;
        }

        if status >= 400 {
            let body_text = response
                .text()
                .unwrap_or_else(|_| "<unable to read body>".to_string());
            bail!(
                "download failed status={} code={} url={} body={}",
                status,
                status_error_code(status),
                url,
                body_text
            );
        }

        let bytes = response
            .bytes()
            .with_context(|| format!("failed to read download response for {}", url))?;
        return Ok(bytes.to_vec());
    }

    bail!("download exhausted retries for {}", url)
}

fn request_multipart<F>(
    client: &Client,
    bearer_token: &str,
    url: &str,
    form_factory: F,
) -> Result<Value>
where
    F: Fn() -> Form,
{
    for attempt in 1..=MAX_RETRIES {
        let response = client
            .post(url)
            .bearer_auth(bearer_token)
            .multipart(form_factory())
            .send()
            .with_context(|| format!("request failed for POST {}", url))?;
        let status = response.status().as_u16();
        let text = response.text().unwrap_or_default();
        let json_val =
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "raw": text }));

        if (status == 429 || status >= 500) && attempt < MAX_RETRIES {
            thread::sleep(Duration::from_millis((attempt as u64) * 300));
            continue;
        }

        if status >= 400 {
            bail!(
                "multipart request failed [{}] status={} code={} url={} body={}",
                "POST",
                status,
                status_error_code(status),
                url,
                json_val
            );
        }

        return Ok(json!({ "status": status, "body": json_val }));
    }

    bail!("multipart request exhausted retries for {}", url)
}

fn api_request(
    config: &Config,
    method: &str,
    relative_path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let api = api_profile(config)?;
    let url_preview = format!("{}{}", normalized_base_url(api), relative_path);

    // Safety gate. Mutating methods short-circuit to a structured dry-run
    // body when --apply is not set. Mirrors the One transport's behavior so
    // the safety story is identical regardless of which surface is in use.
    let mutating = is_mutating_method(method);
    if mutating && !server_apply() {
        if debug_trace() {
            eprintln!(
                "[debug] server✗ {} {} dry-run (no --apply)",
                method,
                ayx_core::observability::redact_url(&url_preview)
            );
        }
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "apply": false,
            "mutating": true,
            "method": method,
            "url": url_preview,
            "would_send": body.unwrap_or(Value::Null),
            "message": format!(
                "Server API {} {} would be sent. Re-run with --apply to execute.",
                method, relative_path
            ),
        }));
    }

    let client = build_client(api)?;
    let token = resolve_bearer_token(api, &client)?;
    let url = format!("{}{}", normalized_base_url(api), relative_path);
    if debug_trace() {
        eprintln!(
            "[debug] server→ {} {} (mutating={}, body={})",
            method,
            ayx_core::observability::redact_url(&url),
            mutating,
            body.is_some()
        );
    }
    let result = request_json(&client, method, &url, &token, body);
    if debug_trace() {
        match &result {
            Ok(v) => {
                let status = v.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
                eprintln!("[debug] server← {} status={}", method, status);
            }
            Err(e) => eprintln!("[debug] server← {} error: {}", method, e),
        }
    }
    result
}

fn is_mutating_method(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::profile::{
        ApiAuth, ApiProfile, Config, MongoDatabases, MongoEmbedded, MongoMode, MongoProfile,
        ServerProfile,
    };

    #[test]
    fn base_url_is_normalized() {
        let api = ApiProfile {
            base_url: "http://localhost/webapi".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Pat,
                pat: Some("token".to_string()),
                client_id: None,
                client_secret: None,
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        };

        assert_eq!(normalized_base_url(&api), "http://localhost/webapi/");
    }

    #[test]
    fn pat_mode_returns_token() {
        let api = ApiProfile {
            base_url: "http://localhost/webapi/".to_string(),
            auth: ApiAuth {
                mode: ApiAuthMode::Pat,
                pat: Some("abc".to_string()),
                client_id: None,
                client_secret: None,
                client_secret_ref: None,
                scope: None,
            },
            timeout_ms: None,
            derived: false,
        };

        let client = build_client(&api).expect("client should build");
        let token = resolve_bearer_token(&api, &client).expect("token should resolve");
        assert_eq!(token, "abc");
    }

    #[test]
    fn transfer_dry_run_writes_audit() {
        let profile = Config {
            profile_name: "test".to_string(),
            server_api: None,
            mongo: MongoProfile {
                mode: MongoMode::Embedded,
                databases: MongoDatabases {
                    gallery_name: "AlteryxGallery".to_string(),
                    service_name: "AlteryxService".to_string(),
                },
                embedded: Some(MongoEmbedded {
                    runtime_settings_path: Some("docs/fixtures/RuntimeSettings.xml".to_string()),
                    alteryx_service_path: None,
                    restore_target_path: None,
                }),
                managed: None,
            },
            api: Some(ApiProfile {
                base_url: "http://localhost/webapi/".to_string(),
                auth: ApiAuth {
                    mode: ApiAuthMode::Pat,
                    pat: Some("abc".to_string()),
                    client_id: None,
                    client_secret: None,
                    client_secret_ref: None,
                    scope: None,
                },
                timeout_ms: Some(1000),
                derived: false,
            }),
            alteryx_one: None,
            observability: None,
            server: Some(ServerProfile {
                webapi_url: "http://localhost/webapi/".to_string(),
                curator_api_key: "abc".to_string(),
                curator_api_secret: "secret".to_string(),
                curator_api_secret_ref: None,
                verify_tls: Some(true),
                derived: false,
            }),
            sqlserver: None,
            upgrade: None,
        };

        let temp_dir = std::env::temp_dir().join(format!(
            "ayx-server-api-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let env =
            workflow_transfer_owner_envelope(&profile, "wf-1", "owner-2", true, false, &temp_dir)
                .expect("dry run should succeed");

        assert_eq!(env.data["dry_run"], true);
        let artifact = env.data["audit_artifact"]
            .as_str()
            .expect("artifact path missing");
        assert!(Path::new(artifact).exists());
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
