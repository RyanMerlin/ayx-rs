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
    let value = json(&["discover", "one", "--deep", "--output", "json"]);
    assert_eq!(value["data"]["schema_version"], 1);

    let mut names = Vec::new();
    command_names(&value["data"]["tree"], "", &mut names);
    for expected in [
        "one workspace config get",
        "one workspace config set",
        "one workspace config schema",
        "one workspace config reset",
        "one workspace members list",
        "one workspace members admins",
        "one workspace groups create",
        "one workspace groups update",
        "one workspace groups delete",
        "one workspace groups members add",
        "one workspace groups members remove",
        "one workspace groups roles set",
        "one workspace cloud-configs list",
        "one workspace cloud-configs create",
        "one workspace cloud-configs update",
        "one workspace transfer start",
        "one workspace transfer assets",
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

    for removed in [
        "one workspace people",
        "one workspace admins",
        "one workspace current-configuration",
        "one workspace current-configuration-schema",
        "one workspace create-group",
        "one workspace groups-global",
        "one workspace switch",
    ] {
        assert!(
            !names.iter().any(|name| name == removed),
            "removed flat alias unexpectedly remains visible: {removed}"
        );
    }
}

#[test]
fn trailing_json_output_is_supported_for_discovery_and_help() {
    let discovery = run(&["discover", "--deep", "--output", "json"]);
    assert!(discovery.status.success());
    let value: Value = serde_json::from_slice(&discovery.stdout).expect("valid discovery JSON");
    assert_eq!(value["data"]["binary"], "ayx");

    let help = run(&["one", "workflows", "--help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    // Clap wraps long option descriptions, so the guidance sentence may span
    // lines.  Collapse whitespace before matching rather than pinning the test
    // to a particular terminal width.
    let unwrapped = help_text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        unwrapped.contains("Put it after the complete command path"),
        "--output help should tell agents where to place the flag:
{help_text}"
    );
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
