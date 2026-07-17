//! `ayx-registry` — action, workflow, and capability registry for `ayx`.
//!
//! This crate is the "agent substrate" layer of the toolset. The CLI itself
//! is a command surface; the registry layers on top of it a small,
//! declarative model of *named playbooks* (actions) and *multi-step
//! orchestrations* (workflows) so an LLM or operator can ask "what's the
//! recipe for X?" and get a curated answer instead of having to discover
//! the surface flag-by-flag.
//!
//! # Design
//!
//! Three primitives:
//!
//! - **Capability** — a single addressable thing the CLI can do, identified
//!   by a stable id (`mongo.backup`, `one.flow.list`, …). Capabilities are
//!   produced by the CLI's capability registry; this crate consumes them
//!   indirectly via the catalog when the registry resolver wants to point
//!   an action at a concrete command.
//! - **Action** — a small declarative recipe: a trigger pattern (when does
//!   this apply), guardrails (read-only? mutating? `--apply` required?),
//!   the canonical command sequence, validation steps, and rollback notes.
//!   Actions are leaf-level: an agent can run one without further planning.
//! - **Workflow** — a higher-order recipe that strings actions together
//!   into a multi-step skill (e.g. `governance-go-live` =
//!   `backup` → `apply-rbac` → `verify-permissions` → `audit-report`).
//!
//! Actions and workflows live as YAML files on disk under a *search path*:
//!
//!   1. `$AYX_REGISTRY_DIR` if set (operator override / dev override)
//!   2. `${AYX_CONFIG_HOME}/registry/`
//!   3. The crate-bundled `actions/` and `workflows/` directories (shipped
//!      with the binary as `include_str!` fallbacks).
//!
//! Layer 3 is the "stdlib" of canonical recipes; layers 1-2 let operators
//! add their own or override ours without rebuilding the binary.
//!
//! # Versioning + safety
//!
//! Every action carries a `safety` field (one of `read_only`, `mutating`,
//! `destructive`). The CLI refuses to *run* (vs. *describe*) a mutating or
//! destructive action without `--apply`; this is the same gate the One API
//! transport already enforces, but lifted to the registry layer so the
//! check happens before any command would fire.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;
use walkdir::WalkDir;

mod io_schema;
mod stdlib;

pub mod executor;
pub mod validate;

/// Errors surfaced from the registry layer.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry path '{path}' could not be read: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("action '{id}' not found")]
    ActionNotFound { id: String },
    #[error("workflow '{id}' not found")]
    WorkflowNotFound { id: String },
    #[error("duplicate id '{id}' loaded from both '{first}' and '{second}'")]
    DuplicateId {
        id: String,
        first: String,
        second: String,
    },
    #[error("action '{id}' is {safety:?}; --apply required to run (use `describe` to inspect)")]
    ApplyRequired { id: String, safety: Safety },
    /// Covers two related failure classes for an `input_schema`/
    /// `output_schema` declaration:
    ///
    /// - **Grammar** (`Step 2`): the declared schema document itself isn't
    ///   well-formed under the `io_schema` subset — caught at parse/insert
    ///   time, before the malformed schema can reach an executor.
    /// - **Contract** (`Steps 3-4`): the schema document is grammatically
    ///   valid, but violates a composition invariant — a required
    ///   placeholder is missing from `required`, a composed/referenced
    ///   action disagrees on a shared property's definition, or an action
    ///   composition cycle exists. Caught at `Registry::finalize`, once
    ///   every override directory and the bundled stdlib have loaded.
    ///
    /// `path` is the owning file's `source_path` (or bundled resource
    /// label); `owner_kind` is `"action"` or `"workflow"`; `location` is a
    /// JSON-pointer-like pointer into the offending schema field (grammar
    /// errors) or a synthetic pointer such as `/steps` (a composition
    /// cycle) — always relative to the owning action/workflow, never the
    /// bare schema document.
    #[error("{owner_kind} '{owner_id}' ({path}) — {location}: {message}")]
    SchemaContract {
        path: String,
        owner_kind: &'static str,
        owner_id: String,
        location: String,
        message: String,
    },
}

