use std::process::Command;

use serde_json::Value;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .output()
        .expect("ayx binary should run")
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert!(
        output.status.success(),
        "command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("command should emit JSON")
}

fn command_names(tree: &Value, prefix: &str, names: &mut Vec<String>) {
    let Some(name) = tree.get("name").and_then(Value::as_str) else {
        return;
    };
    let path = if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix} {name}")
    };
    names.push(path.clone());
    if let Some(children) = tree.get("subcommands").and_then(Value::as_array) {
        for child in children {
            command_names(child, &path, names);
        }
    }
}

#[test]
fn discovery_exposes_agent_crud_paths_from_live_tree() {
    let value = json(&["discover", "one", "--deep", "--output", "json-full"]);
    assert_eq!(value["data"]["schema_version"], 1);

    let mut names = Vec::new();
    command_names(&value["data"]["tree"], "", &mut names);
    for expected in [
        "one workspace admins",
        "one workspace create-group",
        "one workspace update-group",
        "one workspace delete-group",
        "one workflows copy",
        "one workflows delete",
        "one plans create",
        "one plans update",
        "one plans delete",
        "one scheduling create",
        "one scheduling update",
        "one scheduling delete",
        "one connections create",
        "one connections update",
        "one connections delete",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}"
        );
    }
}

#[test]
fn trailing_json_output_is_supported_for_discovery_and_help() {
    let discovery = run(&["discover", "--deep", "--output", "json-full"]);
    assert!(discovery.status.success());
    let value: Value = serde_json::from_slice(&discovery.stdout).expect("valid discovery JSON");
    assert_eq!(value["data"]["binary"], "ayx");

    let help = run(&["one", "workflows", "--help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("Put this after the complete command path"));
}

#[test]
fn standard_envelope_parser_contract_is_explicit() {
    let object = serde_json::json!({
        "ok": true,
        "data": {"response": {"id": 42}, "status_code": 200}
    });
    assert_eq!(object["data"]["response"]["id"], 42);
    assert_eq!(object["data"]["status_code"], 200);

    let paginated = serde_json::json!({
        "ok": true,
        "data": {"items": [{"id": "abc"}], "page_envelopes": [{"status_code": 200}]}
    });
    assert_eq!(paginated["data"]["items"][0]["id"], "abc");
    assert_eq!(paginated["data"]["page_envelopes"][0]["status_code"], 200);
}
