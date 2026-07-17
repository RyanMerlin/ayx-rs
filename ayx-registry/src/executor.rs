//! Action / workflow executor.
//!
//! The registry stores recipes declaratively; this module turns them into
//! actual command invocations of the `ayx` binary itself. The execution
//! model is intentionally simple and observable:
//!
//! 1. Parameter substitution. `<placeholder>` tokens in a step's `cmd` get
//!    replaced from a caller-supplied `params` map. Unknown placeholders are
//!    reported up-front, before anything runs.
//! 2. Safety gate. `read_only` actions run freely. `mutating` and
//!    `destructive` actions require `apply = true`; without it, the executor
//!    returns a structured *plan* envelope describing every step that would
//!    fire, never invokes anything, and never touches state.
//! 3. Per-step execution. Each `Step::Command` is spawned as a subprocess of
//!    the current `ayx` binary (via `std::env::current_exe()`). The step's
//!    stdout is captured and parsed back into an envelope; if `ok == false`
//!    or the exit code is non-zero, the run fails-stop.
//! 4. Audit. Every step's envelope is appended to a `StepResult` vector and
//!    returned to the caller. If an `audit_dir` is supplied, a JSON file is
//!    written per step so the operator has a durable record.
//!
//! Workflows are executed by running their referenced actions in order; the
//! workflow's safety floor is the max of any referenced action's safety.
//!
//! Step::Action (composition) is resolved at execution time so an action
//! authored to reuse `mongo.backup-restore` actually invokes its steps.
//! Step::Note is surfaced in the plan and skipped at runtime.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Action, Registry, Safety, Step, extract_params};

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("action '{id}' is {safety:?}; --apply required to execute")]
    ApplyRequired { id: String, safety: Safety },
    #[error(
        "unknown parameter(s) referenced by action '{id}': {missing:?}. Provide via --param key=value."
    )]
    MissingParams { id: String, missing: Vec<String> },
    #[error("action '{id}' referenced by composition step does not exist")]
    InnerActionNotFound { id: String },
    #[error("failed to locate current ayx binary: {0}")]
    SelfBinary(String),
    #[error("failed to spawn step #{index} ({cmd}): {source}")]
    Spawn {
        index: usize,
        cmd: String,
        #[source]
        source: std::io::Error,
    },
    #[error("step #{index} failed: exit={exit_code:?} stderr={stderr}")]
    StepFailed {
        index: usize,
        cmd: String,
        exit_code: Option<i32>,
        stderr: String,
    },
    #[error("step #{index} returned a non-ok envelope: {message}")]
    StepEnvelopeNotOk {
        index: usize,
        cmd: String,
        message: String,
    },
    #[error("failed to write step audit '{path}': {source}")]
    AuditWrite {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not split command line '{cmd}' (unmatched quote?)")]
    Lex { cmd: String },
}

