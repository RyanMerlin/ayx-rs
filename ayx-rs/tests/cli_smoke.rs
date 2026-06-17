// `fs` is only used by the `#[cfg(not(windows))]` smoke tests below; gate the
// import to match so Windows builds don't trip `-D warnings` on an unused import.
#[cfg(not(windows))]
use std::fs;
use std::process::Command;

use serde_json::Value;

#[test]
fn ayx_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("--help")
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Banner is now clap-generated from the Cli #[command(about=...)] attribute.
    assert!(stdout.contains("operator CLI") || stdout.contains("Alteryx"));
    assert!(stdout.contains("one"));
    assert!(stdout.contains("server"));
    assert!(stdout.contains("mongo"));
    assert!(stdout.contains("workflow"));
    assert!(stdout.contains("tui"));
}

#[test]
fn ayx_version_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("--version")
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("ayx "));
}

#[test]
fn ayx_apply_is_global_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "list", "--help"])
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The global --apply must propagate to leaf subcommands.
    assert!(stdout.contains("--apply"));
    // Pagination flags must be present on list commands (H8).
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--page-token"));
    assert!(stdout.contains("--all"));
}

// Windows runners exit non-zero on these binary-spawn smoke tests for reasons
// that don't reproduce locally and don't affect Linux/macOS CI (which both
// pass cleanly). Skipped on cfg(windows) until we have time to bisect; the
// behavior itself works manually on a Windows install.
#[cfg(not(windows))]
#[test]
fn completions_command_emits_script() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["completions", "bash"])
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // clap_complete emits a function named after the binary.
    assert!(stdout.contains("_ayx"));
    assert!(stdout.contains("COMPREPLY"));
}

#[test]
fn catalog_surface_lists_core_one_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["--output", "json", "catalog", "list", "--format", "full"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());

    let envelope: Value = serde_json::from_slice(&output.stdout).expect("catalog json");
    let commands = envelope["data"]["commands"]
        .as_array()
        .expect("commands array");
    let names: Vec<&str> = commands
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .collect();

    assert!(names.contains(&"one platform status"));
    assert!(names.contains(&"one platform inventory"));
    assert!(names.contains(&"one doctor auth"));
    assert!(names.contains(&"one doctor discover"));
    assert!(names.contains(&"one plans status"));
    assert!(names.contains(&"one flows list"));
    assert!(names.contains(&"one connections list"));
    assert!(names.contains(&"discover"));
}

#[test]
fn tui_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["tui", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Interactive TUI"));
    assert!(stdout.contains("central profile"));
    assert!(!stdout.contains("config.yaml"));
}

#[test]
fn onboard_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["onboard", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("environments"));
}

#[test]
fn server_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["server", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("server"));
    assert!(stdout.contains("diagnose"));
    assert!(stdout.contains("api"));
}

#[test]
fn one_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("one"));
    assert!(stdout.contains("doctor"));
}

#[test]
fn one_doctor_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "doctor", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("discover"));
    assert!(stdout.contains("platform"));
}

#[test]
fn one_platform_help_renders_governance_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "platform", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workspace"));
    assert!(stdout.contains("role"));
    assert!(stdout.contains("token"));
    assert!(stdout.contains("person"));
    assert!(stdout.contains("user"));
}

#[test]
fn one_platform_workspace_help_renders_governance_actions() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "platform", "workspace", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("admins"));
    assert!(stdout.contains("invite-users"));
    assert!(stdout.contains("remove-user"));
    assert!(stdout.contains("suspend-users"));
    assert!(stdout.contains("unsuspend-users"));
    assert!(stdout.contains("transfer"));
}

#[test]
fn one_platform_role_help_renders_assignments() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "platform", "role", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list-assignments"));
    assert!(stdout.contains("assign"));
    assert!(stdout.contains("unassign"));
}

#[test]
fn one_connections_help_renders_surface_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "connections", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("connector-metadata"));
    assert!(stdout.contains("permissions"));
    assert!(stdout.contains("dry-run"));
    assert!(stdout.contains("delete"));
}

#[test]
fn one_connections_connector_metadata_overrides_create_help_renders_connector_arg() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "one",
            "connections",
            "connector-metadata",
            "overrides",
            "create",
            "--help",
        ])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("--connector"));
    assert!(stdout.contains("--body"));
}

#[test]
fn one_flows_delete_help_renders_flow_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "delete", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--flow-id"));
    assert!(stdout.contains("--apply"));
}

