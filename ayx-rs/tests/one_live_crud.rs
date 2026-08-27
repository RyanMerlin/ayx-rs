//! Explicit, serialized live CRUD canary lane.
//!
//! This test is intentionally skipped unless AYX_ONE_LIVE_CRUD is enabled and
//! an explicit named profile is supplied. It never uses existing resources as
//! mutation targets.

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tempfile::TempDir;

fn enabled() -> bool {
    matches!(
        std::env::var("AYX_ONE_LIVE_CRUD").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn run(args: &[String]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .envs(std::env::vars().filter(|(key, _)| key == "AYX_CONFIG_HOME" || key == "AYX_PROFILE"))
        .output()
        .expect("ayx binary should run");
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
        "json".to_string(),
    ]);
    args
}

fn with_apply_full(parts: &[&str]) -> Vec<String> {
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
    serde_json::from_str(stdout).unwrap_or_else(|error| panic!("invalid JSON: {error}\n{stdout}"))
}

fn require_ok(stdout: &str, stderr: &str, label: &str) -> Value {
    let value = parse(stdout);
    assert!(
        value["ok"].as_bool() == Some(true),
        "{label} returned a non-ok envelope\nstdout: {stdout}\nstderr: {stderr}"
    );
    if let Some(status) = value["data"]["status_code"].as_u64() {
        assert!(
            (200..300).contains(&status),
            "{label} reported ok with non-2xx status {status}\nstdout: {stdout}"
        );
    }
    if let Some(pages) = value["data"]["page_envelopes"].as_array() {
        for page in pages {
            let status = page["status_code"].as_u64().unwrap_or(0);
            assert!(
                (200..300).contains(&status),
                "{label} reported ok with non-2xx page status {status}\nstdout: {stdout}"
            );
        }
    }
    value
}

fn response(value: &Value) -> Option<&Value> {
    value
        .pointer("/data/response")
        .or_else(|| value.pointer("/data"))
        .or_else(|| value.is_object().then_some(value))
}

fn items(value: &Value) -> Vec<&Value> {
    value
        .pointer("/data/items")
        .or_else(|| value.pointer("/data/response/items"))
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn id_from(value: &Value) -> Option<String> {
    let object = response(value)?;
    ["id", "groupId", "planId", "scheduleId", "workflowId"]
        .iter()
        .find_map(|key| match object.get(*key) {
            Some(Value::String(id)) => Some(id.clone()),
            Some(Value::Number(id)) => Some(id.to_string()),
            _ => None,
        })
}

fn list_ids(value: &Value) -> Vec<String> {
    items(value).into_iter().filter_map(id_from).collect()
}

fn first_id(value: &Value) -> Option<String> {
    items(value).into_iter().find_map(id_from)
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
    format!("ayx-agent-canary-{millis}")
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
                "CRUD cleanup failed\nargs: {args:?}\nstdout: {stdout}\nstderr: {stderr}"
            );
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        while let Some(args) = self.commands.pop() {
            let (success, stdout, stderr) = run(&args);
            if !success {
                eprintln!(
                    "CRUD cleanup failed\nargs: {args:?}\nstdout: {stdout}\nstderr: {stderr}"
                );
            }
        }
    }
}

