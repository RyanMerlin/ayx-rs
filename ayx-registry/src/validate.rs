//! Cross-validation: every `cmd:` and `capability:` reference in a tactic
//! should resolve to a real entry in the CLI's command catalog. Run this
//! out-of-band (via `ayx tactics validate`) rather than at load time so a
//! tactic that drifts ahead of the binary doesn't bork the whole tool.
//!
//! The validator is intentionally permissive: it consumes a `Catalog` trait
//! the CLI implements over `COMMAND_SPECS`, so the registry crate doesn't
//! need to know about that data structure. Findings are returned as a
//! structured vec; nothing here aborts.

use serde::Serialize;

use crate::{Registry, Safety, Step};

/// Catalog adapter the CLI implements over its own COMMAND_SPECS. The
/// registry doesn't take a direct dependency on the dispatcher — callers
/// pass closures that resolve the question.
pub trait CatalogLookup {
    /// Is this `ayx ...`-style command path known? Pass the part after the
    /// `ayx` binary name, e.g. "mongo backup".
    fn has_command_path(&self, path: &str) -> bool;
    /// Is this `capability:` id known (e.g. "mongo.backup")?
    fn has_capability(&self, id: &str) -> bool;
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationFinding {
    pub tactic_id: String,
    pub step_index: usize,
    pub kind: FindingKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    UnknownCommand,
    UnknownCapability,
    MalformedCommand,
    UnknownInnerTactic,
    /// A mutating-or-destructive tactic's command step looks like it WOULD
    /// mutate (POST/PUT/PATCH/DELETE for an API, or `mutate`/`backup`/
    /// `restore`/`apply` keyword) but the cmd string does not include
    /// `--apply`. The executor's safety gate catches this at runtime, but
    /// surfacing it at lint time saves a turn.
    ApplyMissing,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ValidationReport {
    pub findings: Vec<ValidationFinding>,
    pub tactics_checked: usize,
    pub workflows_checked: usize,
    /// Tactics that workflows reference but the registry doesn't contain.
    pub workflow_dangling_tactics: Vec<(String, String)>,
}

impl ValidationReport {
    pub fn ok(&self) -> bool {
        self.findings.is_empty() && self.workflow_dangling_tactics.is_empty()
    }
}

pub fn validate<C: CatalogLookup>(registry: &Registry, catalog: &C) -> ValidationReport {
    let mut report = ValidationReport::default();

    for tactic in registry.tactics.values() {
        report.tactics_checked += 1;
        for (index, step) in tactic.steps.iter().enumerate() {
            match step {
                Step::Command {
                    cmd, capability, ..
                } => {
                    // Extract the command path (subcommand chain, ignoring
                    // flags and their values). We stop at the first arg
                    // starting with `-` or `<`.
                    let path = command_path_from_cmd(cmd);
                    match path {
                        Some(path) if !path.is_empty() => {
                            if !catalog.has_command_path(&path) {
                                report.findings.push(ValidationFinding {
                                    tactic_id: tactic.id.clone(),
                                    step_index: index,
                                    kind: FindingKind::UnknownCommand,
                                    detail: format!(
                                        "command path '{path}' is not in the catalog; check tactic step or rename"
                                    ),
                                });
                            }
                        }
                        _ => {
                            report.findings.push(ValidationFinding {
                                tactic_id: tactic.id.clone(),
                                step_index: index,
                                kind: FindingKind::MalformedCommand,
                                detail: format!(
                                    "cmd does not start with 'ayx <subcommand>': '{cmd}'"
                                ),
                            });
                        }
                    }
                    if let Some(cap) = capability
                        && !catalog.has_capability(cap)
                    {
                        report.findings.push(ValidationFinding {
                            tactic_id: tactic.id.clone(),
                            step_index: index,
                            kind: FindingKind::UnknownCapability,
                            detail: format!("capability id '{cap}' is not in the catalog"),
                        });
                    }
                    // Apply-missing lint: a mutating-or-destructive tactic
                    // step whose command looks like it would mutate state
                    // must include `--apply` in the cmd. The runtime gate
                    // catches missing `--apply` too, but flagging it at
                    // lint time saves a wasted execution turn.
                    if tactic.safety != Safety::ReadOnly
                        && step_looks_mutating(cmd)
                        && !cmd.contains("--apply")
                    {
                        report.findings.push(ValidationFinding {
                            tactic_id: tactic.id.clone(),
                            step_index: index,
                            kind: FindingKind::ApplyMissing,
                            detail: format!(
                                "tactic safety is '{}' and step appears to mutate state, but cmd does not include --apply: '{cmd}'",
                                tactic.safety.as_str()
                            ),
                        });
                    }
                }
                Step::Tactic { id, .. } => {
                    if registry.tactic(id).is_err() {
                        report.findings.push(ValidationFinding {
                            tactic_id: tactic.id.clone(),
                            step_index: index,
                            kind: FindingKind::UnknownInnerTactic,
                            detail: format!("composition references unknown tactic '{id}'"),
                        });
                    }
                }
                Step::Note { .. } => {}
            }
        }
    }

    for w in registry.workflows.values() {
        report.workflows_checked += 1;
        for tid in &w.tactics {
            if registry.tactic(tid).is_err() {
                report
                    .workflow_dangling_tactics
                    .push((w.id.clone(), tid.clone()));
            }
        }
    }

    report
}

/// Heuristic: does this command line look like it will mutate state?
///
/// True if the catalog command path matches a known mutation verb (POST/PUT
/// /PATCH/DELETE-style names) or the cmd contains a strong mutation keyword.
/// False positives are fine (they just become lint hints); false negatives
/// would be silent.
fn step_looks_mutating(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    // Dry-run preview steps are deliberately non-mutating, so exempt them
    // even if the verb otherwise looks dangerous. Two shapes:
    //   - explicit flag: `--dry-run`
    //   - dedicated subcommand: `something-dry-run`
    if lower.contains("--dry-run") || lower.contains("-dry-run ") || lower.ends_with("-dry-run") {
        return false;
    }
    // Hard signals: REST-verb-named subcommands plus the explicit `mutate`
    // verb. `apply` keywords are intentionally excluded — we WANT the cmd
    // to include `--apply`.
    const MUTATION_TOKENS: &[&str] = &[
        " create",
        " delete",
        " update",
        " patch",
        " import",
        " transfer",
        " migrate",
        " mutate",
        " restore",
        " invite",
        " suspend",
        " unsuspend",
        " deactivate",
        " activate",
        " pause",
        " resume",
        " rotate",
        " reset",
        " backup",
        " publish",
        " run",
        " copy",
    ];
    MUTATION_TOKENS.iter().any(|t| lower.contains(t))
}

/// Global flags that may appear *before* the subcommand chain. The
/// extractor skips these (and their values, where applicable) so a tactic
/// like `ayx --environment <env> --apply one flows list` resolves to
/// `one flows list`, not the empty string.
///
/// Boolean flags consume no value. Value flags consume the next token.
const GLOBAL_BOOL_FLAGS: &[&str] = &["--apply", "--verbose", "-v", "--debug", "--no-verify-tls"];
const GLOBAL_VALUE_FLAGS: &[&str] = &["--output", "--environment", "--profile"];

/// Extract `mongo backup` from `ayx mongo backup --profile <profile> ...`.
///
/// Stops at the first token that starts with `-` (a flag) or `<` (a
/// placeholder). Returns `None` if the command does not start with `ayx`.
/// Skips known global flags (and their values) that may appear before the
/// subcommand chain so `ayx --environment <env> one flows list` resolves
/// to `one flows list`.
fn command_path_from_cmd(cmd: &str) -> Option<String> {
    let mut iter = cmd.split_whitespace().peekable();
    if iter.next()? != "ayx" {
        return None;
    }
    // Skip any leading global flags + their values.
    while let Some(&tok) = iter.peek() {
        if GLOBAL_BOOL_FLAGS.contains(&tok) {
            iter.next();
        } else if GLOBAL_VALUE_FLAGS.contains(&tok) {
            iter.next();
            iter.next(); // consume the value
        } else if let Some(name) = tok.strip_prefix("--") {
            // `--output=json` style: skip in one shot.
            if name.contains('=') {
                iter.next();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    for tok in iter {
        if tok.starts_with('-') || tok.starts_with('<') {
            break;
        }
        parts.push(tok);
    }
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyCatalog;
    impl CatalogLookup for EmptyCatalog {
        fn has_command_path(&self, _: &str) -> bool {
            false
        }
        fn has_capability(&self, _: &str) -> bool {
            false
        }
    }

    struct PermissiveCatalog;
    impl CatalogLookup for PermissiveCatalog {
        fn has_command_path(&self, _: &str) -> bool {
            true
        }
        fn has_capability(&self, _: &str) -> bool {
            true
        }
    }

    #[test]
    fn extracts_command_path() {
        assert_eq!(
            command_path_from_cmd("ayx mongo backup --profile <profile> --apply"),
            Some("mongo backup".to_string())
        );
        assert_eq!(
            command_path_from_cmd("ayx one flows list"),
            Some("one flows list".to_string())
        );
        assert_eq!(command_path_from_cmd("foo bar"), None);
    }

    #[test]
    fn empty_catalog_flags_every_command() {
        let reg = Registry::load_default().unwrap();
        let report = validate(&reg, &EmptyCatalog);
        assert!(!report.ok());
        assert!(report.tactics_checked > 0);
        // Every command step should produce an UnknownCommand finding.
        let unknown_cmds = report
            .findings
            .iter()
            .filter(|f| matches!(f.kind, FindingKind::UnknownCommand))
            .count();
        assert!(unknown_cmds > 0);
    }

    #[test]
    fn permissive_catalog_passes() {
        let reg = Registry::load_default().unwrap();
        let report = validate(&reg, &PermissiveCatalog);
        // Workflow → tactic refs still need to resolve, but those are
        // already in the bundled stdlib, so the report should be clean.
        assert!(report.ok(), "findings: {:?}", report.findings);
    }
}
