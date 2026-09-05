//! TTY-gated selector resolution (ADR 0004).
//!
//! When a command's required selector is omitted on an interactive terminal,
//! fetch the candidates and offer a picker. Off a terminal, or under
//! `--no-input`, fail closed with a `MissingSelector` that `main()` turns into
//! a structured remediation naming the list command.

use std::fmt;
use std::io::IsTerminal;

use anyhow::{Result, bail};
use ayx_core::envelope::Envelope;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectItem {
    pub id: String,
    pub label: String,
}

impl fmt::Display for SelectItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.label.is_empty() || self.label == self.id {
            write!(f, "{}", self.id)
        } else {
            write!(f, "{}  ({})", self.label, self.id)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SelectPolicy {
    pub no_input: bool,
    pub interactive_terminal: bool,
}

impl SelectPolicy {
    /// Prompting needs both stdin (keys) and stdout (the list) to be
    /// terminals. `AYX_NO_INPUT` is honored the same way `cmd::confirm`
    /// honors it, so a picker can't fire under an agent host or CI runner
    /// that sets the env var instead of (or in addition to) `--no-input`.
    pub fn from_runtime(no_input: bool) -> Self {
        Self {
            no_input: no_input || std::env::var_os("AYX_NO_INPUT").is_some(),
            interactive_terminal: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        }
    }

    pub fn may_prompt(self) -> bool {
        !self.no_input && self.interactive_terminal
    }
}

/// A required selector was omitted and no picker may run. `main()` downcasts
/// this to attach `remediation.commands = [list_command]`.
#[derive(Debug)]
pub struct MissingSelector {
    pub what: &'static str,
    pub list_command: &'static str,
}

impl fmt::Display for MissingSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "validation: {} is required when not running interactively; run `{}` to find one",
            self.what, self.list_command
        )
    }
}

impl std::error::Error for MissingSelector {}

#[derive(Debug)]
pub struct SelectionCancelled;

impl fmt::Display for SelectionCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("validation: selection cancelled")
    }
}

impl std::error::Error for SelectionCancelled {}

/// Return `given` when present; otherwise prompt (if allowed) or fail closed.
pub fn resolve_selector(
    what: &'static str,
    list_command: &'static str,
    given: Option<String>,
    policy: SelectPolicy,
    fetch: impl FnOnce() -> Result<Vec<SelectItem>>,
) -> Result<String> {
    if let Some(id) = given {
        return Ok(id);
    }
    if !policy.may_prompt() {
        return Err(MissingSelector { what, list_command }.into());
    }
    let items = fetch()?;
    if items.is_empty() {
        bail!("validation: no {what} candidates were returned; nothing to select");
    }
    match inquire::Select::new(&format!("Select a {what}:"), items)
        .with_page_size(12)
        .prompt()
    {
        Ok(item) => Ok(item.id),
        Err(inquire::InquireError::OperationCanceled)
        | Err(inquire::InquireError::OperationInterrupted) => Err(SelectionCancelled.into()),
        Err(other) => bail!("picker failed: {other}"),
    }
}

/// Build picker rows from a normalized list envelope (`data.items[]`, each
/// with an `id`). The label is the first present key from `label_keys`.
pub fn items_from_envelope(envelope: &Envelope, label_keys: &[&str]) -> Result<Vec<SelectItem>> {
    if !envelope.ok {
        bail!("{}", envelope.message);
    }
    let items = envelope
        .data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = match item.get("id")? {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            let label = label_keys
                .iter()
                .find_map(|key| item.get(*key).and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            Some(SelectItem { id, label })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_core::envelope::Envelope;
    use serde_json::json;

    fn never_fetch() -> anyhow::Result<Vec<SelectItem>> {
        panic!("fetch must not run when a selector was given or prompting is not allowed");
    }

    #[test]
    fn a_given_selector_is_returned_untouched() {
        let policy = SelectPolicy {
            no_input: false,
            interactive_terminal: true,
        };
        let id = resolve_selector(
            "workflow id",
            "ayx one workflows list",
            Some("01X".into()),
            policy,
            never_fetch,
        )
        .unwrap();
        assert_eq!(id, "01X");
    }

    #[test]
    fn missing_selector_off_tty_is_a_typed_error() {
        let policy = SelectPolicy {
            no_input: false,
            interactive_terminal: false,
        };
        let err = resolve_selector(
            "workflow id",
            "ayx one workflows list --output json",
            None,
            policy,
            never_fetch,
        )
        .unwrap_err();
        let missing = err.downcast_ref::<MissingSelector>().expect("typed error");
        assert_eq!(missing.list_command, "ayx one workflows list --output json");
        assert!(err.to_string().starts_with("validation:"));
    }

    #[test]
    fn from_runtime_honors_ayx_no_input_env_var() {
        // nextest gives each test its own process, so mutating this
        // process-global env var here doesn't race with other tests. Mirrors
        // the convention in cmd/confirm.rs's own AYX_NO_INPUT test.
        //
        // SAFETY: single-threaded at this point in the test process; no
        // other thread reads/writes env vars concurrently with this call.
        unsafe {
            std::env::set_var("AYX_NO_INPUT", "1");
        }
        let policy = SelectPolicy::from_runtime(false);
        assert!(
            policy.no_input,
            "AYX_NO_INPUT must be honored even when the --no-input flag was not passed"
        );
        assert!(!policy.may_prompt());
        // SAFETY: same single-threaded context as the set_var above.
        unsafe {
            std::env::remove_var("AYX_NO_INPUT");
        }
    }

    #[test]
    fn no_input_blocks_prompting_even_on_a_tty() {
        let policy = SelectPolicy {
            no_input: true,
            interactive_terminal: true,
        };
        assert!(!policy.may_prompt());
        let err = resolve_selector("flow id", "ayx one flows list", None, policy, never_fetch)
            .unwrap_err();
        assert!(err.downcast_ref::<MissingSelector>().is_some());
    }

    #[test]
    fn items_from_envelope_maps_ids_and_first_matching_label() {
        let env = Envelope::ok_with_data(
            "list",
            json!({ "items": [
                { "id": "a", "name": "Alpha" },
                { "id": 7, "title": "Seven" },
                { "id": "c" },
                { "name": "no id" }
            ]}),
        );
        let items = items_from_envelope(&env, &["name", "title"]).unwrap();
        assert_eq!(
            items,
            vec![
                SelectItem {
                    id: "a".into(),
                    label: "Alpha".into()
                },
                SelectItem {
                    id: "7".into(),
                    label: "Seven".into()
                },
                SelectItem {
                    id: "c".into(),
                    label: String::new()
                },
            ]
        );
        assert_eq!(items[0].to_string(), "Alpha  (a)");
        assert_eq!(items[2].to_string(), "c");
    }

    #[test]
    fn items_from_a_failed_envelope_is_an_error() {
        let env = Envelope::err_coded(
            ayx_core::envelope::ErrorCode::AuthFailed,
            "nope",
            json!(null),
        );
        assert!(items_from_envelope(&env, &["name"]).is_err());
    }
}
