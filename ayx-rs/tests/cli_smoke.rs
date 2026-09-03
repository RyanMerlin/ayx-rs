//! CLI smoke tests that spawn the compiled `ayx` binary.
//!
//! Runs on all platforms, including Windows. These spawn invocations previously
//! aborted on `windows-latest` with `STATUS_STACK_OVERFLOW` (0xC00000FD) because
//! clap builds the deep command tree on the 1 MiB MSVC main-thread stack during
//! `Cli::parse()`. The `ayx-rs` build script now reserves a 16 MiB main-thread
//! stack on Windows, so the whole suite runs everywhere (issue #59 Part 2).

use std::fs;
use std::process::Command;

/// Assert that a command invocation does NOT produce a clap downcast panic.
///
/// The clap downcast panic message ("Mismatch between definition and access")
/// fires at argument-parse time before any network call. A command that
/// collides its local `--output` arg id with the global `--output` id will
/// abort here even with `--help` inputs. We tolerate non-zero exit (auth
/// errors, missing required args, etc.) but never tolerate a panic abort or
/// the specific clap panic string.
fn assert_no_clap_panic(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("ayx binary should run for args: {args:?}"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Mismatch between definition and access"),
        "clap downcast panic detected for args {args:?}\nstderr:\n{stderr}"
    );
    // A panic abort on Linux typically produces a non-zero exit + "panicked at"
    // in stderr. Guard against that as well.
    assert!(
        !stderr.contains("panicked at"),
        "process panicked for args {args:?}\nstderr:\n{stderr}"
    );
}

use serde_json::Value;

fn assert_json_output_works_before_and_after(base_args: &[&str], check: fn(&Value)) {
    for args in [
        {
            let mut v = vec!["--output", "json"];
            v.extend_from_slice(base_args);
            v
        },
        {
            let mut v = base_args.to_vec();
            v.extend_from_slice(&["--output", "json"]);
            v
        },
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
            .args(&args)
            .output()
            .unwrap_or_else(|_| panic!("ayx binary should run for args: {args:?}"));

        assert!(
            output.status.success(),
            "command failed for args {args:?}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| {
            panic!("expected JSON stdout for args: {args:?}\nstdout:\n{stdout}")
        });
        check(&json);
    }
}

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
    let commands_section = stdout
        .split("Options:")
        .next()
        .unwrap_or(&stdout)
        .split("Commands:")
        .nth(1)
        .unwrap_or(&stdout);
    assert!(
        commands_section
            .lines()
            .any(|line| line.trim_start().starts_with("designer "))
    );
    assert!(
        !commands_section
            .lines()
            .any(|line| line.trim_start().starts_with("workflow "))
    );
    assert!(stdout.contains("one"));
    assert!(stdout.contains("server"));
    assert!(stdout.contains("mongo"));
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
        .args([
            "catalog",
            "list",
            "--format",
            "full",
            "--output",
            "json-full",
        ])
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

    assert!(names.contains(&"one login"));
    assert!(names.contains(&"one logout"));
    assert!(names.contains(&"one whoami"));
    assert!(names.contains(&"one auth status"));
    assert!(names.contains(&"one inventory"));
    assert!(names.contains(&"one doctor auth"));
    assert!(names.contains(&"one doctor discover"));
    assert!(names.contains(&"one plans list"));
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
fn one_login_help_mentions_workspace_password_storage() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "login", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--save-workspace-password"));
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
    assert!(stdout.contains("identity"));
}

#[test]
fn one_help_renders_governance_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workspace"));
    assert!(stdout.contains("role"));
    assert!(stdout.contains("token"));
    assert!(stdout.contains("person"));
    assert!(stdout.contains("whoami"));
    assert!(stdout.contains("login"));
    assert!(!stdout.contains("platform"));
}

#[test]
fn one_workspace_help_renders_governance_actions() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "workspace", "--help"])
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
fn one_role_help_renders_assignments() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "role", "--help"])
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
fn one_datasets_help_renders_surface_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "datasets", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("wrangled"));
    assert!(stdout.contains("imported"));
    assert!(stdout.contains("Count datasets in the user-facing One dataset library"));
}

#[test]
fn one_datasets_wrangled_detail_help_renders_required_positional_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "datasets", "wrangled", "detail", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<ID>"));
    assert!(!stdout.contains("--wrangled-id"));
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
    assert!(stdout.contains("<CONNECTOR>"));
    assert!(!stdout.contains("--connector"));
    assert!(stdout.contains("--body"));
}

#[test]
fn one_flows_delete_help_renders_positional_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "delete", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<ID>"));
    assert!(!stdout.contains("--flow-id"));
    assert!(stdout.contains("--apply"));
}

#[test]
fn one_flows_folders_delete_help_renders_positional_id() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "folders", "delete", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<ID>"));
    assert!(!stdout.contains("--folder-id"));
    assert!(stdout.contains("--apply"));
}

#[test]
fn one_token_help_renders_access_token_actions() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "token", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("detail"));
    assert!(stdout.contains("delete"));
}

