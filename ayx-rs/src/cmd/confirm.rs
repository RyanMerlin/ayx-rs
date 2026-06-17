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

/// Prompt the operator on a TTY; refuse non-TTY callers that haven't
/// passed `--yes`. Returns `Err` on rejection.
///
/// `consent` short-circuits the prompt entirely — pass `cli.yes` from the
/// global `--yes` flag.
pub fn require_tty_confirmation(consent: bool, message: &str) -> Result<()> {
    if consent {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
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