/// Classifies the blast radius of an action / workflow. The registry refuses
/// to execute anything beyond `ReadOnly` unless the caller passes `apply`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Safety {
    /// Pure inspection: backups, diagnostics, list/describe operations.
    ReadOnly,
    /// Writes to state but reversible (e.g. add a user, attach a connection).
    Mutating,
    /// Hard to reverse without backups (delete person, bulk update, schema
    /// migration). The CLI requires an additional confirmation flag.
    Destructive,
}

impl Safety {
    pub fn as_str(self) -> &'static str {
        match self {
            Safety::ReadOnly => "read_only",
            Safety::Mutating => "mutating",
            Safety::Destructive => "destructive",
        }
    }

    pub fn requires_apply(self) -> bool {
        !matches!(self, Safety::ReadOnly)
    }

    /// Ordinal rank for comparing safety levels. Higher = more dangerous.
    pub fn rank(self) -> u8 {
        match self {
            Safety::ReadOnly => 0,
            Safety::Mutating => 1,
            Safety::Destructive => 2,
        }
    }

    /// Take the more-dangerous of two safety classifications.
    pub fn max(self, other: Safety) -> Safety {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// A trigger pattern — when should this action surface as a candidate?
/// The matching is intentionally simple (substring + tag) so a future LLM
/// resolver can compose its own embeddings on top without us locking in a
/// brittle regex DSL today.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Trigger {
    /// Free-text patterns the resolver substring-matches against the task
    /// description. Case-insensitive.
    #[serde(default)]
    pub task_keywords: Vec<String>,
    /// Tags (e.g. `mongo`, `one`, `governance`, `migration`) for coarse
    /// filtering. Actions matching all tags listed in a query rank higher.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One concrete step inside an action. Either a shell command line that the
/// operator runs, or a reference to another action (composition).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// A specific `ayx ...` command to invoke. Stored as a *template* (no
    /// shell interpolation by us) so callers can pretty-print and copy.
    Command {
        /// The full command line, e.g. `ayx mongo backup --profile <profile>`.
        cmd: String,
        /// One-sentence rationale for the step.
        why: String,
        /// Optional. Names the capability id this step exercises so the
        /// catalog can cross-link.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability: Option<String>,
    },
    /// Invoke another action by id. Lets common building blocks be reused.
    Action { id: String, why: String },
    /// A note to the operator — nothing to execute, but worth surfacing
    /// (e.g. "verify ticket is open before proceeding").
    Note { text: String },
}

/// A validation step that proves the action achieved its intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validation {
    /// Human description: "queue is empty", "schedule is paused", etc.
    pub describe: String,
    /// Optional command whose output the operator should inspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_cmd: Option<String>,
}

/// Wire-format version for action YAML files. Bump only on a breaking change
/// to the schema. `2` marks the 0.14.0 `tactic` → `action` rename, which
/// changed both the `kind:` step tag and a workflow's `actions:` key.
///
/// NOTE: nothing currently *compares* against this — it is descriptive
/// metadata, not an enforced gate. What actually keeps a pre-0.14.0 file out
/// is the `.action.yaml` extension match in `load_dir` (a legacy
/// `*.tactic.yaml` is skipped with a warning), plus serde rejecting a stale
/// `kind: tactic` step. Enforcement is tracked separately; do not read this
/// constant as a guarantee.
pub const CURRENT_ACTION_SCHEMA: u32 = 2;

