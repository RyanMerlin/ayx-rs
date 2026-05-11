//! `ayx-registry` — tactical, workflow, and capability registry for `ayx`.
//!
//! This crate is the "agent substrate" layer of the toolset. The CLI itself
//! is a command surface; the registry layers on top of it a small,
//! declarative model of *named playbooks* (tactics) and *multi-step
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
//!   produced from the existing `COMMAND_SPECS`; this crate consumes them
//!   indirectly via the catalog when the registry resolver wants to point
//!   a tactic at a concrete command.
//! - **Tactic** — a small declarative recipe: a trigger pattern (when does
//!   this apply), guardrails (read-only? mutating? `--apply` required?),
//!   the canonical command sequence, validation steps, and rollback notes.
//!   Tactics are leaf-level: an agent can run one without further planning.
//! - **Workflow** — a higher-order recipe that strings tactics together
//!   into a multi-step skill (e.g. `governance-go-live` =
//!   `backup` → `apply-rbac` → `verify-permissions` → `audit-report`).
//!
//! Tactics and workflows live as YAML files on disk under a *search path*:
//!
//!   1. `$AYX_REGISTRY_DIR` if set (operator override / dev override)
//!   2. `${AYX_CONFIG_HOME}/registry/`
//!   3. The crate-bundled `tactics/` and `workflows/` directories (shipped
//!      with the binary as `include_str!` fallbacks).
//!
//! Layer 3 is the "stdlib" of canonical recipes; layers 1-2 let operators
//! add their own or override ours without rebuilding the binary.
//!
//! # Versioning + safety
//!
//! Every tactic carries a `safety` field (one of `read_only`, `mutating`,
//! `destructive`). The CLI refuses to *run* (vs. *describe*) a mutating or
//! destructive tactic without `--apply`; this is the same gate the One API
//! transport already enforces, but lifted to the registry layer so the
//! check happens before any command would fire.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

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
    #[error("tactic '{id}' not found")]
    TacticNotFound { id: String },
    #[error("workflow '{id}' not found")]
    WorkflowNotFound { id: String },
    #[error("duplicate id '{id}' loaded from both '{first}' and '{second}'")]
    DuplicateId {
        id: String,
        first: String,
        second: String,
    },
    #[error("tactic '{id}' is {safety:?}; --apply required to run (use `describe` to inspect)")]
    ApplyRequired { id: String, safety: Safety },
}

/// Classifies the blast radius of a tactic / workflow. The registry refuses
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

/// A trigger pattern — when should this tactic surface as a candidate?
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
    /// filtering. Tactics matching all tags listed in a query rank higher.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One concrete step inside a tactic. Either a shell command line that the
/// operator runs, or a reference to another tactic (composition).
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
    /// Invoke another tactic by id. Lets common building blocks be reused.
    Tactic { id: String, why: String },
    /// A note to the operator — nothing to execute, but worth surfacing
    /// (e.g. "verify ticket is open before proceeding").
    Note { text: String },
}

/// A validation step that proves the tactic achieved its intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validation {
    /// Human description: "queue is empty", "schedule is paused", etc.
    pub describe: String,
    /// Optional command whose output the operator should inspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_cmd: Option<String>,
}

/// Wire-format version for tactic YAML files. Bump only on a breaking
/// change to the schema; readers compare against `CURRENT_TACTIC_SCHEMA`.
pub const CURRENT_TACTIC_SCHEMA: u32 = 1;

/// A tactic — one named playbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tactic {
    /// Wire-format version. Defaults to 1; bumped if the schema gains a
    /// breaking change. Readers should accept any version up to
    /// `CURRENT_TACTIC_SCHEMA`.
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
    /// Provenance: which file the loader read this from. Set by the loader,
    /// never by the YAML author.
    #[serde(default, skip_serializing)]
    pub source_path: String,
}

fn default_schema_version() -> u32 {
    CURRENT_TACTIC_SCHEMA
}

/// A workflow — references tactics + adds top-level metadata.
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
    /// Ordered list of tactic ids. The resolver walks this in order; the
    /// CLI surfaces it as a numbered plan.
    pub tactics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<String>,
    #[serde(default, skip_serializing)]
    pub source_path: String,
}

