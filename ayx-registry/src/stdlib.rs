//! Crate-bundled stdlib of canonical tactics and workflows.
//!
//! Each YAML is `include_str!`'d so the binary ships with a working
//! registry even when `${AYX_CONFIG_HOME}/registry/` is empty. Operators
//! who want to override a recipe drop a same-id YAML in their config home
//! and the loader (which walks operator dirs first) keeps their version.

use crate::{Registry, RegistryError, Tactic, Workflow};

/// One bundled-resource pair: parsed body + its on-disk-like path label.
struct Bundled {
    path: &'static str,
    body: &'static str,
}

const TACTICS: &[Bundled] = &[
    Bundled {
        path: "bundled:tactics/mongo-backup-restore.tactic.yaml",
        body: include_str!("../tactics/mongo-backup-restore.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/mongo-doctor.tactic.yaml",
        body: include_str!("../tactics/mongo-doctor.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/one-workspace-migrate.tactic.yaml",
        body: include_str!("../tactics/one-workspace-migrate.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/server-auth-saml-diagnose.tactic.yaml",
        body: include_str!("../tactics/server-auth-saml-diagnose.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/one-flow-promote.tactic.yaml",
        body: include_str!("../tactics/one-flow-promote.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/one-scheduling-pause.tactic.yaml",
        body: include_str!("../tactics/one-scheduling-pause.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/server-upgrade-preflight.tactic.yaml",
        body: include_str!("../tactics/server-upgrade-preflight.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/server-logs-triage.tactic.yaml",
        body: include_str!("../tactics/server-logs-triage.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/mongo-queue-stuck.tactic.yaml",
        body: include_str!("../tactics/mongo-queue-stuck.tactic.yaml"),
    },
    Bundled {
        path: "bundled:tactics/workflow-cloud-convert-bulk.tactic.yaml",
        body: include_str!("../tactics/workflow-cloud-convert-bulk.tactic.yaml"),
    },
];

const WORKFLOWS: &[Bundled] = &[
    Bundled {
        path: "bundled:workflows/governance-go-live.workflow.yaml",
        body: include_str!("../workflows/governance-go-live.workflow.yaml"),
    },
    Bundled {
        path: "bundled:workflows/backup-restore.workflow.yaml",
        body: include_str!("../workflows/backup-restore.workflow.yaml"),
    },
];

pub(crate) fn install_into(reg: &mut Registry) -> Result<(), RegistryError> {
    for b in TACTICS {
        let mut tactic: Tactic =
            serde_yaml::from_str(b.body).map_err(|source| RegistryError::Parse {
                path: b.path.to_string(),
                source,
            })?;
        tactic.source_path = b.path.to_string();
        reg.insert_tactic(tactic)?;
    }
    for b in WORKFLOWS {
        let mut workflow: Workflow =
            serde_yaml::from_str(b.body).map_err(|source| RegistryError::Parse {
                path: b.path.to_string(),
                source,
            })?;
        workflow.source_path = b.path.to_string();
        reg.insert_workflow(workflow)?;
    }
    Ok(())
}
