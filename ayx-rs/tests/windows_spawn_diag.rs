//! TEMPORARY diagnostic (issue #59 Part 2): capture the exact exit code of the
//! spawned `ayx` binary on `windows-latest` runners to root-cause the cli_smoke
//! spawn quirk. Windows-only; deleted once the root cause is confirmed and the
//! real fix lands. This test is *expected to fail* on a broken Windows build so
//! that nextest surfaces the per-invocation exit codes in its output.
#![cfg(windows)]

use std::process::Command;

#[test]
fn windows_spawn_exit_codes() {
    // A gradient from the shallowest parse (`--version`) to a deep help render.
    // All of these are parse-only / local: none touch the network, auth, or
    // config, so a non-zero exit isolates the failure to clap parse + help
    // rendering on the process main thread.
    let probes: &[&[&str]] = &[
        &["--version"],
        &["--help"],
        &["one", "--help"],
        &["one", "platform", "workspace", "--help"],
        &["one", "datasets", "wrangled", "detail", "--help"],
        &["--output", "json", "catalog", "list", "--format", "full"],
    ];

    let mut summary = String::from("\n=== ayx windows spawn diagnostic (issue #59 Part 2) ===\n");
    let mut all_ok = true;

    for args in probes {
        match Command::new(env!("CARGO_BIN_EXE_ayx")).args(*args).output() {
            Ok(out) => {
                let ok = out.status.success();
                all_ok &= ok;
                let code = out.status.code();
                let hex = code.map(|c| format!("0x{:08X}", c as u32));
                summary.push_str(&format!(
                    "args {args:?} -> success={ok} code={code:?} ({}) stdout={}B stderr={}B\n",
                    hex.as_deref().unwrap_or("no-code"),
                    out.stdout.len(),
                    out.stderr.len(),
                ));
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.trim().is_empty() {
                    summary.push_str(&format!("    stderr: {}\n", stderr.trim()));
                }
            }
            Err(err) => {
                all_ok = false;
                summary.push_str(&format!("args {args:?} -> SPAWN ERROR: {err}\n"));
            }
        }
    }

    summary.push_str("NOTE: exit code 3221225725 (0xC00000FD) == STATUS_STACK_OVERFLOW\n");

    assert!(all_ok, "{summary}");
}