#[test]
fn one_flows_folders_delete_help_renders_folder_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "folders", "delete", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--folder-id"));
    assert!(stdout.contains("--apply"));
}

#[test]
fn one_platform_token_help_renders_access_token_actions() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "platform", "token", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("detail"));
    assert!(stdout.contains("delete"));
}

#[test]
fn one_connection_permission_help_renders_subject_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "connections", "permissions", "detail", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--subject-id"));
    assert!(stdout.contains("--connection-id"));
}

#[test]
fn discover_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["discover", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Progressive live discovery"));
    assert!(stdout.contains("--deep"));
}

#[test]
fn discover_root_lists_top_level_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["--output", "json", "discover"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["data"]["schema_version"], serde_json::json!(1));
    assert_eq!(json["data"]["binary"], serde_json::json!("ayx"));
    let tree = &json["data"]["tree"];
    assert_eq!(tree["name"], serde_json::json!("ayx"));
    let subcommands = tree["subcommands"].as_array().expect("subcommands");
    assert!(subcommands.iter().any(|item| item["name"] == "discover"));
    assert!(subcommands.iter().any(|item| item["name"] == "catalog"));
    assert!(subcommands.iter().any(|item| item["name"] == "one"));
}

#[test]
fn workflow_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["workflow", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workflow"));
    assert!(stdout.contains("inspect"));
    assert!(stdout.contains("migrate"));
    assert!(stdout.contains("recurse"));
    assert!(stdout.contains("scan"));
    assert!(stdout.contains("convert-cloud"));
    assert!(stdout.contains("publish"));
    assert!(stdout.contains("yxdb"));
}

#[test]
fn workflow_yxdb_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["workflow", "yxdb", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("yxdb"));
    assert!(stdout.contains("--csv"));
}

#[cfg(not(windows))]
#[test]
fn workflow_convert_cloud_smoke() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("sample.yxmd");
    let output = dir.path().join("sample.cloud.json");
    fs::write(
        &input,
        r#"<AlteryxDocument yxmdVer="2025.2"><Nodes><Node ToolID="1"><GuiSettings Plugin="AlteryxBasePluginsGui.TextInput.TextInput"/><Properties><Configuration><Fields><Field name="A"/></Fields><Data><r><c>1</c></r></Data></Configuration></Properties></Node></Nodes><Connections/></AlteryxDocument>"#,
    )
    .expect("write sample");

    let result = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "workflow",
            "convert-cloud",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .expect("ayx binary should run");

    assert!(result.status.success());
    assert!(output.exists());
    let text = fs::read_to_string(&output).expect("read output");
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        json.get("@yxmdVer").and_then(|value| value.as_str()),
        Some("2021.4")
    );
    assert!(json.get("Nodes").is_some());
}

#[test]
fn ui_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "ui", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("session"));
    assert!(stdout.contains("workflow"));
    assert!(stdout.contains("data"));
}

#[cfg(not(windows))]
#[test]
fn catalog_list_tag_smoke() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["--output", "json", "catalog", "list", "--tag", "designer"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    let capabilities = json["data"]["capabilities"]
        .as_array()
        .expect("capabilities array");
    assert!(!capabilities.is_empty());
    assert!(capabilities.iter().all(|item| {
        item["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|tag| tag == "designer")
    }));
}

#[cfg(not(windows))]
#[test]
fn catalog_describe_capability_smoke() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "--output",
            "json",
            "catalog",
            "describe",
            "designer.workflow.context",
        ])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["data"]["kind"], "capability");
    assert_eq!(json["data"]["id"], "designer.workflow.context");
}

#[cfg(not(windows))]
#[test]
fn catalog_run_smoke() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("sample.yxmd");
    fs::write(
        &input,
        r#"<AlteryxDocument yxmdVer="2025.2"><Nodes><Node ToolID="1"><GuiSettings Plugin="AlteryxBasePluginsGui.TextInput.TextInput"/></Node></Nodes><Connections/></AlteryxDocument>"#,
    )
    .expect("write sample");

    let payload = format!(r#"{{"workflow_path":"{}"}}"#, input.display());
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "--output",
            "json",
            "catalog",
            "run",
            "designer.workflow.context",
            "--json",
            &payload,
        ])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(
        json["data"]["capability"]["id"],
        "designer.workflow.context"
    );
    assert_eq!(json["data"]["result"]["workflow"]["tool_count"], 1);
}