/// An action — one named playbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Wire-format version. Omitted in a file means "current"
    /// (`CURRENT_ACTION_SCHEMA`), not 1 — so this field cannot by itself
    /// identify an older file. Not currently compared by any reader; see
    /// `CURRENT_ACTION_SCHEMA`.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub summary: String,
    pub safety: Safety,
    #[serde(default)]
    pub trigger: Trigger,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub validations: Vec<Validation>,
    /// Optional rollback note: "restore from <audit_file>", etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<String>,
    /// Optional JSON-Schema-shaped contract (see `io_schema` for the
    /// supported subset) describing the parameter object this action's
    /// `<placeholder>` steps accept. Absent means "no explicit contract" —
    /// the loader still derives an *inferred* permissive contract from the
    /// action's placeholders (see `Registry::effective_action_input_schema`),
    /// it just isn't author-declared or strictly validated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    /// Optional JSON-Schema-shaped contract describing the `ActionRun`
    /// record this action produces in a successful run's `Envelope.data`.
    /// Absent means "not declared / not output-validated" — never an
    /// invented guarantee about the run record's shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// Provenance: which file the loader read this from. Set by the loader,
    /// never by the YAML author.
    #[serde(default, skip_serializing)]
    pub source_path: String,
}

fn default_schema_version() -> u32 {
    CURRENT_ACTION_SCHEMA
}

/// A workflow — references actions + adds top-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub summary: String,
    pub safety: Safety,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Ordered list of action ids. The resolver walks this in order; the
    /// CLI surfaces it as a numbered plan.
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<String>,
    /// Optional JSON-Schema-shaped contract describing the parameter object
    /// this workflow accepts. When declared it must be exactly the union of
    /// its referenced actions' effective input contracts (see
    /// `Registry::effective_workflow_input_schema`); when absent an
    /// inferred permissive union is used instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    /// Optional JSON-Schema-shaped contract describing the `WorkflowRun`
    /// record this workflow produces in a successful run's `Envelope.data`.
    /// There is no output binding/dataflow model between actions, so this
    /// is never derived from child action outputs — only from what's
    /// explicitly declared here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing)]
    pub source_path: String,
}

/// Loaded registry — both actions and workflows, indexed by id.
#[derive(Debug, Default)]
pub struct Registry {
    pub actions: BTreeMap<String, Action>,
    pub workflows: BTreeMap<String, Workflow>,
    /// Paths actually scanned (for diagnostics).
    pub sources: Vec<PathBuf>,
}

impl Registry {
    /// Load the registry using the default search path:
    /// `$AYX_REGISTRY_DIR`, then `${AYX_CONFIG_HOME}/registry/`, then the
    /// crate-bundled stdlib.
    pub fn load_default() -> Result<Self, RegistryError> {
        let mut reg = Registry::default();
        for path in default_search_paths() {
            if path.exists() {
                reg.load_dir(&path)?;
            }
        }
        // Stdlib fallback always loads — operator overrides above will win on
        // duplicate ids by virtue of being inserted first.
        stdlib::install_into(&mut reg)?;
        reg.propagate_workflow_safety();
        Ok(reg)
    }

    /// Auto-promote every workflow's declared safety to the max of any
    /// referenced action's safety. Prevents a workflow declared `mutating`
    /// from quietly composing a `destructive` action and gaining a weaker
    /// gate than it should have. Idempotent.
    pub fn propagate_workflow_safety(&mut self) {
        let action_safety: std::collections::BTreeMap<String, Safety> = self
            .actions
            .iter()
            .map(|(id, t)| (id.clone(), t.safety))
            .collect();
        for workflow in self.workflows.values_mut() {
            let mut effective = workflow.safety;
            for tid in &workflow.actions {
                if let Some(s) = action_safety.get(tid) {
                    effective = effective.max(*s);
                }
            }
            workflow.safety = effective;
        }
    }

