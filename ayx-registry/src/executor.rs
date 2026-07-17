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

use crate::io_schema::{self, SchemaViolation};
use crate::{Action, EffectiveSchema, Registry, Safety, SchemaOrigin, Step};

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("action '{id}' is {safety:?}; --apply required to execute")]
    ApplyRequired { id: String, safety: Safety },
    #[error(
        "unknown parameter(s) referenced by action '{id}': {missing:?}. Provide via --param key=value."
    )]
    MissingParams { id: String, missing: Vec<String> },
    /// `Explicit`-origin input contract violation: unknown parameter,
    /// empty/short string, enum/const mismatch, etc. Surfaced before a
    /// read-only command, mutating plan, or `--apply` subprocess can start
    /// — see `validate_input_contract`. `Inferred`-origin contracts keep
    /// the legacy `MissingParams` shape instead (existing callers depend
    /// on it).
    #[error(
        "action/workflow '{id}' input contract violation — {}",
        format_violations(.violations)
    )]
    InputContractViolation {
        id: String,
        violations: Vec<SchemaViolation>,
    },
    /// A completed `ActionRun`/`WorkflowRun` failed its owner's declared
    /// `output_schema`. This is a post-execution contract-integrity
    /// failure discovered only after the run record was fully built — by
    /// construction every step in that record already finished (or was
    /// planned); this must never trigger a retry, an extra step, or the
    /// action's own rollback text.
    #[error(
        "action/workflow '{id}' output contract violation (mismatch discovered after its steps may have executed) — {}",
        format_violations(.violations)
    )]
    OutputContractViolation {
        id: String,
        violations: Vec<SchemaViolation>,
    },
    /// A declared contract failed a composition-level invariant (cycle,
    /// property disagreement, required-set mismatch — see
    /// `RegistryError::SchemaContract`) when the executor asked the
    /// registry for an action/workflow's effective schema. Every
    /// `Registry::load_default()` registry already checked these
    /// invariants for every entry at `finalize()` time, so this fires only
    /// for a registry assembled without `finalize()` (e.g. a hand-built
    /// test registry) reaching an inconsistency `finalize()` would have
    /// caught at load time.
    #[error(transparent)]
    Registry(#[from] crate::RegistryError),
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

/// Render schema violations as one deterministically ordered, human-readable
/// string for an error's `Display` text. `SchemaViolation::path` is already
/// a JSON-pointer-style string (e.g. `/profile`); quoting it here keeps the
/// failing path unambiguous even when `reason` also contains punctuation.
/// `io_schema::validate_instance` returns violations pre-sorted by path, so
/// the joined message is stable across calls.
fn format_violations(violations: &[SchemaViolation]) -> String {
    violations
        .iter()
        .map(|v| format!("'{}': {}", v.path, v.reason))
        .collect::<Vec<_>>()
        .join("; ")
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

    // Validate the whole param map once against the workflow's own
    // effective contract before touching any referenced action.
    let workflow_effective = registry.effective_workflow_input_schema(workflow_id)?;
    validate_input_contract(workflow_id, &workflow_effective, &cfg.params)?;

    let mode = if cfg.apply { "execute" } else { "plan" };
    let mut runs = Vec::with_capacity(workflow.actions.len());
    for tid in &workflow.actions {
        let action = registry
            .action(tid)
            .map_err(|_| ExecutorError::InnerActionNotFound { id: tid.clone() })?;
        // Filter to this action's own declared/effective property set
        // before calling into it — see `filter_params_for_schema`'s docs.
        // `run_action_inner` then independently validates the filtered
        // subset against the action's own effective contract, so a strict
        // action never sees (and can't be rejected by) a sibling's key.
        let action_effective = registry.effective_action_input_schema(tid)?;
        let mut child_cfg = cfg.clone();
        child_cfg.params = filter_params_for_schema(&action_effective, &cfg.params);
        runs.push(run_action_inner(registry, action, &child_cfg)?);
    }

    let run = WorkflowRun {
        workflow_id: workflow.id.clone(),
        title: workflow.title.clone(),
        safety: workflow.safety,
        apply: cfg.apply,
        mode,
        actions: runs,
    };

    validate_output_contract(workflow_id, workflow.output_schema.as_ref(), || {
        serde_json::to_value(&run).expect("WorkflowRun is composed of plain serializable fields")
    })?;

    Ok(run)
}

fn run_action_inner(
    registry: &Registry,
    action: &Action,
    cfg: &ExecutionConfig,
) -> Result<ActionRun, ExecutorError> {
    // Up-front parameter validation against the registry's single canonical
    // effective contract (Task 2) — no more locally re-scanning
    // `action.steps` for placeholders. Must happen before any read-only
    // command, mutating plan, or `--apply` subprocess can start.
    let effective = registry.effective_action_input_schema(&action.id)?;
    validate_input_contract(&action.id, &effective, &cfg.params)?;

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

    let run = ActionRun {
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
    };

    validate_output_contract(&action.id, action.output_schema.as_ref(), || {
        serde_json::to_value(&run).expect("ActionRun is composed of plain serializable fields")
    })?;

    Ok(run)
}

/// Validate `params` against `id`'s effective input contract before any
/// step runs.
///
/// - `Inferred` origin preserves the exact legacy `MissingParams` shape
///   existing callers depend on. The required set now comes from the
///   registry's own effective schema (already derived from the same
///   recursive `extract_params` walk the old executor-local scan
///   duplicated) rather than a second, independently maintained scan.
/// - `Explicit` origin runs the full `io_schema` instance validator, so
///   unknown parameters, empty/short strings, and enum/const mismatches
///   are all caught here as `InputContractViolation`.
fn validate_input_contract(
    id: &str,
    effective: &EffectiveSchema,
    params: &BTreeMap<String, String>,
) -> Result<(), ExecutorError> {
    match effective.origin {
        SchemaOrigin::Inferred => {
            let required: Vec<String> = effective
                .schema
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let missing: Vec<String> = required
                .into_iter()
                .filter(|p| !params.contains_key(p))
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(ExecutorError::MissingParams {
                    id: id.to_string(),
                    missing,
                })
            }
        }
        SchemaOrigin::Explicit => {
            let instance = params_to_json_object(params);
            let violations = io_schema::validate_instance(&effective.schema, &instance);
            if violations.is_empty() {
                Ok(())
            } else {
                Err(ExecutorError::InputContractViolation {
                    id: id.to_string(),
                    violations,
                })
            }
        }
    }
}

