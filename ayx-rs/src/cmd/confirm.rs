//! Interactive confirmation helper for destructive operations.
//!
//! Pattern: before any genuinely destructive action (delete a flow, drop
//! a workspace, restore a backup over live data) the dispatcher calls
//! `require_tty_confirmation` with a human-readable description. The
//! helper:
//!
//! - **On a TTY**: prompts the operator and waits for `yes` (case-
//!   insensitive). Anything else bails with `Validation`.
//! - **Off a TTY** (CI, pipe, redirected stdin): no-op when consent is
//!   passed, refuse otherwise. Automation must pass `--yes` explicitly
//!   via the global CLI flag.

use anyhow::{Result, bail};
use std::io::{self, IsTerminal, Write};

/// Controls whether a mutating command may obtain consent interactively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfirmationPolicy {
    pub yes: bool,
    pub no_input: bool,
}

impl ConfirmationPolicy {
    pub const fn new(yes: bool, no_input: bool) -> Self {
        Self { yes, no_input }
    }
}

/// Enforce mutation consent using an explicit policy.
pub fn require_confirmation(policy: ConfirmationPolicy, message: &str) -> Result<()> {
    if policy.yes {
        return Ok(());
    }
    if policy.no_input || !io::stdin().is_terminal() {
        bail!(
            "destructive operation requires confirmation. Re-run with --yes (non-interactive) or attach a TTY for the interactive prompt. Action: {message}"
        );
    }
    eprintln!("{message}");
    eprint!("Type 'yes' to proceed: ");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    if buf.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        bail!(
            "operator declined the operation (response: '{}')",
            buf.trim()
        );
    }
}

/// Prompt the operator on a TTY; refuse non-TTY callers that haven't
/// passed `--yes`. Returns `Err` on rejection.
///
/// `consent` short-circuits the prompt entirely — pass `cli.yes` from the
/// global `--yes` flag.
pub fn require_tty_confirmation(consent: bool, message: &str) -> Result<()> {
    // Keep the public compatibility helper, while allowing the CLI's global
    // policy to reach legacy call sites until they are migrated individually.
    let no_input = std::env::var_os("AYX_NO_INPUT").is_some();
    require_confirmation(ConfirmationPolicy::new(consent, no_input), message)
}

/// Build a consistent warning for governance actions that change access.
///
/// We keep the phrasing short and explicit so destructive IAM flows all feel
/// like the same class of operation in help text and interactive prompts.
pub fn access_change_message(action: &str, subject: &str, profile: &str) -> String {
    format!(
        "About to {action} {subject} on profile '{profile}'. This changes access and may affect active users. Review carefully before proceeding."
    )
}

/// Build a consistent warning for destructive non-governance actions.
pub fn destructive_action_message(action: &str, subject: &str, profile: &str) -> String {
    format!(
        "About to {action} {subject} on profile '{profile}'. This is destructive and may affect live workflows or users. Review carefully before proceeding."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_input_refuses_without_reading_stdin() {
        let err = require_confirmation(
            ConfirmationPolicy::new(false, true),
            "delete the test resource",
        )
        .expect_err("noninteractive mutation must require explicit consent");
        assert!(err.to_string().contains("--yes"));
    }

    #[test]
    fn yes_bypasses_even_when_no_input_is_set() {
        require_confirmation(
            ConfirmationPolicy::new(true, true),
            "delete the test resource",
        )
        .expect("explicit consent should bypass prompting");
    }

    #[test]
    fn tty_confirmation_refuses_when_ayx_no_input_env_is_set() {
        // nextest (the workspace's test runner, see CONTRIBUTING.md) gives
        // each test its own process, so mutating this process-global env var
        // here doesn't race with other tests. This mirrors the convention in
        // ayx-core's `install_test_keyring_store` for the same reason.
        //
        // SAFETY: single-threaded at this point in the test process; no
        // other thread reads/writes env vars concurrently with this call.
        unsafe {
            std::env::set_var("AYX_NO_INPUT", "1");
        }
        let err = require_tty_confirmation(false, "delete the test resource")
            .expect_err("AYX_NO_INPUT must refuse rather than auto-approve or prompt");
        assert!(err.to_string().contains("--yes"));
        // SAFETY: same single-threaded context as the set_var above.
        unsafe {
            std::env::remove_var("AYX_NO_INPUT");
        }
    }
}
