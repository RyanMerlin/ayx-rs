//! Dispatch for `ayx actions` and nested workflow registry commands.
//!
//! Moved out of `main.rs` because the registry surface is a self-contained
//! feature with no `load_profile` closure dependency — easy to lift into
//! its own module. The `LiveCatalog` adapter that lets the registry's
//! validator query the live command surface and the capability registry
//! without taking a direct dependency on either lives here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use ayx_core::envelope::Envelope;
use serde_json::Value;
use serde_json::json;

use crate::capability;
use crate::cmd::command_surface;
use crate::{ActionsCommand, WorkflowsCommand};

/// Catalog adapter — let the registry's validator query the live command
/// surface and capability registry without depending on either.
///
/// Backed by the *entire* visible live command tree (not just `catalog`'s
/// curated scope), so an action referencing a real but not-yet-annotated
/// command is never falsely reported as unknown. `command_names` is a
/// snapshot of `command_surface::visible_commands()`'s canonical whitespace
/// `name` set, captured once by `LiveCatalog::new()` — `has_command_path`
/// then does a plain set lookup instead of re-walking the clap tree on every
/// call, which matters because `actions validate` checks many command
/// references in a single run.
struct LiveCatalog {
    command_names: BTreeSet<String>,
}

impl LiveCatalog {
    fn new() -> Self {
        let command_names = command_surface::visible_commands()
            .into_iter()
            .map(|cmd| cmd.name)
            .collect();
        Self { command_names }
    }
}

impl ayx_registry::validate::CatalogLookup for LiveCatalog {
    fn has_command_path(&self, path: &str) -> bool {
        self.command_names.contains(path)
    }
    fn has_capability(&self, id: &str) -> bool {
        capability::has_capability(id)
    }
}