fn live_read(parts: &[&str], label: &str) -> Value {
    // Verification reads need page status and raw fields; ordinary agent
    // inspection should continue to use compact json.
    let args = with_json_full(parts);
    let (success, stdout, stderr) = run(&args);
    assert!(
        success,
        "{label} failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    require_ok(&stdout, &stderr, label)
}

fn live_full_read(parts: &[&str], label: &str) -> Value {
    let args = with_json_full(parts);
    let (success, stdout, stderr) = run(&args);
    assert!(
        success,
        "{label} failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    require_ok(&stdout, &stderr, label)
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

    let version_args = command(&["--version"]);
    let (success, stdout, stderr) = run(&version_args);
    assert!(
        success,
        "version failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.starts_with("ayx "),
        "unexpected version output: {stdout}"
    );
    let profile_status = live_full_read(&["profile", "current"], "profile current");
    assert!(profile_status.to_string().contains(&profile));
    live_read(&["one", "auth", "status"], "auth status");
    let workspace = live_full_read(&["one", "workspace", "current"], "workspace current");
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

    let discovery = live_full_read(&["discover", "one", "--deep"], "discover one");
    assert!(discovery.pointer("/data/tree/subcommands").is_some());
    let baseline_groups = live_full_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups baseline",
    );
    let baseline_plans = live_read(&["one", "plans", "list"], "plans baseline");
    let baseline_schedules = live_read(&["one", "scheduling", "list"], "schedules baseline");
    let baseline_workflows = live_read(&["one", "workflows", "list"], "workflows baseline");
    let baseline_connections = live_read(&["one", "connections", "list"], "connections baseline");

    // Groups: create -> list/read -> update -> verify -> delete.
    let group_payload = write_payload(
        &temp,
        "group.json",
        &serde_json::json!({"name": format!("{prefix}-group"), "members": []}),
    );
    let dry = with_json_full(&[
        "one",
        "workspace",
        "create-group",
        &workspace_id,
        "--body",
        &group_payload,
    ]);
    let (success, stdout, stderr) = run(&dry);
    assert!(
        success,
        "group dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    let dry_value = require_ok(&stdout, &stderr, "group create dry-run");
    assert_eq!(dry_value["data"]["dry_run"], true);
    let applied = with_apply_full(&[
        "one",
        "workspace",
        "create-group",
        &workspace_id,
        "--body",
        &group_payload,
    ]);
    let (success, stdout, stderr) = run(&applied);
    assert!(
        success,
        "group create failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    let group = require_ok(&stdout, &stderr, "group create");
    let group_id = id_from(&group).expect("group create must return an ID");
    cleanup.push(with_apply(&[
        "one",
        "workspace",
        "delete-group",
        &workspace_id,
        &group_id,
    ]));
    live_full_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups after create",
    );
    let group_update_payload = write_payload(
        &temp,
        "group-update.json",
        &serde_json::json!({"name": format!("{prefix}-group-updated"), "members": []}),
    );
    let dry = with_json_full(&[
        "one",
        "workspace",
        "update-group",
        &workspace_id,
        &group_id,
        "--body",
        &group_update_payload,
    ]);
    let (success, stdout, stderr) = run(&dry);
    assert!(
        success,
        "group update dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        require_ok(&stdout, &stderr, "group update dry-run")["data"]["dry_run"],
        true
    );
    let applied = with_apply_full(&[
        "one",
        "workspace",
        "update-group",
        &workspace_id,
        &group_id,
        "--body",
        &group_update_payload,
    ]);
    let (success, stdout, stderr) = run(&applied);
    assert!(
        success,
        "group update failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    require_ok(&stdout, &stderr, "group update");
    let after_group = live_full_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups after update",
    );
    assert!(
        after_group
            .to_string()
            .contains(&format!("{prefix}-group-updated"))
    );

    // Plans: create -> detail -> update -> verify -> delete.
    let plan_payload = write_payload(
        &temp,
        "plan.json",
        &serde_json::json!({"name": format!("{prefix}-plan")}),
    );
    let dry = with_json_full(&["one", "plans", "create", "--body", &plan_payload]);
    let (success, stdout, stderr) = run(&dry);
    assert!(
        success,
        "plan dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        require_ok(&stdout, &stderr, "plan create dry-run")["data"]["dry_run"],
        true
    );
    let applied = with_apply_full(&["one", "plans", "create", "--body", &plan_payload]);
    let (success, stdout, stderr) = run(&applied);
    assert!(
        success,
        "plan create failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    let plan = require_ok(&stdout, &stderr, "plan create");
    let plan_id = id_from(&plan).expect("plan create must return an ID");
    cleanup.push(with_apply(&["one", "plans", "delete", &plan_id]));
    live_read(&["one", "plans", "detail", &plan_id], "plan detail");
    let plan_update_payload = write_payload(
        &temp,
        "plan-update.json",
        &serde_json::json!({"name": format!("{prefix}-plan-updated")}),
    );
    let dry = with_json_full(&[
        "one",
        "plans",
        "update",
        &plan_id,
        "--body",
        &plan_update_payload,
    ]);
    let (success, stdout, stderr) = run(&dry);
    assert!(
        success,
        "plan update dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        require_ok(&stdout, &stderr, "plan update dry-run")["data"]["dry_run"],
        true
    );
    let applied = with_apply_full(&[
        "one",
        "plans",
        "update",
        &plan_id,
        "--body",
        &plan_update_payload,
    ]);
    let (success, stdout, stderr) = run(&applied);
    assert!(
        success,
        "plan update failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    require_ok(&stdout, &stderr, "plan update");
    assert!(
        live_full_read(
            &["one", "plans", "detail", &plan_id],
            "plan detail after update"
        )
        .to_string()
        .contains(&format!("{prefix}-plan-updated"))
    );

    // Workflows: copy an existing workflow, inspect it, then delete the copy.
    if let Some(source_id) = first_id(&baseline_workflows) {
        let copy_name = format!("{prefix}-workflow");
        let dry = with_json_full(&["one", "workflows", "copy", &source_id, "--name", &copy_name]);
        let (success, stdout, stderr) = run(&dry);
        assert!(
            success,
            "workflow copy dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            require_ok(&stdout, &stderr, "workflow copy dry-run")["data"]["dry_run"],
            true
        );
        let applied =
            with_apply_full(&["one", "workflows", "copy", &source_id, "--name", &copy_name]);
        let (success, stdout, stderr) = run(&applied);
        assert!(
            success,
            "workflow copy failed\nstdout: {stdout}\nstderr: {stderr}"
        );
        let copy = require_ok(&stdout, &stderr, "workflow copy");
        let copy_id = id_from(&copy).expect("workflow copy must return an ID");
        cleanup.push(with_apply(&["one", "workflows", "delete", &copy_id]));
        live_read(
            &["one", "workflows", "detail", &copy_id],
            "workflow copy detail",
        );
    } else {
        eprintln!(
            "one_live_crud: workflow copy classified blocked_by_fixture (no source workflow)"
        );
    }

    // Connections are deliberately dry-run-only until a disposable connector
    // fixture with valid credentials is supplied.
    let connection_payload = write_payload(&temp, "connection.json", &serde_json::json!({}));
    let connection_dry = with_json_full(&[
        "one",
        "connections",
        "create",
        "--body",
        &connection_payload,
    ]);
    let (success, stdout, stderr) = run(&connection_dry);
    if success {
        assert_eq!(
            require_ok(&stdout, &stderr, "connection create dry-run")["data"]["dry_run"],
            true
        );
        eprintln!(
            "one_live_crud: connections CRUD classified blocked_by_fixture (no disposable connector credentials configured)"
        );
    } else {
        eprintln!(
            "one_live_crud: connections CRUD classified blocked_by_fixture (dry-run rejected body)\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    let _ = baseline_connections;

    // Schedules require a valid workflow target and remain gated by the live
    // API's workspace tier. Exercise the dry-run shape when a source exists.
    if let Some(workflow_id) = first_id(&baseline_workflows) {
        let schedule_payload = write_payload(
            &temp,
            "schedule.json",
            &serde_json::json!({
                "name": format!("{prefix}-schedule"),
                "tasks": [{"runWorkflow": {"workflowId": workflow_id}}],
                "triggers": [{"timeBased": {"daily": {"hourOfDay": 0, "minuteOfHour": 0, "onlyWorkingDays": false, "utcStartDateTime": "2030-01-01T00:00:00Z", "utcEndDateTime": "2030-01-02T00:00:00Z"}, "timezone": "UTC"}}]
            }),
        );
        let dry = with_json_full(&["one", "scheduling", "create", "--body", &schedule_payload]);
        let (success, stdout, stderr) = run(&dry);
        if success {
            assert_eq!(
                require_ok(&stdout, &stderr, "schedule create dry-run")["data"]["dry_run"],
                true
            );
            let applied =
                with_apply_full(&["one", "scheduling", "create", "--body", &schedule_payload]);
            let (success, stdout, stderr) = run(&applied);
            if success {
                let schedule = require_ok(&stdout, &stderr, "schedule create");
                let schedule_id = id_from(&schedule).expect("schedule create must return an ID");
                cleanup.push(with_apply(&["one", "scheduling", "delete", &schedule_id]));
                live_read(
                    &["one", "scheduling", "detail", &schedule_id],
                    "schedule detail",
                );
                let update_payload = write_payload(
                    &temp,
                    "schedule-update.json",
                    &serde_json::json!({
                        "name": format!("{prefix}-schedule-updated"),
                        "tasks": [{"runWorkflow": {"workflowId": workflow_id}}],
                        "triggers": [{"timeBased": {"daily": {"hourOfDay": 0, "minuteOfHour": 0, "onlyWorkingDays": false, "utcStartDateTime": "2030-01-01T00:00:00Z", "utcEndDateTime": "2030-01-02T00:00:00Z"}, "timezone": "UTC"}}]
                    }),
                );
                let dry = with_json_full(&[
                    "one",
                    "scheduling",
                    "update",
                    &schedule_id,
                    "--body",
                    &update_payload,
                ]);
                let (success, stdout, stderr) = run(&dry);
                assert!(
                    success,
                    "schedule update dry-run failed\nstdout: {stdout}\nstderr: {stderr}"
                );
                assert_eq!(
                    require_ok(&stdout, &stderr, "schedule update dry-run")["data"]["dry_run"],
                    true
                );
                let applied = with_apply_full(&[
                    "one",
                    "scheduling",
                    "update",
                    &schedule_id,
                    "--body",
                    &update_payload,
                ]);
                let (success, stdout, stderr) = run(&applied);
                assert!(
                    success,
                    "schedule update failed\nstdout: {stdout}\nstderr: {stderr}"
                );
                require_ok(&stdout, &stderr, "schedule update");
                let disable = with_apply_full(&["one", "scheduling", "disable", &schedule_id]);
                let (success, stdout, stderr) = run(&disable);
                assert!(
                    success,
                    "schedule disable failed\nstdout: {stdout}\nstderr: {stderr}"
                );
                require_ok(&stdout, &stderr, "schedule disable");
                let enable = with_apply_full(&["one", "scheduling", "enable", &schedule_id]);
                let (success, stdout, stderr) = run(&enable);
                assert!(
                    success,
                    "schedule enable failed\nstdout: {stdout}\nstderr: {stderr}"
                );
                require_ok(&stdout, &stderr, "schedule enable");
            } else if stderr.contains("permission_denied") || stderr.contains("not_found") {
                eprintln!(
                    "one_live_crud: schedule CRUD classified blocked_by_scope\nstdout: {stdout}\nstderr: {stderr}"
                );
            } else {
                panic!(
                    "schedule create failed after successful dry-run\nstdout: {stdout}\nstderr: {stderr}"
                );
            }
        } else {
            eprintln!(
                "one_live_crud: schedule CRUD classified blocked_by_scope/fixture\nstdout: {stdout}\nstderr: {stderr}"
            );
        }
    } else {
        eprintln!(
            "one_live_crud: schedule CRUD classified blocked_by_fixture (no workflow target)"
        );
    }

    // Run cleanup before the final assertions; Drop remains as the panic-path
    // fallback if any assertion above fails.
    cleanup.run_now();
    let final_groups = live_full_read(
        &["one", "workspace", "groups", &workspace_id],
        "groups final",
    );
    let final_plans = live_read(&["one", "plans", "list"], "plans final");
    let final_schedules = live_read(&["one", "scheduling", "list"], "schedules final");
    let final_workflows = live_read(&["one", "workflows", "list"], "workflows final");
    assert_eq!(
        list_ids(&final_groups),
        list_ids(&baseline_groups),
        "group baseline changed"
    );
    assert_eq!(
        list_ids(&final_plans),
        list_ids(&baseline_plans),
        "plan baseline changed"
    );
    assert_eq!(
        list_ids(&final_schedules),
        list_ids(&baseline_schedules),
        "schedule baseline changed"
    );
    assert_eq!(
        list_ids(&final_workflows),
        list_ids(&baseline_workflows),
        "workflow baseline changed"
    );
    eprintln!("one_live_crud: completed with zero canary residue");
}