#[test]
fn one_connection_permission_help_renders_ordered_positional_ids() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "connections", "permissions", "detail", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<CONNECTION-ID>"));
    assert!(stdout.contains("<SUBJECT-ID>"));
    assert!(!stdout.contains("--subject-id"));
    assert!(!stdout.contains("--connection-id"));
    assert!(!stdout.contains("--aid"));
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
        .args(["discover", "--output", "json-full"])
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
fn output_json_works_when_flag_is_trailing_for_discover() {
    assert_json_output_works_before_and_after(&["discover"], |json| {
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["schema_version"], serde_json::json!("ayx.output.v1"));
        assert_eq!(json["command"], serde_json::json!("discover"));
        assert_eq!(json["data"]["kind"], serde_json::json!("raw"));
    });
}

#[test]
fn output_json_works_when_flag_is_trailing_for_catalog_list() {
    assert_json_output_works_before_and_after(&["catalog", "list", "--format", "full"], |json| {
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["schema_version"], serde_json::json!("ayx.output.v1"));
        assert_eq!(json["command"], serde_json::json!("catalog"));
        assert_eq!(json["data"]["kind"], serde_json::json!("raw"));
    });
}

#[test]
fn output_json_works_when_flag_is_trailing_for_actions_list() {
    assert_json_output_works_before_and_after(&["actions", "list"], |json| {
        assert_eq!(json["ok"], serde_json::json!(true));
        let actions = json["data"]["items"].as_array().expect("actions array");
        assert!(!actions.is_empty());
    });
}

#[test]
fn actions_export_json_full_is_parseable_and_contains_raw_yaml() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["actions", "export", "mongo.doctor", "--output", "json-full"])
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid full json");
    assert_eq!(json["ok"], serde_json::json!(true));
    assert!(json["data"]["yaml"].as_str().unwrap().contains("id:"));
    assert!(
        json["data"]["save_hint"]
            .as_str()
            .unwrap()
            .contains("jq -r '.data.yaml'")
    );
}

#[test]
fn output_json_works_when_flag_is_trailing_for_workflows_list() {
    assert_json_output_works_before_and_after(&["actions", "workflows", "list"], |json| {
        assert_eq!(json["ok"], serde_json::json!(true));
        let workflows = json["data"]["items"].as_array().expect("workflows array");
        assert!(!workflows.is_empty());
    });
}

#[test]
fn old_top_level_workflows_command_no_longer_resolves() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["workflows", "list"])
        .output()
        .expect("ayx binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Which message clap emits depends on its parse path: the hidden global
    // `environment_tail` positional makes it report an unexpected argument
    // rather than an unrecognized subcommand. Accept either — the contract
    // under test is that the old top-level path no longer resolves, not the
    // wording clap happens to pick.
    assert!(
        stderr.contains("unexpected argument 'workflows'")
            || stderr.contains("unrecognized subcommand 'workflows'")
    );
}

#[test]
fn workflow_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["designer", "workflow", "--help"])
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
        .args(["designer", "workflow", "yxdb", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("yxdb"));
    assert!(stdout.contains("--csv"));
}

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
            "designer",
            "workflow",
            "convert-cloud",
            "--input",
            input.to_str().unwrap(),
            "--output-path",
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

#[cfg(feature = "ui")]
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

#[cfg(not(feature = "ui"))]
#[test]
fn ui_help_is_absent_without_feature() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "ui", "--help"])
        .output()
        .expect("ayx binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // clap renders the bin name from argv[0]'s file name — `ayx.exe` on Windows,
    // `ayx` elsewhere — so match the platform-invariant tail of the usage line.
    // `one`'s subcommand is now required (arg_required_else_help + non-Option
    // command field, so bare `ayx one` shows real clap help instead of a
    // hand-rolled string) -- usage renders `<COMMAND>`, not `[COMMAND]`.
    assert!(stderr.contains("one [OPTIONS] <COMMAND>"));
    assert!(
        stderr.contains("unexpected argument 'ui' found")
            || stderr.contains("unrecognized subcommand 'ui'")
    );
}

#[test]
fn catalog_list_tag_smoke() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "catalog",
            "list",
            "--tag",
            "designer",
            "--output",
            "json-full",
        ])
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

#[test]
fn catalog_describe_capability_smoke() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "catalog",
            "describe",
            "designer.workflow.context",
            "--output",
            "json-full",
        ])
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(json["data"]["kind"], "capability");
    assert_eq!(json["data"]["id"], "designer.workflow.context");
}

#[test]
fn catalog_run_smoke() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("sample.yxmd");
    fs::write(
        &input,
        r#"<AlteryxDocument yxmdVer="2025.2"><Nodes><Node ToolID="1"><GuiSettings Plugin="AlteryxBasePluginsGui.TextInput.TextInput"/></Node></Nodes><Connections/></AlteryxDocument>"#,
    )
    .expect("write sample");

    // Build the payload with serde so the path is JSON-escaped. On Windows the
    // tempdir path contains backslashes, which are invalid JSON escapes if
    // interpolated raw.
    let payload = serde_json::json!({ "workflow_path": input.to_string_lossy() }).to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "catalog",
            "run",
            "designer.workflow.context",
            "--json",
            &payload,
            "--output",
            "json-full",
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

