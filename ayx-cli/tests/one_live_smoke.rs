use std::process::Command;

fn live_smoke_enabled() -> bool {
    matches!(
        std::env::var("AYX_ONE_LIVE_SMOKE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn run_ayx(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args(args)
        .output()
        .expect("ayx binary should run");

    assert!(
        output.status.success(),
        "command failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn one_platform_workspace_current_live() {
    if !live_smoke_enabled() {
        return;
    }

    let stdout = run_ayx(&[
        "--output",
        "json",
        "one",
        "platform",
        "workspace",
        "current",
    ]);
    assert!(stdout.contains("\"ok\": true"));
    assert!(stdout.contains("\"surface\": \"platform\""));
}

#[test]
fn one_plans_count_live() {
    if !live_smoke_enabled() {
        return;
    }

    let stdout = run_ayx(&["--output", "json", "one", "plans", "count"]);
    assert!(stdout.contains("\"ok\": true"));
    assert!(stdout.contains("\"surface\": \"plans\""));
}

#[test]
fn one_doctor_discover_live() {
    if !live_smoke_enabled() {
        return;
    }

    let stdout = run_ayx(&["--output", "json", "one", "doctor", "discover"]);
    assert!(stdout.contains("\"ok\": true"));
    assert!(stdout.contains("\"checks\""));
}