pub fn execute_actions(apply: bool, command: ActionsCommand) -> Result<Envelope> {
    match command {
        ActionsCommand::List { tag, safety } => {
            let reg = ayx_registry::Registry::load_default()?;
            // Normalize the safety filter once. Unrecognized values bail
            // with a validation error so the user knows their filter never
            // matched anything (vs. silently returning an empty list).
            let safety_filter: Option<ayx_registry::Safety> = match safety
                .as_deref()
                .map(|s| s.to_ascii_lowercase())
            {
                None => None,
                Some(s) if s == "read_only" || s == "readonly" => {
                    Some(ayx_registry::Safety::ReadOnly)
                }
                Some(s) if s == "mutating" => Some(ayx_registry::Safety::Mutating),
                Some(s) if s == "destructive" => Some(ayx_registry::Safety::Destructive),
                Some(s) => bail!(
                    "unknown --safety value '{s}'; expected one of: read_only, mutating, destructive"
                ),
            };
            let mut actions: Vec<_> = reg
                .actions
                .values()
                .filter(|t| match &tag {
                    Some(needle) => t.trigger.tags.iter().any(|tag| {
                        tag.to_ascii_lowercase()
                            .contains(&needle.to_ascii_lowercase())
                    }),
                    None => true,
                })
                .filter(|t| match safety_filter {
                    Some(s) => t.safety == s,
                    None => true,
                })
                .map(|t| {
                    json!({
                        "id": t.id,
                        "title": t.title,
                        "safety": t.safety.as_str(),
                        "tags": t.trigger.tags,
                        "source": t.source_path,
                    })
                })
                .collect();
            actions.sort_by(|a, b| {
                a["id"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["id"].as_str().unwrap_or(""))
            });
            Ok(Envelope::ok_with_data(
                format!("{} action(s)", actions.len()),
                json!({ "actions": actions }),
            ))
        }
        ActionsCommand::Describe { id } => {
            let reg = ayx_registry::Registry::load_default()?;
            let action = reg.action(&id)?;
            let effective = reg.effective_action_input_schema(&id)?;
            let described =
                overlay_effective_input_schema(serde_json::to_value(action)?, effective);
            Ok(Envelope::ok_with_data(
                format!("action '{}'", action.id),
                described,
            ))
        }
        ActionsCommand::Resolve { task, limit } => {
            let reg = ayx_registry::Registry::load_default()?;
            let mut hits = reg.resolve(&task);
            hits.truncate(limit);
            // Enrich each hit with the action's summary so text-mode tables
            // are self-explanatory (no follow-up `actions describe` needed
            // just to know what each candidate does).
            let enriched: Vec<Value> = hits
                .iter()
                .map(|h| {
                    let summary = reg
                        .action(&h.action_id)
                        .map(|t| t.summary.lines().next().unwrap_or("").to_string())
                        .unwrap_or_default();
                    json!({
                        "action_id": h.action_id,
                        "title": h.title,
                        "safety": h.safety.as_str(),
                        "score": h.score,
                        "summary": summary,
                    })
                })
                .collect();
            Ok(Envelope::ok_with_data(
                format!("{} candidate action(s) for '{}'", enriched.len(), task),
                json!({ "task": task, "hits": enriched }),
            ))
        }
        ActionsCommand::Run {
            id,
            param,
            param_file,
            audit_dir,
            prompt_missing,
        } => {
            let reg = ayx_registry::Registry::load_default()?;
            let mut cfg = ayx_registry::executor::ExecutionConfig {
                apply,
                audit_dir,
                ..Default::default()
            };
            if let Some(path) = param_file.as_ref() {
                load_params_from_file(path, &mut cfg.params)?;
            }
            for (k, v) in param {
                cfg.params.insert(k, v);
            }
            if prompt_missing {
                prompt_missing_action_params(&reg, &id, &mut cfg)?;
            }
            let run = ayx_registry::executor::run_action(&reg, &id, &cfg)?;
            let mode = run.mode;
            Ok(Envelope::ok_with_data(
                format!(
                    "action '{}' {}: {} step(s){}",
                    run.action_id,
                    mode,
                    run.steps.len(),
                    if apply {
                        ""
                    } else {
                        " (use --apply to execute)"
                    }
                ),
                serde_json::to_value(run)?,
            ))
        }
        ActionsCommand::Export { id } => {
            let reg = ayx_registry::Registry::load_default()?;
            let action = reg.action(&id)?;
            let yaml = serde_yaml::to_string(action)
                .map_err(|e| anyhow!("failed to serialize action: {e}"))?;
            Ok(Envelope::ok_with_data(
                format!("action '{}' exported", id),
                json!({
                    "action_id": id,
                    "source": action.source_path.clone(),
                    "bytes": yaml.len(),
                    "yaml": yaml,
                    "save_hint": format!(
                        "For raw YAML, run `ayx actions export {id} --output json | jq -r '.data.yaml' > ${{AYX_CONFIG_HOME}}/registry/{}.action.yaml` to fork the bundled stdlib version.",
                        id.replace('.', "-")
                    ),
                }),
            ))
        }
        ActionsCommand::Validate => {
            let reg = ayx_registry::Registry::load_default()?;
            let catalog = LiveCatalog::new();
            let report = ayx_registry::validate::validate(&reg, &catalog);
            Ok(Envelope::ok_with_data(
                format!(
                    "validate: {} finding(s) across {} action(s), {} workflow(s)",
                    report.findings.len(),
                    report.actions_checked,
                    report.workflows_checked
                ),
                serde_json::to_value(&report)?,
            ))
        }
        ActionsCommand::Workflows { command } => execute_workflows(apply, command),
    }
}