    /// Load a directory of YAML files into this registry. Walks recursively;
    /// `*.action.yaml` parse as actions, `*.workflow.yaml` as workflows.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), RegistryError> {
        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".action.yaml") || name.ends_with(".action.yml") {
                let body = fs::read_to_string(p).map_err(|source| RegistryError::Io {
                    path: p.display().to_string(),
                    source,
                })?;
                let mut action: Action =
                    serde_yaml::from_str(&body).map_err(|source| RegistryError::Parse {
                        path: p.display().to_string(),
                        source,
                    })?;
                action.source_path = p.display().to_string();
                self.insert_action(action)?;
                self.sources.push(p.to_path_buf());
            } else if name.ends_with(".workflow.yaml") || name.ends_with(".workflow.yml") {
                let body = fs::read_to_string(p).map_err(|source| RegistryError::Io {
                    path: p.display().to_string(),
                    source,
                })?;
                let mut workflow: Workflow =
                    serde_yaml::from_str(&body).map_err(|source| RegistryError::Parse {
                        path: p.display().to_string(),
                        source,
                    })?;
                workflow.source_path = p.display().to_string();
                self.insert_workflow(workflow)?;
                self.sources.push(p.to_path_buf());
            } else if name.ends_with(".tactic.yaml") || name.ends_with(".tactic.yml") {
                // 0.14.0 renamed the `tactic` noun to `action`, including this
                // file extension. Staying silent here would let a bundled action
                // quietly reclaim the id an operator had overridden — and if
                // their override tightened `safety`, the gate would silently
                // relax. Say so loudly instead; the file is still not loaded.
                eprintln!(
                    "warning: ignoring legacy registry file '{}' — the `tactic` \
                     concept was renamed to `action` in 0.14.0. Rename it to \
                     '*.action.yaml' and change any `kind: tactic` step to \
                     `kind: action` (a workflow's `tactics:` key is now `actions:`). \
                     Until then this file is NOT loaded and any bundled action \
                     sharing its id will be used instead.",
                    p.display()
                );
            }
        }
        Ok(())
    }

    /// Insert an action, preserving the earlier copy on duplicates (operator
    /// overrides win because they're loaded first).
    pub(crate) fn insert_action(&mut self, t: Action) -> Result<(), RegistryError> {
        // Grammar-check any declared schema *before* the duplicate-id logic
        // below can decide to silently keep an earlier copy and drop this
        // one — a malformed declaration must fail loudly at its own file,
        // not vanish because another file happened to load first.
        if let Some(schema) = &t.input_schema {
            check_schema_grammar(
                "input_schema",
                schema,
                io_schema::SchemaRole::Input,
                "action",
                &t.id,
                &t.source_path,
            )?;
        }
        if let Some(schema) = &t.output_schema {
            check_schema_grammar(
                "output_schema",
                schema,
                io_schema::SchemaRole::Output,
                "action",
                &t.id,
                &t.source_path,
            )?;
        }
        if let Some(existing) = self.actions.get(&t.id) {
            // Operator override already present — keep it.
            if existing.source_path != t.source_path {
                return Ok(());
            }
            return Err(RegistryError::DuplicateId {
                id: t.id.clone(),
                first: existing.source_path.clone(),
                second: t.source_path.clone(),
            });
        }
        self.actions.insert(t.id.clone(), t);
        Ok(())
    }

    pub(crate) fn insert_workflow(&mut self, w: Workflow) -> Result<(), RegistryError> {
        if let Some(schema) = &w.input_schema {
            check_schema_grammar(
                "input_schema",
                schema,
                io_schema::SchemaRole::Input,
                "workflow",
                &w.id,
                &w.source_path,
            )?;
        }
        if let Some(schema) = &w.output_schema {
            check_schema_grammar(
                "output_schema",
                schema,
                io_schema::SchemaRole::Output,
                "workflow",
                &w.id,
                &w.source_path,
            )?;
        }
        if let Some(existing) = self.workflows.get(&w.id) {
            if existing.source_path != w.source_path {
                return Ok(());
            }
            return Err(RegistryError::DuplicateId {
                id: w.id.clone(),
                first: existing.source_path.clone(),
                second: w.source_path.clone(),
            });
        }
        self.workflows.insert(w.id.clone(), w);
        Ok(())
    }

    pub fn action(&self, id: &str) -> Result<&Action, RegistryError> {
        self.actions
            .get(id)
            .ok_or_else(|| RegistryError::ActionNotFound { id: id.to_string() })
    }

    pub fn workflow(&self, id: &str) -> Result<&Workflow, RegistryError> {
        self.workflows
            .get(id)
            .ok_or_else(|| RegistryError::WorkflowNotFound { id: id.to_string() })
    }

    /// Resolve a free-text task description to ranked candidate actions.
    ///
    /// Ranking is dumb on purpose: count of keyword + tag matches. A future
    /// LLM resolver can swap this for an embedding match without changing
    /// the public surface.
    ///
    /// Performance: each action's keywords/tags/title are lowercased *once*
    /// per `resolve` call (kept on the stack as `Vec<String>`s) rather than
    /// per-comparison. For a 10-action library × 5 keywords each that's
    /// 50 → 0 redundant allocations per query.
    pub fn resolve(&self, task: &str) -> Vec<ResolveHit> {
        let needle = task.to_ascii_lowercase();
        let needle_words: Vec<&str> = needle
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        let mut hits: Vec<ResolveHit> = self
            .actions
            .values()
            .map(|t| {
                let mut score = 0u32;
                // Lowercase each keyword/tag/title once per action. The old
                // code allocated a fresh String per comparison — fine for 10
                // actions, painful at the 100-action registry size that's a
                // plausible target.
                for kw in &t.trigger.task_keywords {
                    let kw_lower = kw.to_ascii_lowercase();
                    if needle.contains(&kw_lower) {
                        score += 3;
                    }
                }
                for tag in &t.trigger.tags {
                    let tag_lower = tag.to_ascii_lowercase();
                    if needle_words
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(&tag_lower))
                    {
                        score += 2;
                    }
                }
                // `contains(&needle)` against the title — only allocate the
                // lowercased title when there's a chance it could match.
                if !needle.is_empty() {
                    let title_lower = t.title.to_ascii_lowercase();
                    if title_lower.contains(&needle) {
                        score += 1;
                    }
                }
                ResolveHit {
                    action_id: t.id.clone(),
                    title: t.title.clone(),
                    safety: t.safety,
                    score,
                }
            })
            .filter(|h| h.score > 0)
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.action_id.cmp(&b.action_id)));
        hits
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveHit {
    pub action_id: String,
    pub title: String,
    pub safety: Safety,
    pub score: u32,
}