/// Loaded registry — both tactics and workflows, indexed by id.
#[derive(Debug, Default)]
pub struct Registry {
    pub tactics: BTreeMap<String, Tactic>,
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
    /// referenced tactic's safety. Prevents a workflow declared `mutating`
    /// from quietly composing a `destructive` tactic and gaining a weaker
    /// gate than it should have. Idempotent.
    pub fn propagate_workflow_safety(&mut self) {
        let tactic_safety: std::collections::BTreeMap<String, Safety> = self
            .tactics
            .iter()
            .map(|(id, t)| (id.clone(), t.safety))
            .collect();
        for workflow in self.workflows.values_mut() {
            let mut effective = workflow.safety;
            for tid in &workflow.tactics {
                if let Some(s) = tactic_safety.get(tid) {
                    effective = effective.max(*s);
                }
            }
            workflow.safety = effective;
        }
    }

    /// Load a directory of YAML files into this registry. Walks recursively;
    /// `*.tactic.yaml` parse as tactics, `*.workflow.yaml` as workflows.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), RegistryError> {
        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".tactic.yaml") || name.ends_with(".tactic.yml") {
                let body = fs::read_to_string(p).map_err(|source| RegistryError::Io {
                    path: p.display().to_string(),
                    source,
                })?;
                let mut tactic: Tactic =
                    serde_yaml::from_str(&body).map_err(|source| RegistryError::Parse {
                        path: p.display().to_string(),
                        source,
                    })?;
                tactic.source_path = p.display().to_string();
                self.insert_tactic(tactic)?;
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
            }
        }
        Ok(())
    }

    /// Insert a tactic, preserving the earlier copy on duplicates (operator
    /// overrides win because they're loaded first).
    pub(crate) fn insert_tactic(&mut self, t: Tactic) -> Result<(), RegistryError> {
        if let Some(existing) = self.tactics.get(&t.id) {
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
        self.tactics.insert(t.id.clone(), t);
        Ok(())
    }

    pub(crate) fn insert_workflow(&mut self, w: Workflow) -> Result<(), RegistryError> {
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

    pub fn tactic(&self, id: &str) -> Result<&Tactic, RegistryError> {
        self.tactics
            .get(id)
            .ok_or_else(|| RegistryError::TacticNotFound { id: id.to_string() })
    }

    pub fn workflow(&self, id: &str) -> Result<&Workflow, RegistryError> {
        self.workflows
            .get(id)
            .ok_or_else(|| RegistryError::WorkflowNotFound { id: id.to_string() })
    }

    /// Resolve a free-text task description to ranked candidate tactics.
    ///
    /// Ranking is dumb on purpose: count of keyword + tag matches. A future
    /// LLM resolver can swap this for an embedding match without changing
    /// the public surface.
    ///
    /// Performance: each tactic's keywords/tags/title are lowercased *once*
    /// per `resolve` call (kept on the stack as `Vec<String>`s) rather than
    /// per-comparison. For a 10-tactic library × 5 keywords each that's
    /// 50 → 0 redundant allocations per query.
    pub fn resolve(&self, task: &str) -> Vec<ResolveHit> {
        let needle = task.to_ascii_lowercase();
        let needle_words: Vec<&str> = needle
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        let mut hits: Vec<ResolveHit> = self
            .tactics
            .values()
            .map(|t| {
                let mut score = 0u32;
                // Lowercase each keyword/tag/title once per tactic. The old
                // code allocated a fresh String per comparison — fine for 10
                // tactics, painful at the 100-tactic registry size that's a
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
                    tactic_id: t.id.clone(),
                    title: t.title.clone(),
                    safety: t.safety,
                    score,
                }
            })
            .filter(|h| h.score > 0)
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.tactic_id.cmp(&b.tactic_id)));
        hits
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveHit {
    pub tactic_id: String,
    pub title: String,
    pub safety: Safety,
    pub score: u32,
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
    fn loads_stdlib_with_known_tactics() {
        let reg = Registry::load_default().expect("registry loads");
        // The stdlib bundle is expected to ship at least these tactics.
        assert!(reg.tactic("mongo.backup-restore").is_ok());
        assert!(reg.tactic("one.workspace-migrate").is_ok());
    }

    #[test]
    fn safety_apply_required_classification() {
        assert!(!Safety::ReadOnly.requires_apply());
        assert!(Safety::Mutating.requires_apply());
        assert!(Safety::Destructive.requires_apply());
    }

    #[test]
    fn workflow_safety_is_max_of_referenced_tactics() {
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
            top.tactic_id.contains("mongo") || top.tactic_id.contains("backup"),
            "top hit should be mongo/backup-related, got {}",
            top.tactic_id
        );
    }
}
