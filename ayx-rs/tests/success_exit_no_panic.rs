//! Regression test for the Windows exit-panic bug shipped in v0.13.1.
//!
//! `ayx-one-api`'s `ONE_HTTP_CLIENT` thread-local caches a
//! `reqwest::blocking::Client` the first time a live One API call is made.
//! `reqwest`'s blocking client is a thin handle around an
//! `Arc<InnerClientHandle>` whose `Drop` joins the dedicated background
//! thread that runs its internal tokio runtime. When the last reference —
//! the cached one — drops as part of this thread-local's own teardown, that
//! join runs from inside an OS-invoked thread-local destructor callback
//! during process exit (on Windows, via FLS), which is fragile and panicked:
//!
//!   thread 'main' panicked at .../thread/lifecycle.rs:226:14:
//!   threads should not terminate unexpectedly
//!   fatal runtime error: thread local panicked on drop, aborting
//!
//! This corrupted the exit code of every *successful* live command on
//! Windows (`ayx one flows list/count`, etc.) even though the command's own
//! result was correct — a serious problem for any scripting/CI usage.
//!
//! Two things were tried here, in order — both are worth knowing about,
//! since a future maintainer could otherwise "simplify" by reverting the
//! one that actually works:
//!
//! 1. Routing `main()`'s `ok=true` success branch through
//!    `std::process::exit()`, like the `ok=false`/`Err` branches already
//!    did. **This alone does NOT fix it** — confirmed by this exact test
//!    failing on `windows-latest` CI with the identical panic even with
//!    that change in place. `process::exit()` only skips libstd's own
//!    at-exit/destructor bookkeeping; it does not suppress an OS-level
//!    FLS/TLS destructor callback already registered for a
//!    non-trivially-droppable thread-local — both exit paths reach the same
//!    OS-level process termination, where the FLS callback fires either way.
//! 2. **The actual fix:** `ONE_HTTP_CLIENT` now stores
//!    `ManuallyDrop<Client>` (see `ayx-one-api/src/lib.rs`). `ManuallyDrop`
//!    makes the stored value provably non-drop-needing, so the compiler
//!    never registers a destructor for this thread-local at all — the
//!    `Client` (and its background thread) is intentionally leaked for the
//!    process's lifetime, which is harmless for a short-lived CLI
//!    invocation. This is a language-level guarantee, not a theory about
//!    Windows teardown timing, which is why it's trusted over (1). The
//!    `process::exit()` change from (1) is kept anyway (harmless, matches
//!    the other two branches) but is not what fixes this bug.
//!
//! This is the only regression coverage for that class of bug: it spawns
//! the actual compiled `ayx` binary (not an in-process call, which can't
//! exercise process-exit behavior at all) against a local `httpmock` server
//! stubbing `GET /v4/flows/count` — the literal command Merlin hit live —
//! so the child process populates `ONE_HTTP_CLIENT` and returns through the
//! same success path that crashed. It needs no live credentials and runs on
//! all three CI platforms via the standard `cargo nextest run --workspace`
//! step, so a Windows-only regression here is no longer invisible to CI.
//!
//! Runs under `cargo nextest run` (process-per-test isolation), not the
//! default `cargo test` harness — see `ayx-one-api/tests/transport_smoke.rs`
//! for why sharing a process with `httpmock`'s async server and this crate's
//! blocking `reqwest` client is the thing to avoid, not `httpmock` itself.

use std::fs;
use std::process::Command;

use httpmock::prelude::*;
use tempfile::TempDir;

/// Isolated config home with a single One profile carrying a bare
/// `access_token` (no `oauth_client_id`/`refresh_token`) pointed at the mock
/// server. `resolve_one_access_token` returns a plain `access_token` directly
/// with no refresh/workspace-preflight call, so the command reaches the mock
/// in one request, exactly like a real successful command would.
fn config_home_with_mock_profile(base_url: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let profiles = temp.path().join("profiles");
    fs::create_dir_all(&profiles).expect("profiles dir");
    fs::write(
        profiles.join("mock.yaml"),
        format!(
            "profile_name: mock\n\
             alteryx_one:\n\
            \x20 account_email: user@example.com\n\
            \x20 base_url: {base_url}\n\
            \x20 access_token: mock-access-token\n"
        ),
    )
    .expect("write profile");
    temp
}

#[test]
fn successful_live_command_exits_zero_without_thread_local_panic() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v4/flows/count");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"count": 0}"#);
    });

    let home = config_home_with_mock_profile(&server.base_url());

    // The exact command Merlin reproduced the crash with: successful output,
    // then a panic on process exit.
    let output = Command::new(env!("CARGO_BIN_EXE_ayx"))
        .args([
            "--output",
            "json-full",
            "one",
            "flows",
            "count",
            "--profile",
            "mock",
        ])
        .env("AYX_CONFIG_HOME", home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("ayx binary should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The actual regression signature: a corrupted exit code from a runtime
    // abort during thread-local teardown, not a command-logic failure.
    assert!(
        output.status.success(),
        "a successful command must exit 0, not crash on drop\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "stderr must not contain a panic (thread-local drop panic regression)\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("fatal runtime error"),
        "stderr must not contain a fatal runtime error (aborting)\nstderr:\n{stderr}"
    );

    // Confirm the command actually reached the mock and got a real success
    // envelope back, not a false green from an early bail-out.
    assert!(
        stdout.contains("\"ok\": true"),
        "expected a successful envelope\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\"surface\": \"flow\""),
        "expected surface flow\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\"operation\": \"count\""),
        "expected operation count\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\"count\": 0"),
        "expected the mocked response body to round-trip\nstdout:\n{stdout}"
    );
}