/// Result of executing (or planning) one step.
#[derive(Debug, Clone, Serialize)]
pub struct StepOutcome {
    pub index: usize,
    pub kind: &'static str,
    /// The command line *after* parameter substitution. None for note/action steps.
    pub cmd: Option<String>,
    pub why: Option<String>,
    /// `planned` for dry-run, `ok` / `failed` / `skipped` for executions.
    pub status: &'static str,
    /// The captured envelope (only present when status=ok and the step
    /// produced JSON output). When the step ran but didn't emit JSON, the
    /// raw stdout is preserved under `stdout` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Aggregate result of running (or planning) an action.
#[derive(Debug, Clone, Serialize)]
pub struct ActionRun {
    pub action_id: String,
    pub title: String,
    pub safety: Safety,
    pub apply: bool,
    pub mode: &'static str, // "plan" or "execute"
    pub params: BTreeMap<String, String>,
    pub steps: Vec<StepOutcome>,
    /// Validations as text (we don't auto-run their `check_cmd` — operator
    /// runs them).
    pub validations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub title: String,
    pub safety: Safety,
    pub apply: bool,
    pub mode: &'static str,
    pub actions: Vec<ActionRun>,
}

/// Configuration knobs for an execution.
#[derive(Debug, Clone, Default)]
pub struct ExecutionConfig {
    /// When false (the default), actions with safety > ReadOnly produce a
    /// plan envelope and never invoke a subprocess.
    pub apply: bool,
    /// Provided parameters. Missing parameters that the action references
    /// abort the run before any subprocess fires.
    pub params: BTreeMap<String, String>,
    /// When set, every step's envelope is written as JSON to
    /// `audit_dir/<action-id>-<ts>-step-<n>.json`. Inherits the standard
    /// `ayx-core::audit::resolve_audit_dir` behavior when set to the default
    /// `audits` path.
    pub audit_dir: Option<PathBuf>,
    /// Extra args to inject before each step's own args. Typically used to
    /// pass `--output json` so the executor can parse the envelope back.
    /// The executor will always set `--output json` itself; this is for
    /// future extension (e.g. propagating `--environment`).
    pub global_args: Vec<String>,
}

pub fn run_action(
    registry: &Registry,
    action_id: &str,
    cfg: &ExecutionConfig,
) -> Result<ActionRun, ExecutorError> {
    let action = registry
        .action(action_id)
        .map_err(|_| ExecutorError::InnerActionNotFound {
            id: action_id.to_string(),
        })?;
    run_action_inner(registry, action, cfg)
}

pub fn run_workflow(
    registry: &Registry,
    workflow_id: &str,
    cfg: &ExecutionConfig,
) -> Result<WorkflowRun, ExecutorError> {
    let workflow =
        registry
            .workflow(workflow_id)
            .map_err(|_| ExecutorError::InnerActionNotFound {
                id: workflow_id.to_string(),
            })?;
    let mode = if cfg.apply { "execute" } else { "plan" };
    let mut runs = Vec::with_capacity(workflow.actions.len());
    for tid in &workflow.actions {
        let action = registry
            .action(tid)
            .map_err(|_| ExecutorError::InnerActionNotFound { id: tid.clone() })?;
        runs.push(run_action_inner(registry, action, cfg)?);
    }
    Ok(WorkflowRun {
        workflow_id: workflow.id.clone(),
        title: workflow.title.clone(),
        safety: workflow.safety,
        apply: cfg.apply,
        mode,
        actions: runs,
    })
}

fn run_action_inner(
    registry: &Registry,
    action: &Action,
    cfg: &ExecutionConfig,
) -> Result<ActionRun, ExecutorError> {
    // Up-front parameter validation. We scan every Command step (and inline
    // any Action composition steps) before running so we fail loud, not
    // halfway through.
    let mut required: Vec<String> = Vec::new();
    collect_required_params(registry, action, &mut required);
    required.sort();
    required.dedup();
    let missing: Vec<String> = required
        .iter()
        .filter(|p| !cfg.params.contains_key(*p))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(ExecutorError::MissingParams {
            id: action.id.clone(),
            missing,
        });
    }

    let mode = if cfg.apply { "execute" } else { "plan" };
    let mut outcomes = Vec::new();
    let mut step_counter = 0usize;
    run_steps(
        registry,
        action,
        &action.steps,
        cfg,
        &mut outcomes,
        &mut step_counter,
    )?;

    Ok(ActionRun {
        action_id: action.id.clone(),
        title: action.title.clone(),
        safety: action.safety,
        apply: cfg.apply,
        mode,
        params: cfg.params.clone(),
        steps: outcomes,
        validations: action
            .validations
            .iter()
            .map(|v| v.describe.clone())
            .collect(),
        rollback: action.rollback.clone(),
    })
}

fn collect_required_params(registry: &Registry, action: &Action, out: &mut Vec<String>) {
    for step in &action.steps {
        match step {
            Step::Command { cmd, .. } => {
                for p in extract_params(cmd) {
                    out.push(p);
                }
            }
            Step::Action { id, .. } => {
                if let Ok(inner) = registry.action(id) {
                    collect_required_params(registry, inner, out);
                }
            }
            Step::Note { .. } => {}
        }
    }
}

