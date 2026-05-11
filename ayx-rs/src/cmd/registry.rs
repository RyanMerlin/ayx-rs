//! Dispatch for `ayx tactics` and `ayx workflows`.
//!
//! Moved out of `main.rs` because the registry surface is a self-contained
//! feature with no `load_profile` closure dependency — easy to lift into
//! its own module. The `LiveCatalog` adapter that lets the registry's
//! validator query `COMMAND_SPECS` and the capability registry without
//! taking a direct dependency on either lives here.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use ayx_core::envelope::Envelope;
use serde_json::json;
use serde_json::Value;

use crate::capability;
use crate::{TacticsCommand, WorkflowsCommand, COMMAND_SPECS};

/// Catalog adapter — let the registry's validator query the CLI's
/// `COMMAND_SPECS` and capability registry without depending on either.
struct LiveCatalog;
impl ayx_registry::validate::CatalogLookup for LiveCatalog {
    fn has_command_path(&self, path: &str) -> bool {
        COMMAND_SPECS.iter().any(|spec| spec.name == path)
    }
    fn has_capability(&self, id: &str) -> bool {
        capability::has_capability(id)
    }
}

pub fn execute_tactics(apply: bool, command: TacticsCommand) -> Result<Envelope> {
    match command {
        TacticsCommand::List { tag, safety } => {
            let reg = ayx_registry::Registry::load_default()?;
            // Normalize the safety filter once. Unrecognized values bail
            // with a validation error so the user knows their filter never
            // matched anything (vs. silently returning an empty list).
            let safety_filter: Option<ayx_registry::Safety> = match safety
                .as_deref()
                .map(|s| s.to_ascii_lowercase())
            {
                None => None,
                Some(s) if s == "read_only" || s == "readonly" => Some(ayx_registry::Safety::ReadOnly),
                Some(s) if s == "mutating" => Some(ayx_registry::Safety::Mutating),
                Some(s) if s == "destructive" => Some(ayx_registry::Safety::Destructive),
                Some(s) => bail!("unknown --safety value '{s}'; expected one of: read_only, mutating, destructive"),
            };
            let mut tactics: Vec<_> = reg
                .tactics
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
            tactics.sort_by(|a, b| {
                a["id"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["id"].as_str().unwrap_or(""))
            });
            Ok(Envelope::ok_with_data(
                format!("{} tactic(s)", tactics.len()),
                json!({ "tactics": tactics }),
            ))
        }
        TacticsCommand::Describe { id } => {
            let reg = ayx_registry::Registry::load_default()?;
            let tactic = reg.tactic(&id)?;
            Ok(Envelope::ok_with_data(
                format!("tactic '{}'", tactic.id),
                serde_json::to_value(tactic)?,
            ))
        }
        TacticsCommand::Resolve { task, limit } => {
            let reg = ayx_registry::Registry::load_default()?;
            let mut hits = reg.resolve(&task);
            hits.truncate(limit);
            // Enrich each hit with the tactic's summary so text-mode tables
            // are self-explanatory (no follow-up `tactics describe` needed
            // just to know what each candidate does).
            let enriched: Vec<Value> = hits
                .iter()
                .map(|h| {
                    let summary = reg
                        .tactic(&h.tactic_id)
                        .map(|t| t.summary.lines().next().unwrap_or("").to_string())
                        .unwrap_or_default();
                    json!({
                        "tactic_id": h.tactic_id,
                        "title": h.title,
                        "safety": h.safety.as_str(),
                        "score": h.score,
                        "summary": summary,
                    })
                })
                .collect();
            Ok(Envelope::ok_with_data(
                format!("{} candidate tactic(s) for '{}'", enriched.len(), task),
                json!({ "task": task, "hits": enriched }),
            ))
        }
        TacticsCommand::Run {
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
                prompt_missing_tactic_params(&reg, &id, &mut cfg)?;
            }
            let run = ayx_registry::executor::run_tactic(&reg, &id, &cfg)?;
            let mode = run.mode;
            Ok(Envelope::ok_with_data(
                format!(
                    "tactic '{}' {}: {} step(s){}",
                    run.tactic_id,
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
        TacticsCommand::Export { id } => {
            let reg = ayx_registry::Registry::load_default()?;
            let tactic = reg.tactic(&id)?;
            let yaml = serde_yaml::to_string(tactic)
                .map_err(|e| anyhow!("failed to serialize tactic: {e}"))?;
            print!("{}", yaml);
            Ok(Envelope::ok_with_data(
                format!("tactic '{}' exported", id),
                json!({
                    "tactic_id": id,
                    "source": tactic.source_path.clone(),
                    "bytes": yaml.len(),
                    "save_hint": format!(
                        "Redirect this output into ${{AYX_CONFIG_HOME}}/registry/{}.tactic.yaml to fork the bundled stdlib version.",
                        id.replace('.', "-")
                    ),
                }),
            ))
        }
        TacticsCommand::Validate => {
            let reg = ayx_registry::Registry::load_default()?;
            let report = ayx_registry::validate::validate(&reg, &LiveCatalog);
            Ok(Envelope::ok_with_data(
                format!(
                    "validate: {} finding(s) across {} tactic(s), {} workflow(s)",
                    report.findings.len(),
                    report.tactics_checked,
                    report.workflows_checked
                ),
                serde_json::to_value(&report)?,
            ))
        }
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
                        "tactic_count": w.tactics.len(),
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
            let mut tactic_details = Vec::new();
            let mut missing = Vec::new();
            for tid in &workflow.tactics {
                match reg.tactic(tid) {
                    Ok(t) => tactic_details.push(json!({
                        "id": t.id,
                        "title": t.title,
                        "safety": t.safety.as_str(),
                        "summary": t.summary,
                        "step_count": t.steps.len(),
                    })),
                    Err(_) => missing.push(tid.clone()),
                }
            }
            Ok(Envelope::ok_with_data(
                format!("workflow '{}'", workflow.id),
                json!({
                    "workflow": workflow,
                    "tactics_resolved": tactic_details,
                    "tactics_missing": missing,
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
                    "workflow '{}' {}: {} tactic(s){}",
                    run.workflow_id,
                    mode,
                    run.tactics.len(),
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

fn prompt_missing_tactic_params(
    reg: &ayx_registry::Registry,
    tactic_id: &str,
    cfg: &mut ayx_registry::executor::ExecutionConfig,
) -> Result<()> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    let tactic = reg.tactic(tactic_id)?;
    let mut required: Vec<String> = Vec::new();
    collect_tactic_params(reg, tactic, &mut required);
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

fn prompt_missing_workflow_params(
    reg: &ayx_registry::Registry,
    workflow_id: &str,
    cfg: &mut ayx_registry::executor::ExecutionConfig,
) -> Result<()> {
    let workflow = reg.workflow(workflow_id)?;
    let mut required: Vec<String> = Vec::new();
    for tid in &workflow.tactics {
        if let Ok(t) = reg.tactic(tid) {
            collect_tactic_params(reg, t, &mut required);
        }
    }
    required.sort();
    required.dedup();
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
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

fn collect_tactic_params(
    reg: &ayx_registry::Registry,
    tactic: &ayx_registry::Tactic,
    out: &mut Vec<String>,
) {
    for step in &tactic.steps {
        match step {
            ayx_registry::Step::Command { cmd, .. } => {
                let mut chars = cmd.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '<' {
                        let mut name = String::new();
                        for c in chars.by_ref() {
                            if c == '>' {
                                if !name.is_empty() {
                                    out.push(name);
                                }
                                break;
                            }
                            name.push(c);
                        }
                    }
                }
            }
            ayx_registry::Step::Tactic { id, .. } => {
                if let Ok(inner) = reg.tactic(id) {
                    collect_tactic_params(reg, inner, out);
                }
            }
            ayx_registry::Step::Note { .. } => {}
        }
    }
}