pub fn execute_workflows(apply: bool, command: WorkflowsCommand) -> Result<Envelope> {
    match command {
        WorkflowsCommand::List { tag } => {
            let reg = ayx_registry::Registry::load_default()?;
            let mut workflows: Vec<_> = reg
                .workflows
                .values()
                .filter(|w| match &tag {
                    Some(needle) => w.tags.iter().any(|t| {
                        t.to_ascii_lowercase()
                            .contains(&needle.to_ascii_lowercase())
                    }),
                    None => true,
                })
                .map(|w| {
                    json!({
                        "id": w.id,
                        "title": w.title,
                        "safety": w.safety.as_str(),
                        "action_count": w.actions.len(),
                        "tags": w.tags,
                        "source": w.source_path,
                    })
                })
                .collect();
            workflows.sort_by(|a, b| {
                a["id"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["id"].as_str().unwrap_or(""))
            });
            Ok(Envelope::ok_with_data(
                format!("{} workflow(s)", workflows.len()),
                json!({ "workflows": workflows }),
            ))
        }
        WorkflowsCommand::Explain { id } => {
            let reg = ayx_registry::Registry::load_default()?;
            let workflow = reg.workflow(&id)?;
            let (action_details, missing) = workflow_action_details(&reg, workflow);
            let effective = reg.effective_workflow_input_schema(&id)?;
            let described =
                overlay_effective_input_schema(serde_json::to_value(workflow)?, effective);
            Ok(Envelope::ok_with_data(
                format!("workflow '{}'", workflow.id),
                json!({
                    "workflow": described,
                    "actions_resolved": action_details,
                    "actions_missing": missing,
                }),
            ))
        }
        WorkflowsCommand::Run {
            id,
            param,
            param_file,
            audit_dir,
            prompt_missing,
        } => {
            let reg = ayx_registry::Registry::load_default()?;
            let mut cfg = ayx_registry::executor::ExecutionConfig {
                apply,
                audit_dir,
                ..Default::default()
            };
            if let Some(path) = param_file.as_ref() {
                load_params_from_file(path, &mut cfg.params)?;
            }
            for (k, v) in param {
                cfg.params.insert(k, v);
            }
            if prompt_missing {
                prompt_missing_workflow_params(&reg, &id, &mut cfg)?;
            }
            let run = ayx_registry::executor::run_workflow(&reg, &id, &cfg)?;
            let mode = run.mode;
            Ok(Envelope::ok_with_data(
                format!(
                    "workflow '{}' {}: {} action(s){}",
                    run.workflow_id,
                    mode,
                    run.actions.len(),
                    if apply {
                        ""
                    } else {
                        " (use --apply to execute)"
                    }
                ),
                serde_json::to_value(run)?,
            ))
        }
    }
}

// ─── Discovery descriptor helpers (Task 4) ─────────────────────────────────
//
// `actions describe` / `workflows explain` are the agent-facing source of
// truth for "what does this thing actually require/produce" — the compact
// index endpoints (`actions list`, `actions resolve`, `workflows list`)
// deliberately stay ranking/lookup-only and never carry a full schema; an
// agent should resolve/list to find a candidate id, then call `describe` /
// `explain` on that id before constructing `--param` values.

/// Layer an [`ayx_registry::EffectiveSchema`] onto an already-serialized
/// action/workflow value, in place: adds `input_schema` (round-trips a
/// declared schema exactly as authored, or the loader's inferred permissive
/// fallback for a legacy file) and `input_schema_source`
/// (`"declared"` | `"inferred"`, from `SchemaOrigin::as_str`). Never touches
/// `output_schema` — the stored `Action`/`Workflow` already serializes that
/// field verbatim (present only when declared, never synthesized), so no
/// overlay is needed for it.
///
/// Shared by `ActionsCommand::Describe` and `WorkflowsCommand::Explain` so
/// both discovery endpoints read the exact same effective-contract
/// resolution the registry uses everywhere else (executor enforcement,
/// `--prompt-missing`) — never a second, independently-drifting notion of
/// "what's required".
fn overlay_effective_input_schema(
    mut value: Value,
    effective: ayx_registry::EffectiveSchema,
) -> Value {
    let obj = value
        .as_object_mut()
        .expect("Action/Workflow always serializes to a JSON object");
    obj.insert("input_schema".to_string(), effective.schema);
    obj.insert(
        "input_schema_source".to_string(),
        Value::String(effective.origin.as_str().to_string()),
    );
    value
}

/// Resolve each id in `workflow.actions` against the registry, splitting
/// into resolved action summaries (`actions_resolved`) and dangling ids
/// (`actions_missing`). Factored out of `WorkflowsCommand::Explain` so a
/// test can exercise it against an in-memory registry without going through
/// `Registry::load_default()`; behavior/output shape is unchanged from
/// before Task 4.
fn workflow_action_details(
    reg: &ayx_registry::Registry,
    workflow: &ayx_registry::Workflow,
) -> (Vec<Value>, Vec<String>) {
    let mut action_details = Vec::new();
    let mut missing = Vec::new();
    for tid in &workflow.actions {
        match reg.action(tid) {
            Ok(t) => action_details.push(json!({
                "id": t.id,
                "title": t.title,
                "safety": t.safety.as_str(),
                "summary": t.summary,
                "step_count": t.steps.len(),
            })),
            Err(_) => missing.push(tid.clone()),
        }
    }
    (action_details, missing)
}