// ---- Panic-regression smoke tests for commands that were renamed to use --output-file ----
//
// The clap downcast panic ("Mismatch between definition and access") fires at
// argument-parse time before any network call when a command defines a local
// `--output` arg that collides with the global `--output` arg id.
// These four commands were renamed to `--output-file` / `--output-path` in the
// v0.10.0 hardening pass. The tests verify the collision cannot silently regress.

/// Guard: `one flows export` uses `--output-file`, not `--output`.
/// The clap collision fires before network calls, so we only need `--help`.
#[test]
fn flows_export_output_file_flag_no_clap_panic() {
    assert_no_clap_panic(&["--output", "json", "one", "flows", "export", "--help"]);
}

#[test]
fn flows_export_help_shows_output_file_not_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "flows", "export", "--help"])
        .output()
        .expect("ayx binary should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--output-file"),
        "`one flows export --help` must list --output-file"
    );
    // The local flag must not shadow the global --output flag id.
    // If --output appears it must be the global flag description, not a local one.
}

/// Guard: `server system-info` uses `--output-file`.
#[test]
fn server_system_info_output_file_flag_no_clap_panic() {
    assert_no_clap_panic(&["--output", "json", "server", "system-info", "--help"]);
}

#[test]
fn server_system_info_help_shows_output_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["server", "system-info", "--help"])
        .output()
        .expect("ayx binary should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--output-file"),
        "`server system-info --help` must list --output-file"
    );
}

/// Guard: `server runtime-settings` uses `--output-file`.
#[test]
fn server_runtime_settings_output_file_flag_no_clap_panic() {
    assert_no_clap_panic(&["--output", "json", "server", "runtime-settings", "--help"]);
}

#[test]
fn server_runtime_settings_help_shows_output_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["server", "runtime-settings", "--help"])
        .output()
        .expect("ayx binary should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--output-file"),
        "`server runtime-settings --help` must list --output-file"
    );
}

/// Guard: `tools workspace init` uses `--output-file`.
#[test]
fn tools_workspace_init_output_file_flag_no_clap_panic() {
    assert_no_clap_panic(&["--output", "json", "tools", "workspace", "init", "--help"]);
}

#[test]
fn tools_workspace_init_help_shows_output_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["tools", "workspace", "init", "--help"])
        .output()
        .expect("ayx binary should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--output-file"),
        "`tools workspace init --help` must list --output-file"
    );
}

/// Functional smoke: `tools workspace init --output-file <tmp>` must exit 0
/// and write the file. This exercises the actual command, not just --help.
#[test]
fn tools_workspace_init_creates_output_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("environments.yaml");

    let result = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "--output",
            "json",
            "tools",
            "workspace",
            "init",
            "--output-file",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("ayx binary should run");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("Mismatch between definition and access"),
        "clap panic detected\nstderr:\n{stderr}"
    );
    assert!(
        result.status.success(),
        "tools workspace init should exit 0\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&result.stdout),
        stderr
    );
    assert!(out.exists(), "output file should have been written");
}

/// Functional smoke: `server system-info --output-file <tmp>` with global `--output json`.
/// This command reads from the local runtime settings; it may fail if no Alteryx Server
/// is present, but it MUST NOT panic.
#[test]
fn server_system_info_with_output_file_no_clap_panic_functional() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("sysinfo.json");

    let result = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "--output",
            "json",
            "server",
            "system-info",
            "--output-file",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("ayx binary should run");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("Mismatch between definition and access"),
        "clap panic detected\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "process panicked\nstderr:\n{stderr}"
    );
    // The command may exit non-zero if runtime-settings.xml is absent, but that
    // is an application-level error, not a panic. We only assert no panic here.
}

#[test]
fn one_api_group_help_lists_spec_and_coverage() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "api", "--help"])
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("open-api-spec"));
    assert!(stdout.contains("coverage"));
}

#[test]
fn one_scheduling_lifecycle_help_lists_mutations() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "scheduling", "--help"])
        .output()
        .expect("ayx binary should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in [
        "create", "list", "detail", "update", "enable", "disable", "delete", "count",
    ] {
        assert!(
            stdout
                .lines()
                .any(|line| line.trim_start().starts_with(command)),
            "missing scheduling command '{command}' in help:\n{stdout}"
        );
    }
}

#[test]
fn coverage_from_spec_file_reports_missing() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/one-openapi-min.json"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "one",
            "api",
            "coverage",
            "--spec",
            fixture,
            "--output",
            "json-full",
        ])
        .output()
        .expect("ayx binary should run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let missing = json["data"]["missing"].as_array().expect("missing array");
    assert!(
        missing
            .iter()
            .any(|m| m["path"] == "/v4/specOnlyResource" && m["method"] == "POST")
    );
}

#[test]
fn coverage_check_flag_exits_nonzero_when_missing() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/one-openapi-min.json"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(["one", "api", "coverage", "--spec", fixture, "--check"])
        .output()
        .expect("ayx binary should run");
    assert!(
        !output.status.success(),
        "--check must fail when endpoints are missing"
    );
}