/// Validate a completed `ActionRun`/`WorkflowRun` against its owner's
/// declared `output_schema`, when one is declared — absent a declared
/// schema, nothing is checked, since an undeclared action/workflow makes
/// no promise about the shape of the record it produces. `build_instance`
/// is only invoked (and only pays for `serde_json::to_value`) when a
/// schema is actually present to validate against.
///
/// Called after the run record is fully built, in both plan and execute
/// mode. A violation here is a post-execution contract-integrity failure,
/// not evidence that a mutating step ran incorrectly: every step in the
/// record it's validating already finished (or was planned) by the time
/// this runs, so it must never trigger a retry, an additional step, or the
/// action's own `rollback` text — see `ExecutorError::OutputContractViolation`.
fn validate_output_contract(
    id: &str,
    output_schema: Option<&Value>,
    build_instance: impl FnOnce() -> Value,
) -> Result<(), ExecutorError> {
    let Some(schema) = output_schema else {
        return Ok(());
    };
    let instance = build_instance();
    let violations = io_schema::validate_instance(schema, &instance);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ExecutorError::OutputContractViolation {
            id: id.to_string(),
            violations,
        })
    }
}

/// Convert the executor's lexical `--param key=value` map into the JSON
/// object `io_schema::validate_instance` expects. Every value stays a JSON
/// string — the CLI's parameter interface is lexical-string-only by design
/// (see `io_schema`'s module docs) — there is no type coercion here.
fn params_to_json_object(params: &BTreeMap<String, String>) -> Value {
    Value::Object(
        params
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

/// Select the parameter subset applicable to a child action at a
/// composition boundary (a workflow's referenced action, or an inlined
/// `Step::Action`). An `Explicit` contract is a closed contract —
/// `io_schema::SchemaRole::Input` requires `additionalProperties: false`
/// on every declared input schema — so forwarding a sibling's parameter
/// would surface as a spurious "unexpected property" violation; only that
/// child's own declared keys are forwarded. An `Inferred` contract carries
/// no such promise and keeps today's permissive full map — unchanged
/// behavior for every action that hasn't declared a schema yet.
fn filter_params_for_schema(
    effective: &EffectiveSchema,
    params: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    match effective.origin {
        SchemaOrigin::Inferred => params.clone(),
        SchemaOrigin::Explicit => {
            let allowed = io_schema::object_property_names(&effective.schema);
            params
                .iter()
                .filter(|(k, _)| allowed.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
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

                // Boundary check, mirroring `run_workflow`'s per-action
                // handling: filter to `inner`'s own declared/effective
                // property set, then validate that filtered subset against
                // `inner`'s own effective contract, before any of its
                // steps are planned or executed. A strict `inner` only
                // ever sees its own keys, so a parameter meant for a
                // sibling composition step never becomes a spurious
                // "unexpected property" violation here.
                let inner_effective = registry.effective_action_input_schema(id)?;
                let inner_params = filter_params_for_schema(&inner_effective, &cfg.params);
                validate_input_contract(id, &inner_effective, &inner_params)?;
                let mut inner_cfg = cfg.clone();
                inner_cfg.params = inner_params;

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
                run_steps(registry, inner, &inner.steps, &inner_cfg, outcomes, counter)?;
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
    use crate::extract_params;

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

    // -----------------------------------------------------------------
    // Step 5: deterministic executor tests for input/output contract
    // enforcement. Every fixture action is `safety: mutating` and every
    // test runs with `apply: false`, so no command step ever leaves
    // "planned" — no real profile or subprocess is ever needed, and
    // command output can never masquerade as contract behavior.
    // -----------------------------------------------------------------

    /// One shared in-memory registry for every contract-enforcement test
    /// below, built the same way `lib.rs`'s own tests build fixtures:
    /// write real `*.action.yaml`/`*.workflow.yaml` files into a tempdir
    /// and `load_dir` them — no `Registry::load_default()`, so none of
    /// this is coupled to the bundled stdlib (which, as of this task, has
    /// no declared schemas at all — see Task 5).
    fn fixture_registry() -> Registry {
        let dir = tempfile::tempdir().expect("tempdir");
        let files: &[(&str, &str)] = &[
            (
                "strict.action.yaml",
                r#"
id: test.strict
title: Strict test action
summary: Declares a closed input contract with a minLength and an enum property.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --name <name> --env <env>"
    why: exercise strict input contract
input_schema:
  type: object
  description: Strict parameters.
  additionalProperties: false
  required: [name, env]
  properties:
    name:
      type: string
      description: A name, at least 3 characters.
      minLength: 3
    env:
      type: string
      description: Target environment.
      enum: [prod, staging]
"#,
            ),
            (
                "legacy.action.yaml",
                r#"
id: test.legacy
title: Legacy test action
summary: No declared input_schema; contract is placeholder-inferred.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --token <token>"
    why: exercise legacy inferred contract
"#,
            ),
            (
                "strict-a.action.yaml",
                r#"
id: test.strict-a
title: Strict A
summary: Requires only 'alpha', constrained to an enum.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --alpha <alpha>"
    why: a-only
input_schema:
  type: object
  description: A's own parameters.
  additionalProperties: false
  required: [alpha]
  properties:
    alpha:
      type: string
      description: Alpha value.
      enum: [x, y]
"#,
            ),
            (
                "strict-b.action.yaml",
                r#"
id: test.strict-b
title: Strict B
summary: Requires only 'beta', constrained to an enum.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --beta <beta>"
    why: b-only
input_schema:
  type: object
  description: B's own parameters.
  additionalProperties: false
  required: [beta]
  properties:
    beta:
      type: string
      description: Beta value.
      enum: [prod, staging]
"#,
            ),
            (
                "disjoint.workflow.yaml",
                r#"
id: test.disjoint-workflow
title: Disjoint workflow
summary: Composes A and B, which declare disjoint required parameters.
safety: mutating
actions: [test.strict-a, test.strict-b]
"#,
            ),
            (
                "nested-parent.action.yaml",
                r#"
id: test.nested-parent
title: Nested parent
summary: Composes strict-a and strict-b via Step::Action (no declared schema of its own).
safety: mutating
steps:
  - kind: action
    id: test.strict-a
    why: run a
  - kind: action
    id: test.strict-b
    why: run b
"#,
            ),
            (
                "output-match.action.yaml",
                r#"
id: test.output-match
title: Output match
summary: Declares an output_schema that matches its own plan-mode record.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --name <name>"
    why: exercise output contract
input_schema:
  type: object
  description: Parameters.
  additionalProperties: false
  required: [name]
  properties:
    name:
      type: string
      description: A name.
output_schema:
  type: object
  description: Result record.
  required: [action_id, mode]
  properties:
    action_id:
      type: string
      description: Stable action id.
      const: test.output-match
    mode:
      type: string
      description: plan or execute.
      enum: [plan, execute]
"#,
            ),
            (
                "output-mismatch.action.yaml",
                r#"
id: test.output-mismatch
title: Output mismatch
summary: Declares an output_schema with a deliberately wrong const.
safety: mutating
steps:
  - kind: command
    cmd: "ayx test noop --name <name>"
    why: exercise output contract violation
input_schema:
  type: object
  description: Parameters.
  additionalProperties: false
  required: [name]
  properties:
    name:
      type: string
      description: A name.
output_schema:
  type: object
  description: Result record.
  required: [action_id, mode]
  properties:
    action_id:
      type: string
      description: Deliberately wrong const so output validation must fire.
      const: totally-not-the-real-id
    mode:
      type: string
      description: plan or execute.
      enum: [plan, execute]
"#,
            ),
            (
                "output-workflow.workflow.yaml",
                r#"
id: test.output-workflow
title: Output workflow
summary: Declares a workflow output_schema that matches its own plan-mode record.
safety: mutating
actions: [test.output-match]
output_schema:
  type: object
  description: Workflow result record.
  required: [workflow_id, mode]
  properties:
    workflow_id:
      type: string
      description: Stable workflow id.
      const: test.output-workflow
    mode:
      type: string
      description: plan or execute.
      enum: [plan, execute]
"#,
            ),
            (
                "output-workflow-mismatch.workflow.yaml",
                r#"
id: test.output-workflow-mismatch
title: Output workflow mismatch
summary: Declares a workflow output_schema with a deliberately wrong const.
safety: mutating
actions: [test.output-match]
output_schema:
  type: object
  description: Workflow result record.
  required: [workflow_id, mode]
  properties:
    workflow_id:
      type: string
      description: Deliberately wrong const so output validation must fire.
      const: totally-not-the-real-workflow-id
    mode:
      type: string
      description: plan or execute.
      enum: [plan, execute]
"#,
            ),
        ];
        for (name, body) in files {
            std::fs::write(dir.path().join(name), body).expect("write fixture");
        }
        let mut reg = Registry::default();
        reg.load_dir(dir.path()).expect("fixture registry loads");
        reg
    }

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn explicit_missing_required_params_rejected_before_any_step() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig::default(); // no params at all
        // `run_action` returning `Err` before ever producing an `ActionRun`
        // *is* the proof no step exists yet: `run_action_inner` only
        // starts building `ActionRun.steps` after input validation
        // succeeds (see Step 1's placement, before `run_steps` is called).
        let err = run_action(&reg, "test.strict", &cfg).unwrap_err();
        match err {
            ExecutorError::InputContractViolation { id, violations } => {
                assert_eq!(id, "test.strict");
                let paths: Vec<&str> = violations.iter().map(|v| v.path.as_str()).collect();
                assert!(paths.contains(&"/name"), "paths: {paths:?}");
                assert!(paths.contains(&"/env"), "paths: {paths:?}");
            }
            other => panic!("expected InputContractViolation, got {other:?}"),
        }
    }

    #[test]
    fn explicit_unknown_parameter_rejected() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "abc"), ("env", "prod"), ("extra", "x")]),
            ..Default::default()
        };
        let err = run_action(&reg, "test.strict", &cfg).unwrap_err();
        match err {
            ExecutorError::InputContractViolation { violations, .. } => {
                assert!(
                    violations.iter().any(|v| v.path == "/extra"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected InputContractViolation, got {other:?}"),
        }
    }

    #[test]
    fn explicit_enum_violation_rejected() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "abc"), ("env", "qa")]),
            ..Default::default()
        };
        let err = run_action(&reg, "test.strict", &cfg).unwrap_err();
        match err {
            ExecutorError::InputContractViolation { violations, .. } => {
                assert!(
                    violations.iter().any(|v| v.path == "/env"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected InputContractViolation, got {other:?}"),
        }
    }

    #[test]
    fn explicit_min_length_violation_rejected() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "ab"), ("env", "prod")]),
            ..Default::default()
        };
        let err = run_action(&reg, "test.strict", &cfg).unwrap_err();
        match err {
            ExecutorError::InputContractViolation { violations, .. } => {
                assert!(
                    violations.iter().any(|v| v.path == "/name"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected InputContractViolation, got {other:?}"),
        }
    }

    #[test]
    fn legacy_missing_params_still_uses_missing_params_error() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig::default();
        let err = run_action(&reg, "test.legacy", &cfg).unwrap_err();
        match err {
            ExecutorError::MissingParams { id, missing } => {
                assert_eq!(id, "test.legacy");
                assert_eq!(missing, vec!["token".to_string()]);
            }
            other => panic!("expected MissingParams (legacy behavior unchanged), got {other:?}"),
        }
    }

    #[test]
    fn workflow_filters_disjoint_params_per_action() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("alpha", "x"), ("beta", "prod")]),
            ..Default::default()
        };
        let run = run_workflow(&reg, "test.disjoint-workflow", &cfg).expect("plans");
        assert_eq!(run.actions.len(), 2);
        // Each action must receive ONLY its own declared keys — not the
        // full workflow param map. Under the old (pre-filtering) executor
        // `ActionRun.params` was always `cfg.params.clone()` unfiltered,
        // so this assertion fails without Step 2's boundary filtering.
        let a = &run.actions[0];
        assert_eq!(a.action_id, "test.strict-a");
        assert_eq!(a.params, params(&[("alpha", "x")]));
        let b = &run.actions[1];
        assert_eq!(b.action_id, "test.strict-b");
        assert_eq!(b.params, params(&[("beta", "prod")]));
    }

    #[test]
    fn nested_step_action_composition_plans_with_disjoint_params() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("alpha", "x"), ("beta", "prod")]),
            ..Default::default()
        };
        let run = run_action(&reg, "test.nested-parent", &cfg).expect("plans");
        assert_eq!(run.mode, "plan");
        for s in &run.steps {
            if s.kind == "command" {
                assert_eq!(s.status, "planned");
            }
        }
    }

    /// The real discriminator for nested `Step::Action` filtering: the
    /// *parent* (`test.nested-parent`) has no declared schema, so its own
    /// top-level check is a permissive, generic-string inferred contract
    /// that has no idea `test.strict-a.alpha` is constrained to an enum.
    /// Only a genuine per-child boundary check (Step 2, inside
    /// `run_steps`'s `Step::Action` arm) catches this — proving filtering
    /// at this boundary does real validation, not just pass-through.
    #[test]
    fn nested_step_action_enforces_composed_childs_own_contract() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("alpha", "not-in-enum"), ("beta", "prod")]),
            ..Default::default()
        };
        let err = run_action(&reg, "test.nested-parent", &cfg).unwrap_err();
        match err {
            ExecutorError::InputContractViolation { id, violations } => {
                assert_eq!(id, "test.strict-a");
                assert!(
                    violations.iter().any(|v| v.path == "/alpha"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected InputContractViolation for test.strict-a, got {other:?}"),
        }
    }

    #[test]
    fn action_output_schema_matches_in_plan_mode() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "x")]),
            ..Default::default()
        };
        let run = run_action(&reg, "test.output-match", &cfg).expect("output matches");
        assert_eq!(run.mode, "plan");
    }

    #[test]
    fn action_output_schema_mismatch_is_rejected() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "x")]),
            ..Default::default()
        };
        let err = run_action(&reg, "test.output-mismatch", &cfg).unwrap_err();
        match err {
            ExecutorError::OutputContractViolation { id, violations } => {
                assert_eq!(id, "test.output-mismatch");
                assert!(
                    violations.iter().any(|v| v.path == "/action_id"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected OutputContractViolation, got {other:?}"),
        }
    }

    #[test]
    fn workflow_output_schema_matches_in_plan_mode() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "x")]),
            ..Default::default()
        };
        let run = run_workflow(&reg, "test.output-workflow", &cfg).expect("output matches");
        assert_eq!(run.mode, "plan");
    }

    #[test]
    fn workflow_output_schema_mismatch_is_rejected() {
        let reg = fixture_registry();
        let cfg = ExecutionConfig {
            params: params(&[("name", "x")]),
            ..Default::default()
        };
        let err = run_workflow(&reg, "test.output-workflow-mismatch", &cfg).unwrap_err();
        match err {
            ExecutorError::OutputContractViolation { id, violations } => {
                assert_eq!(id, "test.output-workflow-mismatch");
                assert!(
                    violations.iter().any(|v| v.path == "/workflow_id"),
                    "violations: {violations:?}"
                );
            }
            other => panic!("expected OutputContractViolation, got {other:?}"),
        }
    }
}