/// Grammar-validate one declared schema field (`input_schema` or
/// `output_schema`) against the `io_schema` subset for `role`, translating
/// the first violation (violations are pre-sorted by pointer path) into a
/// [`RegistryError::SchemaContract`] carrying full provenance. Shared by
/// `insert_action`/`insert_workflow` so filesystem-loaded and bundled
/// (`stdlib::install_into`) YAML get the exact same check.
fn check_schema_grammar(
    field_name: &'static str,
    schema: &Value,
    role: io_schema::SchemaRole,
    owner_kind: &'static str,
    owner_id: &str,
    source_path: &str,
) -> Result<(), RegistryError> {
    if let Err(mut violations) = io_schema::validate_schema(schema, role) {
        // Pre-sorted by io_schema::validate_schema; take the first so the
        // caller gets one actionable, deterministic error rather than a
        // truncated dump of every violation.
        let v = violations.remove(0);
        return Err(RegistryError::SchemaContract {
            path: source_path.to_string(),
            owner_kind,
            owner_id: owner_id.to_string(),
            location: format!("/{field_name}{}", v.path),
            message: v.reason,
        });
    }
    Ok(())
}

/// Canonical JSON serialization, used to compare two schema property
/// definitions "byte-for-byte after canonical JSON object ordering".
/// `serde_json::Map` is BTreeMap-backed in this workspace (the
/// `preserve_order` feature is not enabled), so `to_string` already
/// produces deterministic, sorted-key output at every nesting level —
/// no separate canonicalization pass is needed.
fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(dir) = std::env::var("AYX_REGISTRY_DIR") {
        paths.push(PathBuf::from(dir));
    }
    if let Ok(home) = ayx_core::profile::ayx_config_home() {
        paths.push(home.join("registry"));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_stdlib_with_known_actions() {
        let reg = Registry::load_default().expect("registry loads");
        // The stdlib bundle is expected to ship at least these actions.
        assert!(reg.action("mongo.backup-restore").is_ok());
        assert!(reg.action("one.workspace-migrate").is_ok());
    }

    /// A pre-0.14.0 `*.tactic.yaml` must NOT be loaded: the 0.14.0 rename
    /// changed the extension, the `kind:` step tag, and the workflow key, so
    /// the file's contents are no longer valid. Loading it would be wrong;
    /// loading it *silently* would be worse — an operator override that
    /// tightened `safety` would be dropped and the bundled action would
    /// reclaim the id with a weaker gate. `load_dir` skips it and warns.
    #[test]
    fn legacy_tactic_yaml_is_not_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("legacy-override.tactic.yaml"),
            "id: legacy.override\n\
             title: Legacy override\n\
             summary: Should not load\n\
             safety: destructive\n\
             steps:\n\
             \x20 - kind: note\n\
             \x20   text: nope\n",
        )
        .expect("write legacy file");

        let mut reg = Registry::default();
        reg.load_dir(dir.path())
            .expect("load_dir skips, never errors");

        assert!(
            reg.action("legacy.override").is_err(),
            "legacy *.tactic.yaml must not be loaded"
        );
        assert!(
            reg.sources.is_empty(),
            "a skipped legacy file must not be recorded as a source"
        );
    }

    /// The new extension must still load, so the test above is proving the
    /// extension gate rather than a broken loader.
    #[test]
    fn action_yaml_extension_still_loads() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("fresh.action.yaml"),
            "id: fresh.one\n\
             title: Fresh action\n\
             summary: Should load\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: note\n\
             \x20   text: yep\n",
        )
        .expect("write action file");

        let mut reg = Registry::default();
        reg.load_dir(dir.path()).expect("loads");

        assert!(reg.action("fresh.one").is_ok(), "*.action.yaml must load");
    }

    /// Step 2: a well-formed declared `input_schema`/`output_schema` loads
    /// and round-trips onto the parsed `Action` unchanged.
    #[test]
    fn declared_action_schema_loads() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("declared.action.yaml"),
            "id: schema.declared\n\
             title: Declared schema action\n\
             summary: Has an explicit contract\n\
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
             \x20     description: Named profile.\n\
             output_schema:\n\
             \x20 type: object\n\
             \x20 description: Result.\n\
             \x20 required: [action_id]\n\
             \x20 properties:\n\
             \x20   action_id:\n\
             \x20     type: string\n\
             \x20     description: Stable action id.\n\
             \x20     const: schema.declared\n",
        )
        .expect("write action file");

        let mut reg = Registry::default();
        reg.load_dir(dir.path()).expect("declared schema loads");

        let action = reg.action("schema.declared").expect("action present");
        let input = action.input_schema.as_ref().expect("input_schema present");
        assert_eq!(input["required"], json!(["profile"]));
        let output = action
            .output_schema
            .as_ref()
            .expect("output_schema present");
        assert_eq!(
            output["properties"]["action_id"]["const"],
            json!("schema.declared")
        );
    }

    /// Step 2: a malformed `input_schema` fails at load time (not later, as
    /// an opaque executor error), and the error names the owning file,
    /// action id, and a JSON-pointer-like location.
    #[test]
    fn malformed_input_schema_reports_provenance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.action.yaml");
        std::fs::write(
            &path,
            "id: schema.bad-input\n\
             title: Bad input schema\n\
             summary: input_schema uses an unsupported keyword\n\
             safety: read_only\n\
             steps:\n\
             \x20 - kind: note\n\
             \x20   text: nope\n\
             input_schema:\n\
             \x20 type: object\n\
             \x20 additionalProperties: false\n\
             \x20 properties: {}\n\
             \x20 pattern: \"^x$\"\n",
        )
        .expect("write action file");

        let mut reg = Registry::default();
        let err = reg.load_dir(dir.path()).unwrap_err();
        match err {
            RegistryError::SchemaContract {
                path: err_path,
                owner_kind,
                owner_id,
                location,
                ..
            } => {
                assert_eq!(err_path, path.display().to_string());
                assert_eq!(owner_kind, "action");
                assert_eq!(owner_id, "schema.bad-input");
                assert!(
                    location.starts_with("/input_schema/"),
                    "location should be scoped under /input_schema, got {location}"
                );
            }
            other => panic!("expected SchemaContract, got {other:?}"),
        }
    }

    /// Step 2: same check, `output_schema` field, and workflow owner kind.
    #[test]
    fn malformed_output_schema_on_workflow_reports_provenance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.workflow.yaml");
        std::fs::write(
            &path,
            "id: schema.bad-output-workflow\n\
             title: Bad output schema workflow\n\
             summary: output_schema root is not type object\n\
             safety: read_only\n\
             actions: []\n\
             output_schema:\n\
             \x20 type: string\n",
        )
        .expect("write workflow file");

        let mut reg = Registry::default();
        let err = reg.load_dir(dir.path()).unwrap_err();
        match err {
            RegistryError::SchemaContract {
                path: err_path,
                owner_kind,
                owner_id,
                location,
                ..
            } => {
                assert_eq!(err_path, path.display().to_string());
                assert_eq!(owner_kind, "workflow");
                assert_eq!(owner_id, "schema.bad-output-workflow");
                assert!(
                    location.starts_with("/output_schema/"),
                    "location should be scoped under /output_schema, got {location}"
                );
            }
            other => panic!("expected SchemaContract, got {other:?}"),
        }
    }

    /// Step 2: the same grammar check applies to the bundled stdlib path
    /// (`stdlib::install_into`), not just `load_dir` — proven indirectly by
    /// `load_default` (which calls both) still succeeding, since none of
    /// the bundled v2 files declare a schema yet (Task 5).
    #[test]
    fn bundled_stdlib_has_no_schema_grammar_violations() {
        Registry::load_default().expect("bundled stdlib passes grammar + finalization");
    }

    #[test]
    fn safety_apply_required_classification() {
        assert!(!Safety::ReadOnly.requires_apply());
        assert!(Safety::Mutating.requires_apply());
        assert!(Safety::Destructive.requires_apply());
    }

    #[test]
    fn workflow_safety_is_max_of_referenced_actions() {
        let reg = Registry::load_default().expect("registry loads");
        // ops.backup-restore references mongo.backup-restore (mutating) and
        // mongo.doctor (read_only). Effective safety must be at least
        // Mutating.
        let w = reg.workflow("ops.backup-restore").unwrap();
        assert!(w.safety.rank() >= Safety::Mutating.rank());
        // governance.go-live references one.workspace-migrate (destructive)
        // — even if declared lower, must promote to destructive.
        let w = reg.workflow("governance.go-live").unwrap();
        assert_eq!(w.safety, Safety::Destructive);
    }

    #[test]
    fn safety_max_is_commutative() {
        assert_eq!(Safety::ReadOnly.max(Safety::Mutating), Safety::Mutating);
        assert_eq!(
            Safety::Mutating.max(Safety::Destructive),
            Safety::Destructive
        );
        assert_eq!(
            Safety::Destructive.max(Safety::ReadOnly),
            Safety::Destructive
        );
    }

    #[test]
    fn resolve_ranks_by_keyword_match() {
        let reg = Registry::load_default().expect("registry loads");
        let hits = reg.resolve("I need to back up mongo before a migration");
        assert!(!hits.is_empty(), "expected at least one resolve hit");
        let top = &hits[0];
        assert!(
            top.action_id.contains("mongo") || top.action_id.contains("backup"),
            "top hit should be mongo/backup-related, got {}",
            top.action_id
        );
    }
}
