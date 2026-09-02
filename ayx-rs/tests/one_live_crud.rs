//! Explicit, serialized live canary lane for current, non-legacy One APIs.
//!
//! This test is intentionally skipped unless both AYX_ONE_LIVE_CRUD=1 and a
//! named AYX_ONE_LIVE_PROFILE are supplied. Credential material is read only
//! from AYX_ONE_BIGQUERY_CREDENTIAL_FILE, is held in the test process, and is
//! written only to an OS-temporary payload file. It is never logged.

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tempfile::TempDir;

const VIEWER_POLICY_ID: &str = "25704008";

fn enabled() -> bool {
    matches!(
        std::env::var("AYX_ONE_LIVE_CRUD").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn run(args: &[String]) -> (bool, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ayx"));
    command
        .args(args)
        // Do not inherit arbitrary environment variables into the child. In
        // particular, fixture paths and cloud credentials stay in this test
        // process and are never accidentally sent to the CLI.
        .envs(
            std::env::vars()
                .filter(|(key, _)| matches!(key.as_str(), "AYX_CONFIG_HOME" | "AYX_PROFILE")),
        );
    if let Ok(profile) = std::env::var("AYX_ONE_LIVE_PROFILE") {
        command.env("AYX_ONE_LIVE_PROFILE", profile);
    }
    let output = command.output().expect("ayx binary should run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn command(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn with_json_full(parts: &[&str]) -> Vec<String> {
    let mut args = command(parts);
    args.extend(["--output".to_string(), "json-full".to_string()]);
    args
}

fn with_apply(parts: &[&str]) -> Vec<String> {
    let mut args = command(parts);
    args.extend([
        "--apply".to_string(),
        "--yes".to_string(),
        "--output".to_string(),
        "json-full".to_string(),
    ]);
    args
}

fn parse(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| panic!("invalid JSON: {error}"))
}

fn require_ok(stdout: &str, stderr: &str, label: &str) -> Value {
    let value = parse(stdout);
    assert!(
        value["ok"].as_bool() == Some(true),
        "{label} returned a non-ok envelope\nstderr: {stderr}"
    );
    assert_success_status(&value, label);
    value
}

fn require_ok_silent(stdout: &str, stderr: &str, label: &str) -> Value {
    // Token responses may contain a one-time secret. The CLI redacts it, but
    // this helper also avoids including captured stdout in assertion text.
    let value = parse(stdout);
    assert!(
        value["ok"].as_bool() == Some(true),
        "{label} returned a non-ok envelope; stderr: {stderr}"
    );
    assert_success_status(&value, label);
    value
}

fn assert_success_status(value: &Value, label: &str) {
    if let Some(status) = value["data"]["status_code"].as_u64() {
        assert!(
            (200..300).contains(&status),
            "{label} reported status {status}"
        );
    }
    if let Some(pages) = value["data"]["page_envelopes"].as_array() {
        for page in pages {
            let status = page["status_code"].as_u64().unwrap_or(0);
            assert!(
                (200..300).contains(&status),
                "{label} reported page status {status}"
            );
        }
    }
}

fn response(value: &Value) -> Option<&Value> {
    value
        .pointer("/data/response")
        .or_else(|| value.pointer("/data"))
}

fn collection_items(value: &Value) -> Vec<&Value> {
    [
        "/data/items",
        "/data/response/items",
        "/data/response/data",
        "/data/data",
    ]
    .iter()
    .find_map(|path| value.pointer(path).and_then(Value::as_array))
    .map(|items| items.iter().collect())
    .unwrap_or_default()
}

fn id_from_object(object: &Value) -> Option<String> {
    [
        "id",
        "groupId",
        "planId",
        "scheduleId",
        "workflowId",
        "tokenId",
    ]
    .iter()
    .find_map(|key| match object.get(*key) {
        Some(Value::String(id)) => Some(id.clone()),
        Some(Value::Number(id)) => Some(id.to_string()),
        _ => None,
    })
}

fn id_from(value: &Value) -> Option<String> {
    let object = response(value)?;
    id_from_object(object).or_else(|| object.get("tokenInfo").and_then(id_from_object))
}

fn collection_ids(value: &Value) -> Vec<String> {
    collection_items(value)
        .into_iter()
        .filter_map(id_from_object)
        .collect()
}

fn first_id(value: &Value) -> Option<String> {
    collection_items(value).into_iter().find_map(id_from_object)
}

fn write_payload(temp: &TempDir, name: &str, value: &Value) -> String {
    let path = temp.path().join(name);
    fs::write(
        &path,
        serde_json::to_vec_pretty(value).expect("payload JSON"),
    )
    .expect("payload");
    path.to_string_lossy().into_owned()
}

fn unique_prefix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    format!("ayx-live-canary-{millis}")
}

fn is_scope_blocked(stdout: &str, stderr: &str) -> bool {
    let text = format!("{stdout}\n{stderr}");
    text.contains("\"error_code\": \"permission_denied\"")
        || text.contains("\"status_code\": 403")
        || text.contains("AccessControlException")
}

struct Cleanup {
    commands: Vec<Vec<String>>,
}

impl Cleanup {
    fn push(&mut self, command: Vec<String>) {
        self.commands.push(command);
    }

    fn run_now(&mut self) {
        while let Some(args) = self.commands.pop() {
            let (success, stdout, stderr) = run(&args);
            assert!(
                success,
                "canary cleanup failed\nargs: {args:?}\nstderr: {stderr}\nstdout was intentionally suppressed"
            );
            let _ = stdout;
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        while let Some(args) = self.commands.pop() {
            let (success, _stdout, stderr) = run(&args);
            if !success {
                eprintln!("canary cleanup failed for {:?}: {}", args, stderr.trim());
            }
        }
    }
}

fn live_read(parts: &[&str], label: &str) -> Value {
    let args = with_json_full(parts);
    let (success, stdout, stderr) = run(&args);
    assert!(success, "{label} failed\nstderr: {stderr}");
    require_ok(&stdout, &stderr, label)
}

fn live_read_silent(parts: &[&str], label: &str) -> Value {
    let args = with_json_full(parts);
    let (success, stdout, stderr) = run(&args);
    assert!(success, "{label} failed\nstderr: {stderr}");
    require_ok_silent(&stdout, &stderr, label)
}

fn build_bigquery_payload(temp: &TempDir, prefix: &str, template: &Value) -> Option<String> {
    let path = std::env::var_os("AYX_ONE_BIGQUERY_CREDENTIAL_FILE")?;
    let fixture = fs::read_to_string(path).expect("BigQuery credential fixture must be readable");
    let credential: Value = serde_json::from_str(&fixture)
        .expect("BigQuery credential fixture must contain a JSON service-account key");
    let mut body = template.clone();
    body["name"] = json!(format!("{prefix}-bigquery"));
    body["description"] = json!("Disposable Alteryx One BigQuery live validation connection");
    body["credentialType"] = json!("apiKey");
    body["credentials"] = json!([{"apiKey": fixture}]);
    if credential.get("project_id").is_some() {
        body["params"]["projectId"] = credential["project_id"].clone();
    }
    Some(write_payload(temp, "bigquery-connection.json", &body))
}

#[test]
fn one_live_crud_canary_matrix() {
    if !enabled() {
        eprintln!("one_live_crud: skipped; set AYX_ONE_LIVE_CRUD=1 to enable");
        return;
    }
    let profile = std::env::var("AYX_ONE_LIVE_PROFILE")
        .expect("AYX_ONE_LIVE_CRUD requires AYX_ONE_LIVE_PROFILE");
    assert!(
        !profile.trim().is_empty(),
        "live CRUD profile must be named"
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let prefix = unique_prefix();
    let mut cleanup = Cleanup {
        commands: Vec::new(),
    };

    let (success, _stdout, stderr) = run(&command(&["--version"]));
    assert!(success, "version failed: {stderr}");
    let profile_status = live_read(&["profile", "current"], "profile current");
    assert!(profile_status.to_string().contains(&profile));
    live_read_silent(&["one", "auth", "status"], "auth status");
    let workspace = live_read(&["one", "workspace", "current"], "workspace current");
    let workspace_response = response(&workspace).expect("workspace response");
    let workspace_id = workspace_response["id"]
        .as_i64()
        .or_else(|| workspace_response["workspace"]["id"].as_i64())
        .expect("current workspace response must include numeric id")
        .to_string();
    let workspace_name = workspace_response["name"]
        .as_str()
        .or_else(|| workspace_response["displayName"].as_str())
        .unwrap_or("unknown");
    eprintln!(
        "one_live_crud: profile={profile}, workspace={workspace_name} ({workspace_id}), prefix={prefix}"
    );

    let workflows_before = live_read(&["one", "workflows", "list", "--all"], "workflows baseline");
    let connections_before = live_read(
        &["one", "connections", "list", "--all"],
        "connections baseline",
    );
    let groups_before = live_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups baseline",
    );
    let tokens_before = live_read_silent(&["one", "token"], "tokens baseline");
    let role_list = live_read_silent(&["one", "role", "list"], "roles baseline");
    assert!(
        role_list.to_string().contains(VIEWER_POLICY_ID),
        "Viewer policy {VIEWER_POLICY_ID} was not present in the live role response"
    );

    let template = live_read(
        &[
            "one",
            "connections",
            "connector-metadata",
            "template",
            "bigquery",
        ],
        "BigQuery connector template",
    );
    let connection_path = build_bigquery_payload(&temp, &prefix, &template["data"]);
    if let Some(connection_path) = connection_path {
        let (success, stdout, stderr) = run(&with_json_full(&[
            "one",
            "connections",
            "dry-run",
            "--body",
            &connection_path,
        ]));
        if !success && !is_scope_blocked(&stdout, &stderr) {
            panic!("connection server dry-run failed unexpectedly: {stderr}");
        }
        if !success {
            eprintln!("one_live_crud: connection server dry-run blocked_by_scope");
        }

        let dry = live_read(
            &["one", "connections", "create", "--body", &connection_path],
            "connection create dry-run",
        );
        assert_eq!(dry["data"]["dry_run"], true);
        let (success, stdout, stderr) = run(&with_apply(&[
            "one",
            "connections",
            "create",
            "--body",
            &connection_path,
        ]));
        if !success {
            if is_scope_blocked(&stdout, &stderr) {
                eprintln!("one_live_crud: connection CRUD blocked_by_scope");
            } else {
                panic!("connection create failed: {stderr}");
            }
        } else {
            let created = require_ok(&stdout, &stderr, "connection create");
            let connection_id = id_from(&created).expect("connection create must return an ID");
            cleanup.push(with_apply(&[
                "one",
                "connections",
                "delete",
                &connection_id,
            ]));
            live_read(
                &["one", "connections", "detail", &connection_id],
                "connection detail",
            );
            let status = live_read(
                &["one", "connections", "status", &connection_id],
                "connection status",
            );
            assert_eq!(status["data"]["response"]["result"], "SUCCESS");
        }
    } else {
        eprintln!(
            "one_live_crud: connection CRUD blocked_by_fixture (set AYX_ONE_BIGQUERY_CREDENTIAL_FILE to a local service-account JSON key)"
        );
        let body = write_payload(
            &temp,
            "bigquery-template-canary.json",
            &json!({
                "name": format!("{prefix}-bigquery"),
                "description": "fixture unavailable",
                "type": "jdbc",
                "vendor": "bigquery",
                "vendorName": "bigquery",
                "credentialType": "apiKey",
                "params": {"projectId": "<string>"}
            }),
        );
        let (success, stdout, stderr) = run(&with_json_full(&[
            "one",
            "connections",
            "dry-run",
            "--body",
            &body,
        ]));
        assert!(
            success || is_scope_blocked(&stdout, &stderr),
            "connection dry-run was neither accepted nor scope-blocked: {stderr}"
        );
    }

    let group_body = write_payload(
        &temp,
        "group.json",
        &json!({"name": format!("{prefix}-group"), "members": []}),
    );
    let dry = live_read(
        &[
            "one",
            "workspace",
            "create-group",
            &workspace_id,
            "--body",
            &group_body,
        ],
        "group create dry-run",
    );
    assert_eq!(dry["data"]["dry_run"], true);
    let created = live_read(
        &[
            "one",
            "workspace",
            "create-group",
            &workspace_id,
            "--body",
            &group_body,
            "--apply",
            "--yes",
        ],
        "group create",
    );
    let group_id = id_from(&created).expect("group create must return an ID");
    cleanup.push(with_apply(&[
        "one",
        "workspace",
        "delete-group",
        &workspace_id,
        &group_id,
    ]));

    let group_roles_add = write_payload(
        &temp,
        "group-roles-add.json",
        &json!({"roleIds": [VIEWER_POLICY_ID.parse::<u64>().expect("policy id")]}),
    );
    let dry = live_read(
        &[
            "one",
            "workspace",
            "set-group-roles",
            &workspace_id,
            &group_id,
            "--body",
            &group_roles_add,
        ],
        "group role assignment dry-run",
    );
    assert_eq!(dry["data"]["dry_run"], true);
    let (assigned, stdout, stderr) = run(&with_apply(&[
        "one",
        "workspace",
        "set-group-roles",
        &workspace_id,
        &group_id,
        "--body",
        &group_roles_add,
    ]));
    if assigned {
        require_ok(&stdout, &stderr, "group role assignment");

        let (verified, verify_stdout, verify_stderr) = run(&with_json_full(&[
            "one",
            "role",
            "list-assignments",
            VIEWER_POLICY_ID,
        ]));
        if !verified {
            assert!(
                is_scope_blocked(&verify_stdout, &verify_stderr),
                "role assignment verification failed unexpectedly: {verify_stderr}"
            );
            eprintln!("one_live_crud: role assignment verification blocked_by_scope (403)");
        }

        let group_roles_remove =
            write_payload(&temp, "group-roles-remove.json", &json!({"roleIds": []}));
        let dry = live_read(
            &[
                "one",
                "workspace",
                "set-group-roles",
                &workspace_id,
                &group_id,
                "--body",
                &group_roles_remove,
            ],
            "group role removal dry-run",
        );
        assert_eq!(dry["data"]["dry_run"], true);
        let (removed, remove_stdout, remove_stderr) = run(&with_apply(&[
            "one",
            "workspace",
            "set-group-roles",
            &workspace_id,
            &group_id,
            "--body",
            &group_roles_remove,
        ]));
        if !removed {
            eprintln!(
                "one_live_crud: role removal failed; manual cleanup required for group id {group_id}"
            );
            std::mem::forget(cleanup);
            panic!("role removal failed: {remove_stderr}");
        }
        require_ok(&remove_stdout, &remove_stderr, "group role removal");
    } else if is_scope_blocked(&stdout, &stderr) {
        eprintln!("one_live_crud: role assignment blocked_by_scope");
    } else {
        panic!("role assignment failed unexpectedly: {stderr}");
    }

    let token_body = write_payload(
        &temp,
        "token.json",
        &json!({
            "name": format!("ayx-live-canary-{prefix}"),
            "description": "Disposable Alteryx One live validation token",
            "lifetimeSeconds": 86400
        }),
    );
    let dry = live_read_silent(
        &["one", "token", "create", "--body", &token_body],
        "token create dry-run",
    );
    assert_eq!(dry["data"]["dry_run"], true);
    let created = live_read_silent(
        &[
            "one",
            "token",
            "create",
            "--body",
            &token_body,
            "--apply",
            "--yes",
        ],
        "token create",
    );
    let _secret = created.pointer("/data/response/tokenValue");
    let token_id = id_from(&created).or_else(|| {
        let tokens = live_read_silent(&["one", "token"], "tokens after create");
        collection_items(&tokens).into_iter().find_map(|item| {
            (item["description"].as_str() == Some("Disposable Alteryx One live validation token"))
                .then(|| id_from_object(item))
                .flatten()
        })
    });
    let token_id = token_id.expect("token create must return or expose an ID");
    cleanup.push(with_apply(&["one", "token", "delete", &token_id]));
    let tokens_after_create = live_read_silent(&["one", "token"], "tokens after create");
    assert!(collection_ids(&tokens_after_create).contains(&token_id));
    live_read_silent(&["one", "token", "detail", &token_id], "token detail");

    if let Some(source_id) = first_id(&workflows_before) {
        live_read_silent(
            &["one", "workflows", "detail", &source_id],
            "workflow detail",
        );
        live_read_silent(
            &["one", "workflows", "dependencies", &source_id],
            "workflow dependencies",
        );
        live_read_silent(
            &["one", "workflows", "engines", &source_id],
            "workflow engines",
        );
        live_read_silent(&["one", "workflows", "tools"], "workflow tools");

        let copy_name = format!("{prefix}-workflow");
        let dry = live_read(
            &["one", "workflows", "copy", &source_id, "--name", &copy_name],
            "workflow copy dry-run",
        );
        assert_eq!(dry["data"]["dry_run"], true);
        let created = live_read(
            &[
                "one",
                "workflows",
                "copy",
                &source_id,
                "--name",
                &copy_name,
                "--apply",
                "--yes",
            ],
            "workflow copy",
        );
        let copy_id = id_from(&created).expect("workflow copy must return an ID");
        cleanup.push(with_apply(&["one", "workflows", "delete", &copy_id]));
        live_read_silent(
            &["one", "workflows", "detail", &copy_id],
            "workflow copy detail",
        );
    } else {
        eprintln!("one_live_crud: workflow copy blocked_by_fixture (no source workflow)");
    }

    cleanup.run_now();

    let groups_after = live_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups final",
    );
    let connections_after = live_read(
        &["one", "connections", "list", "--all"],
        "connections final",
    );
    let tokens_after = live_read_silent(&["one", "token"], "tokens final");
    let workflows_after = live_read(&["one", "workflows", "list", "--all"], "workflows final");
    assert_eq!(
        collection_ids(&groups_after),
        collection_ids(&groups_before)
    );
    assert_eq!(
        collection_ids(&connections_after),
        collection_ids(&connections_before)
    );
    assert_eq!(
        collection_ids(&tokens_after),
        collection_ids(&tokens_before)
    );
    assert_eq!(
        collection_ids(&workflows_after),
        collection_ids(&workflows_before)
    );
    eprintln!("one_live_crud: completed with zero canary residue");
}
