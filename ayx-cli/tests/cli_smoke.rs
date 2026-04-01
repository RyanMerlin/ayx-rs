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
    assert!(stdout.contains("mongo"));
    assert!(stdout.contains("server api"));
    assert!(stdout.contains("one"));
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
    assert!(stdout.contains("upgrade"));
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