// ─── Param helpers (moved out of main.rs) ──────────────────────────────────

fn load_params_from_file(path: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("failed to read --param-file '{}'", path.display()))?;
    let value: serde_yaml::Value = serde_yaml::from_str(&body)
        .with_context(|| format!("failed to parse --param-file '{}'", path.display()))?;
    let map = value
        .as_mapping()
        .ok_or_else(|| anyhow!("--param-file '{}' must be a YAML map", path.display()))?;
    for (k, v) in map {
        let key = k
            .as_str()
            .ok_or_else(|| anyhow!("non-string key in --param-file '{}'", path.display()))?;
        let str_val = match v {
            serde_yaml::Value::String(s) => s.clone(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::Null => String::new(),
            other => serde_yaml::to_string(other)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
        };
        out.entry(key.to_string()).or_insert(str_val);
    }
    Ok(())
}

/// Pull the `required` property names out of an effective input-schema
/// [`Value`] (as returned by `Registry::effective_action_input_schema` /
/// `effective_workflow_input_schema`). Both `Explicit` (author-declared) and
/// `Inferred` (loader-synthesized) schemas carry a `required` array under
/// the `io_schema` grammar, so this one reader covers both origins — the
/// single required-key source for both `describe`/`explain` (indirectly,
/// via the schema itself) and `--prompt-missing` (Task 4, Step 2).
fn required_keys(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Interactively fill any `required` key not already present in
/// `cfg.params`, sorted + deduped so prompt order is stable and no key is
/// asked twice. Caller is responsible for the TTY gate — this always
/// prompts if called, matching the pre-Task-4 helpers' behavior once they'd
/// passed their own `is_terminal()` check.
fn prompt_for_required_params(
    mut required: Vec<String>,
    cfg: &mut ayx_registry::executor::ExecutionConfig,
) -> Result<()> {
    use std::io::Write;
    required.sort();
    required.dedup();
    for key in required {
        if cfg.params.contains_key(&key) {
            continue;
        }
        eprint!("[param] {key}: ");
        let _ = std::io::stderr().flush();
        let mut value = String::new();
        std::io::stdin().read_line(&mut value)?;
        let v = value.trim().to_string();
        if !v.is_empty() {
            cfg.params.insert(key, v);
        }
    }
    Ok(())
}

fn prompt_missing_action_params(
    reg: &ayx_registry::Registry,
    action_id: &str,
    cfg: &mut ayx_registry::executor::ExecutionConfig,
) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    let effective = reg.effective_action_input_schema(action_id)?;
    prompt_for_required_params(required_keys(&effective.schema), cfg)
}

