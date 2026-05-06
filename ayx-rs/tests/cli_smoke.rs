use std::fs;
use std::process::Command;

#[test]
fn ayx_help_renders() {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .arg("--help")
        .output()
        .expect("ayx binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AYX Rust CLI"));
    assert!(stdout.contains("one"));
    assert!(stdout.contains("server"));
    assert!(stdout.contains("mongo"));
    assert!(stdout.contains("workflow"));
    assert!(stdout.contains("tui"));
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
    assert!(stdout.contains("profile setup"));
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