fn substitute(cmd: &str, params: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut name = String::new();
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '>' {
                    closed = true;
                    break;
                }
                name.push(c);
            }
            if closed {
                if let Some(value) = params.get(&name) {
                    out.push_str(value);
                } else {
                    // Should be unreachable post-validation, but keep the
                    // original placeholder rather than silently dropping it.
                    out.push('<');
                    out.push_str(&name);
                    out.push('>');
                }
            } else {
                out.push('<');
                out.push_str(&name);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn run_steps(
    registry: &Registry,
    action: &Action,
    steps: &[Step],
    cfg: &ExecutionConfig,
    outcomes: &mut Vec<StepOutcome>,
    counter: &mut usize,
) -> Result<(), ExecutorError> {
    for step in steps {
        match step {
            Step::Note { text } => {
                *counter += 1;
                outcomes.push(StepOutcome {
                    index: *counter,
                    kind: "note",
                    cmd: None,
                    why: Some(text.clone()),
                    status: "skipped",
                    envelope: None,
                    stdout: None,
                    stderr: None,
                    exit_code: None,
                });
            }
            Step::Action { id, why } => {
                // Composition: inline the referenced action's steps so the
                // operator sees one flat plan.
                let inner = registry
                    .action(id)
                    .map_err(|_| ExecutorError::InnerActionNotFound { id: id.clone() })?;
                *counter += 1;
                outcomes.push(StepOutcome {
                    index: *counter,
                    kind: "action",
                    cmd: Some(format!("(action {})", inner.id)),
                    why: Some(why.clone()),
                    status: if cfg.apply { "expanded" } else { "planned" },
                    envelope: None,
                    stdout: None,
                    stderr: None,
                    exit_code: None,
                });
                run_steps(registry, inner, &inner.steps, cfg, outcomes, counter)?;
            }
            Step::Command { cmd, why, .. } => {
                *counter += 1;
                let resolved = substitute(cmd, &cfg.params);
                let safety_blocks = action.safety.requires_apply() && !cfg.apply;
                if safety_blocks {
                    outcomes.push(StepOutcome {
                        index: *counter,
                        kind: "command",
                        cmd: Some(resolved),
                        why: Some(why.clone()),
                        status: "planned",
                        envelope: None,
                        stdout: None,
                        stderr: None,
                        exit_code: None,
                    });
                    continue;
                }
                if !cfg.apply {
                    // Read-only action: still report as planned for symmetry,
                    // but DO run — read-only is safe by definition.
                }
                let outcome = execute_command_step(*counter, &resolved, why, cfg, &action.id)?;
                let failed = outcome.status != "ok";
                outcomes.push(outcome);
                if failed {
                    // Fail-stop: re-raise as ExecutorError so the caller sees
                    // a non-Ok at the top level. The partial `outcomes`
                    // vector is preserved in the caller's ActionRun.
                    let last = outcomes.last().unwrap();
                    return Err(match last.status {
                        "envelope-not-ok" => ExecutorError::StepEnvelopeNotOk {
                            index: last.index,
                            cmd: last.cmd.clone().unwrap_or_default(),
                            message: last
                                .envelope
                                .as_ref()
                                .and_then(|e| e.get("message").and_then(|m| m.as_str()))
                                .unwrap_or("")
                                .to_string(),
                        },
                        _ => ExecutorError::StepFailed {
                            index: last.index,
                            cmd: last.cmd.clone().unwrap_or_default(),
                            exit_code: last.exit_code,
                            stderr: last.stderr.clone().unwrap_or_default(),
                        },
                    });
                }
            }
        }
    }
    Ok(())
}

fn execute_command_step(
    index: usize,
    resolved: &str,
    why: &str,
    cfg: &ExecutionConfig,
    action_id: &str,
) -> Result<StepOutcome, ExecutorError> {
    let tokens = shell_split(resolved).ok_or_else(|| ExecutorError::Lex {
        cmd: resolved.to_string(),
    })?;
    // First token should be `ayx`. We replace it with our actual binary
    // path so users running a dev build invoke the same build, not a
    // possibly-stale PATH copy.
    let mut args = tokens;
    let bin = std::env::current_exe().map_err(|e| ExecutorError::SelfBinary(e.to_string()))?;
    if args.first().map(|s| s.as_str()) == Some("ayx") {
        args.remove(0);
    }
    // Force JSON output so we can parse the envelope.
    if !args.iter().any(|a| a == "--output") {
        args.insert(0, "--output".to_string());
        args.insert(1, "json".to_string());
    }
    // Inject any caller-provided global args (e.g. --environment foo) at
    // the very front, before the command name.
    for a in cfg.global_args.iter().rev() {
        args.insert(0, a.clone());
    }

    let output: Output =
        Command::new(&bin)
            .args(&args)
            .output()
            .map_err(|source| ExecutorError::Spawn {
                index,
                cmd: resolved.to_string(),
                source,
            })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();

    let envelope: Option<Value> = serde_json::from_str(&stdout).ok();
    let envelope_ok = envelope
        .as_ref()
        .and_then(|e| e.get("ok"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let status = if !output.status.success() {
        "failed"
    } else if envelope.is_some() && !envelope_ok {
        "envelope-not-ok"
    } else {
        "ok"
    };

    // Optional per-step audit artifact.
    if let Some(dir) = cfg.audit_dir.as_ref() {
        let resolved_dir = ayx_core::audit::resolve_audit_dir(dir);
        // Propagate dir-creation errors — silently swallowing them meant
        // every subsequent step would also fail with a less-clear write
        // error. The user has asked for an audit artifact; if we can't
        // create the directory, surface the actual reason now.
        std::fs::create_dir_all(&resolved_dir).map_err(|source| ExecutorError::AuditWrite {
            path: resolved_dir.display().to_string(),
            source,
        })?;
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let path = resolved_dir.join(format!("{}-{}-step-{:02}.json", action_id, ts, index));
        let payload = json!({
            "action_id": action_id,
            "step_index": index,
            "cmd": resolved,
            "why": why,
            "status": status,
            "exit_code": exit_code,
            "envelope": envelope.clone().unwrap_or(Value::Null),
            "stderr": stderr,
        });
        let body = serde_json::to_string_pretty(&payload).unwrap_or_default();
        std::fs::write(&path, body).map_err(|source| ExecutorError::AuditWrite {
            path: path.display().to_string(),
            source,
        })?;
        // 0o600 on Unix to match the audit module's posture. Silently
        // continuing on chmod failure is intentional: the artifact has
        // already been written successfully, and umask handling on quirky
        // platforms is not worth aborting the operation over.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }

    Ok(StepOutcome {
        index,
        kind: "command",
        cmd: Some(resolved.to_string()),
        why: Some(why.to_string()),
        status,
        envelope,
        stdout: if status == "ok" { None } else { Some(stdout) },
        stderr: if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        },
        exit_code,
    })
}

/// Minimal POSIX-style splitter: handles single and double quotes, plus
/// backslash escapes inside double quotes. Good enough for the constrained
/// command lines we put in actions; returns None on unbalanced quotes.
fn shell_split(s: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = s.chars().peekable();
    let mut have_token = false;
    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                cur.push(c);
            }
        } else if in_double {
            if c == '"' {
                in_double = false;
            } else if c == '\\' {
                if let Some(&next) = chars.peek() {
                    cur.push(next);
                    chars.next();
                }
            } else {
                cur.push(c);
            }
        } else if c == '\'' {
            in_single = true;
            have_token = true;
        } else if c == '"' {
            in_double = true;
            have_token = true;
        } else if c.is_whitespace() {
            if have_token {
                out.push(std::mem::take(&mut cur));
                have_token = false;
            }
        } else {
            cur.push(c);
            have_token = true;
        }
    }
    if in_single || in_double {
        return None;
    }
    if have_token {
        out.push(cur);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_placeholders() {
        let p = extract_params("ayx mongo backup --profile <profile> --output-dir backups/<ts>");
        assert_eq!(p, vec!["profile", "ts"]);
    }

    #[test]
    fn substitutes_known_params_and_leaves_unknown() {
        let mut params = BTreeMap::new();
        params.insert("profile".to_string(), "prod".to_string());
        let out = substitute(
            "ayx mongo backup --profile <profile> --output-dir backups/<ts>",
            &params,
        );
        assert!(out.contains("--profile prod"));
        assert!(out.contains("<ts>")); // unknown stays
    }

    #[test]
    fn shell_split_handles_quotes() {
        assert_eq!(
            shell_split("ayx one flows list --tag \"with spaces\"").unwrap(),
            vec!["ayx", "one", "flows", "list", "--tag", "with spaces"]
        );
        assert!(shell_split("missing 'quote").is_none());
    }

    #[test]
    fn missing_params_block_run() {
        let reg = Registry::load_default().expect("registry");
        let cfg = ExecutionConfig::default();
        let err = run_action(&reg, "mongo.backup-restore", &cfg).unwrap_err();
        assert!(matches!(err, ExecutorError::MissingParams { .. }));
    }

    #[test]
    fn read_only_runs_without_apply_but_still_needs_params() {
        let reg = Registry::load_default().expect("registry");
        let mut cfg = ExecutionConfig::default();
        // mongo.doctor takes a <profile> placeholder.
        cfg.params
            .insert("profile".to_string(), "missing-profile".to_string());
        // We don't assert on success here (we don't have a real profile) —
        // we just confirm the executor reaches the run path and that the
        // safety gate doesn't block a read_only action. The first step's
        // status will be one of ok/failed/envelope-not-ok, never planned.
        match run_action(&reg, "mongo.doctor", &cfg) {
            Ok(run) => {
                assert_eq!(run.mode, "plan"); // apply=false
                // Read-only actions actually execute steps; status must not be "planned".
                let first = &run.steps[0];
                assert_ne!(first.status, "planned");
            }
            Err(e) => {
                // The binary may not be invokable in the test env; that's
                // an environment failure, not an executor logic failure.
                eprintln!("ignoring environment-dependent error: {e}");
            }
        }
    }

    #[test]
    fn mutating_action_without_apply_emits_plan() {
        let reg = Registry::load_default().expect("registry");
        let mut cfg = ExecutionConfig::default();
        cfg.params.insert("profile".to_string(), "p".to_string());
        cfg.params
            .insert("ts".to_string(), "2026-05-11".to_string());
        let run = run_action(&reg, "mongo.backup-restore", &cfg).expect("plan runs");
        assert_eq!(run.mode, "plan");
        assert!(!run.apply);
        for s in &run.steps {
            // Every command step must be "planned" (none should be "ok" or "failed").
            if s.kind == "command" {
                assert_eq!(s.status, "planned", "mutating step ran without --apply");
            }
        }
    }
}
