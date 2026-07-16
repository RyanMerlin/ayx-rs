//! Crate-bundled stdlib of canonical actions and workflows.
//!
//! Each YAML is `include_str!`'d so the binary ships with a working
//! registry even when `${AYX_CONFIG_HOME}/registry/` is empty. Operators
//! who want to override a recipe drop a same-id YAML in their config home
//! and the loader (which walks operator dirs first) keeps their version.

use crate::{Action, Registry, RegistryError, Workflow};

/// One bundled-resource pair: parsed body + its on-disk-like path label.
struct Bundled {
    path: &'static str,
    body: &'static str,
}

const ACTIONS: &[Bundled] = &[
    Bundled {
        path: "bundled:actions/mongo-backup-restore.action.yaml",
        body: include_str!("../actions/mongo-backup-restore.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/mongo-doctor.action.yaml",
        body: include_str!("../actions/mongo-doctor.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/one-workspace-migrate.action.yaml",
        body: include_str!("../actions/one-workspace-migrate.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/server-auth-saml-diagnose.action.yaml",
        body: include_str!("../actions/server-auth-saml-diagnose.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/one-flow-promote.action.yaml",
        body: include_str!("../actions/one-flow-promote.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/one-scheduling-pause.action.yaml",
        body: include_str!("../actions/one-scheduling-pause.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/server-upgrade-preflight.action.yaml",
        body: include_str!("../actions/server-upgrade-preflight.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/server-logs-triage.action.yaml",
        body: include_str!("../actions/server-logs-triage.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/mongo-queue-stuck.action.yaml",
        body: include_str!("../actions/mongo-queue-stuck.action.yaml"),
    },
    Bundled {
        path: "bundled:actions/workflow-cloud-convert-bulk.action.yaml",
        body: include_str!("../actions/workflow-cloud-convert-bulk.action.yaml"),
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
    for b in ACTIONS {
        let mut action: Action =
            serde_yaml::from_str(b.body).map_err(|source| RegistryError::Parse {
                path: b.path.to_string(),
                source,
            })?;
        action.source_path = b.path.to_string();
        reg.insert_action(action)?;
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
