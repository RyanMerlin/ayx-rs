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