fn prompt_missing_workflow_params(
    reg: &ayx_registry::Registry,
    workflow_id: &str,
    cfg: &mut ayx_registry::executor::ExecutionConfig,
) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    let effective = reg.effective_workflow_input_schema(workflow_id)?;
    prompt_for_required_params(required_keys(&effective.schema), cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ayx_registry::validate::CatalogLookup;

    // `mongo mutate` and `mongo undo` are real, visible clap commands; any
    // remediation action referencing either must not be falsely reported as
    // unknown by `ayx actions validate`. Backed by the live command surface
    // directly, so this is really just a regression guard against `mongo
    // mutate`/`mongo undo` losing their `#[command(about = ...)]` and
    // dropping out of the visible tree.
    #[test]
    fn live_catalog_knows_mongo_mutate_and_undo() {
        let catalog = LiveCatalog::new();
        assert!(
            catalog.has_command_path("mongo mutate"),
            "'mongo mutate' is missing from the live command tree"
        );
        assert!(
            catalog.has_command_path("mongo undo"),
            "'mongo undo' is missing from the live command tree"
        );
    }

    // End-to-end adapter coverage (Task 3, Step 3): prove `LiveCatalog`
    // tracks command *reality* (the live clap tree), not an arbitrary
    // curated catalog subset.

    // `mongo status` is a real, visible command AND is referenced by a
    // bundled legacy action (`mongo-backup-restore.action.yaml`, step
    // `ayx mongo status --profile <profile>`). It also happens to carry a
    // `CATALOG_METADATA` row (curated), so this exercises the "known legacy
    // action command still resolves" case end-to-end through `LiveCatalog`.
    #[test]
    fn live_catalog_resolves_a_known_legacy_action_command() {
        let catalog = LiveCatalog::new();
        assert!(
            catalog.has_command_path("mongo status"),
            "'mongo status' — referenced by mongo-backup-restore.action.yaml — \
             is missing from the live command tree"
        );
    }

    // `telemetry summary` is a real, visible command with NO
    // `CATALOG_METADATA` row — it only ever shows up under
    // `catalog list --scope all`, never `--scope curated`. Before this task,
    // an action referencing it would still have validated correctly (the
    // interim fix already widened lookup to the full visible tree); this
    // test locks that behavior into the real constructor so a future
    // regression back to a curated-only lookup fails loudly here.
    #[test]
    fn live_catalog_resolves_an_all_scope_only_command() {
        let catalog = LiveCatalog::new();
        assert!(
            catalog.has_command_path("telemetry summary"),
            "'telemetry summary' has no CATALOG_METADATA row but is a real, \
             visible command — it must still resolve through LiveCatalog"
        );
    }

    // An invented command that has never existed in the clap tree must
    // remain unknown — the adapter should not become permissive by accident
    // (e.g. a substring match instead of exact set membership).
    #[test]
    fn live_catalog_rejects_an_invented_command() {
        let catalog = LiveCatalog::new();
        assert!(
            !catalog.has_command_path("mongo definitely-not-a-real-subcommand"),
            "an invented command path must not resolve"
        );
    }

    // Final-review regression guard (post-Task-3 finding): `permissive_catalog_passes`
    // in `ayx-registry`'s own test suite exercises `validate()` against the real
    // bundled registry, but with a `PermissiveCatalog` stub that answers `true` to
    // every lookup — so it can never catch a bundled action's `cmd:` drifting away
    // from a real, current clap command path. This test closes that gap by running
    // the *real* `LiveCatalog` (the live clap tree) against the *real* bundled
    // registry end-to-end, the same combination `ayx actions validate` uses at
    // runtime. It caught `server-logs-triage.action.yaml` calling the non-existent
    // top-level `ayx server-logs discover`/`context` instead of the real nested
    // `ayx server server-logs discover`/`context` — a drift the old curated
    // `COMMAND_SPECS` catalog masked with a stale top-level entry.
    #[test]
    fn live_catalog_end_to_end_validate_report_is_clean() {
        let reg = ayx_registry::Registry::load_default().expect("bundled registry must load");
        let catalog = LiveCatalog::new();
        let report = ayx_registry::validate::validate(&reg, &catalog);
        assert!(
            report.ok(),
            "bundled actions/workflows must validate cleanly against the live \
             command tree; findings: {:?}; dangling workflow actions: {:?}",
            report.findings,
            report.workflow_dangling_actions
        );
    }

    // -- Task 4: discovery descriptors + prompt-helper consolidation ----
    //
    // No bundled action/workflow declares a schema yet (that's Task 5), so
    // these build tiny in-memory registries via `Registry::default()` +
    // `load_dir` + `finalize()` — the same loader path `Registry::load_default`
    // uses, minus the bundled stdlib — rather than depending on bundled
    // fixtures or env-var-overriding the default search path.

    /// Step 1/4: a declared action's descriptor round-trips its own
    /// `input_schema` (required list, property descriptions) verbatim, is
    /// tagged `input_schema_source: "declared"`, and carries its declared
    /// `output_schema` through untouched.
    #[test]
    fn describe_action_reports_declared_schema_and_required_properties() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("declared.action.yaml"),
            "id: test.describe-declared\n\
             title: Declared schema action\n\
             summary: Exercises a fully declared input/output contract\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <profile>\"\n\
             \x20   why: check\n\
             input_schema:\n\
             \x20 type: object\n\
             \x20 description: Parameters.\n\
             \x20 additionalProperties: false\n\
             \x20 required: [profile]\n\
             \x20 properties:\n\
             \x20   profile:\n\
             \x20     type: string\n\
             \x20     description: Named profile to check.\n\
             output_schema:\n\
             \x20 type: object\n\
             \x20 description: Doctor check result.\n\
             \x20 required: [healthy]\n\
             \x20 properties:\n\
             \x20   healthy:\n\
             \x20     type: boolean\n\
             \x20     description: Whether mongo reported healthy.\n",
        )
        .expect("write action file");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let action = reg
            .action("test.describe-declared")
            .expect("action present");
        let effective = reg
            .effective_action_input_schema("test.describe-declared")
            .expect("effective schema resolves");
        let described =
            overlay_effective_input_schema(serde_json::to_value(action).unwrap(), effective);

        assert_eq!(described["input_schema_source"], json!("declared"));
        assert_eq!(described["input_schema"]["required"], json!(["profile"]));
        assert_eq!(
            described["input_schema"]["properties"]["profile"]["description"],
            json!("Named profile to check.")
        );
        assert_eq!(described["output_schema"]["required"], json!(["healthy"]));
        assert_eq!(
            described["output_schema"]["properties"]["healthy"]["description"],
            json!("Whether mongo reported healthy.")
        );
    }

    /// Step 1/4: a legacy action with no declared schema gets an inferred
    /// permissive contract (`input_schema_source: "inferred"`, required set
    /// == its placeholders, `additionalProperties: true`) and — critically
    /// — no `output_schema` is synthesized for it; the field stays absent.
    #[test]
    fn describe_action_reports_inferred_schema_with_no_output_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("legacy.action.yaml"),
            "id: test.describe-legacy\n\
             title: Legacy action\n\
             summary: No declared schema; loader infers one\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <profile>\"\n\
             \x20   why: check\n",
        )
        .expect("write action file");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let action = reg.action("test.describe-legacy").expect("action present");
        let effective = reg
            .effective_action_input_schema("test.describe-legacy")
            .expect("effective schema resolves");
        let described =
            overlay_effective_input_schema(serde_json::to_value(action).unwrap(), effective);

        assert_eq!(described["input_schema_source"], json!("inferred"));
        assert_eq!(described["input_schema"]["required"], json!(["profile"]));
        assert_eq!(
            described["input_schema"]["additionalProperties"],
            json!(true)
        );
        assert!(
            described.get("output_schema").is_none(),
            "a legacy action must never have a synthesized output_schema: {described:?}"
        );
    }

    /// Step 1/4: workflow `explain` exposes its own effective schema
    /// alongside the pre-existing `actions_resolved`/`actions_missing`
    /// split (`workflow_action_details`, unchanged output shape). No
    /// declared workflow schema here, so the union is inferred from its
    /// two resolvable actions.
    #[test]
    fn workflow_action_details_and_effective_schema_expose_composition() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("leaf-a.action.yaml"),
            "id: test.wf-leaf-a\n\
             title: Leaf A\n\
             summary: uses alpha\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <alpha>\"\n\
             \x20   why: check\n",
        )
        .expect("write leaf a");
        std::fs::write(
            dir.path().join("leaf-b.action.yaml"),
            "id: test.wf-leaf-b\n\
             title: Leaf B\n\
             summary: uses beta\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <beta>\"\n\
             \x20   why: check\n",
        )
        .expect("write leaf b");
        std::fs::write(
            dir.path().join("wf.workflow.yaml"),
            "id: test.wf-composed\n\
             title: Composed workflow\n\
             summary: two resolvable actions, one dangling reference\n\
             safety: read_only\n\
             actions: [test.wf-leaf-a, test.wf-leaf-b, test.wf-does-not-exist]\n",
        )
        .expect("write workflow");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let workflow = reg.workflow("test.wf-composed").expect("workflow present");
        let (resolved, missing) = workflow_action_details(&reg, workflow);
        assert_eq!(resolved.len(), 2, "resolved: {resolved:?}");
        assert_eq!(missing, vec!["test.wf-does-not-exist".to_string()]);

        let effective = reg
            .effective_workflow_input_schema("test.wf-composed")
            .expect("effective schema resolves");
        let described =
            overlay_effective_input_schema(serde_json::to_value(workflow).unwrap(), effective);

        assert_eq!(described["input_schema_source"], json!("inferred"));
        assert_eq!(
            required_keys(&described["input_schema"]),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    /// Step 2: the required-key source (`required_keys` over the effective
    /// schema) covers a composed action's transitive placeholders, not just
    /// its own direct command steps — the same guarantee
    /// `effective_action_input_schema` already enforces for declared
    /// schemas, now proven for the inferred/prompting path too.
    #[test]
    fn required_keys_covers_transitive_action_composition() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("child.action.yaml"),
            "id: test.compose-child\n\
             title: Child\n\
             summary: uses region\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <region>\"\n\
             \x20   why: check\n",
        )
        .expect("write child");
        std::fs::write(
            dir.path().join("parent.action.yaml"),
            "id: test.compose-parent\n\
             title: Parent\n\
             summary: composes child, also uses profile directly\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <profile>\"\n\
             \x20   why: check\n\
             \x20 - kind: action\n\
             \x20   id: test.compose-child\n\
             \x20   why: compose\n",
        )
        .expect("write parent");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let effective = reg
            .effective_action_input_schema("test.compose-parent")
            .expect("effective schema resolves");
        assert_eq!(
            required_keys(&effective.schema),
            vec!["profile".to_string(), "region".to_string()],
            "the parent's required-key set must include its composed child's \
             transitive placeholder"
        );
    }

    /// Step 2: `--prompt-missing`'s TTY gate must still no-op with zero
    /// stdin reads when stdin isn't a terminal (the harness running this
    /// test process never has one) — including for a composed action whose
    /// required-key set spans a child action, so the consolidation onto
    /// `effective_action_input_schema` didn't accidentally move the TTY
    /// check after any registry/schema work that could itself fail.
    #[test]
    fn prompt_missing_action_params_is_a_noop_off_tty_even_for_composed_action() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("child.action.yaml"),
            "id: test.prompt-child\n\
             title: Child\n\
             summary: uses region\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <region>\"\n\
             \x20   why: check\n",
        )
        .expect("write child");
        std::fs::write(
            dir.path().join("parent.action.yaml"),
            "id: test.prompt-parent\n\
             title: Parent\n\
             summary: composes child\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: action\n\
             \x20   id: test.prompt-child\n\
             \x20   why: compose\n",
        )
        .expect("write parent");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let mut cfg = ayx_registry::executor::ExecutionConfig::default();
        prompt_missing_action_params(&reg, "test.prompt-parent", &mut cfg)
            .expect("no-op off a TTY");
        assert!(
            cfg.params.is_empty(),
            "no params should have been filled off-TTY: {:?}",
            cfg.params
        );
    }

    /// Step 2: same TTY no-op guarantee on the workflow path.
    #[test]
    fn prompt_missing_workflow_params_is_a_noop_off_tty() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("leaf.action.yaml"),
            "id: test.prompt-wf-leaf\n\
             title: Leaf\n\
             summary: uses profile\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: command\n\
             \x20   cmd: \"ayx mongo doctor --profile <profile>\"\n\
             \x20   why: check\n",
        )
        .expect("write leaf");
        std::fs::write(
            dir.path().join("wf.workflow.yaml"),
            "id: test.prompt-wf\n\
             title: WF\n\
             summary: one action\n\
             safety: read_only\n\
             actions: [test.prompt-wf-leaf]\n",
        )
        .expect("write workflow");

        let mut reg = ayx_registry::Registry::default();
        reg.load_dir(dir.path()).expect("loads");
        reg.finalize().expect("finalizes");

        let mut cfg = ayx_registry::executor::ExecutionConfig::default();
        prompt_missing_workflow_params(&reg, "test.prompt-wf", &mut cfg).expect("no-op off a TTY");
        assert!(
            cfg.params.is_empty(),
            "no params should have been filled off-TTY: {:?}",
            cfg.params
        );
    }
}
